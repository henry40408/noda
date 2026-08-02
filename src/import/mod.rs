//! Bringing a notebook in from somewhere else.
//!
//! One format-specific parser per source, and one shared back end. The parser's
//! whole job is to produce [`Incoming`]; everything after that — minting ids,
//! writing files, resolving links, committing — is the same work whatever the
//! notes came from, so adding a second source is a parser and nothing else.

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

/// The frontmatter field naming what an importer would not translate.
///
/// One field for every source rather than one per format: `doctor` needs a
/// single check, and a note is either finished or it is not.
pub const UNCONVERTED: &str = "unconverted";

/// The frontmatter field holding what the source called this note. It is what
/// makes a second import able to say "already here" instead of making a
/// duplicate, and what lets somebody trace a note back to the export.
pub const SOURCE_KEY: &str = "source_key";

/// How a source's own bodies become Markdown, given a way to resolve the names
/// notes use for each other. A source that is already Markdown has none.
pub type Converter<'a> = dyn Fn(&str, &wikitext::Resolve) -> wikitext::Converted + 'a;

/// One note as a source hands it over, before noda has given it an identity.
pub struct Incoming {
    pub title: String,
    /// The body exactly as the source held it. Nothing is converted here: the
    /// import writes this first and commits it, so that whatever a converter
    /// does next is a second commit with the original still behind it.
    pub body: String,
    pub tags: Vec<String>,
    /// RFC 3339, when the source had one to give.
    pub created: Option<String>,
    pub updated: Option<String>,
    /// Frontmatter lines to carry through untouched — the fields the source had
    /// that noda has no opinion about.
    pub extra: Vec<String>,
    /// What the source calls this note, for resolving the links other notes make
    /// to it. A tiddler's is its title.
    pub key: String,
}

/// How an import went, for the one-screen summary it prints.
#[derive(Default)]
struct Report {
    written: usize,
    converted: usize,
    /// Why a tiddler did not become a note, worst-first as the source gave them.
    skipped: Vec<(String, String)>,
    /// Constructs left in `WikiText`, and how many notes carry each.
    left: BTreeMap<&'static str, usize>,
}

/// Writes an import into the active notebook, as two commits.
///
/// **Two, deliberately.** The first holds every note exactly as the source
/// wrote it; the second holds the conversion. Nothing an importer does can
/// therefore be lost: `noda diff` shows the whole conversion before it is
/// pushed anywhere, and `noda restore <note> <the first commit>` brings any
/// note back to the text the export actually contained. That is why the body
/// is not converted on the way in and why the original is not copied into the
/// frontmatter — git already keeps it, and keeps it better.
///
/// `convert` is what turns a body, given a way to resolve the source's own
/// names. A source whose notes are already Markdown passes `None` and gets the
/// first commit and nothing else.
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

    // Everything the notebook already holds, by the key its source gave it. A
    // second run of the same import is a no-op rather than a second copy.
    let held = already_here(&notebook)?;
    let mut taken = notebook.taken_ids()?;

    let mut named: Vec<(String, String, Incoming)> = Vec::new();
    for note in incoming {
        if let Some(file) = held.get(&note.key) {
            report
                .skipped
                .push((note.title, format!("already imported as {file}")));
            continue;
        }
        if let Err(e) = check(&note) {
            report.skipped.push((note.title, e.to_string()));
            continue;
        }
        let id = note::mint_id(&taken);
        taken.insert(note::normalize_id(&id));
        let slug = note::slugify(&note.title);
        named.push((id, slug, note));
    }

    if named.is_empty() {
        return Ok(summary(&report, source, None));
    }

    // Pass one: every note as the source wrote it.
    let mut files: Vec<PathBuf> = Vec::new();
    for (id, slug, note) in &named {
        let file = note::file_name(id, slug);
        std::fs::write(notebook.path.join(&file), render(note, &note.body, &[]))?;
        files.push(PathBuf::from(file));
        report.written += 1;
    }
    commit(
        &notebook,
        &files,
        &format!("import: {} notes from {source}", report.written),
    )?;

    // Pass two: the conversion, with every name now resolvable — which is the
    // whole reason it cannot happen before the files exist.
    let Some(convert) = convert else {
        return Ok(summary(&report, source, None));
    };
    let by_key: HashMap<&str, String> = named
        .iter()
        .map(|(id, slug, note)| (note.key.as_str(), note::file_name(id, slug)))
        .collect();
    let resolve = |key: &str| by_key.get(key).cloned();

    let mut changed: Vec<PathBuf> = Vec::new();
    for (id, slug, note) in &named {
        let converted = convert(&note.body, &resolve);
        let left: Vec<&str> = converted.left.iter().copied().collect();
        if converted.text == note.body && left.is_empty() {
            continue;
        }
        for name in &left {
            *report.left.entry(*name).or_default() += 1;
        }
        let file = note::file_name(id, slug);
        std::fs::write(
            notebook.path.join(&file),
            render(note, &converted.text, &left),
        )?;
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

/// The note as it goes to disk: the source's own fields carried through, plus
/// the two noda adds.
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

/// Whether noda can hold this note at all. A source may carry a title or a tag
/// noda's own files cannot spell — and the answer is to say which and leave it
/// out, never to write a note that reads back as something else.
fn check(note: &Incoming) -> Result<()> {
    note::validate_title(&note.title)?;
    for tag in &note.tags {
        note::validate_tag(tag)?;
    }
    // Two notes may share a slug — the id keeps their filenames apart — but a
    // title with nothing alphanumeric in it has no slug at all.
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

/// The one screen an import prints: what landed, what did not, and what is left
/// to do by hand.
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
    if !report.skipped.is_empty() {
        let mut why: BTreeMap<&str, usize> = BTreeMap::new();
        for (_, reason) in &report.skipped {
            *why.entry(reason.as_str()).or_default() += 1;
        }
        let _ = writeln!(out, "\n{}", style::paint(style::MUTED, "not imported:"));
        for (reason, count) in &why {
            let _ = writeln!(out, "  {count} {reason}");
        }
    }
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
