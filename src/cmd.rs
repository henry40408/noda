//! Command implementations. Each one takes `Paths` explicitly so tests can run
//! against a throwaway root without touching the real environment.

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

    // Commented-out defaults change nothing, but they are the only way anyone
    // finds out what can be set.
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

/// Where a value came from is the question people have when the editor is not
/// the one they expected.
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

/// One setting's effective value, unadorned so it can be read by a script.
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

/// Writes the starter template first if the file is missing — nobody wants to
/// be dropped into an empty buffer.
pub fn config_edit(paths: &Paths) -> Result<String> {
    Config::write_template(paths)?;
    let path = paths.config_dir().join("config.toml");
    run_editor(&configured_editor(paths), &path)?;
    // A typo becomes an error now rather than at the next command, when the
    // connection to this edit would be lost.
    Config::load(paths)?;
    Ok(format!("{}", path.display()))
}

/// Every setting as it currently resolves.
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

/// The git side comes from the user's own configuration, not a notebook's: this
/// answers for the next notebook as much as the current one.
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
    // Whatever git itself would use, asked in the same order.
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

    // Before the editor opens: nobody should compose a note only to be told
    // its title cannot be written. `add_in` checks again, being reachable
    // on its own.
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

/// `add`, in a notebook the caller already has open and with the body written.
///
/// The notebook is passed because `noda web` opens one per request, and a second
/// handle on the same repository defeats the point. No editor either: a browser
/// arrives with the body in hand, and a command that might open one is not
/// something a request can call.
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

    // Two notes may share a slug; the id in front keeps the filenames apart.
    let slug = note::slugify(&title);
    let id = note::mint_id(&notebook.taken_ids()?);
    // Both, same value: a note never changed was changed as recently as it was
    // made, and writing one would make every reader infer the other.
    let now = note::now();
    let note = Note {
        title,
        tags,
        created: Some(now.clone()),
        updated: Some(now),
        extra: Vec::new(),
        body: body.trim_start_matches('\n').to_string(),
    };

    let file = note::file_name(&id, &slug);
    std::fs::write(notebook.path.join(&file), note.render())?;
    notebook.commit(&[Path::new(&file)], &format!("add: {slug}"))?;

    Ok(summary(&id, &slug, &note.tags))
}

/// A struct rather than a row of arguments: three formats times three subsets,
/// and every caller cares about two at most.
#[derive(Default)]
pub struct List<'a> {
    /// List another notebook instead of the active one.
    pub notebook: Option<&'a str>,
    /// Anything more selective than one tag is `search`'s job.
    pub tag: Option<&'a str>,
    pub format: Format,
    pub only: Only,
    /// NUL rather than newline, which is what makes `noda ls -q0 | xargs -0`
    /// correct rather than nearly correct for a name with a space in it.
    pub null: bool,
    pub sort: Sort,
    /// Applied after `sort`, so it reverses whichever order was asked for and,
    /// alone, the default one — `ls(1)`'s bargain with `-r`.
    ///
    /// The files turn with the notes: one listing whose halves ran different
    /// ways is not an order anyone asked for.
    pub reverse: bool,
    /// The slug and both timestamps as well as the title.
    ///
    /// Off by default: the slug is the title with the spaces taken out, so the
    /// pair says everything twice, and the stamps are forty columns nobody
    /// asked for. Neither costs anything to read.
    ///
    /// One flag rather than one per column — `ls(1)` settled that a long format
    /// is a density, not a selection. `--json` carries every field either way,
    /// because what a program reads should not depend on a terminal's width.
    pub long: bool,
}

/// What order the notes come out in.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Sort {
    /// What a notebook walk already produces.
    #[default]
    Slug,
    /// Newest first: the question put to a time is nearly always "what is
    /// recent", so it runs the opposite way to `Title`.
    Created,
    Updated,
    /// Alphabetical.
    Title,
}

impl Sort {
    /// For a screen with room to draw all four at once. Written here so a page's
    /// list and the ring [`Sort::next`] walks cannot come apart.
    pub const ALL: [Sort; 4] = [Sort::Slug, Sort::Created, Sort::Updated, Sort::Title];

    /// What `--sort` is spelled with, and what a screen should call it.
    pub fn name(self) -> &'static str {
        match self {
            Sort::Slug => "slug",
            Sort::Created => "created",
            Sort::Updated => "updated",
            Sort::Title => "title",
        }
    }

    /// [`Sort::name`]'s inverse, for the one caller receiving an order as text:
    /// a browser, where it rides in the address.
    pub fn named(said: &str) -> Option<Sort> {
        Sort::ALL.into_iter().find(|sort| sort.name() == said)
    }

    /// For a key with one press and four orders to reach, walking the list
    /// `--sort` lists.
    pub fn next(self) -> Sort {
        match self {
            Sort::Slug => Sort::Created,
            Sort::Created => Sort::Updated,
            Sort::Updated => Sort::Title,
            Sort::Title => Sort::Slug,
        }
    }
}

/// How a listing is written out.
#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// Aligned columns for a person to read.
    #[default]
    Table,
    /// One object, for a program to read.
    Json,
    /// One identifier per line and nothing else.
    Quiet,
}

/// Whether a command that changes a note moves its `updated`.
///
/// `Stamp` is the honest reading of what happened. `Keep` is `--no-touch`, and
/// exists because `updated` is a field you are allowed to own: a typo fixed or
/// a tag added is not the note being rewritten, and an imported note keeps the
/// dates its old system gave it.
///
/// `add` has no say — both fields are written the moment a note is made.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Touch {
    /// Set `updated` to now.
    #[default]
    Stamp,
    /// Leave `updated` exactly as it was found.
    Keep,
}

/// Which half of the notebook to list.
#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub enum Only {
    #[default]
    Everything,
    Notes,
    Files,
}

/// Reads a `TiddlyWiki` 5 export and writes it into the active notebook.
///
/// The reading and the writing are separate on purpose: `import::tiddlywiki`
/// knows what a tiddler is, `import::write` knows what a notebook is, and the
/// next source noda learns is the first of those and none of the second.
/// Several files are one import rather than several. A wiki exported in pieces
/// has links running between the pieces, and a link can only be rewritten
/// against notes that exist by the time it is — so every file is read before
/// anything is written, and one that cannot be read stops the import before it
/// has touched the notebook.
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

    // After the sort, so every order gets a reverse for free.
    if options.reverse {
        notes.reverse();
        files.reverse();
    }

    match options.format {
        Format::Json => return Ok(as_json(&name, &notes, &files)),
        Format::Quiet => return Ok(as_identifiers(&notes, &files, options.null)),
        Format::Table => {}
    }

    // Nothing invents a time, so the column says so rather than leaving a hole.
    let stamp = |value: Option<String>| value.unwrap_or_else(|| "-".to_string());
    let rows: Vec<(String, String, String, String, String, Vec<String>)> = notes
        .into_iter()
        .map(|file| {
            (
                file.id,
                file.slug,
                stamp(file.note.created),
                stamp(file.note.updated),
                file.note.title,
                file.note.tags,
            )
        })
        .collect();

    if rows.is_empty() && files.is_empty() {
        return Ok(String::new());
    }

    let id_width = rows.iter().map(|r| display_width(&r.0)).max().unwrap_or(0);
    let slug_width = rows.iter().map(|r| display_width(&r.1)).max().unwrap_or(0);
    let created_width = rows.iter().map(|r| display_width(&r.2)).max().unwrap_or(0);
    let updated_width = rows.iter().map(|r| display_width(&r.3)).max().unwrap_or(0);
    let title_width = rows.iter().map(|r| display_width(&r.4)).max().unwrap_or(0);
    let mut out = String::new();
    for (id, slug, created, updated, title, tags) in rows {
        // `-l` extends the default row rather than rearranging it: id and title
        // stay the first two columns, so a script cutting fields off the front
        // reads the same thing either way.
        //
        // Tags are last in both, being the one thing a note may not have —
        // anywhere else, their absence would shift every column behind them.
        //
        // The title is the one uncoloured column, which is what makes it the one
        // the eye lands on, and it is the note's own words.
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
        out.push_str(line.trim_end());
        out.push('\n');
    }

    // Under a heading rather than mixed in: with no id, title or tags, a row of
    // theirs would be three empty columns.
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

/// The instant a stamp names, `None` when absent or unreadable; both sort last.
///
/// Parsed rather than compared as text: noda's own stamps would sort as strings,
/// but an imported note carries its old offset, and
/// `2019-03-14T16:21:00+08:00` sorts after `2019-03-14T09:00:00Z` as text while
/// coming before it in fact.
fn instant(stamp: Option<&String>) -> Option<jiff::Timestamp> {
    stamp?.parse().ok()
}

/// Public because the browser offers the same orders, and an order that came out
/// differently by route would be two features wearing one name. The reverse is
/// the caller's, applied afterwards.
pub fn sort_notes(notes: &mut [notebook::NoteFile], sort: Sort) {
    match sort {
        // The walk already sorts by slug.
        Sort::Slug => {}
        Sort::Title => notes.sort_by(|a, b| {
            a.note
                .title
                .cmp(&b.note.title)
                // Two notes may share a title; the id keeps the order stable.
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
}

/// Hand-written rather than derived: five string fields do not justify the
/// supply-chain surface of a serialization crate. The escaping is the part that
/// has to be right, and it is tested.
///
/// Each note carries its filename, because that is what a script needs next and
/// deriving it means knowing noda's naming rule.
fn as_json(notebook: &str, notes: &[notebook::NoteFile], files: &[String]) -> String {
    let mut out = String::from("{\"notebook\":");
    out.push_str(&json_string(notebook));
    out.push_str(",\"notes\":[");
    for (index, file) in notes.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        // Always present: a key that came and went would make every reader
        // test for it.
        let stamp = |value: Option<&String>| match value {
            Some(text) => json_string(text),
            None => "null".to_string(),
        };
        let _ = write!(
            out,
            "{{\"id\":{},\"slug\":{},\"file\":{},\"title\":{},\"created\":{},\"updated\":{},\"tags\":[",
            json_string(&file.id),
            json_string(&file.slug),
            json_string(&note::file_name(&file.id, &file.slug)),
            json_string(&file.note.title),
            stamp(file.note.created.as_ref()),
            stamp(file.note.updated.as_ref()),
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

/// A note's id, a file's name — the one listing addressing its two halves
/// differently, because a file's name is its identity.
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
            // No shorthand, and cannot be written literally.
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

/// Only the block between the `---` lines: the body is the user's prose.
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

/// Adding a tag a note carries, or removing one it lacks, is not an error — it
/// leaves nothing to commit.
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

/// Parsed away from the notes it applies to, so a run aimed at forty is refused
/// before the first is opened rather than halfway through.
#[derive(Debug, Clone, PartialEq, Eq)]
enum TagEdit {
    Add(String),
    Remove(String),
}

fn parse_tags(changes: &[String], key: Option<&str>) -> Result<Vec<TagEdit>> {
    changes
        .iter()
        .map(|change| {
            // The list takes hyphen values so `-q3` is a tag, which means a
            // flag written after them arrives as one more change: `--no-touch`
            // would strip to `-no-touch` and remove a tag nobody has, looking
            // like it worked.
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

/// What a change did to one note, with nothing committed.
///
/// The commit is the caller's: one command is one commit, but a queue from the
/// browser is one commit for the lot, and that difference must not become a
/// second account of what a change *means*.
struct Applied {
    id: String,
    slug: String,
    /// What git has to be told about, relative to the notebook.
    files: Vec<String>,
    /// The line the command prints for this note.
    summary: String,
    /// False when the note already said what it was asked to say.
    changed: bool,
}

impl Applied {
    fn paths(&self) -> Vec<&Path> {
        self.files.iter().map(Path::new).collect()
    }
}

/// Writes the tag changes into one note's file. Nothing is committed.
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

/// `edit` for a caller with no editor to hand — a browser arrives with the text
/// typed. Everything after the write is `edit`'s own, because what a change
/// *means* has one implementation.
///
/// Only the body: the frontmatter is left exactly as found, so a note from
/// another program keeps its arrangement, and the title and tags have their own
/// commands rather than a `<textarea>` full of YAML.
pub fn rewrite_in(notebook: &Notebook, key: &str, body: &str, touch: Touch) -> Result<String> {
    let located = locate(notebook, key)?;
    let before = std::fs::read_to_string(&located.path)?;
    let after = note::set_body(&before, body)
        .ok_or_else(|| Error::msg(format!("{}: not a note", located.path.display())))?;
    std::fs::write(&located.path, &after)?;
    settle(notebook, &located, &before, touch)
}

/// Read it back, refuse it if it is no longer a note, stamp it, commit it.
///
/// Shared because after the write there is nothing to tell `edit` and
/// `rewrite_in` apart.
fn settle(notebook: &Notebook, located: &Located, before: &str, touch: Touch) -> Result<String> {
    let after = std::fs::read_to_string(&located.path)?;
    if after == *before {
        return Ok(format!("{}  (unchanged)", located.slug));
    }

    // A rejected edit stays on disk to be fixed or thrown away, never silently
    // discarded. No id to guard: it is in the filename, which an editor never
    // touches.
    let edited = Note::parse(&after).map_err(|e| {
        Error::msg(format!(
            "{}: {e}\nthe file was left as you saved it and was not committed",
            located.path.display()
        ))
    })?;

    // One field set in place; everything else is committed exactly as saved,
    // including the order the block was just arranged in. Under `--no-touch`
    // nothing is written back over what the editor left, an `updated` it changed
    // itself included.
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

/// Retitles a note. The slug follows the new title; the id never moves.
///
/// Which is why links to it go stale rather than broken — the destination names
/// a dead path and a live id, so `backlinks` still answers. Every reader outside
/// noda sees only the dead path, so a retitle says which notes are in that
/// position and `update_links` rewrites them.
///
/// Opt-in for `file mv --update-links`'s reason: it edits the prose of notes the
/// command was not pointed at. The walk is skipped when the slug does not move.
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

    // Only the slug moves, so identity and history survive untold.
    let was = note::file_name(&located.id, &located.slug);
    let file = note::file_name(&located.id, &slug);
    std::fs::write(notebook.path.join(&file), note.render())?;
    let mut changed = vec![file.clone()];
    let mut retarget = None;
    if slug != located.slug {
        std::fs::remove_file(&located.path)?;
        changed.push(was.clone());
    }

    // Finding out costs a read of every note, so the walk is skipped unless the
    // rename moved something or the flag asked outright — which is how links
    // left stale by an earlier rename get repaired.
    if slug != located.slug || update_links {
        // Taken after the rename, so a self-link is read under the name it has
        // now rather than the one it had a moment ago.
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

/// The commit that removed it stays, so `git revert` brings the note back with
/// its id intact.
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

/// Removes one note's file. Nothing is committed.
fn apply_remove(notebook: &Notebook, key: &str) -> Result<Applied> {
    // Deleting a file does not require understanding it, and refusing would
    // disable the one command that clears up a broken note.
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

/// The two changes that mean something said about a *set* of notes. A retitle
/// needs one title per note and `add` has no set, so neither is here.
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
    /// The line this step contributes to the commit message, and the line the
    /// browser shows for it in the queue. One wording, so what you read before
    /// sending is what the history says afterwards.
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

/// `1 note` / `3 notes`, for the several places that have to say how many.
fn count(n: usize, thing: &str) -> String {
    if n == 1 {
        format!("1 {thing}")
    } else {
        format!("{n} {thing}s")
    }
}

/// Whether a change could be carried out at all, without opening anything.
///
/// What the browser asks before it will put one in its queue: a tag that cannot
/// be written down should be refused where it was typed, not at the end of a
/// sitting when the whole queue is sent.
pub fn check(change: &Change) -> Result<()> {
    match change {
        Change::Tag { changes, .. } => parse_tags(changes, None).map(|_| ()),
        Change::Remove => Ok(()),
    }
}

/// Several changes across several notes, committed at once.
///
/// One commit, because a queue is one intention: twelve commits saying "these
/// twelve notes are no longer q3" bury the fact under the work of carrying it
/// out. The same code writes the same files as `tag` and `rm`, with the commit
/// boundary moved out one level.
///
/// What can be refused is refused before anything is written. What cannot be
/// known in advance — a note deleted from another window mid-queue — is reported
/// without stopping the rest, the earlier changes being on disk by then.
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
                // Neither a failure nor a change — one of the reasons a set is
                // worth acting on at all.
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

/// One step is its own subject; several get a count and a body listing them,
/// which is what `git log --oneline` wants either way.
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

/// Writes the notebook's `README.md` and commits it.
///
/// For the reader outside noda: a git host renders it above the file list, so
/// the first thing anyone meets of a pushed notebook is this or a wall of
/// `k3f9m2p1-*.md`.
///
/// Its own command rather than a flag on `notebook add`, because a notebook
/// wants a README the day it is pushed somewhere people can see, not the day it
/// is created.
///
/// Fixed prose on purpose: every line stays true however many notes arrive. An
/// index would not, and it is the one thing this storage model refuses — nothing
/// in the repository restates the filenames, so a generated list would go stale
/// from the next `noda add` onward.
pub fn readme(paths: &Paths, force: bool) -> Result<String> {
    let notebook = Notebook::open_active(paths)?;
    let path = notebook.path.join(notebook::README_FILE);

    // Prose someone wrote, and the template is not worth losing it for.
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

/// With the notebook's name where a reader would otherwise have to guess it.
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

`created` is set once and never moves; `updated` follows every change. `tags` is optional.
noda reads those four fields and leaves everything else in the block alone, so any other
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

/// Copies files into the active notebook and commits them.
///
/// A wrapped copy, but into a directory only noda can name — the command exists
/// so nothing about a notebook requires knowing where it is.
///
/// It says nothing about notes: which note uses a file is written in that note's
/// prose, and a command taking a note here would say it in two places.
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
        // Only where a name is chosen: manufacturing one that reads as a note
        // is a different question from removing an existing file.
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

/// A commit like any other, so `git revert` brings it back. A note is refused
/// rather than deleted: `rm` is where a note goes, one of them having an
/// identity to lose.
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

/// Renames one of the notebook's files.
///
/// Every link that named the old file now names nothing, so this always says
/// which — `doctor --links`' walk, paid because a rename is rare and the damage
/// is otherwise silent.
///
/// `update_links` rewrites them instead, opt-in because it edits the prose of
/// notes the command was not pointed at. Even then they are re-read: a
/// destination written with backslash escapes cannot be located, and one that
/// was not rewritten is reported rather than assumed fixed.
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
    /// The notes whose bodies were rewritten, by filename.
    rewritten: Vec<String>,
    /// Still naming the old name: not asked for, or out of reach.
    stranded: Vec<String>,
}

impl Retarget {
    /// Nothing when no note ever named the old name.
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
/// them either way.
///
/// The predicate is what the two renames disagree about: an attachment's name is
/// its whole identity, so `file mv` matches the name it just left, while a note
/// keeps its id, so `mv` matches that and catches a destination written two
/// renames ago.
///
/// Every note is read whichever it is — `doctor --links`' cost. Nothing is
/// assumed fixed: a note that was touched is read back, because `link::rewrite`
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
        // The frontmatter is carried byte for byte, so a rename cannot reformat
        // what somebody wrote by hand nor move `updated` on unchanged prose.
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

/// So the tools noda does not wrap can be pointed at it:
/// `pandoc "$(noda path meeting-notes)"`, `cd "$(noda path)"`.
pub fn path(paths: &Paths, key: Option<&str>) -> Result<String> {
    let notebook = Notebook::open_active(paths)?;
    let Some(key) = key else {
        return Ok(format!("{}\n", notebook.path.display()));
    };

    // One key can mean both, and asking twice is a resolve and a `stat`.
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
        // `resolve` already names the candidates for an ambiguous key; only
        // "no such note" needs widening, this command having been asked both.
        (Err(Error::Msg(said)), false) if said.starts_with(notebook::NOT_FOUND) => Err(Error::msg(
            format!("nothing called `{key}` — the notebook holds no note and no file by that name"),
        )),
        (Err(other), false) => Err(other),
    }
}

/// When the name is a note's, say so and name the right command: "no such file"
/// about a file plainly sitting there is the unhelpful version.
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

/// Such a name reads as a note that lost its frontmatter, and `doctor` would
/// report it broken from the moment it appeared.
fn refuse_a_notes_name(name: &str) -> Result<()> {
    if note::names_a_note(name) {
        return Err(Error::msg(format!(
            "{name} claims a note's id — a file noda would then report as a broken note"
        )));
    }
    Ok(())
}

/// One flat directory, so a name is a name and never a path. A leading `.` is
/// refused because noda's walk skips dotfiles: such a file would be committed
/// and then never mentioned by anything that lists what the notebook holds.
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

/// One notebook as `notebook_ls` lays it out, read before any of it is measured.
struct Row {
    name: String,
    /// `None` has nowhere to sync to, which is not the same as having somewhere
    /// and never using it: the column is skipped for the first, padded for the
    /// second.
    remote: Option<String>,
    drift: Option<(usize, usize)>,
}

/// Notebooks, the active one marked `*`, each with where it stands.
///
/// `noda status` speaks only about the active notebook, so one you have not
/// opened in a fortnight can be thirty commits behind with nothing saying so.
/// The browser's shelf has said it all along.
///
/// [`Notebook::drift`] rather than [`Notebook::status`] is what makes it
/// affordable per row — two refs compared, against two walks of the working
/// tree — and nothing goes to the network, so an unreachable remote costs no
/// more than any other.
pub fn notebook_ls(paths: &Paths) -> Result<String> {
    let names = Notebook::list(paths)?;
    if names.is_empty() {
        return Ok(String::new());
    }
    let active = notebook::active_name(paths).ok();

    // Gathered whole first: no column's width is known until every notebook
    // has been asked.
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
        // Muted unless there is something to do: on a dozen rows of facts,
        // `2 to push` is the one the eye must find without reading.
        let words = standing(remote.as_deref(), drift);
        let painted = match drift {
            Some((ahead, behind)) if remote.is_some() && (ahead > 0 || behind > 0) => words,
            _ => style::paint(style::MUTED, &words),
        };
        // The column is absent for such a row, not empty: a `no remote` parked
        // under a heading of URLs reads as a value that went missing.
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

/// Not a commit, and so cannot be undone: the active notebook is refused
/// outright and everything else is confirmed first.
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

/// Silence is no. Piped there is nobody to ask, so it is refused rather than
/// assumed — `--force` is how a script says it meant it.
fn ask_at_the_terminal(question: &str) -> Result<bool> {
    use std::io::IsTerminal;

    if !std::io::stdin().is_terminal() {
        return Err(Error::msg(
            "there is no terminal to confirm at — pass `--force` if you mean it",
        ));
    }
    // The question goes to stderr so that stdout carries only the outcome.
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

/// How much of a matching line to show around the match.
const EXCERPT_WIDTH: usize = 72;
/// How much of it may sit before the match, so the match itself stays visible.
const EXCERPT_LEAD: usize = 28;

/// Case-insensitive substring, not word: a notebook of Chinese or Japanese
/// notes has no spaces to tokenise on. Several terms mean all of them.
pub fn search(paths: &Paths, tokens: &[String]) -> Result<String> {
    let query = Query::parse(tokens)?;
    // Only text terms can point at a line.
    let terms = query.excerpt_terms();

    let notebook = Notebook::open_active(paths)?;
    let mut rows = Vec::new();
    for file in notebook.notes()? {
        // Not the raw file, or `---` and the keys would be searchable text.
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

    // `ls`'s row: a hit is a note, and `ls` settled what a note looks like.
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
        // A hit in the title or the tags is already visible above; only a hit in
        // the body needs to be quoted back.
        if let Some(excerpt) = excerpt {
            out.push_str(&" ".repeat(id_width + 2));
            out.push_str(&excerpt);
            out.push('\n');
        }
    }
    Ok(out)
}

/// The first body line holding a term, cut down to something that fits a
/// terminal, with the match itself picked out.
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
/// change how many bytes a character takes, so the way back to the original is
/// recorded rather than assumed — every offset returned is a char boundary.
///
/// Shared with the browser, which picks the same match out of the same prose:
/// what `search` quotes back on one line, `tui` highlights where it sits.
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

/// Nothing touches the network — the drift is measured against the last fetch.
/// A command for orienting yourself has to work on a train.
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

    // Rows that never say anything are how the ones that do get skipped.
    if status.files > 0 {
        rows.push(("files", status.files.to_string()));
    }
    rows.push(("changes", changes));

    // "0 problems" on every healthy notebook teaches people to skip the line.
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
        // Continuation lines sit under the first, so it still reads as two
        // columns.
        let mut lines = value.lines();
        let _ = writeln!(out, "{}  {}", pad(key, width), lines.next().unwrap_or(""));
        for line in lines {
            let _ = writeln!(out, "{}  {line}", pad("", width));
        }
    }
    Ok(out)
}

/// One kind gets one line, which already says how many. Several get a total
/// first, so the size is legible before the breakdown.
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
    // Detection without a remedy is a trap, so name the command.
    let _ = write!(
        out,
        "{}",
        style::paint(style::MUTED, "run `noda doctor` to look at these")
    );
    out.trim_end().to_string()
}

/// Naming every one is how a wholesale problem puts a line per note on screen.
fn elide(subjects: &[String]) -> String {
    /// Enough to recognise, few enough for one line.
    const SHOWN: usize = 3;

    let mut shown: Vec<&str> = subjects.iter().take(SHOWN).map(String::as_str).collect();
    if subjects.len() > SHOWN {
        shown.push("…");
    }
    shown.join("; ")
}

/// Diagnoses what noda cannot simply act on, and adopts the notes only waiting
/// for an id.
///
/// Nothing derived is left to rebuild — the files *are* the record. What arrives
/// from outside is: a note written by hand, a file copied in, two machines that
/// minted one id without meeting.
///
/// Exactly one of those has a repair that loses nothing: frontmatter without an
/// id is a note that has said what it is and only lacks a name. The other two
/// are reported and left alone, because only their author knows whether to
/// discard an identity or whether a file was ever a note.
///
/// `links` and `times` are flags because they cost a full read of the notebook
/// and a full walk of history. The hooks check needs neither and is here because
/// a hook is the purest thing noda cannot act on: it can neither run nor delete
/// it.
///
/// Where `status` elides, this names every file.
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

    // Free: `scan` already parsed the frontmatter this reads one field of.
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

    // A commit like any other, so an unwanted repair is revertible.
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

/// The ways a link and a file can fail to meet.
///
/// Nothing is repaired: an orphan may be an attachment whose note went or a file
/// parked on purpose, and the only repair is deleting something noda cannot
/// regenerate; a broken link may be a typo or a file not copied in yet.
///
/// A stale link is the one case noda does know the answer to, and is reported
/// anyway — acting on it edits the prose of notes this command was not pointed
/// at, which noda does only when asked in so many words.
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
            // Indented under it: the answer, not another line of the report.
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

/// noda writes and commits in the same breath, so the honest gap is under a
/// second. The allowance covers a slow commit, not a judgement call — an edit
/// made outside noda is discovered minutes or days later, never inside one.
const COMMIT_LAG: i64 = 60;

/// Checked against themselves and against git. The walk of history is what puts
/// this behind a flag.
///
/// Nothing is repaired: a stale `updated` means the note was edited outside
/// noda, and the only fix is overwriting somebody's record with a guess.
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

        // The only witness to a change noda did not make, and it can only say
        // that the file changed — which is all that is claimed here.
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

/// The notes an importer could not finish translating.
///
/// `unconverted:` is the record and this is the handle — `search` reads title,
/// tags and body, so a field it does not know would be written where nothing
/// could find it. A tag would have been findable, but tags belong to whoever
/// writes the notes.
///
/// Nothing is repaired: what the remaining `WikiText` should say is a question
/// only its author can answer.
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

    // What is left, not which notes have it: after an import that can be most
    // of the notebook, burying every other check.
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

    // A cap nobody is told about reads as a complete list.
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

/// The hooks that will never fire.
///
/// Not behind a flag: what makes a check opt-in is its cost, and this reads one
/// directory `doctor` was walking anyway. Out of `Problem` and so out of
/// `status`, because a script left in `.git` is not something the notebook
/// holds.
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
    // Knowing *why* they are dead is what says git would have fired them.
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

/// Detection without a remedy is a trap, and the two noda refuses to settle are
/// where saying nothing leaves someone stuck.
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

/// The drift phrased as what there is left to do.
///
/// One wording, here because three screens say it — and they had already grown
/// two spellings, `never synced` and `never fetched`. The first is accurate: a
/// first push clears that state as readily as a fetch, so a fetch is not what it
/// waits for.
///
/// Plain text, because one of the three renders into HTML; see
/// [`describe_drift`].
pub fn drifted(drift: Option<(usize, usize)>) -> String {
    match drift {
        None => "never synced".to_string(),
        Some((0, 0)) => "in sync".to_string(),
        Some((ahead, 0)) => format!("{ahead} to push"),
        Some((0, behind)) => format!("{behind} to pull"),
        Some((ahead, behind)) => format!("{ahead} to push, {behind} to pull"),
    }
}

/// The same, for a notebook that may have no remote at all — which has not
/// drifted from anything, and for which `never synced` would name a state it can
/// never leave.
pub fn standing(remote: Option<&str>, drift: Option<(usize, usize)>) -> String {
    match remote {
        None => "no remote".to_string(),
        Some(_) => drifted(drift),
    }
}

/// [`drifted`], painted for the one screen with room for the caveat — which is
/// the whole reason `status` is instant: it reports the last sync's news.
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

/// The arrow `status` and the TUI already use for this direction. A margin and
/// not a column: one character on a few rows, where a column would cost a
/// heading and a width on every row to say nothing on most.
///
/// Shared with the TUI's log screen, which draws it through different machinery
/// — the one thing the two must not differ on is the character.
pub const UNPUSHED: &str = "↑";

/// The notebook's history, or one note's.
///
/// **The rows the remote has not seen carry a mark.** `noda status` says how
/// many there are to push, and the question straight after is *which*.
pub fn log(paths: &Paths, key: Option<&str>, max: Option<usize>) -> Result<String> {
    let notebook = Notebook::open_active(paths)?;
    // A note whose frontmatter has gone is precisely the one whose past you
    // want, and the id is in the filename either way.
    let id = match key {
        Some(key) => Some(find(&notebook, key)?.id),
        None => None,
    };

    // Once for the listing, and free with no remote: see `Notebook::unpushed`.
    let unpushed = notebook.unpushed(&notebook.branch()?)?;

    let entries = notebook.log(id.as_deref(), max)?;
    let mut shown = 0;
    let mut out = String::new();
    for entry in &entries {
        let mark = if unpushed.contains(&entry.id) {
            shown += 1;
            style::paint(style::MUTED, UNPUSHED)
        } else {
            // So the ids stay in one column either way.
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

    // `-n` can cut above the oldest unpushed commit, leaving the marks on
    // screen a subset presenting itself as the whole.
    //
    // Whole-notebook only: against one note's log, `unpushed` counts a different
    // set of commits, so the subtraction would produce a number about nothing.
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

/// The notes that link to something — a note, or one of the notebook's files.
///
/// Inbound only, which is why it is not called `links`: what a note points at is
/// in the note, and what points at it is the half nothing could tell you.
///
/// Its own command rather than a flag on `ls`, because `ls` reads a directory
/// and this parses every body. It takes a file as readily as a note, as
/// `noda path` does — the walk that answers either answers both.
pub fn backlinks(paths: &Paths, key: &str, format: Format) -> Result<String> {
    let notebook = Notebook::open_active(paths)?;

    // As `path` resolves it, and for its reason: one key can name both.
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

    // `ls`'s row: there is one shape for naming a note.
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

/// It names its subject, so a script that asked about `meeting-notes` gets the
/// filename actually resolved — the thing a retitle moves.
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
/// Its own command rather than a flag on `ls`, on `deleted`'s precedent: `ls`
/// reads a directory and this parses every body, and one command must not carry
/// two costs that far apart.
///
/// There is no `noda done`. Ticking a box needs an address noda does not have —
/// line numbers move, text prefixes collide, and giving each item an id would
/// make the file a noda-only format, which is what GFM checkboxes avoided.
/// `noda edit <note>` types one `x`.
pub fn todo(paths: &Paths, json: bool) -> Result<String> {
    todo_on(paths, json, &today()?)
}

/// The *local* date, the only kind a due date can be compared against: nobody
/// writes `due:2026-08-10` meaning UTC. Public because the browser decides what
/// is overdue the same way or not at all.
pub fn today() -> Result<String> {
    let (seconds, offset_minutes) = notebook::local_now()?;
    // The date half of a `YYYY-MM-DD HH:MM`, which is ASCII throughout.
    Ok(format_time(seconds, offset_minutes)[..DATE_WIDTH].to_string())
}

/// `todo` with today given explicitly, so a test can say what "overdue" means
/// without freezing the clock — `edit_with`'s shape.
///
/// Getting the zone wrong is not a rounding error: east of UTC an item that went
/// overdue at midnight would stay unmarked until morning, which is exactly when
/// a todo list is read.
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

    // Before the empty check: an empty list is an answer.
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
        // Never truncated: a cut-off action item has to be opened to read.
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

/// `due` is carried and `overdue` is not: a program has its own clock and its
/// own idea of which day it is in.
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

/// `log`'s columns in `log`'s order. No line numbers — in prose the unit
/// somebody is looking for is a paragraph, not a row. Body only; `blame` says
/// why.
pub fn blame(paths: &Paths, key: &str) -> Result<String> {
    let notebook = Notebook::open_active(paths)?;
    // The filename is enough, as for `log` and `diff`.
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
            // Padded to the width of a time so the prose stays in one column.
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

/// The notes history holds that the notebook no longer does, newest loss first.
///
/// Its own command rather than a flag on `ls`: `ls` reads a directory and this
/// walks all of history, and every column `ls` prints describes something that
/// exists — a deleted note's title is a different claim under the same heading.
///
/// The revision printed is the deleting commit's *parent*, because that is what
/// `restore` needs. Leaving the `~1` to be worked out would be a problem
/// reported without its remedy.
pub fn deleted(paths: &Paths, notebook_name: Option<&str>, json: bool) -> Result<String> {
    let name = match notebook_name {
        Some(name) => name.to_string(),
        None => notebook::active_name(paths)?,
    };
    let notebook = Notebook::open(paths, &name)?;
    let gone = notebook.deleted()?;

    // Before the empty check: an empty list is an answer.
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

/// Hand-written, for `as_json`'s reason.
///
/// Ids in full, because an abbreviation can one day stop being unique. Times in
/// UTC, so a program does not have to take a position on whose clock it was.
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

/// The spelling noda writes everywhere, so a script never meets two.
fn rfc3339(seconds: i64) -> String {
    jiff::Timestamp::from_second(seconds)
        .map(|time| time.strftime("%Y-%m-%dT%H:%M:%SZ").to_string())
        .unwrap_or_default()
}

/// Uncommitted changes, or what the last commit changed. A plain unified diff
/// with nothing wrapped around it, so `git apply` will take it.
///
/// `remote` asks **what a push would carry** instead — see
/// [`Notebook::diff_remote`] — the third layer after `status`'s count and
/// `log`'s margin. A flag rather than its own command, because what comes back
/// is a patch either way.
pub fn diff(paths: &Paths, key: Option<&str>, remote: bool) -> Result<String> {
    let notebook = Notebook::open_active(paths)?;
    // Seeing what changed is how you find out why it will not parse.
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

/// As a new commit: the restore moves history forward like every other
/// change.
pub fn restore(paths: &Paths, key: &str, rev: &str, touch: Touch) -> Result<String> {
    let notebook = Notebook::open_active(paths)?;
    let commit = notebook.revision(rev)?;

    // Found without being read: `restore` is about to write over the file, and
    // refusing on a broken one makes the command for undoing damage another
    // casualty of it.
    let current = find(&notebook, key).ok();
    // From the filename, or from history — which is how this undoes `noda rm`.
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

    // Only the contents travel back; a note still here keeps today's name, and
    // a note that is gone returns under the one it had.
    let slug = match &current {
        Some(found) => found.slug.clone(),
        None => slug_then,
    };
    let path = notebook.note_path(&id, &slug);

    let restored = Note::parse(&text)
        .map_err(|e| Error::msg(format!("the copy of `{key}` at {rev} cannot be read: {e}")))?;
    // Held aside, or restoring the same revision twice would never say "no
    // change": the first restore writes a timestamp history cannot have.
    // `--no-touch` removes the reason and the exception together.
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

    // The file changed just now, whatever history says about itself. `created`
    // is left alone: the note is the same one it always was.
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
    // Redacted even though it was just typed: this is the line that stays in
    // the scrollback.
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

/// Commit, pull, push — in that order, so local work is never left behind by a
/// merge and the push always carries it.
pub fn sync(paths: &Paths) -> Result<String> {
    sync_in(&Notebook::open_active(paths)?)
}

/// `sync`, in a notebook the caller already has open.
///
/// The order is the whole of it, and why the browser calls this rather than the
/// three steps: a pull before the commit merges into a tree missing what
/// somebody just typed, and a push before the pull is refused.
pub fn sync_in(notebook: &Notebook) -> Result<String> {
    let mut lines = Vec::new();
    if notebook.commit_all("sync: local changes")? {
        lines.push("commit: local changes".to_string());
    }
    lines.push(notebook.pull()?);
    lines.push(notebook.push()?);
    Ok(lines.join("\n"))
}

/// Marks the notebook as it stands, so that moment can be named later.
///
/// Commits the working tree first, on `sync`'s terms: a snapshot that quietly
/// left out what is on disk would be a snapshot of something nobody has. The
/// message defaults to the name, because inventing prose on somebody's behalf is
/// worse than repeating what they said.
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

/// `deleted`'s three leading columns in its order, answering the same question
/// about a different kind of thing.
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

/// A note reference resolved to a file, the reading kept separate: a command
/// that only needs to know *which* note has no business failing on a file it is
/// not going to read.
struct Found {
    /// From the filename, so an unparseable file still says which note it is.
    id: String,
    slug: String,
    path: PathBuf,
    /// Kept rather than thrown, so a command that needs the note fails with the
    /// parse error itself.
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

/// A note reference resolved to everything the commands that read a note need.
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

/// For the callers lining something else up against it.
pub const TIME_WIDTH: usize = "0000-00-00 00:00".len();

/// How wide a `due:` date prints, for the rows that have none.
pub const DATE_WIDTH: usize = "0000-00-00".len();

/// In the zone the commit was made in, as git does. Absolute rather than "3
/// days ago": testable without freezing the clock, and it sorts. Public because
/// the browser prints the same commits down the same columns.
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

/// Howard Hinnant's `civil_from_days`. Fifteen lines beats a date dependency.
///
/// The casts drop a sign that cannot be there — the algorithm yields a month in
/// `1..=12` and a day in `1..=31`, which
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

/// The padding goes *outside* the escape sequences: [`display_width`] would
/// otherwise measure them, and the caller's tail trim cannot reach spaces parked
/// before a reset.
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

/// Trimmed, because the frontmatter reads back trimmed, and refused when they
/// carry something it cannot round-trip.
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

/// Opens `$EDITOR` on a scratch file and returns what was written. The buffer
/// lives in the cache dir, never in the notebook, so an abandoned edit can't
/// leave a stray file in the repo.
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

    /// **The list a screen draws and the ring a key walks are one list.** An
    /// order added to one and not the other is not a compile error — it is a
    /// browser quietly offering three of the four.
    #[test]
    fn every_order_is_on_the_ring_the_key_walks() {
        let mut at = Sort::default();
        for sort in Sort::ALL {
            assert_eq!(at, sort, "the ring and the list came apart");
            at = at.next();
        }
        assert_eq!(at, Sort::default(), "the ring stopped coming round");
    }

    /// The browser receives an order as text, under the name `--sort` accepts.
    /// Anything else is a hand-edited address.
    #[test]
    fn an_order_is_read_back_out_of_the_name_it_is_written_with() {
        for sort in Sort::ALL {
            assert_eq!(Sort::named(sort.name()), Some(sort));
        }
        assert_eq!(Sort::named("newest"), None);
        assert_eq!(Sort::named(""), None);
    }

    /// What `todo` gets wrong asking UTC what day it is: already the 3rd in
    /// Taipei, still the 2nd in London, and the eight hours between the two
    /// answers are the morning.
    #[test]
    fn a_local_date_is_not_the_utc_one() {
        // 2026-08-02T23:00:00Z.
        let instant = 1_785_711_600;
        assert_eq!(&format_time(instant, 480)[..10], "2026-08-03");
        assert_eq!(&format_time(instant, 0)[..10], "2026-08-02");
        // And west of UTC the error runs the other way: still the 2nd there.
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
        // Already at or past the target width: never truncate, never panic.
        assert_eq!(pad("reading-log", 4), "reading-log");
    }

    /// Three callers say this sentence, and one of them saying `never fetched`
    /// while another said `never synced` is what this stops happening again.
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

    /// `never synced` would name a state a remoteless notebook can never leave.
    /// The two answers come apart only here.
    #[test]
    fn a_notebook_with_no_remote_is_not_merely_unsynced() {
        assert_eq!(drifted(None), "never synced");
        assert_eq!(standing(None, None), "no remote");
        // No remote outranks the drift: nowhere to push to is nowhere to be
        // two commits from.
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

        // İ is one char and two lowered, so the byte length changes; the offsets
        // must still land on the original's char boundaries.
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

        // A short line is quoted whole, with nothing to elide.
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

        // A file that is not a note is passed through rather than mangled.
        assert_eq!(dim_frontmatter("no frontmatter\n"), "no frontmatter\n");
        assert_eq!(
            dim_frontmatter("---\nunterminated\n"),
            "---\nunterminated\n"
        );
    }

    /// The text under the escape sequences.
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
        // The same instant, written in London and in Taipei.
        assert_eq!(format_time(1_785_073_605, 0), "2026-07-26 13:46");
        assert_eq!(format_time(1_785_073_605, 480), "2026-07-26 21:46");
    }

    #[test]
    fn every_day_lands_on_a_real_calendar_date() {
        // The unsigned casts in `civil_from_days` are safe only because a
        // negative month or day is impossible. Four centuries say so out loud.
        let mut previous = None;
        for days in -73_000..=73_000 {
            let (year, month, day) = civil_from_days(days);
            assert!((1..=12).contains(&month), "day {days} gave month {month}");
            assert!((1..=31).contains(&day), "day {days} gave day {day}");
            // And the calendar only ever moves forwards.
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
        // A leap day in a year divisible by 400, and the second before the epoch.
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
