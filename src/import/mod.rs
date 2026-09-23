//! Bringing a notebook in from somewhere else.
//!
//! A source's parser produces [`Incoming`]; everything after is shared, so a
//! new source is only a parser.

pub mod tiddlywiki;
pub mod wikitext;

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use crate::note::{self, Note};
use crate::notebook::Notebook;
use crate::paths::Paths;
use crate::style;
use crate::{Error, Result};

/// The frontmatter field naming what an importer did not translate; shared by
/// every source so `doctor` has one check.
pub const UNCONVERTED: &str = "unconverted";

/// What the source called this note, so re-importing it skips it.
pub const SOURCE_KEY: &str = "source_key";

/// How a source's bodies become Markdown, given a way to resolve the names
/// notes use for each other.
pub type Converter<'a> = dyn Fn(&str, &wikitext::Resolve) -> wikitext::Converted + 'a;

/// One note as a source hands it over, before noda has given it an identity.
pub struct Incoming {
    pub title: String,
    /// As the source held it; committed before conversion, so history keeps it.
    pub body: String,
    pub tags: Vec<String>,
    /// RFC 3339, when the source had one to give.
    pub created: Option<String>,
    pub updated: Option<String>,
    /// Other source fields, carried through untouched.
    pub extra: Vec<String>,
    /// What other notes link to it by; a tiddler's title.
    pub key: String,
}

#[derive(Default)]
struct Report {
    written: usize,
    converted: usize,
    /// Title and reason for each note that was not written.
    skipped: Vec<(String, String)>,
    /// Notes written with the source's text whose conversion could not be.
    not_converted: Vec<(String, String)>,
    /// Constructs left in `WikiText`, and how many notes carry each.
    left: BTreeMap<&'static str, usize>,
}

/// Writes an import into the active notebook, as two commits.
///
/// The source's own text, then the conversion, so `noda diff` shows the
/// conversion and `restore` reaches the original. A source already in Markdown
/// passes `convert: None` and gets only the first.
pub fn write(
    paths: &Paths,
    source: &str,
    incoming: Vec<Incoming>,
    skipped: Vec<(String, String)>,
    convert: Option<&Converter>,
) -> Result<String> {
    let notebook = Notebook::open_active(paths)?;
    let mut report = Report {
        skipped,
        ..Report::default()
    };

    let held = already_here(&notebook)?;
    let mut taken = notebook.taken_ids()?;

    // Where every source name ends up, for rewriting links; seeded with what
    // is already held, since a wiki may be imported in pieces.
    let mut by_key = held.clone();

    let mut named: Vec<(String, String, Incoming)> = Vec::new();
    for note in incoming {
        if let Some(file) = held.get(&note.key) {
            report
                .skipped
                .push((note.title, format!("already imported as {file}")));
            continue;
        }
        // Exports taken in pieces overlap; the first copy lands.
        if by_key.contains_key(&note.key) {
            report
                .skipped
                .push((note.title, "given twice in this import".to_string()));
            continue;
        }
        if let Err(e) = check(&note) {
            report.skipped.push((note.title, e.to_string()));
            continue;
        }
        let id = note::mint_id(&taken);
        taken.insert(note::normalize_id(&id));
        let slug = note::slugify(&note.title);
        by_key.insert(note.key.clone(), note::file_name(&id, &slug));
        named.push((id, slug, note));
    }

    if named.is_empty() {
        return Ok(summary(&report, source, None));
    }

    // Pass one: every note as the source wrote it. A write that fails is
    // skipped rather than ending the run, which would leave the rest written
    // but uncommitted.
    let mut files: Vec<PathBuf> = Vec::new();
    let mut written: Vec<(String, String, Incoming)> = Vec::new();
    for (id, slug, note) in named {
        let file = note::file_name(&id, &slug);
        if let Err(e) = std::fs::write(notebook.path.join(&file), render(&note, &note.body, &[])) {
            by_key.remove(&note.key);
            report.skipped.push((note.title, e.to_string()));
            continue;
        }
        files.push(PathBuf::from(file));
        report.written += 1;
        written.push((id, slug, note));
    }

    if written.is_empty() {
        return Ok(summary(&report, source, None));
    }
    commit(
        &notebook,
        &files,
        &format!("import: {} notes from {source}", report.written),
    )?;

    // Pass two: the conversion, once every name is resolvable.
    let Some(convert) = convert else {
        return Ok(summary(&report, source, None));
    };
    let resolve = |key: &str| by_key.get(key).cloned();

    let mut changed: Vec<PathBuf> = Vec::new();
    for (id, slug, note) in &written {
        let converted = convert(&note.body, &resolve);
        let left: Vec<&str> = converted.left.iter().copied().collect();
        if converted.text == note.body && left.is_empty() {
            continue;
        }
        let file = note::file_name(id, slug);
        // As in pass one: reported, not fatal; the source's text is committed.
        if let Err(e) = std::fs::write(
            notebook.path.join(&file),
            render(note, &converted.text, &left),
        ) {
            report
                .not_converted
                .push((note.title.clone(), e.to_string()));
            continue;
        }
        // Counted only once the `unconverted:` field is on disk.
        for name in &left {
            *report.left.entry(*name).or_default() += 1;
        }
        changed.push(PathBuf::from(file));
        report.converted += 1;
    }
    if !changed.is_empty() {
        commit(
            &notebook,
            &changed,
            &format!("import: convert {} notes from {source}", report.converted),
        )?;
    }
    Ok(summary(&report, source, Some(&notebook)))
}

/// The source's own fields carried through, plus `source_key` and
/// `unconverted`.
fn render(note: &Incoming, body: &str, left: &[&str]) -> String {
    let mut extra = note.extra.clone();
    extra.push(format!("{SOURCE_KEY}: {}", note.key));
    if !left.is_empty() {
        extra.push(format!("{UNCONVERTED}: {}", left.join(", ")));
    }
    Note {
        title: note.title.clone(),
        tags: note.tags.clone(),
        created: note.created.clone(),
        updated: note.updated.clone(),
        pinned: None,
        extra,
        body: body.to_string(),
    }
    .render()
}

/// What the notebook already holds, keyed by the name its source knew it as.
fn already_here(notebook: &Notebook) -> Result<HashMap<String, String>> {
    let prefix = format!("{SOURCE_KEY}: ");
    let (notes, _) = notebook.inventory()?;
    Ok(notes
        .into_iter()
        .filter_map(|file| {
            let key = file
                .note
                .extra
                .iter()
                .find_map(|line| line.strip_prefix(&prefix))?;
            Some((
                key.trim().to_string(),
                note::file_name(&file.id, &file.slug),
            ))
        })
        .collect())
}

/// Refuses a note noda could not write faithfully, rather than write one that
/// reads back as something else.
fn check(note: &Incoming) -> Result<()> {
    note::validate_title(&note.title)?;
    for tag in &note.tags {
        note::validate_tag(tag)?;
    }
    // A title with nothing alphanumeric has no slug.
    if note::slugify(&note.title).is_empty() {
        return Err(Error::msg("the title makes no filename"));
    }
    if note.extra.iter().any(|line| {
        line.starts_with(&format!("{SOURCE_KEY}:")) || line.starts_with(&format!("{UNCONVERTED}:"))
    }) {
        return Err(Error::msg(format!(
            "carries its own `{SOURCE_KEY}` or `{UNCONVERTED}` field, which the import would overwrite"
        )));
    }
    Ok(())
}

fn commit(notebook: &Notebook, files: &[PathBuf], message: &str) -> Result<()> {
    let paths: Vec<&Path> = files.iter().map(PathBuf::as_path).collect();
    notebook.commit(&paths, message)
}

/// A heading and the reasons under it, counted rather than listing every
/// title, which a large export would make unreadable.
fn reasons(out: &mut String, heading: &str, entries: &[(String, String)]) {
    if entries.is_empty() {
        return;
    }
    let mut why: BTreeMap<&str, usize> = BTreeMap::new();
    for (_, reason) in entries {
        *why.entry(reason.as_str()).or_default() += 1;
    }
    let _ = writeln!(out, "\n{}", style::paint(style::MUTED, heading));
    for (reason, count) in &why {
        let _ = writeln!(out, "  {count} {reason}");
    }
}

fn summary(report: &Report, source: &str, notebook: Option<&Notebook>) -> String {
    let mut out = String::new();
    let noun = |n: usize| if n == 1 { "note" } else { "notes" };
    let _ = writeln!(
        out,
        "imported  {} {} from {source}",
        report.written,
        noun(report.written)
    );
    if report.converted > 0 {
        let _ = writeln!(
            out,
            "converted {} {}",
            report.converted,
            noun(report.converted)
        );
    }
    if !report.left.is_empty() {
        let _ = writeln!(
            out,
            "\n{}",
            style::paint(
                style::MUTED,
                "left as WikiText, and named in each note's `unconverted:` field:"
            )
        );
        for (name, count) in &report.left {
            let _ = writeln!(out, "  {count} {} {name}", noun(*count));
        }
    }
    reasons(&mut out, "not imported:", &report.skipped);
    reasons(
        &mut out,
        "imported, but left as the source wrote them:",
        &report.not_converted,
    );
    if notebook.is_some() && report.converted > 0 {
        let _ = write!(
            out,
            "\n{}",
            style::paint(
                style::MUTED,
                "`noda diff` shows the conversion; the commit before it holds the originals"
            )
        );
    }
    out.trim_end().to_string()
}
