//! Command implementations. Each one takes `Paths` explicitly so tests can run
//! against a throwaway root without touching the real environment.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::config::{self, Config};
use crate::link;
use crate::note::{self, Note};
use crate::notebook::{self, Notebook, Problem};
use crate::paths::Paths;
use crate::query::Query;
use crate::remote;
use crate::style;
use crate::todo;
use crate::{Error, Result};

/// Name of the notebook `noda init` creates when config does not say otherwise.
pub const DEFAULT_NOTEBOOK: &str = config::DEFAULT_NOTEBOOK;

/// Scratch file used when composing a note in `$EDITOR`.
const EDIT_FILE: &str = "NOTE_EDITMSG.md";

/// Creates the XDG directories, a default notebook, and points `active` at it.
/// Safe to run more than once.
pub fn init(paths: &Paths) -> Result<String> {
    paths.create_dirs()?;
    let mut lines = Vec::new();

    // A config full of commented-out defaults changes nothing, but it is the
    // only way anyone finds out what can be set.
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

/// Shows every setting, its effective value, and where that value came from —
/// which is the question people actually have when an editor is not the one
/// they expected.
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

/// Opens `config.toml` in the editor, writing the starter template first if the
/// file is not there — nobody wants to be dropped into an empty buffer.
pub fn config_edit(paths: &Paths) -> Result<String> {
    Config::write_template(paths)?;
    let path = paths.config_dir().join("config.toml");
    run_editor(&configured_editor(paths), &path)?;
    // Reading it back turns a typo into an error now rather than at the next
    // command, when the connection to this edit would be lost.
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
    vec![
        ("editor".to_string(), editor, editor_source),
        ("author".to_string(), author, author_source),
        ("notebook".to_string(), notebook, notebook_source),
    ]
}

/// The identity commits are made under, and where it came from.
fn author(paths: &Paths, config: &Config) -> (String, config::Source) {
    if let Some(author) = config.get("author") {
        return (author.to_string(), config::Source::File);
    }
    // The notebook's own repo config, then the user's global one — whatever git
    // itself would use, asked in the same order.
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

    // Checked before the editor opens: nobody should compose a note only to be
    // told afterwards that its title or its tags cannot be written down.
    if let Some(title) = title {
        note::validate_title(title)?;
    }
    let tags = clean_tags(tags)?;

    let body = match content {
        Some(text) => text.to_string(),
        None => compose_in_editor(paths, title)?,
    };
    let title = match title {
        Some(t) if !t.trim().is_empty() => t.trim().to_string(),
        _ => derive_title(&body)
            .ok_or_else(|| Error::msg("aborted: the note is empty, so it has no title"))?,
    };

    // Two notes may share a slug: the id in front of it keeps the filenames
    // apart, which is what git needs. `resolve` asks for an id when a slug turns
    // out to name more than one note.
    let slug = note::slugify(&title);
    let id = note::mint_id(&notebook.taken_ids()?);
    // Both, with the same value: a note that has never been changed has been
    // changed as recently as it was made. Writing only one would buy a tidier
    // file at the cost of every reader having to know the other is implied.
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

/// What `ls` was asked for. A struct rather than a row of arguments because the
/// shapes multiply: three formats times three subsets, and every caller cares
/// about two of them at most.
#[derive(Default)]
pub struct List<'a> {
    /// List another notebook instead of the active one.
    pub notebook: Option<&'a str>,
    /// Only notes carrying this tag. Anything more selective than one tag is
    /// `search`'s job — the query language lives there so that it lives in one
    /// place.
    pub tag: Option<&'a str>,
    pub format: Format,
    pub only: Only,
    /// Separate the identifiers `Format::Quiet` prints with NUL rather than a
    /// newline. A file's name may contain a space, and this is what makes
    /// `noda ls -q0 | xargs -0` correct rather than nearly correct.
    pub null: bool,
    pub sort: Sort,
    /// Show `created` and `updated` in the table. Off by default: the two of
    /// them are forty columns wide, which is the whole reason this is a flag
    /// rather than the default — reading them costs nothing, since `ls` has
    /// already parsed the frontmatter to get the title.
    ///
    /// `--json` carries them either way. What a program reads should not depend
    /// on a flag meant for what fits on a terminal.
    pub time: bool,
}

/// What order the notes come out in.
#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub enum Sort {
    /// By slug, which is what a notebook walk already produces.
    #[default]
    Slug,
    /// Newest first — the question put to a time is nearly always "what is
    /// recent", so these run the opposite way to `Title`.
    Created,
    Updated,
    /// Alphabetical.
    Title,
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

/// Which half of the notebook to list.
#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub enum Only {
    #[default]
    Everything,
    Notes,
    Files,
}

/// Lists notes as `id  slug  title  tags`, aligned.
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

    // Asking for one tag is asking about notes, so the notebook's other files
    // are not an answer to it.
    let files = if tag.is_some() || options.only == Only::Notes {
        Vec::new()
    } else {
        files
    };

    match options.format {
        Format::Json => return Ok(as_json(&name, &notes, &files)),
        Format::Quiet => return Ok(as_identifiers(&notes, &files, options.null)),
        Format::Table => {}
    }

    // A note may have no times at all — nothing invents one, so the column says
    // so rather than leaving a hole the eye has to measure.
    let stamp = |value: Option<String>| value.unwrap_or_else(|| "-".to_string());
    let rows: Vec<(String, String, String, String, String, String)> = notes
        .into_iter()
        .map(|file| {
            (
                file.id,
                file.slug,
                stamp(file.note.created),
                stamp(file.note.updated),
                file.note.title,
                file.note.tags.join(", "),
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
    let mut out = String::new();
    for (id, slug, created, updated, title, tags) in rows {
        // The fixed-width columns first, the ones that grow with the prose
        // after: a title runs to whatever length it runs to, and putting a
        // column behind it would leave nothing lined up.
        let mut line = format!("{}  {}", pad(&id, id_width), pad(&slug, slug_width));
        if options.time {
            let _ = write!(
                line,
                "  {}  {}",
                pad(&created, created_width),
                pad(&updated, updated_width)
            );
        }
        let _ = write!(line, "  {title}");
        if !tags.is_empty() {
            line.push_str("  [");
            line.push_str(&tags);
            line.push(']');
        }
        out.push_str(line.trim_end());
        out.push('\n');
    }

    // The notebook's other files, under a heading rather than mixed in: they
    // have no id, no title and no tags, so a row of theirs would be three empty
    // columns. The heading only appears when there is something under it.
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

/// The instant a stamp names, for ordering. `None` when the field is absent or
/// cannot be read; both sort last.
///
/// Parsed rather than compared as text. noda's own stamps are fixed-width UTC
/// and would sort correctly as strings, but an imported note carries the offset
/// its own system used — and `2019-03-14T16:21:00+08:00` sorts after
/// `2019-03-14T09:00:00Z` as text while coming before it in fact.
fn instant(stamp: Option<&String>) -> Option<jiff::Timestamp> {
    stamp?.parse().ok()
}

fn sort_notes(notes: &mut [notebook::NoteFile], sort: Sort) {
    match sort {
        // Already in this order: the walk sorts by slug before returning.
        Sort::Slug => {}
        Sort::Title => notes.sort_by(|a, b| {
            a.note
                .title
                .cmp(&b.note.title)
                // Two notes may share a title. The id is what tells them apart,
                // and using it keeps the order the same from one run to the next.
                .then_with(|| a.id.cmp(&b.id))
        }),
        Sort::Created | Sort::Updated => notes.sort_by_cached_key(|file| {
            let stamp = match sort {
                Sort::Created => file.note.created.as_ref(),
                _ => file.note.updated.as_ref(),
            };
            // Negated for newest-first, and `None` mapped beyond every real
            // instant so a note with no time to sort by sorts last.
            (
                instant(stamp).map_or(i128::MAX, |t| -t.as_nanosecond()),
                file.id.clone(),
            )
        }),
    }
}

/// The listing as one JSON object, on one line.
///
/// Hand-written rather than derived: noda has no serialization crate, and one
/// object of five string fields does not justify adding the supply-chain
/// surface of one. The escaping is the part that has to be right, and it is
/// tested.
///
/// Each note carries its filename as well as its id and slug, because that is
/// what a script actually needs next and deriving it means knowing noda's naming
/// rule.
fn as_json(notebook: &str, notes: &[notebook::NoteFile], files: &[String]) -> String {
    let mut out = String::from("{\"notebook\":");
    out.push_str(&json_string(notebook));
    out.push_str(",\"notes\":[");
    for (index, file) in notes.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        // Always present, `null` when the note carries no such time. A key that
        // came and went with the data would make every reader test for it.
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

/// One identifier per record: a note's id, a file's name.
///
/// A file has no id — its name is its identity — so this is the one listing
/// where the two halves are addressed differently, and both are what the
/// commands that take them expect.
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
            // Everything else below a space has no shorthand and cannot be
            // written literally.
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

/// Pushes the frontmatter into the background so the note itself reads first.
/// Only the block between the opening `---` lines is touched: the body is the
/// user's prose, and noda has no business colouring that.
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

/// Applies `+tag` / `-tag` changes to a note and commits the result.
/// Adding a tag a note already carries, or removing one it lacks, is not an
/// error — it just leaves nothing to commit.
pub fn tag(paths: &Paths, key: &str, changes: &[String]) -> Result<String> {
    let notebook = Notebook::open_active(paths)?;
    let located = locate(&notebook, key)?;
    let mut note = located.note;
    let before = note.tags.clone();

    for change in changes {
        if let Some(name) = change.strip_prefix('+') {
            let name = name.trim();
            if name.is_empty() {
                return Err(Error::msg("`+` needs a tag name after it"));
            }
            note::validate_tag(name)?;
            if !note.tags.iter().any(|t| t == name) {
                note.tags.push(name.to_string());
            }
        } else if let Some(name) = change.strip_prefix('-') {
            let name = name.trim();
            if name.is_empty() {
                return Err(Error::msg("`-` needs a tag name after it"));
            }
            note.tags.retain(|t| t != name);
        } else {
            return Err(Error::msg(format!(
                "tags must be given as `+{change}` to add or `-{change}` to remove"
            )));
        }
    }

    if note.tags == before {
        return Ok(format!(
            "{}  (no change)",
            summary(&located.id, &located.slug, &note.tags)
        ));
    }

    note.updated = Some(note::now());
    std::fs::write(&located.path, note.render())?;
    notebook.commit(
        &[Path::new(&note::file_name(&located.id, &located.slug))],
        &format!("tag: {}", located.slug),
    )?;
    Ok(summary(&located.id, &located.slug, &note.tags))
}

/// Opens a note in `$EDITOR` and commits whatever was saved.
pub fn edit(paths: &Paths, key: &str) -> Result<String> {
    edit_with(paths, key, &configured_editor(paths))
}

/// `edit`, with the editor given explicitly. Exists so tests can drive the
/// command without mutating process-wide environment variables.
pub fn edit_with(paths: &Paths, key: &str, editor: &str) -> Result<String> {
    let notebook = Notebook::open_active(paths)?;
    let located = locate(&notebook, key)?;
    let before = std::fs::read_to_string(&located.path)?;

    run_editor(editor, &located.path)?;

    let after = std::fs::read_to_string(&located.path)?;
    if after == before {
        return Ok(format!("{}  (unchanged)", located.slug));
    }

    // A rejected edit stays on disk to be fixed or thrown away with
    // `git checkout`, never silently discarded.
    //
    // There is no id to guard here any more. It is in the filename, which an
    // editor never touches, so an edit cannot change which note this is — the
    // one thing this command used to have to check for by hand.
    let edited = Note::parse(&after).map_err(|e| {
        Error::msg(format!(
            "{}: {e}\nthe file was left as you saved it and was not committed",
            located.path.display()
        ))
    })?;

    // `edit` is how a note is usually changed, so it is where `updated` would
    // most often be wrong if noda left it alone. One field is set in place —
    // everything else is committed exactly as it was saved, including the order
    // the block was just arranged in.
    let stamped = note::set_field(&after, "updated", &note::now())
        .expect("the note parsed, so it has a frontmatter block");
    if stamped != after {
        std::fs::write(&located.path, &stamped)?;
    }

    notebook.commit(
        &[Path::new(&note::file_name(&located.id, &located.slug))],
        &format!("edit: {}", located.slug),
    )?;
    Ok(summary(&located.id, &located.slug, &edited.tags))
}

/// Retitles a note. The slug follows the new title; the id never moves.
///
/// Which is why the links to it are stale rather than broken: the destination
/// names a path that is gone and an id that is not, so `backlinks` still answers
/// and `doctor --links` says what the link should have said. Every Markdown
/// reader outside noda sees only the dead path, so a retitle says which notes
/// are in that position, and `update_links` rewrites them.
///
/// Opt-in for the reason `file mv --update-links` is: it edits the prose of
/// notes the command was not pointed at, which nothing else in noda does. The
/// walk that finds them is skipped entirely when the slug does not move — a
/// retitle that leaves the filename alone breaks nothing to report.
pub fn mv(paths: &Paths, key: &str, new_title: &str, update_links: bool) -> Result<String> {
    let notebook = Notebook::open_active(paths)?;
    let located = locate(&notebook, key)?;
    let mut note = located.note;

    let title = new_title.trim();
    if title.is_empty() {
        return Err(Error::msg("a note needs a title"));
    }
    note::validate_title(title)?;

    let slug = note::slugify(title);
    note.title = title.to_string();
    note.updated = Some(note::now());

    // Only the slug half of the filename moves. The id stays, so the note keeps
    // its identity and its history across the rename without anything having to
    // be told about it.
    let was = note::file_name(&located.id, &located.slug);
    let file = note::file_name(&located.id, &slug);
    std::fs::write(notebook.path.join(&file), note.render())?;
    let mut changed = vec![file.clone()];
    let mut retarget = None;
    if slug != located.slug {
        std::fs::remove_file(&located.path)?;
        changed.push(was.clone());
    }

    // A retitle that leaves the filename alone strands nothing, and finding that
    // out costs a read of every note — so the walk is skipped unless the rename
    // moved something or `--update-links` asked for it outright. Asking for it
    // on a retitle that renames nothing is how links left stale by an earlier
    // rename get repaired.
    if slug != located.slug || update_links {
        // The inventory is taken after the rename, so a note that links to
        // itself is read under the name it has now and rewritten from what is on
        // disk rather than from what was there a moment ago.
        let (notes, _) = notebook.inventory()?;
        let id = note::normalize_id(&located.id);
        let found = retarget_links(
            &notebook,
            &notes,
            |target| notebook::linked_note_id(target).as_deref() == Some(id.as_str()),
            &file,
            update_links,
        )?;
        changed.extend(found.rewritten.iter().cloned());
        retarget = Some(found);
    }

    // A note that linked to itself is in the list twice: once as the file that
    // was renamed, once as a body that was rewritten.
    changed.sort();
    changed.dedup();
    let files: Vec<&Path> = changed.iter().map(Path::new).collect();
    notebook.commit(&files, &format!("mv: {} -> {slug}", located.slug))?;

    // Named by id rather than by the filename this rename just left: a link can
    // be two renames behind, and the id is the half of the name that never was.
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

/// Deletes a note. The file goes, but the commit that removed it does not, so
/// `git revert` brings the note back with its id intact.
pub fn rm(paths: &Paths, key: &str) -> Result<String> {
    let notebook = Notebook::open_active(paths)?;
    // Deleting a file does not require understanding it. Refusing to remove a
    // note whose frontmatter is gone leaves the one command that could clear it
    // up unusable exactly when it is wanted.
    let found = find(&notebook, key)?;

    std::fs::remove_file(&found.path)?;
    notebook.commit(
        &[Path::new(&note::file_name(&found.id, &found.slug))],
        &format!("rm: {}", found.slug),
    )?;

    let tags = found
        .note
        .as_ref()
        .map(|note| note.tags.clone())
        .unwrap_or_default();
    Ok(format!(
        "removed  {}",
        summary(&found.id, &found.slug, &tags)
    ))
}

/// Copies files into the active notebook and commits them.
///
/// A notebook is a directory, so this wraps a copy — but the directory is one
/// noda chose and only noda can name, and sending someone to find it themselves
/// is sending them to operate the storage by hand. The command exists so that
/// nothing about a notebook requires knowing where it is.
///
/// It says nothing about notes. Which note uses a file is written in that note's
/// prose as an ordinary Markdown link, and a command that took a note here would
/// put that relationship in two places at once.
pub fn file_add(paths: &Paths, sources: &[PathBuf], rename: Option<&str>) -> Result<String> {
    if rename.is_some() && sources.len() > 1 {
        return Err(Error::msg(
            "`--as` renames one file, so it cannot be given with several",
        ));
    }
    let notebook = Notebook::open_active(paths)?;

    // Every source is checked before any of them is copied: a half-done copy
    // would leave the notebook in a state nobody asked for, and the commit that
    // followed would record it.
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
        // Only where a name is being chosen: refusing to *manufacture* one that
        // reads as a note is not the same question as whether an existing file
        // may be removed.
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

/// Removes one of the notebook's files. A commit like any other, so `git revert`
/// brings it back.
///
/// A note is refused rather than deleted: `rm` is where a note goes, and the two
/// are not interchangeable — one of them has an identity to lose.
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
/// which — the walk that finds out is the same one `doctor --links` pays for,
/// and a rename is rare enough to pay it rather than leave the damage silent.
///
/// `update_links` rewrites those links instead of reporting them. It is opt-in
/// because it edits the prose of notes the command was not pointed at, which
/// nothing else in noda does. Even then the notes are re-read afterwards: a
/// destination written with backslash escapes cannot be located in the source,
/// and one that could not be rewritten is reported rather than assumed fixed.
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
    /// The notes that name the old name still — because the rewrite was not
    /// asked for, or because it could not reach them.
    stranded: Vec<String>,
}

impl Retarget {
    /// The lines a rename adds under its own, or nothing when no note ever
    /// named the old name.
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

/// Follows the links that `names` accepts now that the thing they name is
/// called `new`: rewrites them when `update_links` says so, and reports them
/// either way.
///
/// The predicate is what the two renames disagree about. An attachment's name is
/// the whole of its identity, so `file mv` matches the name it just left. A note
/// keeps its id across every retitle, so `mv` matches on that instead and
/// catches a destination written two renames ago — the one an exact-name match
/// silently walks past, leaving it stale with nothing having said so.
///
/// Every note is read whichever it is — to rewrite the links, or to say which
/// ones still point at the old name. That is `doctor --links`' cost, paid here
/// because a rename is rare and the damage it does is otherwise silent.
///
/// Nothing is assumed fixed. `link::rewrite` cannot locate a destination written
/// with backslash escapes, so a note it touched is read back rather than trusted,
/// and one it could not reach is reported like a note that was never rewritten.
fn retarget_links(
    notebook: &Notebook,
    notes: &[notebook::NoteFile],
    names: impl Fn(&str) -> bool,
    new: &str,
    update_links: bool,
) -> Result<Retarget> {
    // A destination that already reads as `new` names the thing correctly, so it
    // is neither rewritten nor reported.
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
        // Only the body is Markdown. The frontmatter is carried over byte for
        // byte rather than re-rendered, so a rename cannot reformat what
        // somebody wrote by hand — nor move `updated` on a note whose prose is
        // the same prose it always was, pointing somewhere it can be followed.
        let Some((_, body)) = note::split_frontmatter(&text) else {
            stranded.push(name);
            continue;
        };
        // One pass per spelling: a note can name the same note by two names it
        // has had, and each is a different string to find in the source.
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

/// Prints where something lives, so the tools noda does not wrap can be pointed
/// at it: `pandoc "$(noda path meeting-notes)"`, `open "$(noda path
/// diagram.png)"`, `cd "$(noda path)"`.
///
/// Exposing the location on request is not the same as making somebody build it
/// themselves, which is what the absence of this command used to require.
pub fn path(paths: &Paths, key: Option<&str>) -> Result<String> {
    let notebook = Notebook::open_active(paths)?;
    let Some(key) = key else {
        return Ok(format!("{}\n", notebook.path.display()));
    };

    // A note is addressed by id or slug and a file by the name it was given, so
    // one key can in principle mean both. Resolution reads no file, and the file
    // is one `stat`, so asking both costs nothing worth avoiding.
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
        // An ambiguous key is `resolve`'s to explain: it already names the
        // candidates. Only "no such note" needs widening, because this command
        // was asked about a file just as much as about a note.
        (Err(Error::Msg(said)), false) if said.starts_with(notebook::NOT_FOUND) => Err(Error::msg(
            format!("nothing called `{key}` — the notebook holds no note and no file by that name"),
        )),
        (Err(other), false) => Err(other),
    }
}

/// The error for a file the notebook does not hold. When the name is a note's,
/// it says so and names the command that wanted it instead — the two are not
/// interchangeable, and being told only "no such file" about a file plainly
/// sitting there is the unhelpful version.
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

/// Refuses a name that would read as a note which had lost its frontmatter, and
/// which `doctor` would then report as broken from the moment it appeared.
fn refuse_a_notes_name(name: &str) -> Result<()> {
    if let Some(stem) = name.strip_suffix(".md")
        && note::split_stem(stem).is_some()
    {
        return Err(Error::msg(format!(
            "{name} claims a note's id — a file noda would then report as a broken note"
        )));
    }
    Ok(())
}

/// What a file in the notebook may be called.
///
/// A notebook is one flat directory, so a name is a name and never a path. A
/// leading `.` is refused because noda's walk skips dotfiles — a file added
/// under one would be committed and then never mentioned again by any command
/// that lists what the notebook holds.
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

/// Lists notebooks, marking the active one with `*` and showing any remote.
pub fn notebook_ls(paths: &Paths) -> Result<String> {
    let names = Notebook::list(paths)?;
    if names.is_empty() {
        return Ok(String::new());
    }
    let active = notebook::active_name(paths).ok();
    let width = names.iter().map(|n| display_width(n)).max().unwrap_or(0);

    let mut out = String::new();
    for name in names {
        let marker = if active.as_deref() == Some(&name) {
            '*'
        } else {
            ' '
        };
        let remote = Notebook::open(paths, &name)
            .ok()
            .and_then(|notebook| notebook.remote_url())
            .unwrap_or_default();
        let line = format!("{marker} {}  {remote}", pad(&name, width));
        out.push_str(line.trim_end());
        out.push('\n');
    }
    Ok(out)
}

/// Deletes a notebook's local repository. Unlike removing a note this is not a
/// commit and cannot be undone, so the active notebook is refused outright and
/// everything else is confirmed first.
pub fn notebook_rm(paths: &Paths, name: &str, force: bool) -> Result<String> {
    notebook_rm_confirmed(paths, name, force, ask_at_the_terminal)
}

/// `notebook_rm`, with the answer supplied. Exists so tests can decide without a
/// terminal to type at.
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

/// Asks on the terminal, and takes silence for no. Piped or scripted there is
/// nobody to ask, so the deletion is refused rather than assumed — `--force` is
/// how a script says it meant it.
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

/// Full-text search across the active notebook.
///
/// Matching is case-insensitive and by substring, not by word: a notebook of
/// Chinese or Japanese notes has no spaces to tokenise on, and a tokeniser would
/// simply fail to find anything in it. Several terms mean all of them, anywhere
/// in the note.
pub fn search(paths: &Paths, tokens: &[String]) -> Result<String> {
    let query = Query::parse(tokens)?;
    // A `tag:` or an `id:` matched something the body does not contain, so only
    // the text terms can point at a line.
    let terms = query.excerpt_terms();

    let notebook = Notebook::open_active(paths)?;
    let mut rows = Vec::new();
    for file in notebook.notes()? {
        // The note's own fields, not the raw file — otherwise `---` and the
        // frontmatter keys would be searchable text, and they are the container,
        // not the note.
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

    let id_width = rows.iter().map(|r| display_width(&r.0)).max().unwrap_or(0);
    let slug_width = rows.iter().map(|r| display_width(&r.1)).max().unwrap_or(0);
    let mut out = String::new();
    for (id, slug, title, tags, excerpt) in rows {
        let mut line = format!(
            "{}  {}  {title}",
            pad(&id, id_width),
            pad(&slug, slug_width)
        );
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
fn find_ignoring_case(haystack: &str, needle: &str) -> Option<(usize, usize)> {
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

/// Where the active notebook stands, in one screen.
///
/// Nothing here touches the network: the drift is measured against what the
/// last fetch left behind. A command for orienting yourself has to work on a
/// train, and has to be instant.
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

    // Only when the notebook holds some: `files 0` on every notebook that keeps
    // nothing but notes is a row that never says anything, and rows that never
    // say anything are how the ones that do get skipped.
    if status.files > 0 {
        rows.push(("files", status.files.to_string()));
    }
    rows.push(("changes", changes));

    // Only when there is something to say: a row that reads "0 problems" on
    // every healthy notebook teaches people to skip the line that matters.
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
        // A value may run to several lines; they line up under the first rather
        // than under the label, so the table still reads as two columns.
        let mut lines = value.lines();
        let _ = writeln!(out, "{}  {}", pad(key, width), lines.next().unwrap_or(""));
        for line in lines {
            let _ = writeln!(out, "{}  {line}", pad("", width));
        }
    }
    Ok(out)
}

/// What the walk of the notebook turned up, and in what way.
///
/// One kind gets one line, which already says how many — a headline above it
/// would only repeat the number. Several kinds get a total first, so the size
/// of the problem is legible before its breakdown.
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
    // Detection without a remedy is a trap, so the row that reports the problem
    // names the command that looks at it — and where the whole list can be seen.
    let _ = write!(
        out,
        "{}",
        style::paint(style::MUTED, "run `noda doctor` to look at these")
    );
    out.trim_end().to_string()
}

/// The first few subjects, with `…` standing in for the rest. Naming every one
/// is what would let a lost index put a line per note on the screen.
fn elide(subjects: &[String]) -> String {
    /// Enough to recognise what is going on, few enough to stay on one line.
    const SHOWN: usize = 3;

    let mut shown: Vec<&str> = subjects.iter().take(SHOWN).map(String::as_str).collect();
    if subjects.len() > SHOWN {
        shown.push("…");
    }
    shown.join("; ")
}

/// Diagnoses what the notebook holds that noda cannot simply act on, and adopts
/// the notes that are only waiting for an id.
///
/// There is nothing derived to rebuild any more — the id is in the filename, so
/// the files *are* the record and there is no second copy to fall out of step.
/// What is left is what arrives from outside: a note written by hand, a file
/// copied in, two machines that minted one id without ever meeting.
///
/// Exactly one of those has a repair that cannot lose anything: a `*.md` holding
/// frontmatter but no id is a note that has said what it is and only lacks a
/// name, so it is given one. The other two are reported and left alone — an id
/// on two notes means discarding one identity to keep the other, and a file that
/// claims an id without frontmatter might be a broken note or might never have
/// been one. Only their author knows.
///
/// `links` adds the two checks that need every note's prose read rather than its
/// name: see `describe_audit`. It is a flag rather than the default because it
/// is the only part of noda that costs a full read of the notebook.
///
/// One check needs no flag and no notes at all: the hooks in `.git` that noda
/// will never run. It is here rather than anywhere else because this is the
/// command for the things noda cannot act on, and a hook is the purest example —
/// noda cannot run it and will not delete it. See `describe_hooks`.
///
/// Where `status` elides, this names every file: it is the place people are
/// sent to see the full list.
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

    // A commit like every other change noda makes, so a repair that went
    // somewhere unwanted is revertible.
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

/// The ways a link and a file can fail to meet, as lines ready to print.
///
/// Nothing here is repaired, for the same reason throughout: the repair would
/// discard something only the author can weigh. A file nothing links to may be
/// an attachment whose note was deleted, or exactly where you meant to park it —
/// and the only "repair" available is deleting something noda cannot regenerate.
/// A link that names nothing may be a typo, or a file you have not copied in
/// yet.
///
/// A stale link is the one case where noda does know the answer, and it is
/// reported all the same. Acting on it means editing the prose of notes this
/// command was not pointed at, which noda does only when asked in so many words
/// — `file mv --update-links` is the existing shape of that request.
fn describe_audit(audit: &notebook::Audit) -> String {
    let mut out = String::new();

    if !audit.orphans.is_empty() {
        let count = audit.orphans.len();
        let noun = if count == 1 { "file" } else { "files" };
        // `links` either way: the subject of the clause is `no note`, which is
        // singular however many files it fails to reach.
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
            // The name the destination should carry, indented under it: it is
            // the answer, not another line of the report.
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

/// How far a note's last commit may sit after what its `updated` claims before
/// the gap means something.
///
/// noda writes the file and commits it in the same breath, so the honest gap is
/// under a second. The allowance is for a slow commit on a large notebook, not
/// for a judgement call: an edit made outside noda is discovered minutes, hours
/// or days later, never inside a minute.
const COMMIT_LAG: i64 = 60;

/// The times, checked against themselves and against git.
///
/// Reads every note's frontmatter — already paid for — and walks all of history,
/// which is not. Hence the flag, on the same terms as `--links`.
///
/// Nothing here is repaired. `updated` going stale is what happens when a note
/// is edited outside noda, and the only thing noda could do about it is
/// overwrite somebody's record of their own work with a guess.
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

        // git is the only witness to a change noda did not make. It cannot say
        // what changed, only that the file did — which is the whole of what is
        // being claimed here.
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

/// The hooks that will never fire.
///
/// Not behind a flag, unlike `--links` and `--times`. Those are asked for
/// because they are expensive — one reads every note, the other walks all of
/// history — and this reads one directory, which `doctor` was going to do
/// anyway. What makes a check opt-in here is its cost, not its novelty.
///
/// It stays out of `Problem`, and so out of `status`, for the opposite reason:
/// a hook is not a problem with the notes. `status` summarises what the notebook
/// holds, and a script somebody left in `.git` is not something it holds.
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
    // The remedy is the reason: knowing *why* they are dead is what tells you
    // that running the same command through git would have fired them.
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

/// What to do about each kind of problem, as lines ready to print. Detection
/// without a remedy is a trap, and the two noda refuses to settle are exactly
/// the ones where saying nothing would leave someone stuck.
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

/// How far the notebook has drifted, phrased as what there is left to do.
fn describe_drift(drift: Option<(usize, usize)>) -> String {
    let Some((ahead, behind)) = drift else {
        return style::paint(style::MUTED, "never synced");
    };
    let as_of = style::paint(style::MUTED, "(as of the last sync)");
    match (ahead, behind) {
        (0, 0) => format!("in sync {as_of}"),
        (ahead, 0) => format!("{ahead} to push {as_of}"),
        (0, behind) => format!("{behind} to pull {as_of}"),
        (ahead, behind) => format!("{ahead} to push, {behind} to pull {as_of}"),
    }
}

/// The notebook's history, or one note's.
pub fn log(paths: &Paths, key: Option<&str>, max: Option<usize>) -> Result<String> {
    let notebook = Notebook::open_active(paths)?;
    // History is about the file, not its contents: a note whose frontmatter has
    // gone is precisely the one whose past you want to look at. The id comes
    // from the filename, so it is there to be had either way.
    let id = match key {
        Some(key) => Some(find(&notebook, key)?.id),
        None => None,
    };

    let mut out = String::new();
    for entry in notebook.log(id.as_deref(), max)? {
        let line = format!(
            "{}  {}  {}",
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
    Ok(out)
}

/// The notes that link to something — a note, or one of the notebook's files.
///
/// Inbound only, which is why it is not called `links`. What a note points *at*
/// is in the note: `noda show` prints it, and every Markdown reader renders it.
/// What points at the note is the half nothing could tell you.
///
/// A command of its own rather than a flag on `ls`, on the standing rule: `ls`
/// reads a directory, and this reads and parses every note's body — the cost
/// `doctor --links` is a flag for.
///
/// It takes a file as readily as a note, as `noda path` does. "Which notes use
/// this diagram" and "which notes link to this note" are one question asked of
/// two kinds of thing, and the walk that answers either answers both.
pub fn backlinks(paths: &Paths, key: &str, format: Format) -> Result<String> {
    let notebook = Notebook::open_active(paths)?;

    // Resolved exactly as `path` resolves it, and for the same reason: one key
    // can in principle name both, and the command was asked about a file just as
    // much as about a note.
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
        // Before the empty check, as every other listing does it: a program
        // asking for JSON gets a document either way.
        Format::Json => return Ok(backlinks_json(&notebook.name, &subject, &found)),
        // One id per line. No `--null` beside it: what this prints is a note's
        // id, and an id has no spaces to protect.
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

    let ids = found
        .iter()
        .map(|f| display_width(&f.id))
        .max()
        .unwrap_or(0);
    let slugs = found
        .iter()
        .map(|f| display_width(&f.slug))
        .max()
        .unwrap_or(0);
    let mut out = String::new();
    for file in &found {
        let line = format!(
            "{}  {}  {}",
            pad(&file.id, ids),
            pad(&file.slug, slugs),
            file.note.title
        );
        out.push_str(line.trim_end());
        out.push('\n');
    }
    Ok(out)
}

/// The backlinks as one JSON object, on one line, like every other listing.
///
/// It names its subject: a script that asked about `meeting-notes` gets back the
/// filename that was actually resolved, which is the thing a retitle moves.
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
/// A command of its own rather than a flag on `ls` or a field in `search`, on
/// the precedent `deleted` set: `ls` reads a directory and this parses every
/// note's body, and one command must not carry two costs that far apart. `ls`
/// has a standing no on new columns for the same reason, and the search grammar
/// refuses fields that hide a tenfold difference in cost.
///
/// There is no `noda done`. Ticking a box needs an address noda does not have —
/// a note is addressed by id or slug, an item inside one by nothing. Line
/// numbers move and text prefixes collide, and giving each item an id would turn
/// the file into a noda-only format, which is the one thing choosing GFM
/// checkboxes was meant to avoid. `noda edit <note>` types one `x`.
pub fn todo(paths: &Paths, json: bool) -> Result<String> {
    let (seconds, offset_minutes) = notebook::local_now()?;
    // The date half of a `YYYY-MM-DD HH:MM`, which is ASCII throughout.
    todo_on(paths, json, &format_time(seconds, offset_minutes)[..10])
}

/// `todo`, with today given explicitly, so a test can say what "overdue" means
/// without freezing the clock — the same shape `edit_with` uses.
///
/// `today` is the *local* date, which is the only kind a due date can be
/// compared against: nobody writes `due:2026-08-10` meaning UTC. It comes from
/// `notebook::local_now`, the same offset every timestamp noda prints is
/// rendered with. Getting this wrong is not a rounding error — east of UTC an
/// item that went overdue at midnight would stay unmarked until morning, which
/// is exactly when a todo list is read.
pub fn todo_on(paths: &Paths, json: bool, today: &str) -> Result<String> {
    let notebook = Notebook::open_active(paths)?;
    let mut items = Vec::new();
    for file in notebook.notes()? {
        for item in todo::items(&file.note.body) {
            items.push((file.id.clone(), file.slug.clone(), item));
        }
    }

    // Soonest first, and the undated last: a date is a claim about when
    // something has to happen, and an item without one has made no claim. Ties
    // fall back to the slug so a listing does not reshuffle between runs.
    items.sort_by(|(_, left_slug, left), (_, right_slug, right)| {
        match (&left.due, &right.due) {
            (Some(left), Some(right)) => left.cmp(right),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        }
        .then_with(|| left_slug.cmp(right_slug))
    });

    // Before the empty check, as `ls` and `deleted` do it: a program asking for
    // JSON gets a document either way, and an empty list is an answer.
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
        // Never truncated. A real action item is a sentence, and a list that
        // cuts the sentence off is a list you have to open the note to read.
        let due = match &item.due {
            Some(due) if due.as_str() < today => style::paint(style::OVERDUE, due),
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

/// The listing as one JSON object, on one line, like `ls` and `deleted`.
///
/// `due` is carried and `overdue` is not: a program has its own clock and its
/// own idea of which day it is in, and the red in the table is noda answering a
/// question nobody asked a script.
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

/// Who wrote each line of a note, and when.
///
/// The columns `log` uses, in the order it uses them: commit, time, then the
/// thing itself. No line numbers — nothing else in noda prints one, and in prose
/// the unit somebody is looking for is a paragraph, not a row.
///
/// Only the body, and `blame` says why.
pub fn blame(paths: &Paths, key: &str) -> Result<String> {
    let notebook = Notebook::open_active(paths)?;
    // The filename is enough, as it is for `log` and `diff`: a note whose
    // frontmatter has gone is a good candidate for asking what happened to it.
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

/// The notes history holds that the notebook no longer does, most recently lost
/// first.
///
/// A command of its own rather than a flag on `ls`, for two reasons. `ls` reads
/// a directory; this walks all of history, and one command should not carry two
/// costs that far apart. And every column `ls` prints describes something that
/// exists — a deleted note's slug and title are what they were the moment it
/// went, which is a different claim under the same heading.
///
/// The revision printed is not the commit that did the deleting. It is that
/// commit's parent, the last one that still held the note, because that is what
/// `restore` has to be given. Naming the deletion and leaving the `~1` to be
/// worked out would be reporting a problem without its remedy.
pub fn deleted(paths: &Paths, notebook_name: Option<&str>, json: bool) -> Result<String> {
    let name = match notebook_name {
        Some(name) => name.to_string(),
        None => notebook::active_name(paths)?,
    };
    let notebook = Notebook::open(paths, &name)?;
    let gone = notebook.deleted()?;

    // Before the empty check, as `ls` does it: a program asking for JSON gets a
    // document either way, and an empty list is an answer.
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

/// The deletions as one JSON object, on one line. Hand-written, for the reason
/// `as_json` gives.
///
/// Object ids are printed in full rather than abbreviated: `restore` takes
/// either, and an abbreviation is a thing that can one day stop being unique.
/// The times are UTC rather than the commit's own zone — the table shows a
/// person the time they made the commit in, and a program should not have to
/// take a position on whose clock it was.
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

/// A commit time as RFC 3339 UTC — the spelling noda uses everywhere it writes
/// a time itself, so a script never meets two of them.
fn rfc3339(seconds: i64) -> String {
    jiff::Timestamp::from_second(seconds)
        .map(|time| time.strftime("%Y-%m-%dT%H:%M:%SZ").to_string())
        .unwrap_or_default()
}

/// Uncommitted changes, or what the last commit changed. The output is a plain
/// unified diff — no header, nothing wrapped around it — so it stays something
/// `git apply` will take.
pub fn diff(paths: &Paths, key: Option<&str>) -> Result<String> {
    let notebook = Notebook::open_active(paths)?;
    // Only the filename is needed, so a file that will not parse is no obstacle
    // — and seeing what changed is how you find out why it will not.
    let file = match key {
        Some(key) => {
            let found = find(&notebook, key)?;
            Some(note::file_name(&found.id, &found.slug))
        }
        None => None,
    };

    let mut out = String::new();
    notebook
        .diff(file.as_deref())?
        .print(git2::DiffFormat::Patch, |_delta, _hunk, line| {
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

/// Puts a note back the way it was at `rev`, as a new commit. Nothing is
/// rewritten: the restore moves history forward like every other change.
pub fn restore(paths: &Paths, key: &str, rev: &str) -> Result<String> {
    let notebook = Notebook::open_active(paths)?;
    let commit = notebook.revision(rev)?;

    // A note that still exists is found the usual way; one that was removed is
    // looked up in the index as it stood at that commit, so `restore` doubles as
    // the way back from `noda rm`.
    //
    // The file is found without being read. `restore` is about to write over it,
    // so refusing to act on one whose frontmatter has gone turns the command for
    // undoing damage into another casualty of it — and that is the moment a
    // person reaches for it.
    let current = find(&notebook, key).ok();
    // From the filename when the note is still here, and from history when it is
    // not — which is how `restore` doubles as the way back from `noda rm`.
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

    // The id is the note's identity, so a restored note keeps the name it has
    // now; only its contents travel back. A note that is gone comes back under
    // the name it had then.
    let slug = match &current {
        Some(found) => found.slug.clone(),
        None => slug_then,
    };
    let path = notebook.note_path(&id, &slug);

    let restored = Note::parse(&text)
        .map_err(|e| Error::msg(format!("the copy of `{key}` at {rev} cannot be read: {e}")))?;
    // `updated` is noda's record of when the file last changed, not part of what
    // is being restored, so it is held aside for the comparison. Otherwise
    // restoring the same revision twice would never say "no change": the first
    // restore writes a timestamp the copy in history cannot have.
    let ignoring_updated =
        |text: &str| note::set_field(text, "updated", "").unwrap_or_else(|| text.to_string());
    if current
        .as_ref()
        .and_then(|found| std::fs::read_to_string(&found.path).ok())
        .is_some_and(|on_disk| ignoring_updated(&on_disk) == ignoring_updated(&text))
    {
        return Ok(format!(
            "{}  (no change)",
            summary(&id, &slug, &restored.tags)
        ));
    }

    // The contents travel back; the record of when they landed does not. The
    // file changed just now, whatever the copy in history says about itself.
    // `created` is left as it was found: it belongs to the note, and the note is
    // the same one it always was.
    let text = note::set_field(&text, "updated", &note::now())
        .expect("the copy at this revision parsed, so it has a frontmatter block");
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
    Ok(format!("{}  {url}", notebook.name))
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
    Notebook::open_active(paths)?.pull()
}

/// Sends the current branch to the remote.
pub fn push(paths: &Paths) -> Result<String> {
    Notebook::open_active(paths)?.push()
}

/// Commit, pull, push — in that order, so local work is never left behind by a
/// merge and the push always carries it.
///
/// This used to refuse while the notes and the index disagreed, because `sync`
/// commits the whole working tree without asking what is in it and would have
/// made such a disagreement permanent and remote. There is no index now, so
/// there is nothing for the files to disagree with and nothing to guard.
pub fn sync(paths: &Paths) -> Result<String> {
    let notebook = Notebook::open_active(paths)?;
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
/// Commits the working tree first, on the same terms as `sync`: noda commits as
/// it goes, so a clean notebook is the normal state, and a snapshot that quietly
/// left out what is on disk would be a snapshot of something nobody has.
///
/// The message defaults to the name. A tag needs one to be annotated, and
/// inventing prose on somebody's behalf is worse than repeating what they said.
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

/// Every snapshot, newest first.
///
/// The same three columns `deleted` leads with — name, when, which commit — in
/// the same order, because they answer the same question about a different kind
/// of thing.
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
                "cannot tell what to call the notebook from `{url}` — pass a name"
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

/// A note reference resolved to a file, with the reading of it kept separate.
///
/// A command that only needs to know *which* note it was pointed at — to delete
/// it, to show its history, to write an old version over it — has no business
/// failing on a file it is not going to read. `status` already takes that line
/// with the notebook as a whole; these take it one note at a time.
struct Found {
    /// Always known: the filename carries it, so a file that will not parse
    /// still says which note it is.
    id: String,
    slug: String,
    path: PathBuf,
    /// What came of reading the file, kept rather than thrown so the commands
    /// that do need the note can fail with the parse error itself.
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

/// How wide `format_time` prints, for the one caller that has to line something
/// else up against it.
const TIME_WIDTH: usize = "0000-00-00 00:00".len();

/// How wide a `due:` date prints, for the rows that have none.
const DATE_WIDTH: usize = "0000-00-00".len();

/// `YYYY-MM-DD HH:MM`, in the timezone the commit was made in — the same choice
/// git makes by default. Absolute rather than "3 days ago": it is testable
/// without freezing the clock, and it sorts.
fn format_time(seconds: i64, offset_minutes: i32) -> String {
    let local = seconds + i64::from(offset_minutes) * 60;
    let (year, month, day) = civil_from_days(local.div_euclid(86_400));
    let time = local.rem_euclid(86_400);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}",
        time / 3600,
        (time % 3600) / 60
    )
}

/// Days since the Unix epoch to a calendar date, by Howard Hinnant's
/// `civil_from_days`. Fifteen lines beats a date dependency for one format.
///
/// The two casts drop a sign that cannot be there: for every `i64` input the
/// algorithm yields a month in `1..=12` and a day in `1..=31`, which
/// `every_day_lands_on_a_real_calendar_date` checks across four centuries
/// rather than leaving as a claim in a comment.
#[allow(clippy::cast_sign_loss)]
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    // Shift the epoch to 0000-03-01, which puts the leap day at the end of the
    // year and makes every era exactly 146,097 days.
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

/// Terminal columns a string occupies. East Asian Wide and Fullwidth characters
/// take two cells, so counting `chars()` would leave a CJK slug misaligned in `ls`.
/// This covers the wide blocks in common use rather than the whole of UAX #11.
fn display_width(text: &str) -> usize {
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

/// First non-empty line, with any Markdown heading marker stripped.
fn derive_title(body: &str) -> Option<String> {
    body.lines()
        .map(|line| line.trim_start_matches('#').trim())
        .find(|line| !line.is_empty())
        .map(str::to_string)
}

/// Tags as they will be written: trimmed, because the frontmatter is read back
/// trimmed, and refused when they carry something it cannot round-trip.
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

    /// What `todo` gets wrong if it asks UTC what day it is. At this instant it
    /// is already the 3rd in Taipei and still the 2nd in London, and an item
    /// due on the 2nd is overdue for one of them and not the other. The eight
    /// hours between the two answers are the morning — which is when a todo
    /// list is read.
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

        // Lowercasing changes the byte length here — İ is one char, two lowered.
        // The offsets must still land on the original's char boundaries.
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
        // The unsigned casts in `civil_from_days` are safe only because the
        // algorithm cannot produce a negative month or day. Four centuries of
        // days, either side of the epoch, say so out loud.
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
