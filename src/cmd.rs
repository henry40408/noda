//! Command implementations. Each one takes `Paths` explicitly so tests can run
//! against a throwaway root without touching the real environment.

use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::note::{self, Note};
use crate::notebook::{self, Notebook};
use crate::paths::Paths;
use crate::remote;
use crate::style;
use crate::{Error, Result};

/// Name of the notebook `noda init` creates.
pub const DEFAULT_NOTEBOOK: &str = "default";

/// Scratch file used when composing a note in `$EDITOR`.
const EDIT_FILE: &str = "NOTE_EDITMSG.md";

/// Committed `id ↔ slug` lookup, relative to the notebook root.
const INDEX_PATH: &str = ".noda/index.tsv";

/// Creates the XDG directories, a default notebook, and points `active` at it.
/// Safe to run more than once.
pub fn init(paths: &Paths) -> Result<String> {
    paths.create_dirs()?;
    let mut lines = Vec::new();
    if Notebook::exists(paths, DEFAULT_NOTEBOOK) {
        lines.push(format!("notebook `{DEFAULT_NOTEBOOK}` already exists"));
    } else {
        let notebook = Notebook::create(paths, DEFAULT_NOTEBOOK)?;
        lines.push(format!(
            "created notebook `{DEFAULT_NOTEBOOK}` at {}",
            notebook.path.display()
        ));
    }
    if paths.active_notebook().is_err() {
        paths.set_active_notebook(DEFAULT_NOTEBOOK)?;
        lines.push(format!("active notebook: {DEFAULT_NOTEBOOK}"));
    }
    Ok(lines.join("\n"))
}

/// Creates a note and commits it. `content` of `None` opens `$EDITOR`.
pub fn add(
    paths: &Paths,
    title: Option<&str>,
    content: Option<&str>,
    tags: &[String],
) -> Result<String> {
    let notebook = Notebook::open_active(paths)?;

    let body = match content {
        Some(text) => text.to_string(),
        None => compose_in_editor(paths, title)?,
    };
    let title = match title {
        Some(t) if !t.trim().is_empty() => t.trim().to_string(),
        _ => derive_title(&body)
            .ok_or_else(|| Error::msg("aborted: the note is empty, so it has no title"))?,
    };

    let mut index = notebook.index()?;
    let taken: HashSet<String> = index.iter().map(|(id, _)| id.clone()).collect();
    let slug = unique_slug(&notebook, &note::slugify(&title));
    let note = Note {
        id: note::mint_id(&taken),
        title,
        tags: tags.to_vec(),
        body: body.trim_start_matches('\n').to_string(),
    };

    std::fs::write(notebook.note_path(&slug), note.render())?;
    index.push((note.id.clone(), slug.clone()));
    notebook.write_index(&index)?;
    notebook.commit(
        &[Path::new(&format!("{slug}.md")), Path::new(INDEX_PATH)],
        &format!("add: {slug}"),
    )?;

    Ok(summary(&note.id, &slug, &note.tags))
}

/// Lists notes as `id  slug  title  tags`, aligned.
pub fn ls(paths: &Paths, notebook: Option<&str>, tag: Option<&str>) -> Result<String> {
    let name = match notebook {
        Some(name) => name.to_string(),
        None => paths.active_notebook()?,
    };
    let notebook = Notebook::open(paths, &name)?;

    let rows: Vec<(String, String, String, String)> = notebook
        .notes()?
        .into_iter()
        .filter(|(_, note)| tag.is_none_or(|t| note.tags.iter().any(|nt| nt == t)))
        .map(|(slug, note)| (note.id, slug, note.title, note.tags.join(", ")))
        .collect();

    if rows.is_empty() {
        return Ok(String::new());
    }

    let id_width = rows.iter().map(|r| display_width(&r.0)).max().unwrap_or(0);
    let slug_width = rows.iter().map(|r| display_width(&r.1)).max().unwrap_or(0);
    let mut out = String::new();
    for (id, slug, title, tags) in rows {
        let mut line = format!(
            "{}  {}  {title}",
            pad(&id, id_width),
            pad(&slug, slug_width)
        );
        if !tags.is_empty() {
            line.push_str(&format!("  [{tags}]"));
        }
        out.push_str(line.trim_end());
        out.push('\n');
    }
    Ok(out)
}

/// Prints a note verbatim — frontmatter included, because that is the file.
pub fn show(paths: &Paths, key: &str) -> Result<String> {
    let notebook = Notebook::open_active(paths)?;
    Ok(dim_frontmatter(&std::fs::read_to_string(
        notebook.resolve(key)?,
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
            summary(&note.id, &located.slug, &note.tags)
        ));
    }

    std::fs::write(&located.path, note.render())?;
    notebook.commit(
        &[Path::new(&format!("{}.md", located.slug))],
        &format!("tag: {}", located.slug),
    )?;
    Ok(summary(&note.id, &located.slug, &note.tags))
}

/// Opens a note in `$EDITOR` and commits whatever was saved.
pub fn edit(paths: &Paths, key: &str) -> Result<String> {
    edit_with(paths, key, &configured_editor())
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

    // The file is left exactly as it was saved: a rejected edit stays on disk to
    // be fixed or thrown away with `git checkout`, never silently discarded.
    let edited = Note::parse(&after).map_err(|e| {
        Error::msg(format!(
            "{}: {e}\nthe file was left as you saved it and was not committed",
            located.path.display()
        ))
    })?;
    if edited.id != located.note.id {
        return Err(Error::msg(format!(
            "{}: the id changed from {} to {} — ids are permanent; \
             the file was left as you saved it and was not committed",
            located.path.display(),
            located.note.id,
            edited.id
        )));
    }

    notebook.commit(
        &[Path::new(&format!("{}.md", located.slug))],
        &format!("edit: {}", located.slug),
    )?;
    Ok(summary(&edited.id, &located.slug, &edited.tags))
}

/// Retitles a note. The slug follows the new title; the id never moves.
pub fn mv(paths: &Paths, key: &str, new_title: &str) -> Result<String> {
    let notebook = Notebook::open_active(paths)?;
    let located = locate(&notebook, key)?;
    let mut note = located.note;

    let title = new_title.trim();
    if title.is_empty() {
        return Err(Error::msg("a note needs a title"));
    }

    let base = note::slugify(title);
    let slug = if base == located.slug {
        located.slug.clone()
    } else {
        unique_slug(&notebook, &base)
    };
    note.title = title.to_string();

    std::fs::write(notebook.note_path(&slug), note.render())?;
    let mut changed = vec![format!("{slug}.md")];
    if slug != located.slug {
        std::fs::remove_file(&located.path)?;
        changed.push(format!("{}.md", located.slug));

        let mut index = notebook.index()?;
        for (id, entry) in index.iter_mut() {
            if *id == note.id {
                *entry = slug.clone();
            }
        }
        notebook.write_index(&index)?;
        changed.push(INDEX_PATH.to_string());
    }

    let files: Vec<&Path> = changed.iter().map(Path::new).collect();
    notebook.commit(&files, &format!("mv: {} -> {slug}", located.slug))?;
    Ok(summary(&note.id, &slug, &note.tags))
}

/// Deletes a note. The file goes, but the commit that removed it does not, so
/// `git revert` brings the note back with its id intact.
pub fn rm(paths: &Paths, key: &str) -> Result<String> {
    let notebook = Notebook::open_active(paths)?;
    let located = locate(&notebook, key)?;

    std::fs::remove_file(&located.path)?;
    let mut index = notebook.index()?;
    index.retain(|(id, _)| *id != located.note.id);
    notebook.write_index(&index)?;
    notebook.commit(
        &[
            Path::new(&format!("{}.md", located.slug)),
            Path::new(INDEX_PATH),
        ],
        &format!("rm: {}", located.slug),
    )?;

    Ok(format!(
        "removed  {}",
        summary(&located.note.id, &located.slug, &located.note.tags)
    ))
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
    let active = paths.active_notebook().ok();
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
    if paths.active_notebook().ok().as_deref() == Some(name) {
        return Err(Error::msg(format!(
            "`{name}` is the active notebook — switch with `noda use <name>` first"
        )));
    }

    if !force {
        let notes = Notebook::open(paths, name)
            .and_then(|notebook| notebook.notes())
            .map(|notes| notes.len())
            .unwrap_or(0);
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
    if paths.active_notebook().ok().as_deref() == Some(old) {
        paths.set_active_notebook(new)?;
    }
    Ok(format!("renamed notebook `{old}` to `{new}`"))
}

/// The notebook's history, or one note's.
pub fn log(paths: &Paths, key: Option<&str>, max: Option<usize>) -> Result<String> {
    let notebook = Notebook::open_active(paths)?;
    let id = match key {
        Some(key) => Some(locate(&notebook, key)?.note.id),
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

/// Uncommitted changes, or what the last commit changed. The output is a plain
/// unified diff — no header, nothing wrapped around it — so it stays something
/// `git apply` will take.
pub fn diff(paths: &Paths, key: Option<&str>) -> Result<String> {
    let notebook = Notebook::open_active(paths)?;
    let file = match key {
        Some(key) => Some(format!("{}.md", locate(&notebook, key)?.slug)),
        None => None,
    };

    let mut out = String::new();
    notebook
        .diff(file.as_deref())?
        .print(git2::DiffFormat::Patch, |delta, _hunk, line| {
            // The id ↔ slug index changes on nearly every commit and is derived
            // from the notes themselves; showing it would bury the note.
            let generated = delta
                .new_file()
                .path()
                .or_else(|| delta.old_file().path())
                .is_some_and(|path| path.starts_with(notebook::META_DIR));
            if generated {
                return true;
            }

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
    let current = match notebook.resolve(key) {
        Ok(_) => Some(locate(&notebook, key)?),
        Err(_) => None,
    };
    let id = match &current {
        Some(located) => located.note.id.clone(),
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
    let (slug, path) = match &current {
        Some(located) => (located.slug.clone(), located.path.clone()),
        None => {
            let slug = unique_slug(&notebook, &slug_then);
            let path = notebook.note_path(&slug);
            (slug, path)
        }
    };

    let restored = Note::parse(&text)
        .map_err(|e| Error::msg(format!("the copy of `{key}` at {rev} cannot be read: {e}")))?;
    if current
        .as_ref()
        .is_some_and(|located| std::fs::read_to_string(&located.path).ok() == Some(text.clone()))
    {
        return Ok(format!(
            "{}  (no change)",
            summary(&id, &slug, &restored.tags)
        ));
    }

    std::fs::write(&path, &text)?;
    let mut changed = vec![format!("{slug}.md")];
    if current.is_none() {
        let mut index = notebook.index()?;
        index.push((id.clone(), slug.clone()));
        notebook.write_index(&index)?;
        changed.push(INDEX_PATH.to_string());
    }
    let files: Vec<&Path> = changed.iter().map(Path::new).collect();
    notebook.commit(
        &files,
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
    paths.active_notebook()
}

/// A note reference resolved to everything the mutating commands need.
struct Located {
    slug: String,
    path: PathBuf,
    note: Note,
}

fn locate(notebook: &Notebook, key: &str) -> Result<Located> {
    let path = notebook.resolve(key)?;
    let slug = path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| Error::msg(format!("unreadable note filename: {}", path.display())))?
        .to_string();
    let note = Note::parse(&std::fs::read_to_string(&path)?)
        .map_err(|e| Error::msg(format!("{}: {e}", path.display())))?;
    Ok(Located { slug, path, note })
}

/// The one-line acknowledgement every mutating command prints.
fn summary(id: &str, slug: &str, tags: &[String]) -> String {
    if tags.is_empty() {
        format!("{id}  {slug}")
    } else {
        format!("{id}  {slug}  [{}]", tags.join(", "))
    }
}

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

/// Appends `-2`, `-3`, … until the slug is free within the notebook.
fn unique_slug(notebook: &Notebook, base: &str) -> String {
    if !notebook.note_path(base).exists() {
        return base.to_string();
    }
    for n in 2.. {
        let candidate = format!("{base}-{n}");
        if !notebook.note_path(&candidate).exists() {
            return candidate;
        }
    }
    unreachable!("the range is unbounded")
}

fn configured_editor() -> String {
    std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| "vi".to_string())
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

    run_editor(&configured_editor(), &scratch)?;

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
