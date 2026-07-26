//! A note: its stable id, its slug, and the Markdown file that carries both.

use std::collections::HashSet;

use crate::{Error, Result};

/// Crockford base32 — `i`, `l`, `o` and `u` are absent, so an id can't be misread.
const CROCKFORD: &[u8; 32] = b"0123456789abcdefghjkmnpqrstvwxyz";

/// Ids start at four characters (20 bits) and grow only if that space is exhausted.
const ID_LEN: usize = 4;

/// Fallback slug for a title that contains nothing sluggable.
const FALLBACK_SLUG: &str = "note";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    pub id: String,
    pub title: String,
    pub tags: Vec<String>,
    pub body: String,
}

impl Note {
    /// The full file contents: frontmatter, a blank line, then the body.
    pub fn render(&self) -> String {
        let mut out = String::from("---\n");
        out.push_str(&format!("id: {}\n", self.id));
        out.push_str(&format!("title: {}\n", self.title));
        if !self.tags.is_empty() {
            out.push_str(&format!("tags: [{}]\n", self.tags.join(", ")));
        }
        out.push_str("---\n\n");
        out.push_str(&self.body);
        if !out.ends_with('\n') {
            out.push('\n');
        }
        out
    }

    pub fn parse(text: &str) -> Result<Note> {
        let (frontmatter, body) =
            split_frontmatter(text).ok_or_else(|| Error::msg("note has no frontmatter block"))?;

        let mut id = None;
        let mut title = None;
        let mut tags = Vec::new();
        for line in frontmatter.lines() {
            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            let value = value.trim();
            match key.trim() {
                "id" => id = Some(value.to_string()),
                "title" => title = Some(value.to_string()),
                "tags" => tags = parse_tags(value),
                _ => {}
            }
        }

        Ok(Note {
            id: id.ok_or_else(|| Error::msg("note frontmatter has no id"))?,
            title: title.unwrap_or_default(),
            tags,
            body: body.trim_start_matches('\n').to_string(),
        })
    }
}

/// Splits `text` into the frontmatter body and everything after the closing `---`.
fn split_frontmatter(text: &str) -> Option<(&str, &str)> {
    let rest = text.strip_prefix("---\n")?;
    let mut pos = 0;
    for line in rest.split_inclusive('\n') {
        if line.trim_end() == "---" {
            return Some((&rest[..pos], &rest[pos + line.len()..]));
        }
        pos += line.len();
    }
    None
}

fn parse_tags(value: &str) -> Vec<String> {
    value
        .trim_start_matches('[')
        .trim_end_matches(']')
        .split(',')
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_string)
        .collect()
}

/// A filename-safe, human-readable name derived from the title. Alphanumerics are
/// kept (so CJK titles stay legible), everything else collapses to a single `-`.
pub fn slugify(title: &str) -> String {
    let mut slug = String::new();
    let mut pending_dash = false;
    for ch in title.chars() {
        if ch.is_alphanumeric() {
            if pending_dash && !slug.is_empty() {
                slug.push('-');
            }
            pending_dash = false;
            slug.extend(ch.to_lowercase());
        } else {
            pending_dash = true;
        }
    }
    if slug.is_empty() {
        FALLBACK_SLUG.to_string()
    } else {
        slug
    }
}

/// Folds the characters Crockford treats as interchangeable, so a mistyped id
/// still resolves: `I`/`L` are `1`, `O` is `0`, and case never matters.
pub fn normalize_id(input: &str) -> String {
    input
        .chars()
        .flat_map(|c| c.to_lowercase())
        .map(|c| match c {
            'i' | 'l' => '1',
            'o' => '0',
            other => other,
        })
        .collect()
}

/// Mints an id that is not already `taken`, widening the id space if it fills up.
pub fn mint_id(taken: &HashSet<String>) -> String {
    let mut len = ID_LEN;
    loop {
        for _ in 0..64 {
            let candidate = random_id(len);
            if !taken.contains(&candidate) {
                return candidate;
            }
        }
        len += 1;
    }
}

fn random_id(len: usize) -> String {
    let mut bits = random_bits();
    let mut id = String::with_capacity(len);
    for _ in 0..len {
        id.push(CROCKFORD[(bits & 0x1f) as usize] as char);
        bits >>= 5;
    }
    id
}

/// `RandomState` is seeded from the OS for every instance, so hashing a fixed
/// value yields unpredictable bits without depending on an RNG crate.
fn random_bits() -> u64 {
    use std::hash::{BuildHasher, Hasher, RandomState};
    let mut hasher = RandomState::new().build_hasher();
    hasher.write_u64(0);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_collapses_punctuation_and_lowercases() {
        assert_eq!(slugify("Meeting Notes"), "meeting-notes");
        assert_eq!(slugify("  Q3 // planning!! "), "q3-planning");
        assert_eq!(slugify("C++ vs Rust"), "c-vs-rust");
    }

    #[test]
    fn slugify_keeps_cjk_and_falls_back_when_empty() {
        assert_eq!(slugify("會議 筆記"), "會議-筆記");
        assert_eq!(slugify("!!!"), FALLBACK_SLUG);
    }

    #[test]
    fn normalize_id_folds_confusable_characters() {
        assert_eq!(normalize_id("K3F9"), "k3f9");
        assert_eq!(normalize_id("IL0O"), "1100");
    }

    #[test]
    fn minted_ids_use_the_crockford_alphabet_and_avoid_collisions() {
        let taken: HashSet<String> = HashSet::new();
        let id = mint_id(&taken);
        assert_eq!(id.len(), ID_LEN);
        assert!(id.bytes().all(|b| CROCKFORD.contains(&b)), "{id}");

        let taken: HashSet<String> = std::iter::once(id.clone()).collect();
        assert_ne!(mint_id(&taken), id);
    }

    #[test]
    fn render_and_parse_round_trip() {
        let note = Note {
            id: "k3f9".into(),
            title: "Meeting notes".into(),
            tags: vec!["work".into(), "q3".into()],
            body: "Body line one.\nBody line two.\n".into(),
        };
        let text = note.render();
        assert!(text.starts_with("---\nid: k3f9\ntitle: Meeting notes\ntags: [work, q3]\n---\n\n"));
        assert_eq!(Note::parse(&text).unwrap(), note);
    }

    #[test]
    fn parse_tolerates_missing_tags_and_colons_in_titles() {
        let note = Note::parse("---\nid: abcd\ntitle: Rust: a tour\n---\n\nhi\n").unwrap();
        assert_eq!(note.title, "Rust: a tour");
        assert!(note.tags.is_empty());
        assert_eq!(note.body, "hi\n");
    }

    #[test]
    fn parse_rejects_files_without_frontmatter() {
        assert!(Note::parse("just markdown\n").is_err());
        assert!(Note::parse("---\ntitle: no id\n---\n").is_err());
    }
}
