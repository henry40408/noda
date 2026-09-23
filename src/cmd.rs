//! Command implementations. Each takes `Paths` (or, for the `_in` half, an open
//! `Notebook`) explicitly, so tests run against a throwaway root.

use std::collections::{BTreeSet, HashMap};
use std::fmt::Write as _;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::config::{self, Config};
use crate::import;
use crate::link;
use crate::note::{self, Note};
use crate::notebook::{self, Notebook, Problem};
use crate::paths::Paths;
use crate::query::Query;
use crate::remote;
use crate::style;
use crate::todo;
use crate::{Error, Result};

pub const DEFAULT_NOTEBOOK: &str = config::DEFAULT_NOTEBOOK;

/// Scratch file used when composing a note in `$EDITOR`.
const EDIT_FILE: &str = "NOTE_EDITMSG.md";

/// Safe to run more than once.
pub fn init(paths: &Paths) -> Result<String> {
    paths.create_dirs()?;
    let mut lines = Vec::new();

    // Commented-out defaults change nothing but show what can be set.
    if Config::write_template(paths)? {
        lines.push(format!(
            "wrote {}",
            paths.config_dir().join("config.toml").display()
        ));
    }

    let name = Config::load(paths)?
        .get("notebook")
        .unwrap_or(DEFAULT_NOTEBOOK)
        .to_string();
    if Notebook::exists(paths, &name) {
        lines.push(format!("notebook `{name}` already exists"));
    } else {
        let notebook = Notebook::create(paths, &name)?;
        lines.push(format!(
            "created notebook `{name}` at {}",
            notebook.path.display()
        ));
    }
    if paths.active_notebook().is_err() {
        paths.set_active_notebook(&name)?;
        lines.push(format!("active notebook: {name}"));
    }
    Ok(lines.join("\n"))
}

/// Every setting with where its value came from.
pub fn config_show(paths: &Paths) -> Result<String> {
    let config = Config::load(paths)?;
    let rows = effective(paths, &config);

    let key_width = rows.iter().map(|r| display_width(&r.0)).max().unwrap_or(0);
    let value_width = rows.iter().map(|r| display_width(&r.1)).max().unwrap_or(0);
    let mut out = String::new();
    for (key, value, source) in rows {
        let _ = writeln!(
            out,
            "{}  {}  {}",
            pad(&key, key_width),
            pad(&value, value_width),
            style::paint(style::MUTED, &format!("({})", source.label()))
        );
    }
    Ok(out)
}

/// One setting's effective value, unadorned for a script.
pub fn config_get(paths: &Paths, key: &str) -> Result<String> {
    config::validate_key(key)?;
    let config = Config::load(paths)?;
    Ok(effective(paths, &config)
        .into_iter()
        .find(|(name, _, _)| name == key)
        .map(|(_, value, _)| value)
        .unwrap_or_default())
}

pub fn config_set(paths: &Paths, key: &str, value: &str) -> Result<String> {
    let mut config = Config::load(paths)?;
    config.set(key, value)?;
    Ok(format!("{key}  {value}"))
}

pub fn config_unset(paths: &Paths, key: &str) -> Result<String> {
    let mut config = Config::load(paths)?;
    if !config.unset(key)? {
        return Ok(format!("{key}  (was not set)"));
    }
    let now = effective(paths, &config)
        .into_iter()
        .find(|(name, _, _)| name == key);
    match now {
        Some((_, value, source)) => Ok(format!("{key}  {value}  (now from {})", source.label())),
        None => Ok(format!("{key}  unset")),
    }
}

/// Writes the starter template first if the file is missing.
pub fn config_edit(paths: &Paths) -> Result<String> {
    Config::write_template(paths)?;
    let path = paths.config_dir().join("config.toml");
    run_editor(&configured_editor(paths), &path)?;
    // Surface a typo now, not at the next unrelated command.
    Config::load(paths)?;
    Ok(format!("{}", path.display()))
}

fn effective(paths: &Paths, config: &Config) -> Vec<(String, String, config::Source)> {
    let (editor, editor_source) = config::editor(
        config.get("editor"),
        std::env::var("VISUAL").ok(),
        std::env::var("EDITOR").ok(),
    );
    let (author, author_source) = author(paths, config);
    let (notebook, notebook_source) = match config.get("notebook") {
        Some(name) => (name.to_string(), config::Source::File),
        None => (DEFAULT_NOTEBOOK.to_string(), config::Source::Default),
    };
    let (sign, sign_source) = sign(config);
    vec![
        ("editor".to_string(), editor, editor_source),
        ("author".to_string(), author, author_source),
        ("notebook".to_string(), notebook, notebook_source),
        ("sign".to_string(), sign.to_string(), sign_source),
    ]
}

/// Falls back to the user's git config, not a notebook's: this answers for every
/// notebook.
fn sign(config: &Config) -> (bool, config::Source) {
    if let Some(on) = config.sign() {
        return (on, config::Source::File);
    }
    match git2::Config::open_default().and_then(|git| git.get_bool("commit.gpgsign")) {
        Ok(on) => (on, config::Source::Git),
        Err(_) => (false, config::Source::Default),
    }
}

/// The identity commits are made under, and where it came from.
fn author(paths: &Paths, config: &Config) -> (String, config::Source) {
    if let Some(author) = config.get("author") {
        return (author.to_string(), config::Source::File);
    }
    // In the order git itself would ask.
    let from_git = Notebook::open_active(paths)
        .ok()
        .and_then(|notebook| notebook.git_author())
        .or_else(|| {
            let git = git2::Config::open_default().ok()?;
            let name = git.get_string("user.name").ok()?;
            let email = git.get_string("user.email").ok()?;
            Some(format!("{name} <{email}>"))
        });
    match from_git {
        Some(author) => (author, config::Source::Git),
        None => ("noda <noda@localhost>".to_string(), config::Source::Default),
    }
}

/// Creates a note and commits it. `content` of `None` opens `$EDITOR`.
pub fn add(
    paths: &Paths,
    title: Option<&str>,
    content: Option<&str>,
    tags: &[String],
) -> Result<String> {
    let notebook = Notebook::open_active(paths)?;

    // Before the editor opens, so a bad title is not found after composing.
    // `add_in` checks again, being reachable on its own.
    if let Some(title) = title {
        note::validate_title(title)?;
    }
    clean_tags(tags)?;

    let body = match content {
        Some(text) => text.to_string(),
        None => compose_in_editor(paths, title)?,
    };
    add_in(&notebook, title, &body, tags)
}

/// `add` in an already-open notebook with the body written — never opens an
/// editor, so `noda web` can call it.
pub fn add_in(
    notebook: &Notebook,
    title: Option<&str>,
    body: &str,
    tags: &[String],
) -> Result<String> {
    if let Some(title) = title {
        note::validate_title(title)?;
    }
    let tags = clean_tags(tags)?;
    let body = body.replace("\r\n", "\n");
    let title = match title {
        Some(t) if !t.trim().is_empty() => t.trim().to_string(),
        _ => derive_title(&body)
            .ok_or_else(|| Error::msg("aborted: the note is empty, so it has no title"))?,
    };

    let slug = note::slugify(&title);
    let id = note::mint_id(&notebook.taken_ids()?);
    // Both stamps, so no reader has to infer `updated` from `created`.
    let now = note::now();
    let note = Note {
        title,
        tags,
        created: Some(now.clone()),
        updated: Some(now),
        pinned: None,
        extra: Vec::new(),
        body: body.trim_start_matches('\n').to_string(),
    };

    let file = note::file_name(&id, &slug);
    std::fs::write(notebook.path.join(&file), note.render())?;
    notebook.commit(&[Path::new(&file)], &format!("add: {slug}"))?;

    Ok(summary(&id, &slug, &note.tags))
}

/// A struct rather than a row of arguments: every caller sets two fields at most.
#[derive(Default)]
pub struct List<'a> {
    /// Another notebook instead of the active one.
    pub notebook: Option<&'a str>,
    /// Anything more selective than one tag is `search`'s job.
    pub tag: Option<&'a str>,
    pub format: Format,
    pub only: Only,
    /// NUL-separate `Quiet` output, for `xargs -0` and names with spaces.
    pub null: bool,
    pub sort: Sort,
    /// Applied after `sort`, as `ls -r` is; the files turn with the notes.
    pub reverse: bool,
    /// Add the slug and both timestamps. Off by default because the slug
    /// repeats the title; one flag rather than one per column, as `ls -l` is.
    pub long: bool,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Sort {
    /// What a notebook walk already produces.
    #[default]
    Slug,
    /// Newest first, as is `Updated`.
    Created,
    Updated,
    /// Alphabetical.
    Title,
}

impl Sort {
    /// Defined once so a page's list and the ring [`Sort::next`] walks agree.
    pub const ALL: [Sort; 4] = [Sort::Slug, Sort::Created, Sort::Updated, Sort::Title];

    /// How `--sort` spells it, and what a screen calls it.
    pub fn name(self) -> &'static str {
        match self {
            Sort::Slug => "slug",
            Sort::Created => "created",
            Sort::Updated => "updated",
            Sort::Title => "title",
        }
    }

    /// [`Sort::name`]'s inverse, for an order carried in a URL.
    pub fn named(said: &str) -> Option<Sort> {
        Sort::ALL.into_iter().find(|sort| sort.name() == said)
    }

    /// The next order in [`Sort::ALL`], for a key that cycles through them.
    pub fn next(self) -> Sort {
        match self {
            Sort::Slug => Sort::Created,
            Sort::Created => Sort::Updated,
            Sort::Updated => Sort::Title,
            Sort::Title => Sort::Slug,
        }
    }
}

#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// Aligned columns for a person.
    #[default]
    Table,
    /// One object, for a program.
    Json,
    /// One identifier per line and nothing else.
    Quiet,
}

/// Whether a command that changes a note moves its `updated`. `Keep` is
/// `--no-touch`: a fixed typo or an added tag is not a rewrite, and the user
/// may own the field.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Touch {
    /// Set `updated` to now.
    #[default]
    Stamp,
    /// Leave `updated` as it was found.
    Keep,
}

#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub enum Only {
    #[default]
    Everything,
    Notes,
    Files,
}

/// Reads a `TiddlyWiki` 5 export into the active notebook.
///
/// Several files are one import, so links between the pieces resolve: every
/// file is read before anything is written, and an unreadable one stops the
/// import before it touches the notebook.
pub fn import_tiddlywiki(paths: &Paths, files: &[PathBuf], convert: bool) -> Result<String> {
    let mut notes = Vec::new();
    let mut skipped = Vec::new();
    for file in files {
        let text = std::fs::read_to_string(file)
            .map_err(|e| Error::msg(format!("{}: {e}", file.display())))?;
        let export = import::tiddlywiki::read(&text)
            .map_err(|e| Error::msg(format!("{}: {e}", file.display())))?;
        notes.extend(export.notes);
        skipped.extend(export.skipped);
    }
    let converter =
        |body: &str, resolve: &import::wikitext::Resolve| import::wikitext::convert(body, resolve);
    import::write(
        paths,
        "tiddlywiki",
        notes,
        skipped,
        convert.then_some(&converter),
    )
}

/// One note's row as printed strings, so widths are measured over what is written.
struct Listed {
    id: String,
    slug: String,
    created: String,
    updated: String,
    title: String,
    tags: Vec<String>,
    pinned: bool,
}

/// Lists notes as `id  title  [tags]`, aligned, then the notebook's files under
/// their own heading. `-l` adds the slug and both timestamps.
pub fn ls(paths: &Paths, options: &List) -> Result<String> {
    let name = match options.notebook {
        Some(name) => name.to_string(),
        None => notebook::active_name(paths)?,
    };
    let notebook = Notebook::open(paths, &name)?;
    let (notes, files) = notebook.inventory()?;
    let tag = options.tag;

    let mut notes: Vec<notebook::NoteFile> = if options.only == Only::Files {
        Vec::new()
    } else {
        notes
            .into_iter()
            .filter(|file| tag.is_none_or(|t| file.note.tags.iter().any(|nt| nt == t)))
            .collect()
    };
    sort_notes(&mut notes, options.sort);

    // Asking for one tag is asking about notes.
    let mut files = if tag.is_some() || options.only == Only::Notes {
        Vec::new()
    } else {
        files
    };

    if options.reverse {
        notes.reverse();
        files.reverse();
    }

    match options.format {
        Format::Json => return Ok(as_json(&name, &notes, &files)),
        Format::Quiet => return Ok(as_identifiers(&notes, &files, options.null)),
        Format::Table => {}
    }

    // A missing stamp prints as `-` rather than a hole.
    let stamp = |value: Option<String>| value.unwrap_or_else(|| "-".to_string());
    let rows: Vec<Listed> = notes
        .into_iter()
        .map(|file| Listed {
            id: file.id,
            slug: file.slug,
            pinned: file.note.is_pinned(),
            created: stamp(file.note.created),
            updated: stamp(file.note.updated),
            title: file.note.title,
            tags: file.note.tags,
        })
        .collect();

    if rows.is_empty() && files.is_empty() {
        return Ok(String::new());
    }

    let widest = |of: fn(&Listed) -> &String| {
        rows.iter()
            .map(|row| display_width(of(row)))
            .max()
            .unwrap_or(0)
    };
    let id_width = widest(|row| &row.id);
    let slug_width = widest(|row| &row.slug);
    let created_width = widest(|row| &row.created);
    let updated_width = widest(|row| &row.updated);
    let title_width = widest(|row| &row.title);
    let mut out = String::new();
    for Listed {
        id,
        slug,
        created,
        updated,
        title,
        tags,
        pinned,
    } in rows
    {
        // `-l` extends the row rather than rearranging it, so a script cutting
        // id and title off the front reads the same either way. The optional
        // columns (tags, pin) go last so their absence shifts nothing.
        let mut line = column(style::ID, &id, id_width);
        if options.long {
            let _ = write!(
                line,
                "  {}  {}  {}  {}",
                pad(&title, title_width),
                column(style::SLUG, &slug, slug_width),
                column(style::MUTED, &created, created_width),
                column(style::MUTED, &updated, updated_width)
            );
        } else {
            let _ = write!(line, "  {title}");
        }
        if !tags.is_empty() {
            let _ = write!(line, "  {}", style::tags(&tags));
        }
        if pinned {
            let _ = write!(line, "  {}", style::paint(style::PIN, style::PIN_MARK));
        }
        out.push_str(line.trim_end());
        out.push('\n');
    }

    // Under a heading: a file has no id, title or tags to fill a row with.
    if !files.is_empty() {
        if !out.is_empty() {
            out.push('\n');
        }
        let _ = writeln!(out, "{}", style::paint(style::MUTED, "files"));
        for file in files {
            let _ = writeln!(out, "  {file}");
        }
    }
    Ok(out)
}

/// The instant a stamp names, `None` when absent or unreadable. Parsed rather
/// than compared as text because an imported note keeps its old offset:
/// `2019-03-14T16:21:00+08:00` sorts after `2019-03-14T09:00:00Z` as text but
/// is earlier.
fn instant(stamp: Option<&String>) -> Option<jiff::Timestamp> {
    stamp?.parse().ok()
}

/// Public so the browser sorts the same way. Reversing is the caller's, applied
/// afterwards — so under `-r` pinned notes go to the bottom: the pin is part of
/// the order.
pub fn sort_notes(notes: &mut [notebook::NoteFile], sort: Sort) {
    match sort {
        // The walk already sorts by slug.
        Sort::Slug => {}
        Sort::Title => notes.sort_by(|a, b| {
            a.note
                .title
                .cmp(&b.note.title)
                .then_with(|| a.id.cmp(&b.id))
        }),
        Sort::Created | Sort::Updated => notes.sort_by_cached_key(|file| {
            let stamp = match sort {
                Sort::Created => file.note.created.as_ref(),
                _ => file.note.updated.as_ref(),
            };
            // Negated for newest-first, `None` mapped past every real instant.
            (
                instant(stamp).map_or(i128::MAX, |t| -t.as_nanosecond()),
                file.id.clone(),
            )
        }),
    }
    // Last and stable: pins float to the top, keeping their order among
    // themselves. `false` sorts first, hence the negation.
    notes.sort_by_key(|file| !file.note.is_pinned());
}

/// Hand-written: a few string fields do not justify a serialization crate's
/// supply-chain surface; the escaping is tested. Each note carries its filename
/// so a script need not know the naming rule.
fn as_json(notebook: &str, notes: &[notebook::NoteFile], files: &[String]) -> String {
    let mut out = String::from("{\"notebook\":");
    out.push_str(&json_string(notebook));
    out.push_str(",\"notes\":[");
    for (index, file) in notes.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        // Always present, `null` when absent, so no reader tests for the key.
        let stamp = |value: Option<&String>| match value {
            Some(text) => json_string(text),
            None => "null".to_string(),
        };
        let _ = write!(
            out,
            "{{\"id\":{},\"slug\":{},\"file\":{},\"title\":{},\"created\":{},\"updated\":{},\"pinned\":{},\"tags\":[",
            json_string(&file.id),
            json_string(&file.slug),
            json_string(&note::file_name(&file.id, &file.slug)),
            json_string(&file.note.title),
            stamp(file.note.created.as_ref()),
            stamp(file.note.updated.as_ref()),
            // The judgement, not the raw field (`pinned: yes` is not a pin).
            file.note.is_pinned(),
        );
        for (n, tag) in file.note.tags.iter().enumerate() {
            if n > 0 {
                out.push(',');
            }
            out.push_str(&json_string(tag));
        }
        out.push_str("]}");
    }
    out.push_str("],\"files\":[");
    for (index, file) in files.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str(&json_string(file));
    }
    out.push_str("]}\n");
    out
}

/// A note's id, a file's name: each half's identity.
fn as_identifiers(notes: &[notebook::NoteFile], files: &[String], null: bool) -> String {
    let separator = if null { '\0' } else { '\n' };
    let mut out = String::new();
    for file in notes {
        out.push_str(&file.id);
        out.push(separator);
    }
    for file in files {
        out.push_str(file);
        out.push(separator);
    }
    out
}

/// A JSON string literal, quotes included.
fn json_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Prints a note verbatim — frontmatter included, because that is the file.
pub fn show(paths: &Paths, key: &str) -> Result<String> {
    let notebook = Notebook::open_active(paths)?;
    let (id, slug) = notebook.resolve(key)?;
    Ok(dim_frontmatter(&std::fs::read_to_string(
        notebook.note_path(&id, &slug),
    )?))
}

/// Dims only the frontmatter; the body is the user's prose.
fn dim_frontmatter(text: &str) -> String {
    let Some(rest) = text.strip_prefix("---\n") else {
        return text.to_string();
    };
    let Some(end) = rest.find("\n---\n") else {
        return text.to_string();
    };
    format!(
        "{}\n{}",
        style::paint(style::MUTED, &format!("---\n{}\n---", &rest[..end])),
        &rest[end + "\n---\n".len()..]
    )
}

/// Adding a tag a note carries, or removing one it lacks, is a no-op, not an error.
pub fn tag(paths: &Paths, key: &str, changes: &[String], touch: Touch) -> Result<String> {
    let notebook = Notebook::open_active(paths)?;
    tag_in(&notebook, key, changes, touch)
}

/// `tag`, in a notebook the caller already has open.
pub fn tag_in(notebook: &Notebook, key: &str, changes: &[String], touch: Touch) -> Result<String> {
    let edits = parse_tags(changes, Some(key))?;
    let done = apply_tags(notebook, key, &edits, touch)?;
    if done.changed {
        notebook.commit(&done.paths(), &format!("tag: {}", done.slug))?;
    }
    Ok(done.summary)
}

/// Parsed before any note is opened, so a bad change is refused before the
/// first write rather than halfway through a set.
#[derive(Debug, Clone, PartialEq, Eq)]
enum TagEdit {
    Add(String),
    Remove(String),
}

fn parse_tags(changes: &[String], key: Option<&str>) -> Result<Vec<TagEdit>> {
    changes
        .iter()
        .map(|change| {
            // The list takes hyphen values, so a flag after it arrives here:
            // `--no-touch` would silently remove a tag `-no-touch` nobody has.
            if change.starts_with("--") {
                let goes = match key {
                    Some(key) => format!(" — `noda tag {key} {change} +tag`"),
                    None => String::new(),
                };
                return Err(Error::msg(format!(
                    "`{change}` is being read as a tag: the tags take everything after them, so a flag has to come before them{goes}"
                )));
            }
            if let Some(name) = change.strip_prefix('+') {
                let name = name.trim();
                if name.is_empty() {
                    return Err(Error::msg("`+` needs a tag name after it"));
                }
                note::validate_tag(name)?;
                Ok(TagEdit::Add(name.to_string()))
            } else if let Some(name) = change.strip_prefix('-') {
                let name = name.trim();
                if name.is_empty() {
                    return Err(Error::msg("`-` needs a tag name after it"));
                }
                Ok(TagEdit::Remove(name.to_string()))
            } else {
                Err(Error::msg(format!(
                    "tags must be given as `+{change}` to add or `-{change}` to remove"
                )))
            }
        })
        .collect()
}

/// What a change did to one note, uncommitted: one command is one commit, but a
/// browser queue is one commit for the lot, and both share this code.
struct Applied {
    id: String,
    slug: String,
    /// Relative to the notebook.
    files: Vec<String>,
    summary: String,
    /// False when the note already said what it was asked to say.
    changed: bool,
}

impl Applied {
    fn paths(&self) -> Vec<&Path> {
        self.files.iter().map(Path::new).collect()
    }
}

fn apply_tags(notebook: &Notebook, key: &str, edits: &[TagEdit], touch: Touch) -> Result<Applied> {
    let located = locate(notebook, key)?;
    let mut note = located.note;
    let before = note.tags.clone();

    for edit in edits {
        match edit {
            TagEdit::Add(name) => {
                if !note.tags.iter().any(|t| t == name) {
                    note.tags.push(name.clone());
                }
            }
            TagEdit::Remove(name) => note.tags.retain(|t| t != name),
        }
    }

    let file = note::file_name(&located.id, &located.slug);
    if note.tags == before {
        return Ok(Applied {
            summary: format!(
                "{}  (no change)",
                summary(&located.id, &located.slug, &note.tags)
            ),
            id: located.id,
            slug: located.slug,
            files: vec![file],
            changed: false,
        });
    }

    if touch == Touch::Stamp {
        note.updated = Some(note::now());
    }
    std::fs::write(&located.path, note.render())?;
    Ok(Applied {
        summary: summary(&located.id, &located.slug, &note.tags),
        id: located.id,
        slug: located.slug,
        files: vec![file],
        changed: true,
    })
}

/// Floats a note to the top of every listing, or lets it back down.
pub fn pin(paths: &Paths, key: &str, pinned: bool, touch: Touch) -> Result<String> {
    let notebook = Notebook::open_active(paths)?;
    pin_in(&notebook, key, pinned, touch)
}

/// `pin`, in a notebook the caller already has open.
pub fn pin_in(notebook: &Notebook, key: &str, pinned: bool, touch: Touch) -> Result<String> {
    let done = apply_pin(notebook, key, pinned, touch)?;
    if done.changed {
        let verb = if pinned { "pin" } else { "unpin" };
        notebook.commit(&done.paths(), &format!("{verb}: {}", done.slug))?;
    }
    Ok(done.summary)
}

fn apply_pin(notebook: &Notebook, key: &str, pinned: bool, touch: Touch) -> Result<Applied> {
    let located = locate(notebook, key)?;
    let mut note = located.note;
    let before = note.pinned.clone();

    // Unpinning drops the line rather than writing `false`, so pin + unpin
    // round-trips to the original file.
    note.pinned = pinned.then(|| note::PINNED.to_string());

    let file = note::file_name(&located.id, &located.slug);
    let mark = |pinned: bool| if pinned { "pinned" } else { "unpinned" };
    if note.pinned == before {
        return Ok(Applied {
            summary: format!(
                "{}  {}  (no change)",
                summary(&located.id, &located.slug, &note.tags),
                mark(pinned)
            ),
            id: located.id,
            slug: located.slug,
            files: vec![file],
            changed: false,
        });
    }

    if touch == Touch::Stamp {
        note.updated = Some(note::now());
    }
    std::fs::write(&located.path, note.render())?;
    Ok(Applied {
        summary: format!(
            "{}  {}",
            summary(&located.id, &located.slug, &note.tags),
            mark(pinned)
        ),
        id: located.id,
        slug: located.slug,
        files: vec![file],
        changed: true,
    })
}

/// Opens a note in `$EDITOR` and commits whatever was saved.
pub fn edit(paths: &Paths, key: &str, touch: Touch) -> Result<String> {
    edit_with(paths, key, &configured_editor(paths), touch)
}

/// So tests can drive the command without mutating process-wide env.
pub fn edit_with(paths: &Paths, key: &str, editor: &str, touch: Touch) -> Result<String> {
    let notebook = Notebook::open_active(paths)?;
    let located = locate(&notebook, key)?;
    let before = std::fs::read_to_string(&located.path)?;

    run_editor(editor, &located.path)?;
    settle(&notebook, &located, &before, touch)
}

/// `edit` with the new body in hand, for the browser. Only the body: the
/// frontmatter is left as found, and title and tags have their own commands.
pub fn rewrite_in(notebook: &Notebook, key: &str, body: &str, touch: Touch) -> Result<String> {
    let located = locate(notebook, key)?;
    let before = std::fs::read_to_string(&located.path)?;
    let after = note::set_body(&before, body)
        .ok_or_else(|| Error::msg(format!("{}: not a note", located.path.display())))?;
    std::fs::write(&located.path, &after)?;
    settle(notebook, &located, &before, touch)
}

/// Read it back, refuse it if it is no longer a note, stamp it, commit it —
/// shared by `edit` and `rewrite_in`.
fn settle(notebook: &Notebook, located: &Located, before: &str, touch: Touch) -> Result<String> {
    let after = std::fs::read_to_string(&located.path)?;
    if after == *before {
        return Ok(format!("{}  (unchanged)", located.slug));
    }

    // A rejected edit stays on disk, never silently discarded. The id needs no
    // guard: it lives in the filename.
    let edited = Note::parse(&after).map_err(|e| {
        Error::msg(format!(
            "{}: {e}\nthe file was left as you saved it and was not committed",
            located.path.display()
        ))
    })?;

    // Only `updated` is set in place; the rest is committed as saved. Under
    // `--no-touch` nothing is written back, even an `updated` the user edited.
    if touch == Touch::Stamp {
        let stamped = note::set_field(&after, "updated", &note::now())
            .expect("the note parsed, so it has a frontmatter block");
        if stamped != after {
            std::fs::write(&located.path, &stamped)?;
        }
    }

    notebook.commit(
        &[Path::new(&note::file_name(&located.id, &located.slug))],
        &format!("edit: {}", located.slug),
    )?;
    Ok(summary(&located.id, &located.slug, &edited.tags))
}

/// Retitles a note. The slug follows the title; the id never moves, so links to
/// it go stale rather than broken (`backlinks` still finds them by id). A retitle
/// reports them; `update_links` rewrites them — opt-in, because it edits notes
/// the command was not pointed at.
pub fn mv(
    paths: &Paths,
    key: &str,
    new_title: &str,
    update_links: bool,
    touch: Touch,
) -> Result<String> {
    let notebook = Notebook::open_active(paths)?;
    mv_in(&notebook, key, new_title, update_links, touch)
}

/// `mv`, in a notebook the caller already has open.
pub fn mv_in(
    notebook: &Notebook,
    key: &str,
    new_title: &str,
    update_links: bool,
    touch: Touch,
) -> Result<String> {
    let located = locate(notebook, key)?;
    let mut note = located.note;

    let title = new_title.trim();
    if title.is_empty() {
        return Err(Error::msg("a note needs a title"));
    }
    note::validate_title(title)?;

    let slug = note::slugify(title);
    note.title = title.to_string();
    if touch == Touch::Stamp {
        note.updated = Some(note::now());
    }

    let was = note::file_name(&located.id, &located.slug);
    let file = note::file_name(&located.id, &slug);
    std::fs::write(notebook.path.join(&file), note.render())?;
    let mut changed = vec![file.clone()];
    let mut retarget = None;
    if slug != located.slug {
        std::fs::remove_file(&located.path)?;
        changed.push(was.clone());
    }

    // Reads every note, so only when the slug moved or the flag asks — the
    // latter repairs links left stale by an earlier rename.
    if slug != located.slug || update_links {
        // After the rename, so a self-link is read under its new name.
        let (notes, _) = notebook.inventory()?;
        let id = note::normalize_id(&located.id);
        let found = retarget_links(
            notebook,
            &notes,
            |target| notebook::linked_note_id(target).as_deref() == Some(id.as_str()),
            &file,
            update_links,
        )?;
        changed.extend(found.rewritten.iter().cloned());
        retarget = Some(found);
    }

    // A self-linking note is here twice: renamed, and rewritten.
    changed.sort();
    changed.dedup();
    let files: Vec<&Path> = changed.iter().map(Path::new).collect();
    notebook.commit(&files, &format!("mv: {} -> {slug}", located.slug))?;

    // By id, because a link can be two renames behind.
    let subject = format!("{} by an older name", located.id);
    let mut out = summary(&located.id, &slug, &note.tags);
    if let Some(lines) = retarget.map(|found| found.describe(&subject, update_links))
        && !lines.is_empty()
    {
        out.push('\n');
        out.push_str(lines.trim_end());
    }
    Ok(out)
}

/// A commit, so `git revert` brings the note back with its id intact.
pub fn rm(paths: &Paths, key: &str) -> Result<String> {
    let notebook = Notebook::open_active(paths)?;
    rm_in(&notebook, key)
}

/// `rm`, in a notebook the caller already has open.
pub fn rm_in(notebook: &Notebook, key: &str) -> Result<String> {
    let done = apply_remove(notebook, key)?;
    notebook.commit(&done.paths(), &format!("rm: {}", done.slug))?;
    Ok(done.summary)
}

fn apply_remove(notebook: &Notebook, key: &str) -> Result<Applied> {
    // `find`, not `locate`: a note that no longer parses must still be removable.
    let found = find(notebook, key)?;

    std::fs::remove_file(&found.path)?;

    let tags = found
        .note
        .as_ref()
        .map(|note| note.tags.clone())
        .unwrap_or_default();
    Ok(Applied {
        summary: format!("removed  {}", summary(&found.id, &found.slug, &tags)),
        files: vec![note::file_name(&found.id, &found.slug)],
        id: found.id,
        slug: found.slug,
        changed: true,
    })
}

/// The changes that make sense over a set of notes (a retitle needs one title
/// per note).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    Tag { changes: Vec<String>, touch: Touch },
    Remove,
}

/// One change and the notes it is aimed at, by id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub keys: Vec<String>,
    pub change: Change,
}

impl Step {
    /// The line for both the commit message and the browser's queue, so what
    /// you read before sending is what the history says.
    pub fn describe(&self) -> String {
        let notes = count(self.keys.len(), "note");
        match &self.change {
            Change::Tag { changes, touch } => {
                let kept = if *touch == Touch::Keep {
                    ", keeping updated"
                } else {
                    ""
                };
                format!("tag: {} ({notes}{kept})", changes.join(" "))
            }
            Change::Remove => format!("rm: {notes}"),
        }
    }
}

/// `1 note` / `3 notes`.
fn count(n: usize, thing: &str) -> String {
    if n == 1 {
        format!("1 {thing}")
    } else {
        format!("{n} {thing}s")
    }
}

/// Whether a change could be carried out at all, without opening anything — so
/// the browser refuses a bad tag when it is queued, not when the queue is sent.
pub fn check(change: &Change) -> Result<()> {
    match change {
        Change::Tag { changes, .. } => parse_tags(changes, None).map(|_| ()),
        Change::Remove => Ok(()),
    }
}

/// Several changes across several notes, in one commit because a queue is one
/// intention. Writes through the same code as `tag` and `rm`.
///
/// What can be refused is refused before anything is written; what cannot be
/// known in advance (a note deleted mid-queue) is reported without stopping the
/// rest.
pub fn bulk(paths: &Paths, steps: &[Step]) -> Result<String> {
    if steps.is_empty() {
        return Err(Error::msg("there is nothing to send"));
    }
    let mut plan = Vec::new();
    for step in steps {
        if step.keys.is_empty() {
            return Err(Error::msg("a change has to be aimed at a note"));
        }
        plan.push(match &step.change {
            Change::Tag { changes, touch } => Planned::Tag(parse_tags(changes, None)?, *touch),
            Change::Remove => Planned::Remove,
        });
    }

    let notebook = Notebook::open_active(paths)?;
    let mut files: Vec<String> = Vec::new();
    let mut touched: BTreeSet<String> = BTreeSet::new();
    let mut problems: Vec<String> = Vec::new();

    for (step, planned) in steps.iter().zip(&plan) {
        for key in &step.keys {
            let done = match planned {
                Planned::Tag(edits, touch) => apply_tags(&notebook, key, edits, *touch),
                Planned::Remove => apply_remove(&notebook, key),
            };
            match done {
                Ok(done) if done.changed => {
                    files.extend(done.files);
                    touched.insert(done.id);
                }
                Ok(_) => {}
                Err(e) => problems.push(format!("{key}: {e}")),
            }
        }
    }

    files.sort();
    files.dedup();

    let mut report = if touched.is_empty() {
        "nothing to change".to_string()
    } else {
        let paths: Vec<&Path> = files.iter().map(Path::new).collect();
        notebook.commit(&paths, &commit_message(steps))?;
        format!(
            "{} over {}, in one commit",
            count(steps.len(), "change"),
            count(touched.len(), "note")
        )
    };
    for problem in &problems {
        report.push('\n');
        report.push_str(problem);
    }
    Ok(report)
}

/// Tags parsed once rather than once per note.
enum Planned {
    Tag(Vec<TagEdit>, Touch),
    Remove,
}

/// One step is its own subject; several get a count and a body listing them.
fn commit_message(steps: &[Step]) -> String {
    if let [only] = steps {
        return only.describe();
    }
    let notes: BTreeSet<&str> = steps
        .iter()
        .flat_map(|step| step.keys.iter().map(String::as_str))
        .collect();
    let mut message = format!(
        "bulk: {} over {}\n",
        count(steps.len(), "change"),
        count(notes.len(), "note")
    );
    for step in steps {
        message.push('\n');
        message.push_str(&step.describe());
    }
    message
}

/// Writes and commits the notebook's `README.md`, which a git host shows above
/// the file list. Fixed prose rather than an index of notes, which would go
/// stale from the next `noda add`.
pub fn readme(paths: &Paths, force: bool) -> Result<String> {
    let notebook = Notebook::open_active(paths)?;
    let path = notebook.path.join(notebook::README_FILE);

    let existed = path.exists();
    if existed && !force {
        return Err(Error::msg(format!(
            "{} already exists — pass `--force` to overwrite",
            notebook::README_FILE
        )));
    }

    std::fs::write(&path, readme_template(&notebook.name))?;
    let verb = if existed { "update" } else { "add" };
    notebook.commit(
        &[Path::new(notebook::README_FILE)],
        &format!("file: {verb} {}", notebook::README_FILE),
    )?;

    Ok(format!(
        "{} {} in `{}`",
        if existed { "rewrote" } else { "wrote" },
        notebook::README_FILE,
        notebook.name
    ))
}

fn readme_template(name: &str) -> String {
    format!(
        r"# {name}

A [noda](https://github.com/henry40408/noda) notebook: plain Markdown notes kept in git.

## Nothing here needs noda to be read

Every note is a Markdown file at the root of this repository. Open one in this web view,
in an editor, in anything that renders Markdown. noda makes these notes quicker to write
and to search; it is not what makes them readable.

## Filenames

Notes are named `<id>-<slug>.md` — for example `k3f9m2p1-meeting-notes.md`.

- The **id** (`k3f9m2p1`) is the note's permanent identity, and never changes.
- The **slug** (`meeting-notes`) comes from the title, and changes when the title does.

A link from one note to another names the whole filename, so links work in this web view,
and a link survives a retitle because the id in it still names the same note.

## Frontmatter

Each note opens with a block like this:

```yaml
---
title: Reading notes on TAOCP
tags: [books, algorithms]
created: 2019-03-14T08:21:00Z
updated: 2024-11-02T16:40:12Z
---
```

`created` is set once and never moves; `updated` follows every change. `tags` is optional,
and `pinned: true` pins a note to the top of the listing. noda reads those five fields and leaves everything else in the block alone, so any other
field is yours to use.

## Working on it with noda

```console
$ noda clone <this repository's URL> {name}
$ noda use {name}
$ noda ls
```

<!--
  Everything below is yours to write.

  One thing worth leaving out: a list of the notes. Nothing updates it, so it is wrong
  from the next `noda add` onward — `noda ls` is that list, always current. What belongs
  here is what stays true: what this notebook is for, and links to the few notes a reader
  should start from.
-->
"
    )
}

/// Copies files into the active notebook and commits them. Takes no note: which
/// note uses a file is said in that note's prose, not twice.
pub fn file_add(paths: &Paths, sources: &[PathBuf], rename: Option<&str>) -> Result<String> {
    if rename.is_some() && sources.len() > 1 {
        return Err(Error::msg(
            "`--as` renames one file, so it cannot be given with several",
        ));
    }
    let notebook = Notebook::open_active(paths)?;

    // All checked before any is copied, or a half-done copy gets committed.
    let mut planned = Vec::new();
    for source in sources {
        if !source.is_file() {
            return Err(Error::msg(format!(
                "not a file: {} — a notebook holds files, not directories",
                source.display()
            )));
        }
        let name = match rename {
            Some(name) => name.to_string(),
            None => source
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| {
                    Error::msg(format!("cannot read a filename from {}", source.display()))
                })?
                .to_string(),
        };
        validate_file_name(&name)?;
        refuse_a_notes_name(&name)?;
        if notebook.path.join(&name).exists() {
            return Err(Error::msg(format!(
                "the notebook already holds {name} — copy it in under another name with `--as`"
            )));
        }
        planned.push((source.clone(), name));
    }

    for (source, name) in &planned {
        std::fs::copy(source, notebook.path.join(name))?;
    }

    let names: Vec<&str> = planned.iter().map(|(_, name)| name.as_str()).collect();
    let message = match names.as_slice() {
        [one] => format!("file: add {one}"),
        many => format!("file: add {} files", many.len()),
    };
    let files: Vec<&Path> = names.iter().map(Path::new).collect();
    notebook.commit(&files, &message)?;

    let mut out = String::new();
    for name in names {
        let _ = writeln!(out, "added  {name}");
    }
    Ok(out)
}

/// A commit, so `git revert` brings it back. A note is refused: `noda rm` is
/// for those.
pub fn file_rm(paths: &Paths, name: &str) -> Result<String> {
    let notebook = Notebook::open_active(paths)?;
    validate_file_name(name)?;

    let (notes, files) = notebook.inventory()?;
    if !files.iter().any(|file| file == name) {
        return Err(missing_file(&notes, name, "remove it with `noda rm`"));
    }

    std::fs::remove_file(notebook.path.join(name))?;
    notebook.commit(&[Path::new(name)], &format!("file: rm {name}"))?;
    Ok(format!("removed  {name}"))
}

/// Renames one of the notebook's files and always reports the links it broke —
/// a full walk, paid because a rename is rare and the damage otherwise silent.
/// `update_links` rewrites them instead (see `retarget_links`).
pub fn file_mv(paths: &Paths, old: &str, new: &str, update_links: bool) -> Result<String> {
    let notebook = Notebook::open_active(paths)?;
    validate_file_name(old)?;
    validate_file_name(new)?;
    refuse_a_notes_name(new)?;
    if old == new {
        return Err(Error::msg(format!("{old} already is its name")));
    }

    let (notes, files) = notebook.inventory()?;
    if !files.iter().any(|file| file == old) {
        return Err(missing_file(&notes, old, "retitle it with `noda mv`"));
    }
    if notebook.path.join(new).exists() {
        return Err(Error::msg(format!(
            "the notebook already holds {new} — pick a name it does not"
        )));
    }

    std::fs::rename(notebook.path.join(old), notebook.path.join(new))?;
    let mut changed = vec![old.to_string(), new.to_string()];

    let retarget = retarget_links(&notebook, &notes, |target| target == old, new, update_links)?;
    changed.extend(retarget.rewritten.iter().cloned());

    let files: Vec<&Path> = changed
        .iter()
        .map(|name| Path::new(name.as_str()))
        .collect();
    notebook.commit(&files, &format!("file: mv {old} -> {new}"))?;

    let mut out = format!("renamed  {old} -> {new}\n");
    out.push_str(&retarget.describe(old, update_links));
    Ok(out)
}

/// What a rename did to the links that named the thing renamed.
struct Retarget {
    /// Filenames of the notes whose bodies were rewritten.
    rewritten: Vec<String>,
    /// Still naming the old name: not asked for, or out of reach.
    stranded: Vec<String>,
}

impl Retarget {
    /// Empty when no note named the old name.
    fn describe(&self, subject: &str, update_links: bool) -> String {
        let mut out = String::new();
        if !self.rewritten.is_empty() {
            let count = self.rewritten.len();
            let noun = if count == 1 { "note" } else { "notes" };
            let _ = writeln!(out, "updated  {count} {noun}");
        }
        if !self.stranded.is_empty() {
            let mut stranded = self.stranded.clone();
            stranded.sort();
            stranded.dedup();
            let count = stranded.len();
            let (noun, verb) = if count == 1 {
                ("note", "links")
            } else {
                ("notes", "link")
            };
            let still = if update_links { "still " } else { "" };
            let _ = writeln!(out, "{count} {noun} {still}{verb} to {subject}");
            for name in &stranded {
                let _ = writeln!(out, "  {name}");
            }
        }
        out
    }
}

/// Rewrites the links `names` accepts when `update_links` says so, and reports
/// them either way. `file mv` matches the old name; `mv` matches the note's id,
/// catching a link written two renames ago.
///
/// A rewritten note is re-checked rather than assumed fixed: `link::rewrite`
/// cannot locate a destination written with backslash escapes.
fn retarget_links(
    notebook: &Notebook,
    notes: &[notebook::NoteFile],
    names: impl Fn(&str) -> bool,
    new: &str,
    update_links: bool,
) -> Result<Retarget> {
    // Already reading as `new` is correct, so neither rewritten nor reported.
    let outdated = |body: &str| -> Vec<String> {
        link::targets(body)
            .into_iter()
            .filter(|target| target != new && names(target))
            .collect()
    };
    let mut rewritten = Vec::new();
    let mut stranded = Vec::new();

    for file in notes {
        let name = note::file_name(&file.id, &file.slug);
        let targets = outdated(&file.note.body);
        if targets.is_empty() {
            continue;
        }
        if !update_links {
            stranded.push(name);
            continue;
        }
        let path = notebook.path.join(&name);
        let text = std::fs::read_to_string(&path)?;
        // The frontmatter is carried byte for byte, `updated` included.
        let Some((_, body)) = note::split_frontmatter(&text) else {
            stranded.push(name);
            continue;
        };
        // A note can name another by two names it has had.
        let mut fixed = body.to_string();
        for target in &targets {
            if let Some(next) = link::rewrite(&fixed, target, new) {
                fixed = next;
            }
        }
        if fixed == body {
            stranded.push(name);
            continue;
        }
        let prefix = &text[..text.len() - body.len()];
        std::fs::write(&path, format!("{prefix}{fixed}"))?;
        if !outdated(&fixed).is_empty() {
            stranded.push(name.clone());
        }
        rewritten.push(name);
    }

    Ok(Retarget {
        rewritten,
        stranded,
    })
}

/// For the tools noda does not wrap: `pandoc "$(noda path meeting-notes)"`.
pub fn path(paths: &Paths, key: Option<&str>) -> Result<String> {
    let notebook = Notebook::open_active(paths)?;
    let Some(key) = key else {
        return Ok(format!("{}\n", notebook.path.display()));
    };

    // One key can name both.
    let as_note = notebook.resolve(key);
    let as_file = notebook.path.join(key);
    let is_file = !key.contains('/') && !key.contains('\\') && as_file.is_file();

    match (as_note, is_file) {
        (Ok((id, slug)), false) => Ok(format!("{}\n", notebook.note_path(&id, &slug).display())),
        (Err(_), true) => Ok(format!("{}\n", as_file.display())),
        (Ok((id, slug)), true) => Err(Error::msg(format!(
            "`{key}` names both a note and a file — say which:\n  {}\n  {}",
            note::file_name(&id, &slug),
            key
        ))),
        // Only "no such note" needs widening; an ambiguous key's error stands.
        (Err(Error::Msg(said)), false) if said.starts_with(notebook::NOT_FOUND) => Err(Error::msg(
            format!("nothing called `{key}` — the notebook holds no note and no file by that name"),
        )),
        (Err(other), false) => Err(other),
    }
}

/// When the name is a note's, say so and name the right command.
fn missing_file(notes: &[notebook::NoteFile], name: &str, instead: &str) -> Error {
    let is_note = notes
        .iter()
        .any(|file| note::file_name(&file.id, &file.slug) == name);
    if is_note {
        Error::msg(format!("{name} is a note — {instead}"))
    } else {
        Error::msg(format!("the notebook holds no file called {name}"))
    }
}

/// Such a name reads as a note without frontmatter, which `doctor` reports broken.
fn refuse_a_notes_name(name: &str) -> Result<()> {
    if note::names_a_note(name) {
        return Err(Error::msg(format!(
            "{name} claims a note's id — a file noda would then report as a broken note"
        )));
    }
    Ok(())
}

/// One flat directory, so a name is never a path. A leading `.` is refused
/// because the walk skips dotfiles: nothing would ever list it.
fn validate_file_name(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(Error::msg("a file needs a name"));
    }
    if name.contains('/') || name.contains('\\') || name == "." || name == ".." {
        return Err(Error::msg(format!(
            "a file's name cannot be a path: {name}"
        )));
    }
    if name.starts_with('.') {
        return Err(Error::msg(format!(
            "noda does not list dotfiles, so it will not add one: {name}"
        )));
    }
    // Refused rather than cut to fit like a slug: links point at this name.
    if name.len() > note::MAX_FILE_NAME_LEN {
        return Err(Error::msg(format!(
            "a filename has to fit in {} bytes, and this one is {}: {name}",
            note::MAX_FILE_NAME_LEN,
            name.len()
        )));
    }
    Ok(())
}

/// Creates a notebook — a new git repo — optionally pointed at a remote.
pub fn notebook_add(paths: &Paths, name: &str, remote: Option<&str>) -> Result<String> {
    std::fs::create_dir_all(paths.notebooks_dir())?;
    let notebook = Notebook::create(paths, name)?;
    if let Some(url) = remote {
        notebook.set_remote(url)?;
    }
    Ok(format!(
        "created notebook `{name}` at {}",
        notebook.path.display()
    ))
}

struct Row {
    name: String,
    /// `None` omits the column; a remote never synced pads it.
    remote: Option<String>,
    drift: Option<(usize, usize)>,
}

/// Notebooks, the active one marked `*`, each with its drift from its remote.
/// [`Notebook::drift`] rather than [`Notebook::status`] keeps it cheap per row
/// (two refs compared, no working-tree walk), and nothing touches the network.
pub fn notebook_ls(paths: &Paths) -> Result<String> {
    let names = Notebook::list(paths)?;
    if names.is_empty() {
        return Ok(String::new());
    }
    let active = notebook::active_name(paths).ok();

    let rows: Vec<Row> = names
        .into_iter()
        .map(|name| {
            let opened = Notebook::open(paths, &name).ok();
            let remote = opened.as_ref().and_then(Notebook::remote_url);
            let drift = opened.as_ref().and_then(|notebook| {
                let branch = notebook.branch().ok()?;
                notebook.drift(&branch).ok().flatten()
            });
            Row {
                name,
                remote,
                drift,
            }
        })
        .collect();

    let name_width = rows
        .iter()
        .map(|row| display_width(&row.name))
        .max()
        .unwrap_or(0);
    let remote_width = rows
        .iter()
        .map(|row| row.remote.as_deref().map_or(0, display_width))
        .max()
        .unwrap_or(0);

    let mut out = String::new();
    for Row {
        name,
        remote,
        drift,
    } in rows
    {
        let marker = if active.as_deref() == Some(&name) {
            '*'
        } else {
            ' '
        };
        // Muted unless there is something to push or pull.
        let words = standing(remote.as_deref(), drift);
        let painted = match drift {
            Some((ahead, behind)) if remote.is_some() && (ahead > 0 || behind > 0) => words,
            _ => style::paint(style::MUTED, &words),
        };
        let line = match remote.as_deref() {
            Some(remote) => format!(
                "{marker} {}  {}  {painted}",
                pad(&name, name_width),
                pad(remote, remote_width)
            ),
            None => format!("{marker} {}  {painted}", pad(&name, name_width)),
        };
        out.push_str(line.trim_end());
        out.push('\n');
    }
    Ok(out)
}

/// Not a commit and cannot be undone, so the active notebook is refused and any
/// other is confirmed first.
pub fn notebook_rm(paths: &Paths, name: &str, force: bool) -> Result<String> {
    notebook_rm_confirmed(paths, name, force, ask_at_the_terminal)
}

/// So tests can decide without a terminal to type at.
pub fn notebook_rm_confirmed(
    paths: &Paths,
    name: &str,
    force: bool,
    confirm: impl FnOnce(&str) -> Result<bool>,
) -> Result<String> {
    notebook::validate_name(name)?;
    if !Notebook::exists(paths, name) {
        return Err(Error::msg(format!("notebook not found: {name}")));
    }
    if notebook::active_name(paths).ok().as_deref() == Some(name) {
        return Err(Error::msg(format!(
            "`{name}` is the active notebook — switch with `noda use <name>` first"
        )));
    }

    if !force {
        let notes = Notebook::open(paths, name)
            .and_then(|notebook| notebook.notes())
            .map_or(0, |notes| notes.len());
        let plural = if notes == 1 { "" } else { "s" };
        let question = format!(
            "delete notebook `{name}` — {notes} note{plural} and their whole history? \
             this is not a commit and cannot be undone [y/N] "
        );
        if !confirm(&question)? {
            return Ok(format!("kept notebook `{name}`"));
        }
    }

    let dir = paths.notebook_dir(name);
    std::fs::remove_dir_all(&dir)?;
    Ok(format!(
        "removed notebook `{name}` and its history at {}",
        dir.display()
    ))
}

/// Anything but `y`/`yes` is no. With no terminal it refuses; a script passes
/// `--force`.
fn ask_at_the_terminal(question: &str) -> Result<bool> {
    use std::io::IsTerminal;

    if !std::io::stdin().is_terminal() {
        return Err(Error::msg(
            "there is no terminal to confirm at — pass `--force` if you mean it",
        ));
    }
    // stderr, so stdout carries only the outcome.
    let mut stderr = std::io::stderr();
    stderr.write_all(question.as_bytes())?;
    stderr.flush()?;

    let mut answer = String::new();
    std::io::stdin().read_line(&mut answer)?;
    Ok(matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

/// Renames a notebook, carrying the active pointer with it.
pub fn notebook_rename(paths: &Paths, old: &str, new: &str) -> Result<String> {
    notebook::validate_name(old)?;
    notebook::validate_name(new)?;
    if !Notebook::exists(paths, old) {
        return Err(Error::msg(format!("notebook not found: {old}")));
    }
    if paths.notebook_dir(new).exists() {
        return Err(Error::msg(format!("notebook already exists: {new}")));
    }
    std::fs::rename(paths.notebook_dir(old), paths.notebook_dir(new))?;
    if notebook::active_name(paths).ok().as_deref() == Some(old) {
        paths.set_active_notebook(new)?;
    }
    Ok(format!("renamed notebook `{old}` to `{new}`"))
}

/// Characters of a matching line to show.
const EXCERPT_WIDTH: usize = 72;
/// At most this many before the match, so the match stays visible.
const EXCERPT_LEAD: usize = 28;

/// Case-insensitive substring, not word: Chinese or Japanese notes have no
/// spaces to tokenise on. Several terms mean all of them.
pub fn search(paths: &Paths, tokens: &[String]) -> Result<String> {
    let query = Query::parse(tokens)?;
    let terms = query.excerpt_terms();

    let notebook = Notebook::open_active(paths)?;
    let mut rows = Vec::new();
    for file in notebook.notes()? {
        // The parsed note, so frontmatter keys are not searchable text.
        if !query.matches(&file.id, &file.note) {
            continue;
        }
        let note = file.note;
        rows.push((
            file.id,
            file.slug,
            note.title,
            note.tags.join(", "),
            excerpt(&note.body, &terms),
        ));
    }

    if rows.is_empty() {
        return Ok(String::new());
    }

    // `ls`'s row shape.
    let id_width = rows.iter().map(|r| display_width(&r.0)).max().unwrap_or(0);
    let mut out = String::new();
    for (id, _slug, title, tags, excerpt) in rows {
        let mut line = format!("{}  {title}", pad(&id, id_width));
        if !tags.is_empty() {
            line.push_str("  [");
            line.push_str(&tags);
            line.push(']');
        }
        out.push_str(line.trim_end());
        out.push('\n');
        // Only a body hit needs quoting; title and tags are on the row.
        if let Some(excerpt) = excerpt {
            out.push_str(&" ".repeat(id_width + 2));
            out.push_str(&excerpt);
            out.push('\n');
        }
    }
    Ok(out)
}

/// The first body line holding a term, cut to fit, with the match highlighted.
fn excerpt(body: &str, terms: &[String]) -> Option<String> {
    let (line, start, end) = body.lines().find_map(|line| {
        terms
            .iter()
            .find_map(|term| find_ignoring_case(line, term).map(|(start, end)| (line, start, end)))
    })?;

    let before = last_chars(&line[..start], EXCERPT_LEAD);
    let room =
        EXCERPT_WIDTH.saturating_sub(before.chars().count() + line[start..end].chars().count());
    Some(format!(
        "{before}{}{}",
        style::paint(style::MATCH, &line[start..end]),
        first_chars(&line[end..], room)
    ))
}

/// Case-insensitive `find`, as byte offsets into `haystack`. Lowercasing can
/// change a character's byte length, so offsets are mapped back and always land
/// on char boundaries. Shared with the TUI, which highlights the same match.
pub(crate) fn find_ignoring_case(haystack: &str, needle: &str) -> Option<(usize, usize)> {
    let mut lowered = String::with_capacity(haystack.len());
    let mut origin = Vec::with_capacity(haystack.len());
    for (index, ch) in haystack.char_indices() {
        for lower in ch.to_lowercase() {
            let mut buffer = [0u8; 4];
            let encoded = lower.encode_utf8(&mut buffer);
            origin.resize(origin.len() + encoded.len(), index);
            lowered.push_str(encoded);
        }
    }
    origin.push(haystack.len());

    let start = lowered.find(needle)?;
    Some((origin[start], origin[start + needle.len()]))
}

/// The last `max` characters, with a leading `…` when something was cut.
fn last_chars(text: &str, max: usize) -> String {
    let count = text.chars().count();
    if count <= max {
        return text.to_string();
    }
    let tail: String = text.chars().skip(count - max).collect();
    format!("…{}", tail.trim_start())
}

/// The first `max` characters, with a trailing `…` when something was cut.
fn first_chars(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let head: String = text.chars().take(max).collect();
    format!("{}…", head.trim_end())
}

/// Offline: drift is measured against the last fetch.
pub fn status(paths: &Paths) -> Result<String> {
    let notebook = Notebook::open_active(paths)?;
    let status = notebook.status()?;

    let changes = match status.uncommitted {
        0 => "clean".to_string(),
        1 => "1 file uncommitted".to_string(),
        n => format!("{n} files uncommitted"),
    };
    let mut rows = vec![
        (
            "notebook",
            format!(
                "{}  {}",
                notebook.name,
                style::paint(style::MUTED, &format!("({})", status.branch))
            ),
        ),
        ("notes", status.notes.to_string()),
    ];

    // Rows that would always say "0" are left out, so the others get read.
    if status.files > 0 {
        rows.push(("files", status.files.to_string()));
    }
    rows.push(("changes", changes));

    if !status.problems.is_empty() {
        rows.push(("problems", describe_problems(&status.problems)));
    }

    match status.remote {
        None => rows.push((
            "remote",
            style::paint(style::MUTED, "none — set one with `noda remote set <url>`"),
        )),
        Some(url) => {
            rows.push(("remote", url));
            rows.push(("sync", describe_drift(status.drift)));
        }
    }

    let width = rows
        .iter()
        .map(|(key, _)| display_width(key))
        .max()
        .unwrap_or(0);
    let mut out = String::new();
    for (key, value) in rows {
        // Continuation lines align under the value column.
        let mut lines = value.lines();
        let _ = writeln!(out, "{}  {}", pad(key, width), lines.next().unwrap_or(""));
        for line in lines {
            let _ = writeln!(out, "{}  {line}", pad("", width));
        }
    }
    Ok(out)
}

/// One kind gets one line; several get a total first, then the breakdown.
fn describe_problems(problems: &[(Problem, Vec<String>)]) -> String {
    let mut out = String::new();
    if problems.len() > 1 {
        let total: usize = problems.iter().map(|(_, subjects)| subjects.len()).sum();
        let noun = if total == 1 { "problem" } else { "problems" };
        let _ = writeln!(out, "{total} {noun}");
    }
    for (kind, subjects) in problems {
        let _ = writeln!(
            out,
            "{}{}",
            kind.describe(subjects.len()),
            style::paint(style::MUTED, &format!("  ({})", elide(subjects)))
        );
    }
    // Name the remedy.
    let _ = write!(
        out,
        "{}",
        style::paint(style::MUTED, "run `noda doctor` to look at these")
    );
    out.trim_end().to_string()
}

/// The first few subjects, so a wholesale problem stays on one line.
fn elide(subjects: &[String]) -> String {
    const SHOWN: usize = 3;

    let mut shown: Vec<&str> = subjects.iter().take(SHOWN).map(String::as_str).collect();
    if subjects.len() > SHOWN {
        shown.push("…");
    }
    shown.join("; ")
}

/// Diagnoses what noda cannot act on, and adopts notes that only lack an id —
/// the one repair that loses nothing. A shared id or a note-named file without
/// frontmatter is reported and left to its author.
///
/// `links` and `times` are flags because they cost a read of every body and a
/// walk of history. Unlike `status`, this names every file.
pub fn doctor(paths: &Paths, dry_run: bool, links: bool, times: bool) -> Result<String> {
    let notebook = Notebook::open_active(paths)?;
    let scan = notebook.scan()?;
    let problems = scan.problems();

    let audit = if links {
        Some(notebook.audit_links()?)
    } else {
        None
    };
    let mut report = audit.as_ref().map(describe_audit).unwrap_or_default();

    if times {
        let found = describe_times(&notebook.notes()?, &notebook.last_changed()?);
        if !found.is_empty() {
            if !report.is_empty() {
                report.push('\n');
            }
            report.push_str(&found);
        }
    }

    // Always on: it parses no links and walks no history.
    let unconverted = describe_unconverted(&notebook.notes()?);
    if !unconverted.is_empty() {
        if !report.is_empty() {
            report.push('\n');
        }
        report.push_str(&unconverted);
    }

    let hooks = describe_hooks(&notebook.hooks()?);
    if !hooks.is_empty() {
        if !report.is_empty() {
            report.push('\n');
        }
        report.push_str(&hooks);
    }
    let extras = report;

    if problems.is_empty() {
        return Ok(if extras.is_empty() {
            "the notebook is in order".to_string()
        } else {
            extras
        });
    }

    let mut out = String::new();
    for (kind, subjects) in &problems {
        let _ = writeln!(out, "{}", kind.describe(subjects.len()));
        for subject in subjects {
            let _ = writeln!(out, "  {subject}");
        }
    }
    for line in advice(&scan) {
        let _ = writeln!(out, "{line}");
    }
    if !extras.is_empty() {
        let _ = writeln!(out, "{extras}");
    }

    if scan.unnamed.is_empty() {
        return Ok(out.trim_end().to_string());
    }

    let count = scan.unnamed.len();
    let noun = if count == 1 { "note" } else { "notes" };
    if dry_run {
        let _ = write!(
            out,
            "{}",
            style::paint(
                style::MUTED,
                &format!("would adopt {count} {noun} — nothing was changed")
            )
        );
        return Ok(out.trim_end().to_string());
    }

    // A commit, so an unwanted repair is revertible.
    let mut taken = notebook.taken_ids()?;
    let mut changed = Vec::new();
    for file in &scan.unnamed {
        let stem = file.strip_suffix(".md").unwrap_or(file);
        let id = note::mint_id(&taken);
        taken.insert(note::normalize_id(&id));
        let adopted = note::file_name(&id, &note::slugify(stem));
        std::fs::rename(notebook.path.join(file), notebook.path.join(&adopted))?;
        changed.push(file.clone());
        changed.push(adopted);
    }
    let files: Vec<&Path> = changed.iter().map(Path::new).collect();
    notebook.commit(&files, &format!("doctor: adopt {count} {noun}"))?;
    let _ = write!(out, "adopted {count} {noun}");
    Ok(out.trim_end().to_string())
}

/// Orphaned files, stale links and broken links. Nothing is repaired: only the
/// author knows whether an orphan or a broken link is intended, and fixing a
/// stale link edits notes this command was not pointed at.
fn describe_audit(audit: &notebook::Audit) -> String {
    let mut out = String::new();

    if !audit.orphans.is_empty() {
        let count = audit.orphans.len();
        let noun = if count == 1 { "file" } else { "files" };
        // `links` either way: the subject is `no note`, always singular.
        let _ = writeln!(out, "{count} {noun} no note links to");
        for file in &audit.orphans {
            let _ = writeln!(out, "  {file}");
        }
    }

    if !audit.stale.is_empty() {
        let count = audit.stale.len();
        let noun = if count == 1 { "link" } else { "links" };
        let _ = writeln!(out, "{count} stale {noun}");
        for (note, target, now) in &audit.stale {
            let arrow = style::paint(style::MUTED, "->");
            let _ = writeln!(out, "  {note} {arrow} {target}");
            let _ = writeln!(
                out,
                "    {}",
                style::paint(style::MUTED, &format!("now {now}"))
            );
        }
    }

    if !audit.broken.is_empty() {
        let count = audit.broken.len();
        let noun = if count == 1 { "link" } else { "links" };
        let _ = writeln!(out, "{count} broken {noun}");
        for (note, target) in &audit.broken {
            let _ = writeln!(
                out,
                "  {note} {} {target}",
                style::paint(style::MUTED, "->")
            );
        }
    }

    out.trim_end().to_string()
}

/// Seconds a commit may trail the `updated` noda wrote. noda writes and commits
/// together, so this only covers a slow commit; an outside edit shows up far later.
const COMMIT_LAG: i64 = 60;

/// Timestamps checked against themselves and against git. Nothing is repaired:
/// the only fix would overwrite somebody's record with a guess.
fn describe_times(notes: &[notebook::NoteFile], last: &HashMap<String, i64>) -> String {
    let mut unreadable = Vec::new();
    let mut reversed = Vec::new();
    let mut stale = Vec::new();

    for file in notes {
        let name = note::file_name(&file.id, &file.slug);
        for (field, value) in [
            ("created", file.note.created.as_ref()),
            ("updated", file.note.updated.as_ref()),
        ] {
            if let Some(value) = value
                && instant(Some(value)).is_none()
            {
                unreadable.push(format!("{name} {field}: {value}"));
            }
        }

        let created = instant(file.note.created.as_ref());
        let updated = instant(file.note.updated.as_ref());
        if let (Some(created), Some(updated)) = (created, updated)
            && updated < created
        {
            reversed.push(name.clone());
        }

        // git can only say the file changed later — which is all that is claimed.
        if let (Some(updated), Some(committed)) = (updated, last.get(&note::normalize_id(&file.id)))
            && *committed - updated.as_second() > COMMIT_LAG
        {
            stale.push(name.clone());
        }
    }

    let mut out = String::new();
    if !unreadable.is_empty() {
        let count = unreadable.len();
        let noun = if count == 1 { "time" } else { "times" };
        let _ = writeln!(out, "{count} {noun} cannot be read");
        for line in &unreadable {
            let _ = writeln!(out, "  {line}");
        }
    }
    if !reversed.is_empty() {
        let count = reversed.len();
        let noun = if count == 1 { "note" } else { "notes" };
        let _ = writeln!(out, "{count} {noun} changed before being created");
        for name in &reversed {
            let _ = writeln!(out, "  {name}");
        }
    }
    if !stale.is_empty() {
        let count = stale.len();
        let (noun, verb) = if count == 1 {
            ("note", "was")
        } else {
            ("notes", "were")
        };
        let _ = writeln!(out, "{count} {noun} {verb} changed outside noda");
        for name in &stale {
            let _ = writeln!(out, "  {name}");
        }
        let _ = writeln!(
            out,
            "{}",
            style::paint(
                style::MUTED,
                "git has a commit newer than the note's own `updated`"
            )
        );
    }
    out.trim_end().to_string()
}

/// The notes whose `unconverted:` field says an importer could not finish them —
/// the one place that field is surfaced, since `search` does not read it.
/// Nothing is repaired: only the author knows what the `WikiText` should say.
fn describe_unconverted(notes: &[notebook::NoteFile]) -> String {
    let prefix = format!("{}: ", import::UNCONVERTED);
    let mut found: Vec<(String, String)> = notes
        .iter()
        .filter_map(|file| {
            let what = file
                .note
                .extra
                .iter()
                .find_map(|line| line.strip_prefix(&prefix))?;
            Some((
                note::file_name(&file.id, &file.slug),
                what.trim().to_string(),
            ))
        })
        .collect();
    if found.is_empty() {
        return String::new();
    }
    found.sort();

    let mut out = String::new();
    let noun = |n: usize| if n == 1 { "note" } else { "notes" };
    let _ = writeln!(
        out,
        "{} {} {} text an importer did not convert",
        found.len(),
        noun(found.len()),
        if found.len() == 1 { "carries" } else { "carry" }
    );

    // Counted by kind rather than listed by note: after an import that can be
    // most of the notebook.
    let mut kinds: HashMap<&str, usize> = HashMap::new();
    for (_, what) in &found {
        for kind in what.split(',') {
            *kinds.entry(kind.trim()).or_default() += 1;
        }
    }
    let mut kinds: Vec<(&str, usize)> = kinds.into_iter().collect();
    kinds.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
    for (kind, count) in &kinds {
        let _ = writeln!(out, "  {count} {} {kind}", noun(*count));
    }

    // The cap is announced, or the list reads as complete.
    const NAMED: usize = 5;
    let _ = writeln!(out, "  {}", style::paint(style::MUTED, "for example:"));
    for (file, _) in found.iter().take(NAMED) {
        let _ = writeln!(out, "    {file}");
    }
    if found.len() > NAMED {
        let _ = writeln!(
            out,
            "    {}",
            style::paint(
                style::MUTED,
                &format!(
                    "and {} more, each with its own `unconverted:` field",
                    found.len() - NAMED
                )
            )
        );
    }
    out
}

/// The hooks that will never fire. Not behind a flag, being one directory read;
/// not a `Problem` (so not in `status`), since `.git` is not the notebook.
fn describe_hooks(hooks: &[String]) -> String {
    if hooks.is_empty() {
        return String::new();
    }
    let count = hooks.len();
    let noun = if count == 1 { "hook" } else { "hooks" };
    let mut out = String::new();
    let _ = writeln!(out, "{count} git {noun} will never run");
    for hook in hooks {
        let _ = writeln!(out, "  {hook}");
    }
    let _ = write!(
        out,
        "{}",
        style::paint(
            style::MUTED,
            "noda carries its own libgit2 and never calls git, which is what would run them"
        )
    );
    out
}

/// How to settle the two problems noda leaves to the user.
fn advice(scan: &notebook::Scan) -> Vec<String> {
    let mut out = Vec::new();
    if !scan.notes.is_empty() {
        let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        let mut shared = false;
        for (id, _) in &scan.notes {
            if !seen.insert(note::normalize_id(id)) {
                shared = true;
            }
        }
        if shared {
            out.push(
                "  to settle a shared id, rename one of the files so it starts with a \
                 different id"
                    .to_string(),
            );
        }
    }
    if !scan.suspicious.is_empty() {
        out.push(
            "  a file named like a note but holding no frontmatter is either a note that lost \
             it — add a `---` block back — or a file that was never one, which you can rename \
             so it no longer starts with an id"
                .to_string(),
        );
    }
    out
}

/// The drift phrased as what is left to do — one wording for every screen.
/// `never synced`, not `never fetched`: a first push clears it as well as a
/// fetch. Plain text, because one caller renders HTML.
pub fn drifted(drift: Option<(usize, usize)>) -> String {
    match drift {
        None => "never synced".to_string(),
        Some((0, 0)) => "in sync".to_string(),
        Some((ahead, 0)) => format!("{ahead} to push"),
        Some((0, behind)) => format!("{behind} to pull"),
        Some((ahead, behind)) => format!("{ahead} to push, {behind} to pull"),
    }
}

/// [`drifted`], or `no remote` where `never synced` would be a state it can
/// never leave.
pub fn standing(remote: Option<&str>, drift: Option<(usize, usize)>) -> String {
    match remote {
        None => "no remote".to_string(),
        Some(_) => drifted(drift),
    }
}

/// [`drifted`], painted, with the caveat that `status` reports the last sync.
fn describe_drift(drift: Option<(usize, usize)>) -> String {
    let words = drifted(drift);
    match drift {
        None => style::paint(style::MUTED, &words),
        Some(_) => format!(
            "{words} {}",
            style::paint(style::MUTED, "(as of the last sync)")
        ),
    }
}

/// Marks an unpushed commit in `noda log` and the TUI's log screen — the arrow
/// the TUI header uses for commits ahead. A margin, not a column.
pub const UNPUSHED: &str = "↑";

/// The notebook's history, or one note's, with unpushed commits marked.
pub fn log(paths: &Paths, key: Option<&str>, max: Option<usize>) -> Result<String> {
    let notebook = Notebook::open_active(paths)?;
    // `find`, not `locate`: a note with broken frontmatter is the one whose past
    // you want.
    let id = match key {
        Some(key) => Some(find(&notebook, key)?.id),
        None => None,
    };

    // Empty, at no cost, when there is no remote.
    let unpushed = notebook.unpushed(&notebook.branch()?)?;

    let entries = notebook.log(id.as_deref(), max)?;
    let mut shown = 0;
    let mut out = String::new();
    for entry in &entries {
        let mark = if unpushed.contains(&entry.id) {
            shown += 1;
            style::paint(style::MUTED, UNPUSHED)
        } else {
            " ".to_string()
        };
        let line = format!(
            "{mark} {}  {}  {}",
            style::paint(style::COMMIT, &entry.short_id()),
            style::paint(
                style::MUTED,
                &format_time(entry.seconds, entry.offset_minutes)
            ),
            entry.summary
        );
        out.push_str(line.trim_end());
        out.push('\n');
    }

    // `-n` can hide unpushed commits, so say how many. Whole-notebook only:
    // against one note's log the subtraction would mean nothing.
    let hidden = if id.is_none() {
        unpushed.len().saturating_sub(shown)
    } else {
        0
    };
    if hidden > 0 {
        let _ = writeln!(
            out,
            "{}",
            style::paint(
                style::MUTED,
                &format!("{hidden} more to push, below what `-n` shows")
            )
        );
    }
    Ok(out)
}

/// The notes that link to a note or one of the notebook's files. Its own
/// command rather than an `ls` flag, because it parses every body.
pub fn backlinks(paths: &Paths, key: &str, format: Format) -> Result<String> {
    let notebook = Notebook::open_active(paths)?;

    // Resolved as `path` does: one key can name both.
    let as_note = notebook.resolve(key);
    let as_file = notebook.path.join(key);
    let is_file = !key.contains('/') && !key.contains('\\') && as_file.is_file();

    let (subject, found) = match (as_note, is_file) {
        (Ok((id, slug)), false) => (
            note::file_name(&id, &slug),
            notebook.backlinks_to_note(&id)?,
        ),
        (Err(_), true) => (key.to_string(), notebook.backlinks_to_file(key)?),
        (Ok((id, slug)), true) => {
            return Err(Error::msg(format!(
                "`{key}` names both a note and a file — say which:\n  {}\n  {}",
                note::file_name(&id, &slug),
                key
            )));
        }
        (Err(Error::Msg(said)), false) if said.starts_with(notebook::NOT_FOUND) => {
            return Err(Error::msg(format!(
                "nothing called `{key}` — the notebook holds no note and no file by that name"
            )));
        }
        (Err(other), false) => return Err(other),
    };

    match format {
        // Before the empty check: a program asking for JSON gets a document.
        Format::Json => return Ok(backlinks_json(&notebook.name, &subject, &found)),
        // No `--null`: an id has no spaces to protect.
        Format::Quiet => {
            let mut out = String::new();
            for file in &found {
                let _ = writeln!(out, "{}", file.id);
            }
            return Ok(out);
        }
        Format::Table => {}
    }

    if found.is_empty() {
        return Ok(style::paint(
            style::MUTED,
            &format!("nothing links to {subject}"),
        ));
    }

    // `ls`'s row shape.
    let ids = found
        .iter()
        .map(|f| display_width(&f.id))
        .max()
        .unwrap_or(0);
    let mut out = String::new();
    for file in &found {
        let line = format!("{}  {}", pad(&file.id, ids), file.note.title);
        out.push_str(line.trim_end());
        out.push('\n');
    }
    Ok(out)
}

/// Carries the resolved filename as `target`, so a script asking by slug learns
/// what it resolved to.
fn backlinks_json(notebook: &str, subject: &str, found: &[notebook::NoteFile]) -> String {
    let mut out = String::from("{\"notebook\":");
    out.push_str(&json_string(notebook));
    out.push_str(",\"target\":");
    out.push_str(&json_string(subject));
    out.push_str(",\"backlinks\":[");
    for (index, file) in found.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        let _ = write!(
            out,
            "{{\"id\":{},\"slug\":{},\"file\":{},\"title\":{}}}",
            json_string(&file.id),
            json_string(&file.slug),
            json_string(&note::file_name(&file.id, &file.slug)),
            json_string(&file.note.title),
        );
    }
    out.push_str("]}\n");
    out
}

/// Every unticked checkbox in the notebook, soonest due first.
///
/// There is no `noda done`: an item has no stable address (line numbers move,
/// text collides), and ids would make the file noda-only. `noda edit` it.
pub fn todo(paths: &Paths, json: bool) -> Result<String> {
    todo_on(paths, json, &today()?)
}

/// The *local* date: nobody writes `due:2026-08-10` meaning UTC, and east of UTC
/// an item would otherwise go overdue hours late. Public so the browser agrees.
pub fn today() -> Result<String> {
    let (seconds, offset_minutes) = notebook::local_now()?;
    // The date half of `YYYY-MM-DD HH:MM`, ASCII throughout.
    Ok(format_time(seconds, offset_minutes)[..DATE_WIDTH].to_string())
}

/// `todo` with today given explicitly, so a test need not freeze the clock.
pub fn todo_on(paths: &Paths, json: bool, today: &str) -> Result<String> {
    let notebook = Notebook::open_active(paths)?;
    let mut items = Vec::new();
    for file in notebook.notes()? {
        for item in todo::items(&file.note.body) {
            items.push((file.id.clone(), file.slug.clone(), item));
        }
    }

    items.sort_by(|(_, left_slug, left), (_, right_slug, right)| {
        todo::order((left_slug, left), (right_slug, right))
    });

    // Before the empty check: JSON always gets a document.
    if json {
        return Ok(todo_json(&notebook.name, &items));
    }
    if items.is_empty() {
        return Ok(style::paint(style::MUTED, "nothing to do"));
    }

    let width = |pick: fn(&(String, String, todo::Item)) -> &str| {
        items
            .iter()
            .map(|row| display_width(pick(row)))
            .max()
            .unwrap_or(0)
    };
    let ids = width(|(id, ..)| id.as_str());
    let slugs = width(|(_, slug, _)| slug.as_str());

    let mut out = String::new();
    for (id, slug, item) in &items {
        // Never truncated: a cut-off item would have to be opened to read.
        let due = match &item.due {
            Some(due) if item.overdue(today) => style::paint(style::OVERDUE, due),
            Some(due) => style::paint(style::MUTED, due),
            None => " ".repeat(DATE_WIDTH),
        };
        let line = format!(
            "{}  {}  {due}  {}",
            pad(id, ids),
            pad(slug, slugs),
            item.text
        );
        out.push_str(line.trim_end());
        out.push('\n');
    }
    Ok(out)
}

/// `due` but not `overdue`: a program has its own clock and time zone.
fn todo_json(notebook: &str, items: &[(String, String, todo::Item)]) -> String {
    let mut out = String::from("{\"notebook\":");
    out.push_str(&json_string(notebook));
    out.push_str(",\"todo\":[");
    for (index, (id, slug, item)) in items.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        let _ = write!(
            out,
            "{{\"id\":{},\"slug\":{},\"file\":{},\"text\":{},\"due\":{}}}",
            json_string(id),
            json_string(slug),
            json_string(&note::file_name(id, slug)),
            json_string(&item.text),
            match &item.due {
                Some(due) => json_string(due),
                None => "null".to_string(),
            }
        );
    }
    out.push_str("]}\n");
    out
}

/// `log`'s columns in `log`'s order, body only (see `Notebook::blame`). No line
/// numbers: in prose one looks for a paragraph, not a row.
pub fn blame(paths: &Paths, key: &str) -> Result<String> {
    let notebook = Notebook::open_active(paths)?;
    let found = find(&notebook, key)?;
    let lines = notebook.blame(&found.id, &found.slug)?;

    let mut out = String::new();
    for line in lines {
        let when = if line.commit.is_some() {
            style::paint(
                style::MUTED,
                &format_time(line.seconds, line.offset_minutes),
            )
        } else {
            style::paint(style::MUTED, &pad("not committed", TIME_WIDTH))
        };
        let rendered = format!(
            "{}  {when}  {}",
            style::paint(style::COMMIT, &line.short_commit()),
            line.text
        );
        out.push_str(rendered.trim_end());
        out.push('\n');
    }
    Ok(out)
}

/// Notes in history but no longer in the notebook, newest loss first. Its own
/// command rather than an `ls` flag because it walks all of history. The
/// revision printed is the deleting commit's *parent*, which is what `restore`
/// needs.
pub fn deleted(paths: &Paths, notebook_name: Option<&str>, json: bool) -> Result<String> {
    let name = match notebook_name {
        Some(name) => name.to_string(),
        None => notebook::active_name(paths)?,
    };
    let notebook = Notebook::open(paths, &name)?;
    let gone = notebook.deleted()?;

    // Before the empty check: JSON always gets a document.
    if json {
        return Ok(deleted_as_json(&name, &gone));
    }
    if gone.is_empty() {
        return Ok(String::new());
    }

    let id_width = gone.iter().map(|d| display_width(&d.id)).max().unwrap_or(0);
    let slug_width = gone
        .iter()
        .map(|d| display_width(&d.slug))
        .max()
        .unwrap_or(0);

    let mut out = String::new();
    for note in &gone {
        let line = format!(
            "{}  {}  {}  {}  {}",
            pad(&note.id, id_width),
            pad(&note.slug, slug_width),
            style::paint(
                style::MUTED,
                &format_time(note.removed_at, note.offset_minutes)
            ),
            style::paint(style::COMMIT, &note.restore_from_short()),
            note.title
        );
        out.push_str(line.trim_end());
        out.push('\n');
    }

    let _ = write!(
        out,
        "{}",
        style::paint(
            style::MUTED,
            "`noda restore <note> <commit>` with the commit above brings one back"
        )
    );
    Ok(out)
}

/// Hand-written, as `as_json` is. Commit ids in full, since an abbreviation can
/// stop being unique; times in UTC.
fn deleted_as_json(notebook: &str, gone: &[notebook::Deleted]) -> String {
    let mut out = String::from("{\"notebook\":");
    out.push_str(&json_string(notebook));
    out.push_str(",\"deleted\":[");
    for (index, note) in gone.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        let _ = write!(
            out,
            "{{\"id\":{},\"slug\":{},\"file\":{},\"title\":{},\"removed_at\":{},\"removed_in\":{},\"restore_from\":{}}}",
            json_string(&note.id),
            json_string(&note.slug),
            json_string(&note::file_name(&note.id, &note.slug)),
            json_string(&note.title),
            json_string(&rfc3339(note.removed_at)),
            json_string(&note.removed_in.to_string()),
            json_string(&note.restore_from.to_string()),
        );
    }
    out.push_str("]}\n");
    out
}

/// The spelling noda writes everywhere.
fn rfc3339(seconds: i64) -> String {
    jiff::Timestamp::from_second(seconds)
        .map(|time| time.strftime("%Y-%m-%dT%H:%M:%SZ").to_string())
        .unwrap_or_default()
}

/// Uncommitted changes, or what the last commit changed, as a plain unified diff
/// `git apply` will take. `remote` shows what a push would carry instead (see
/// [`Notebook::diff_remote`]).
pub fn diff(paths: &Paths, key: Option<&str>, remote: bool) -> Result<String> {
    let notebook = Notebook::open_active(paths)?;
    // `find`: the diff is how you find out why a note will not parse.
    let file = match key {
        Some(key) => {
            let found = find(&notebook, key)?;
            Some(note::file_name(&found.id, &found.slug))
        }
        None => None,
    };

    let diff = if remote {
        notebook.diff_remote(&notebook.branch()?, file.as_deref())?
    } else {
        notebook.diff(file.as_deref())?
    };

    let mut out = String::new();
    diff.print(git2::DiffFormat::Patch, |_delta, _hunk, line| {
        let text = String::from_utf8_lossy(line.content());
        let painted = match line.origin() {
            '+' => style::paint(style::ADDED, &format!("+{text}")),
            '-' => style::paint(style::REMOVED, &format!("-{text}")),
            ' ' => format!(" {text}"),
            'F' => style::paint(style::HEADING, &text),
            'H' => style::paint(style::HUNK, &text),
            _ => text.into_owned(),
        };
        out.push_str(&painted);
        true
    })?;
    Ok(out)
}

/// Restores a note's contents from `rev`, as a new commit.
pub fn restore(paths: &Paths, key: &str, rev: &str, touch: Touch) -> Result<String> {
    let notebook = Notebook::open_active(paths)?;
    let commit = notebook.revision(rev)?;

    // `find`: a broken note is about to be overwritten, not read.
    let current = find(&notebook, key).ok();
    // From history when the note is gone — which is how this undoes `noda rm`.
    let id = match current.as_ref() {
        Some(found) => found.id.clone(),
        None => notebook
            .id_at(&commit, key)?
            .ok_or_else(|| Error::msg(format!("note not found at {rev}: {key}")))?,
    };

    let Some((slug_then, text)) = notebook.note_at(&commit, &id)? else {
        return Err(Error::msg(format!(
            "`{key}` did not exist at {rev} — `noda log {key}` shows where it did"
        )));
    };

    // Only contents travel back: a present note keeps today's name, a gone one
    // returns under its old one.
    let slug = match &current {
        Some(found) => found.slug.clone(),
        None => slug_then,
    };
    let path = notebook.note_path(&id, &slug);

    let restored = Note::parse(&text)
        .map_err(|e| Error::msg(format!("the copy of `{key}` at {rev} cannot be read: {e}")))?;
    // `updated` is ignored, or restoring the same revision twice would never
    // say "no change" — the first restore stamped it.
    let ignoring_updated =
        |text: &str| note::set_field(text, "updated", "").unwrap_or_else(|| text.to_string());
    if current
        .as_ref()
        .and_then(|found| std::fs::read_to_string(&found.path).ok())
        .is_some_and(|on_disk| match touch {
            Touch::Stamp => ignoring_updated(&on_disk) == ignoring_updated(&text),
            Touch::Keep => on_disk == text,
        })
    {
        return Ok(format!(
            "{}  (no change)",
            summary(&id, &slug, &restored.tags)
        ));
    }

    // The file changed now; `created` stays, it is the same note.
    let text = match touch {
        Touch::Stamp => note::set_field(&text, "updated", &note::now())
            .expect("the copy at this revision parsed, so it has a frontmatter block"),
        Touch::Keep => text,
    };
    std::fs::write(&path, &text)?;
    notebook.commit(
        &[Path::new(&note::file_name(&id, &slug))],
        &format!("restore: {slug} to {}", &commit.id().to_string()[..7]),
    )?;

    Ok(summary(&id, &slug, &restored.tags))
}

/// Points the active notebook at `url`, replacing any remote already set.
pub fn remote_set(paths: &Paths, url: &str) -> Result<String> {
    let url = url.trim();
    if url.is_empty() {
        return Err(Error::msg("a remote needs a URL"));
    }
    let notebook = Notebook::open_active(paths)?;
    notebook.set_remote(url)?;
    // Redacted even though just typed: this line stays in the scrollback.
    Ok(format!("{}  {}", notebook.name, remote::redact(url)))
}

/// Prints the active notebook's remote.
pub fn remote_show(paths: &Paths) -> Result<String> {
    let notebook = Notebook::open_active(paths)?;
    notebook.remote_url().ok_or_else(|| {
        Error::msg(format!(
            "notebook `{}` has no remote — set one with `noda remote set <url>`",
            notebook.name
        ))
    })
}

/// Fetches and integrates the remote branch.
pub fn pull(paths: &Paths) -> Result<String> {
    pull_in(&Notebook::open_active(paths)?)
}

/// `pull`, in a notebook the caller already has open.
pub fn pull_in(notebook: &Notebook) -> Result<String> {
    notebook.pull()
}

/// Sends the current branch to the remote.
pub fn push(paths: &Paths) -> Result<String> {
    push_in(&Notebook::open_active(paths)?)
}

/// `push`, in a notebook the caller already has open.
pub fn push_in(notebook: &Notebook) -> Result<String> {
    notebook.push()
}

/// Commit, pull, push — in that order: a pull before the commit merges into a
/// tree missing local work, and a push before the pull is refused.
pub fn sync(paths: &Paths) -> Result<String> {
    sync_in(&Notebook::open_active(paths)?)
}

/// `sync`, in a notebook the caller already has open.
pub fn sync_in(notebook: &Notebook) -> Result<String> {
    let mut lines = Vec::new();
    if notebook.commit_all("sync: local changes")? {
        lines.push("commit: local changes".to_string());
    }
    lines.push(notebook.pull()?);
    lines.push(notebook.push()?);
    Ok(lines.join("\n"))
}

/// Marks the notebook as it stands, committing the working tree first so the
/// snapshot includes what is on disk. The message defaults to the name.
pub fn snapshot(paths: &Paths, name: &str, message: Option<&str>) -> Result<String> {
    let notebook = Notebook::open_active(paths)?;
    let mut lines = Vec::new();
    if notebook.commit_all(&format!("snapshot: {name}"))? {
        lines.push("commit: local changes".to_string());
    }
    let target = notebook.snapshot(name, message.unwrap_or(name))?;
    lines.push(format!("snapshot: {name} -> {}", notebook::short(target)));
    Ok(lines.join("\n"))
}

/// Name, time, commit, message: time before commit, as `deleted` has them.
pub fn snapshot_ls(paths: &Paths) -> Result<String> {
    let notebook = Notebook::open_active(paths)?;
    let snapshots = notebook.snapshots()?;
    if snapshots.is_empty() {
        return Ok(style::paint(
            style::MUTED,
            "no snapshots — take one with `noda snapshot <name>`",
        ));
    }

    let width = snapshots
        .iter()
        .map(|snapshot| display_width(&snapshot.name))
        .max()
        .unwrap_or(0);
    let mut out = String::new();
    for snapshot in &snapshots {
        let line = format!(
            "{}  {}  {}  {}",
            pad(&snapshot.name, width),
            style::paint(
                style::MUTED,
                &format_time(snapshot.seconds, snapshot.offset_minutes)
            ),
            style::paint(style::COMMIT, &snapshot.short_target()),
            snapshot.message
        );
        out.push_str(line.trim_end());
        out.push('\n');
    }
    Ok(out)
}

/// Clones a remote notebook. The name defaults to the repository's own.
pub fn clone(paths: &Paths, url: &str, name: Option<&str>) -> Result<String> {
    let name = match name {
        Some(name) => name.to_string(),
        None => remote::name_from_url(url).ok_or_else(|| {
            Error::msg(format!(
                "cannot tell what to call the notebook from `{}` — pass a name",
                remote::redact(url)
            ))
        })?,
    };
    let notebook = Notebook::clone(paths, url, &name)?;
    let count = notebook.notes()?.len();
    Ok(format!(
        "cloned `{name}` ({count} notes) to {}\nswitch to it with `noda use {name}`",
        notebook.path.display()
    ))
}

/// Sets the active notebook.
pub fn use_notebook(paths: &Paths, name: &str) -> Result<String> {
    notebook::validate_name(name)?;
    if !Notebook::exists(paths, name) {
        return Err(Error::msg(format!(
            "notebook not found: {name} — create it with `noda notebook add {name}`"
        )));
    }
    paths.set_active_notebook(name)?;
    Ok(format!("active notebook: {name}"))
}

pub fn notebook_current(paths: &Paths) -> Result<String> {
    notebook::active_name(paths)
}

/// A note reference resolved to a file, with the parse result kept rather than
/// thrown: a command that only needs *which* note must not fail on a broken one.
struct Found {
    /// From the filename.
    id: String,
    slug: String,
    path: PathBuf,
    note: Result<Note>,
}

fn find(notebook: &Notebook, key: &str) -> Result<Found> {
    let (id, slug) = notebook.resolve(key)?;
    let path = notebook.note_path(&id, &slug);
    let note = Note::parse(&std::fs::read_to_string(&path)?)
        .map_err(|e| Error::msg(format!("{}: {e}", path.display())));
    Ok(Found {
        id,
        slug,
        path,
        note,
    })
}

/// [`Found`] with the note parsed.
struct Located {
    id: String,
    slug: String,
    path: PathBuf,
    note: Note,
}

fn locate(notebook: &Notebook, key: &str) -> Result<Located> {
    let found = find(notebook, key)?;
    Ok(Located {
        id: found.id,
        slug: found.slug,
        path: found.path,
        note: found.note?,
    })
}

/// The one-line acknowledgement every mutating command prints.
fn summary(id: &str, slug: &str, tags: &[String]) -> String {
    if tags.is_empty() {
        format!("{id}  {slug}")
    } else {
        format!("{id}  {slug}  [{}]", tags.join(", "))
    }
}

/// Width of [`format_time`]'s output.
pub const TIME_WIDTH: usize = "0000-00-00 00:00".len();

/// Width of a `due:` date, for padding rows that have none.
pub const DATE_WIDTH: usize = "0000-00-00".len();

/// In the commit's own zone, as git does. Absolute rather than "3 days ago": it
/// sorts and needs no frozen clock to test. Public so the TUI and web match.
pub fn format_time(seconds: i64, offset_minutes: i32) -> String {
    let local = seconds + i64::from(offset_minutes) * 60;
    let (year, month, day) = civil_from_days(local.div_euclid(86_400));
    let time = local.rem_euclid(86_400);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}",
        time / 3600,
        (time % 3600) / 60
    )
}

/// Howard Hinnant's `civil_from_days`; fifteen lines beats a date dependency.
/// The casts are sign-safe: month and day are always positive, as
/// `every_day_lands_on_a_real_calendar_date` checks across four centuries.
#[allow(clippy::cast_sign_loss)]
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    // Epoch at 0000-03-01: the leap day lands at the year's end and every era
    // is exactly 146,097 days.
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_position = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_position + 2) / 5 + 1) as u32;
    let month = if month_position < 10 {
        month_position + 3
    } else {
        month_position - 9
    } as u32;
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// East Asian Wide and Fullwidth characters take two cells, so `chars()` would
/// misalign a CJK slug. The wide blocks in common use, not all of UAX #11.
pub fn display_width(text: &str) -> usize {
    text.chars()
        .map(|c| match c as u32 {
            0x1100..=0x115F
            | 0x2E80..=0x303E
            | 0x3041..=0x33FF
            | 0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xA000..=0xA4CF
            | 0xAC00..=0xD7A3
            | 0xF900..=0xFAFF
            | 0xFE10..=0xFE19
            | 0xFE30..=0xFE6F
            | 0xFF00..=0xFF60
            | 0xFFE0..=0xFFE6
            | 0x1F300..=0x1F64F
            | 0x1F900..=0x1F9FF
            | 0x20000..=0x3FFFD => 2,
            _ => 1,
        })
        .sum()
}

fn pad(text: &str, width: usize) -> String {
    let spaces = width.saturating_sub(display_width(text));
    format!("{text}{}", " ".repeat(spaces))
}

/// Pads *outside* the escape sequences, so the width ignores them and the
/// caller's trailing trim can reach the spaces.
fn column(style: anstyle::Style, text: &str, width: usize) -> String {
    let spaces = width.saturating_sub(display_width(text));
    format!("{}{}", style::paint(style, text), " ".repeat(spaces))
}

/// First non-empty line, with any Markdown heading marker stripped.
fn derive_title(body: &str) -> Option<String> {
    body.lines()
        .map(|line| line.trim_start_matches('#').trim())
        .find(|line| !line.is_empty())
        .map(str::to_string)
}

/// Trimmed, as the frontmatter reads back, and refused if they cannot round-trip.
fn clean_tags(tags: &[String]) -> Result<Vec<String>> {
    tags.iter()
        .map(|tag| {
            let tag = tag.trim();
            note::validate_tag(tag)?;
            Ok(tag.to_string())
        })
        .collect()
}

fn configured_editor(paths: &Paths) -> String {
    let configured = Config::load(paths)
        .ok()
        .and_then(|config| config.get("editor").map(str::to_string));
    config::editor(
        configured.as_deref(),
        std::env::var("VISUAL").ok(),
        std::env::var("EDITOR").ok(),
    )
    .0
}

/// Runs `editor` on `path`, treating a non-zero exit as an aborted edit.
fn run_editor(editor: &str, path: &Path) -> Result<()> {
    let mut parts = editor.split_whitespace();
    let program = parts
        .next()
        .ok_or_else(|| Error::msg("$EDITOR is set but empty"))?;
    let status = std::process::Command::new(program)
        .args(parts)
        .arg(path)
        .status()
        .map_err(|e| Error::msg(format!("could not start editor `{program}`: {e}")))?;
    if !status.success() {
        return Err(Error::msg(format!(
            "editor `{program}` exited with {status}"
        )));
    }
    Ok(())
}

/// Opens `$EDITOR` on a scratch file in the cache dir — never the notebook, so an
/// abandoned edit leaves nothing in the repo — and returns what was written.
fn compose_in_editor(paths: &Paths, title: Option<&str>) -> Result<String> {
    std::fs::create_dir_all(paths.cache_dir())?;
    let scratch = paths.cache_dir().join(EDIT_FILE);
    let template = match title {
        Some(title) => format!("# {title}\n\n"),
        None => String::new(),
    };
    std::fs::write(&scratch, &template)?;

    run_editor(&configured_editor(paths), &scratch)?;

    let body = std::fs::read_to_string(&scratch)?;
    let _ = std::fs::remove_file(&scratch);
    Ok(body)
}

/// Writes command output to stdout, adding a trailing newline only when needed.
/// Goes through `anstream`, which keeps colour on a terminal and strips it
/// everywhere else — so a redirected `noda show` writes the file byte for byte.
pub fn print(output: &str) -> Result<()> {
    if output.is_empty() {
        return Ok(());
    }
    // Output carrying a NUL is machine-separated by construction — `ls -q0`,
    // written for `xargs -0` — and needs both of the things below skipped.
    // anstream strips NUL along with the escape sequences it is there to
    // remove, and the trailing newline would arrive after a terminator and
    // become part of the next record.
    if output.contains('\0') {
        let mut stdout = std::io::stdout().lock();
        stdout.write_all(output.as_bytes())?;
        return Ok(());
    }
    let mut stdout = anstream::stdout().lock();
    stdout.write_all(output.as_bytes())?;
    if !output.ends_with('\n') {
        stdout.write_all(b"\n")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An order added to `ALL` but not `next` would not fail to compile.
    #[test]
    fn every_order_is_on_the_ring_the_key_walks() {
        let mut at = Sort::default();
        for sort in Sort::ALL {
            assert_eq!(at, sort, "the ring and the list came apart");
            at = at.next();
        }
        assert_eq!(at, Sort::default(), "the ring stopped coming round");
    }

    /// Fixed ids and stamps, so only the sort can move anything.
    fn a_note(id: &str, title: &str, updated: &str, pinned: bool) -> notebook::NoteFile {
        notebook::NoteFile {
            id: id.to_string(),
            slug: note::slugify(title),
            note: note::Note {
                title: title.to_string(),
                tags: Vec::new(),
                created: Some(updated.to_string()),
                updated: Some(updated.to_string()),
                pinned: pinned.then(|| note::PINNED.to_string()),
                extra: Vec::new(),
                body: String::new(),
            },
        }
    }

    /// All four orders, `Slug` included, which does no sorting of its own.
    #[test]
    fn a_pin_floats_to_the_top_of_whichever_order_was_asked_for() {
        for sort in Sort::ALL {
            let mut notes = vec![
                a_note("aaaa1111", "Alpha", "2026-01-01T00:00:00Z", false),
                a_note("bbbb2222", "Bravo", "2026-01-02T00:00:00Z", true),
                a_note("cccc3333", "Charlie", "2026-01-03T00:00:00Z", false),
                a_note("dddd4444", "Delta", "2026-01-04T00:00:00Z", true),
            ];
            sort_notes(&mut notes, sort);
            let ids: Vec<&str> = notes.iter().map(|file| file.id.as_str()).collect();
            assert!(
                notes[0].note.is_pinned() && notes[1].note.is_pinned(),
                "{} let a loose note above a pinned one: {ids:?}",
                sort.name()
            );
            let want = match sort {
                Sort::Created | Sort::Updated => ["dddd4444", "bbbb2222"],
                Sort::Slug | Sort::Title => ["bbbb2222", "dddd4444"],
            };
            assert_eq!(
                [ids[0], ids[1]],
                want,
                "{} lost the order inside the pins",
                sort.name()
            );
        }
    }

    #[test]
    fn an_order_is_read_back_out_of_the_name_it_is_written_with() {
        for sort in Sort::ALL {
            assert_eq!(Sort::named(sort.name()), Some(sort));
        }
        assert_eq!(Sort::named("newest"), None);
        assert_eq!(Sort::named(""), None);
    }

    #[test]
    fn a_local_date_is_not_the_utc_one() {
        // 2026-08-02T23:00:00Z: already the 3rd in Taipei.
        let instant = 1_785_711_600;
        assert_eq!(&format_time(instant, 480)[..10], "2026-08-03");
        assert_eq!(&format_time(instant, 0)[..10], "2026-08-02");
        assert_eq!(&format_time(instant, -300)[..10], "2026-08-02");
    }

    #[test]
    fn wide_characters_count_as_two_columns() {
        assert_eq!(display_width("reading-log"), 11);
        assert_eq!(display_width("會議-筆記"), 9);
    }

    #[test]
    fn pad_aligns_mixed_width_slugs_to_the_same_column() {
        let width = 11;
        assert_eq!(display_width(&pad("會議-筆記", width)), width);
        assert_eq!(display_width(&pad("reading-log", width)), width);
        // Past the target width: never truncate, never panic.
        assert_eq!(pad("reading-log", 4), "reading-log");
    }

    #[test]
    fn a_notebook_says_where_it_stands_in_gits_own_words() {
        assert_eq!(standing(None, None), "no remote");
        assert_eq!(standing(Some("git@x:y.git"), None), "never synced");
        assert_eq!(standing(Some("git@x:y.git"), Some((0, 0))), "in sync");
        assert_eq!(standing(Some("git@x:y.git"), Some((2, 0))), "2 to push");
        assert_eq!(standing(Some("git@x:y.git"), Some((0, 1))), "1 to pull");
        assert_eq!(
            standing(Some("git@x:y.git"), Some((2, 1))),
            "2 to push, 1 to pull"
        );
    }

    #[test]
    fn a_notebook_with_no_remote_is_not_merely_unsynced() {
        assert_eq!(drifted(None), "never synced");
        assert_eq!(standing(None, None), "no remote");
        // No remote outranks the drift.
        assert_eq!(standing(None, Some((2, 0))), "no remote");
    }

    #[test]
    fn derive_title_skips_blank_lines_and_heading_markers() {
        assert_eq!(
            derive_title("\n\n## Deep Work\nbody"),
            Some("Deep Work".into())
        );
        assert_eq!(derive_title("   \n\n"), None);
    }

    #[test]
    fn case_insensitive_find_returns_offsets_into_the_original() {
        let line = "Discuss the Q3 Budget";
        let (start, end) = find_ignoring_case(line, "q3 budget").unwrap();
        assert_eq!(&line[start..end], "Q3 Budget");

        // İ lowercases to two chars, changing the byte length.
        let turkish = "aİb";
        let (start, end) = find_ignoring_case(turkish, "b").unwrap();
        assert_eq!(&turkish[start..end], "b");

        assert_eq!(find_ignoring_case("會議紀錄", "紀錄"), Some((6, 12)));
        assert_eq!(find_ignoring_case("nothing", "here"), None);
    }

    #[test]
    fn excerpts_are_cut_around_the_match() {
        let long = format!("{} needle {}", "before ".repeat(20), "after ".repeat(20));
        let shown = strip(&excerpt(&long, &["needle".to_string()]).unwrap());
        assert!(shown.contains("needle"), "{shown}");
        assert!(shown.starts_with('…'), "the lead is cut: {shown}");
        assert!(shown.ends_with('…'), "the tail is cut: {shown}");
        assert!(shown.chars().count() <= EXCERPT_WIDTH + 2, "{shown}");

        let shown = strip(&excerpt("just the needle here", &["needle".to_string()]).unwrap());
        assert_eq!(shown, "just the needle here");
        assert_eq!(excerpt("no hit", &["needle".to_string()]), None);
    }

    #[test]
    fn dimming_the_frontmatter_changes_nothing_but_the_escapes() {
        let note = "---\nid: k3f9\ntitle: Alpha\n---\n\nbody, with --- in it\n";
        let shown = dim_frontmatter(note);
        assert_ne!(shown, note, "the frontmatter is styled");
        assert_eq!(strip(&shown), note, "and nothing else moves");

        assert_eq!(dim_frontmatter("no frontmatter\n"), "no frontmatter\n");
        assert_eq!(
            dim_frontmatter("---\nunterminated\n"),
            "---\nunterminated\n"
        );
    }

    fn strip(text: &str) -> String {
        let mut out = String::new();
        let mut chars = text.chars();
        while let Some(ch) = chars.next() {
            if ch == '\u{1b}' {
                for escaped in chars.by_ref() {
                    if escaped == 'm' {
                        break;
                    }
                }
            } else {
                out.push(ch);
            }
        }
        out
    }

    #[test]
    fn timestamps_print_in_the_timezone_the_commit_was_made_in() {
        // The same instant in UTC and in Taipei.
        assert_eq!(format_time(1_785_073_605, 0), "2026-07-26 13:46");
        assert_eq!(format_time(1_785_073_605, 480), "2026-07-26 21:46");
    }

    #[test]
    fn every_day_lands_on_a_real_calendar_date() {
        // Backs the unsigned casts in `civil_from_days`.
        let mut previous = None;
        for days in -73_000..=73_000 {
            let (year, month, day) = civil_from_days(days);
            assert!((1..=12).contains(&month), "day {days} gave month {month}");
            assert!((1..=31).contains(&day), "day {days} gave day {day}");
            let now = (year, month, day);
            if let Some(previous) = previous {
                assert!(previous < now, "{previous:?} then {now:?}");
            }
            previous = Some(now);
        }
    }

    #[test]
    fn the_calendar_holds_at_the_awkward_dates() {
        assert_eq!(format_time(0, 0), "1970-01-01 00:00");
        // A 400-year leap day, and the second before the epoch.
        assert_eq!(format_time(951_782_400, 0), "2000-02-29 00:00");
        assert_eq!(format_time(-1, 0), "1969-12-31 23:59");
        // A negative offset can push a commit back across midnight.
        assert_eq!(format_time(1_785_073_605, -840), "2026-07-25 23:46");
    }

    #[test]
    fn summary_omits_empty_tags() {
        assert_eq!(summary("k3f9", "notes", &[]), "k3f9  notes");
        assert_eq!(
            summary("k3f9", "notes", &["work".into(), "q3".into()]),
            "k3f9  notes  [work, q3]"
        );
    }
}
