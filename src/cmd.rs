//! Command implementations. Each one takes `Paths` explicitly so tests can run
//! against a throwaway root without touching the real environment.

use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::note::{self, Note};
use crate::notebook::Notebook;
use crate::paths::Paths;
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
    Ok(std::fs::read_to_string(notebook.resolve(key)?)?)
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
pub fn print(output: &str) -> Result<()> {
    if output.is_empty() {
        return Ok(());
    }
    let mut stdout = std::io::stdout().lock();
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
    fn summary_omits_empty_tags() {
        assert_eq!(summary("k3f9", "notes", &[]), "k3f9  notes");
        assert_eq!(
            summary("k3f9", "notes", &["work".into(), "q3".into()]),
            "k3f9  notes  [work, q3]"
        );
    }
}
