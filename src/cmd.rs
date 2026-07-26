//! Command implementations. Each one takes `Paths` explicitly so tests can run
//! against a throwaway root without touching the real environment.

use std::collections::HashSet;
use std::io::Write;
use std::path::Path;

use crate::note::{self, Note};
use crate::notebook::Notebook;
use crate::paths::Paths;
use crate::{Error, Result};

/// Name of the notebook `noda init` creates.
pub const DEFAULT_NOTEBOOK: &str = "default";

/// Scratch file used when composing a note in `$EDITOR`.
const EDIT_FILE: &str = "NOTE_EDITMSG.md";

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
        &[
            Path::new(&format!("{slug}.md")),
            Path::new(".noda/index.tsv"),
        ],
        &format!("add: {slug}"),
    )?;

    Ok(format!("{}  {}", note.id, slug))
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

/// Opens `$VISUAL`/`$EDITOR` on a scratch file and returns what was written.
/// The buffer lives in the cache dir, never in the notebook, so an abandoned
/// edit can't leave a stray file in the repo.
fn compose_in_editor(paths: &Paths, title: Option<&str>) -> Result<String> {
    let editor = std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| "vi".to_string());
    let mut parts = editor.split_whitespace();
    let program = parts
        .next()
        .ok_or_else(|| Error::msg("$EDITOR is set but empty"))?;

    std::fs::create_dir_all(paths.cache_dir())?;
    let scratch = paths.cache_dir().join(EDIT_FILE);
    let template = match title {
        Some(title) => format!("# {title}\n\n"),
        None => String::new(),
    };
    std::fs::write(&scratch, &template)?;

    let status = std::process::Command::new(program)
        .args(parts)
        .arg(&scratch)
        .status()
        .map_err(|e| Error::msg(format!("could not start editor `{program}`: {e}")))?;
    if !status.success() {
        return Err(Error::msg(format!(
            "editor `{program}` exited with {status}"
        )));
    }

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
}
