//! A note: its Markdown file, and the identity its filename spells out.
//!
//! The id lives in the filename: git forbids two entries at one path, so
//! uniqueness is structural, and two machines each adding a note merge cleanly.
//!
//! The frontmatter's *presence* marks a file as a note (see `notebook::Scan`).
//! noda interprets its own fields and no others; any other field survives a
//! write-back untouched.

use std::collections::HashSet;
use std::fmt::Write as _;

use crate::{Error, Result};

/// Crockford base32: no `i`, `l`, `o` or `u`, so an id can't be misread.
const CROCKFORD: &[u8; 32] = b"0123456789abcdefghjkmnpqrstvwxyz";

/// 40 bits, minted against what the notebook holds.
pub const ID_LEN: usize = 8;

/// Base32 characters one `u64` draw supplies (60 bits).
const CHARS_PER_DRAW: usize = 12;

const FALLBACK_SLUG: &str = "note";

/// A directory entry's limit in bytes on ext4, APFS and HFS+.
pub const MAX_FILE_NAME_LEN: usize = 255;

/// In bytes. [`file_name`] leaves 243 of [`MAX_FILE_NAME_LEN`] for the slug;
/// this is well under it because `ls -l` pads the slug column to the widest one.
/// The title in the frontmatter keeps the whole text.
const MAX_SLUG_LEN: usize = 100;

/// The id is not here: it is the filename.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    pub title: String,
    pub tags: Vec<String>,
    /// RFC 3339, exactly as found and never respelled: an imported `+08:00`
    /// keeps its offset, so `noda show` still matches the file.
    pub created: Option<String>,
    pub updated: Option<String>,
    /// Kept as written; [`Note::is_pinned`] judges it. Absent rather than
    /// `false` when unpinned, so pin + unpin leaves the file byte-for-byte.
    pub pinned: Option<String>,
    /// Frontmatter lines noda does not interpret, in order, so a write-back
    /// keeps them. None can be `---` (it would have closed the block), so
    /// `render` never writes what `parse` cannot read.
    pub extra: Vec<String>,
    pub body: String,
}

impl Note {
    /// The block is always written, even empty: it is what marks a note.
    pub fn render(&self) -> String {
        let mut out = String::from("---\n");
        let _ = writeln!(out, "title: {}", self.title);
        if !self.tags.is_empty() {
            let _ = writeln!(out, "tags: [{}]", self.tags.join(", "));
        }
        if let Some(created) = &self.created {
            let _ = writeln!(out, "created: {created}");
        }
        if let Some(updated) = &self.updated {
            let _ = writeln!(out, "updated: {updated}");
        }
        if let Some(pinned) = &self.pinned {
            let _ = writeln!(out, "pinned: {pinned}");
        }
        // After noda's own fields: their order is kept, their position is not.
        for line in &self.extra {
            let _ = writeln!(out, "{line}");
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

        let mut title = None;
        let mut tags = Vec::new();
        let mut created = None;
        let mut updated = None;
        let mut pinned = None;
        let mut extra = Vec::new();
        for line in frontmatter.lines() {
            let Some((key, value)) = line.split_once(':') else {
                extra.push(line.to_string());
                continue;
            };
            let value = value.trim();
            match key.trim() {
                "title" => title = Some(value.to_string()),
                "tags" => tags = parse_tags(value),
                // Kept as written rather than refusing the note over a typo;
                // `doctor --times` reports it.
                "created" => created = Some(value.to_string()),
                "updated" => updated = Some(value.to_string()),
                "pinned" => pinned = Some(value.to_string()),
                _ => extra.push(line.to_string()),
            }
        }

        Ok(Note {
            title: title.unwrap_or_default(),
            tags,
            created,
            updated,
            pinned,
            extra,
            body: body.trim_start_matches('\n').to_string(),
        })
    }

    /// `true`, case-folded; any other value is not a pin but stays in the file.
    pub fn is_pinned(&self) -> bool {
        self.pinned
            .as_ref()
            .is_some_and(|value| value.eq_ignore_ascii_case("true"))
    }
}

/// What `pin` writes. `unpin` writes no line at all.
pub const PINNED: &str = "true";

/// Splits `text` into the frontmatter body and everything after the closing `---`.
pub(crate) fn split_frontmatter(text: &str) -> Option<(&str, &str)> {
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

/// RFC 3339, UTC, whole seconds: machine-independent and fixed-width, so it
/// sorts as text.
pub fn now() -> String {
    jiff::Timestamp::now()
        .strftime("%Y-%m-%dT%H:%M:%SZ")
        .to_string()
}

/// Sets one frontmatter field, leaving every other byte alone — unlike
/// `render`, which moves unknown fields below noda's own. So `edit` does not
/// reorder a block somebody just arranged by hand.
pub fn set_field(text: &str, key: &str, value: &str) -> Option<String> {
    let (frontmatter, body) = split_frontmatter(text)?;

    let mut out = String::from("---\n");
    let mut written = false;
    for line in frontmatter.lines() {
        if line.split_once(':').is_some_and(|(k, _)| k.trim() == key) {
            // `parse` reads one value, so a duplicate is dropped.
            if !written {
                let _ = writeln!(out, "{key}: {value}");
                written = true;
            }
            continue;
        }
        let _ = writeln!(out, "{line}");
    }
    if !written {
        let _ = writeln!(out, "{key}: {value}");
    }
    out.push_str("---\n");
    out.push_str(body);
    Some(out)
}

/// The body replaced, the frontmatter block left exactly as found (for
/// [`set_field`]'s reason). `CRLF` becomes `LF`: the caller is a browser, and an
/// HTML form submits `CRLF`.
pub fn set_body(text: &str, body: &str) -> Option<String> {
    let (frontmatter, _) = split_frontmatter(text)?;
    let mut out = String::from("---\n");
    out.push_str(frontmatter);
    // `render`'s shape.
    out.push_str("---\n\n");
    out.push_str(body.replace("\r\n", "\n").trim_start_matches('\n'));
    if !out.ends_with('\n') {
        out.push('\n');
    }
    Some(out)
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

pub fn file_name(id: &str, slug: &str) -> String {
    format!("{id}-{slug}.md")
}

/// The id alphabet has no `-`, so the first one is the boundary however long
/// ids grow. The length floor stops `c-vs-rust` reading as the id `c`.
pub fn split_stem(stem: &str) -> Option<(&str, &str)> {
    let (id, slug) = stem.split_once('-')?;
    if slug.is_empty() || !is_id_shaped(id) {
        return None;
    }
    Some((id, slug))
}

/// `.md` alone does not decide it (`README.md`). The test `Notebook::inventory`
/// applies, for anything deciding from a name alone.
pub fn names_a_note(name: &str) -> bool {
    name.strip_suffix(".md").and_then(split_stem).is_some()
}

/// Long enough, and entirely from the id alphabet.
pub fn is_id_shaped(text: &str) -> bool {
    text.len() >= ID_LEN
        && text
            .bytes()
            .all(|b| CROCKFORD.contains(&b.to_ascii_lowercase()))
}

/// A title goes into the frontmatter verbatim, so a line break is refused
/// rather than escaped, keeping `render` and `parse` inverse.
pub fn validate_title(title: &str) -> Result<()> {
    if title.contains(['\n', '\r']) {
        return Err(Error::msg("a title has to fit on one line"));
    }
    Ok(())
}

/// A tag carrying `,`, `[`, `]` or a line break would read back differently.
pub fn validate_tag(tag: &str) -> Result<()> {
    if tag.is_empty() {
        return Err(Error::msg("a tag needs a name"));
    }
    if let Some(bad) = tag.matches(['\n', '\r', ',', '[', ']']).next() {
        let bad = if bad == "\n" || bad == "\r" {
            "a line break".to_string()
        } else {
            format!("`{bad}`")
        };
        return Err(Error::msg(format!("a tag cannot contain {bad}: {tag}")));
    }
    Ok(())
}

/// Alphanumerics kept (so CJK stays legible), everything else collapsed to one
/// `-`, cut to `MAX_SLUG_LEN` bytes.
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
        shorten(&slug)
    }
}

/// Cuts at the last `-` inside the cap, unless that loses more than half the
/// cap; then at the cap, on a UTF-8 character boundary.
fn shorten(slug: &str) -> String {
    if slug.len() <= MAX_SLUG_LEN {
        return slug.to_string();
    }
    let mut cut = MAX_SLUG_LEN;
    while !slug.is_char_boundary(cut) {
        cut -= 1;
    }
    match slug[..cut].rfind('-') {
        Some(dash) if dash * 2 >= MAX_SLUG_LEN => slug[..dash].to_string(),
        _ => slug[..cut].to_string(),
    }
}

/// Folds Crockford's interchangeable characters: `I`/`L` are `1`, `O` is `0`,
/// case never matters.
pub fn normalize_id(input: &str) -> String {
    input
        .chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'i' | 'l' => '1',
            'o' => '0',
            other => other,
        })
        .collect()
}

/// An id not in `taken` (which holds folded ids), lengthened if the space fills.
pub fn mint_id<S: std::hash::BuildHasher>(taken: &HashSet<String, S>) -> String {
    let mut len = ID_LEN;
    loop {
        for _ in 0..64 {
            let candidate = random_id(len);
            if !taken.contains(&normalize_id(&candidate)) {
                return candidate;
            }
        }
        len += 1;
    }
}

fn random_id(len: usize) -> String {
    let mut id = String::with_capacity(len);
    let mut bits = 0u64;
    for n in 0..len {
        // Redraw every 12 characters, or the shift feeds in zeros.
        if n % CHARS_PER_DRAW == 0 {
            bits = random_bits();
        }
        id.push(CROCKFORD[(bits & 0x1f) as usize] as char);
        bits >>= 5;
    }
    id
}

/// `RandomState` is OS-seeded per instance, so hashing a fixed value gives
/// unpredictable bits without an RNG crate.
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
    fn set_body_keeps_the_frontmatter_exactly() {
        let text = "---\nweird: yes\ntitle: Kept\nsomebody-elses: field\n---\n\nold body\n";
        let out = set_body(text, "new body").unwrap();
        assert_eq!(
            out,
            "---\nweird: yes\ntitle: Kept\nsomebody-elses: field\n---\n\nnew body\n"
        );
    }

    #[test]
    fn set_body_takes_the_carriage_returns_out() {
        let text = "---\ntitle: T\n---\n\nold\n";
        let out = set_body(text, "one\r\ntwo\r\n").unwrap();
        assert_eq!(out, "---\ntitle: T\n---\n\none\ntwo\n");
        assert!(!out.contains('\r'), "{out:?}");
    }

    #[test]
    fn set_body_gives_up_on_a_file_with_no_frontmatter() {
        assert!(set_body("just a markdown file\n", "x").is_none());
    }

    #[test]
    fn set_body_accepts_nothing_at_all() {
        let out = set_body("---\ntitle: T\n---\n\nsomething\n", "").unwrap();
        assert_eq!(out, "---\ntitle: T\n---\n\n");
        assert!(Note::parse(&out).is_ok());
    }

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
    fn slugify_cuts_a_long_title_at_a_word_boundary() {
        let slug = slugify(
            "GitHub - coding-horror/basic-computer-games: An updated version of the \
             classic Basic Computer Games book, with well-written examples in a variety \
             of common MEMORY SAFE, SCRIPTING programming languages.",
        );
        assert_eq!(
            slug,
            "github-coding-horror-basic-computer-games-an-updated-version-of-the-classic-basic-computer-games"
        );
        assert!(slug.len() <= MAX_SLUG_LEN, "{} bytes", slug.len());
        assert!(file_name("k3f9m2p1", &slug).len() <= 255);
    }

    #[test]
    fn slugify_cuts_a_single_long_word_at_the_cap() {
        let slug = slugify(&"a".repeat(300));
        assert_eq!(slug.len(), MAX_SLUG_LEN);
    }

    #[test]
    fn slugify_cuts_cjk_on_a_character_boundary() {
        let slug = slugify(&"漢".repeat(200));
        assert_eq!(slug, "漢".repeat(MAX_SLUG_LEN / 3));
        assert!(slug.len() <= MAX_SLUG_LEN, "{} bytes", slug.len());
    }

    #[test]
    fn slugify_leaves_no_trailing_dash() {
        for extra in 0..8 {
            let slug = slugify(&format!("{} {}", "ab ".repeat(40), "c".repeat(extra)));
            assert!(!slug.ends_with('-'), "{slug}");
            assert!(slug.len() <= MAX_SLUG_LEN, "{slug}");
        }
    }

    #[test]
    fn normalize_id_folds_confusable_characters() {
        assert_eq!(normalize_id("K3F9ABCD"), "k3f9abcd");
        assert_eq!(normalize_id("IL0O"), "1100");
    }

    #[test]
    fn minted_ids_use_the_crockford_alphabet_and_avoid_collisions() {
        let taken: HashSet<String> = HashSet::new();
        let id = mint_id(&taken);
        assert_eq!(id.len(), ID_LEN);
        assert!(id.bytes().all(|b| CROCKFORD.contains(&b)), "{id}");

        let taken: HashSet<String> = std::iter::once(normalize_id(&id)).collect();
        assert_ne!(mint_id(&taken), id);
    }

    #[test]
    fn a_widened_id_stays_random_past_the_first_draw() {
        let long = random_id(24);
        assert_eq!(long.len(), 24);
        assert!(
            long[CHARS_PER_DRAW..].chars().any(|c| c != '0'),
            "the tail past one draw is all zeros: {long}"
        );
    }

    #[test]
    fn a_filename_splits_into_the_id_and_the_slug() {
        assert_eq!(
            split_stem("k3f9m2p1-meeting-notes"),
            Some(("k3f9m2p1", "meeting-notes")),
            "the slug keeps its own hyphens"
        );
        assert_eq!(
            split_stem("k3f9m2p1-會議-筆記"),
            Some(("k3f9m2p1", "會議-筆記"))
        );
    }

    #[test]
    fn a_slug_that_merely_starts_with_a_hyphenated_word_is_not_an_id() {
        assert_eq!(
            split_stem("c-vs-rust"),
            None,
            "`c` is too short to be an id"
        );
        assert_eq!(split_stem("meeting-notes"), None);
        assert_eq!(split_stem("nohyphen"), None);
        assert_eq!(split_stem("k3f9m2p1"), None, "an id with no slug after it");
        assert_eq!(split_stem("k3f9m2p1-"), None, "an empty slug");
        // `u` is not in the alphabet.
        assert_eq!(split_stem("untitled-thing"), None);
    }

    #[test]
    fn a_plausible_looking_filename_is_still_id_shaped() {
        assert!(is_id_shaped("abcdefgh"));
        assert_eq!(split_stem("abcdefgh-hello"), Some(("abcdefgh", "hello")));
    }

    #[test]
    fn render_and_parse_round_trip() {
        let note = Note {
            title: "Meeting notes".into(),
            tags: vec!["work".into(), "q3".into()],
            created: Some("2019-03-14T08:21:00Z".into()),
            updated: Some("2024-11-02T16:40:12Z".into()),
            pinned: None,
            extra: Vec::new(),
            body: "Body line one.\nBody line two.\n".into(),
        };
        let text = note.render();
        assert!(text.starts_with(
            "---\ntitle: Meeting notes\ntags: [work, q3]\ncreated: 2019-03-14T08:21:00Z\nupdated: 2024-11-02T16:40:12Z\n---\n\n"
        ));
        assert!(!text.contains("id:"), "the id is the filename: {text}");
        assert_eq!(Note::parse(&text).unwrap(), note);
    }

    #[test]
    fn a_pin_round_trips_after_the_stamps() {
        let note = Note {
            title: "Meeting notes".into(),
            tags: Vec::new(),
            created: None,
            updated: Some("2024-11-02T16:40:12Z".into()),
            pinned: Some(PINNED.into()),
            extra: vec!["source: elsewhere".into()],
            body: "hi\n".into(),
        };
        let text = note.render();
        assert!(
            text.starts_with(
                "---\ntitle: Meeting notes\nupdated: 2024-11-02T16:40:12Z\npinned: true\n\
                 source: elsewhere\n---\n\n"
            ),
            "{text}"
        );
        assert_eq!(Note::parse(&text).unwrap(), note);
        assert!(note.is_pinned());
    }

    #[test]
    fn an_unpinned_note_writes_no_line() {
        let text = "---\ntitle: Alpha\n---\n\nbody\n";
        let mut note = Note::parse(text).unwrap();
        assert!(!note.is_pinned());
        assert_eq!(note.render(), text);

        note.pinned = Some(PINNED.into());
        assert!(note.render().contains("pinned: true"));
        note.pinned = None;
        assert_eq!(note.render(), text);
    }

    #[test]
    fn a_pin_is_true_and_nothing_else_but_survives_being_something_else() {
        for (value, pinned) in [
            ("true", true),
            ("True", true),
            ("TRUE", true),
            ("false", false),
            ("yes", false),
            ("1", false),
            ("", false),
        ] {
            let text = format!("---\ntitle: Alpha\npinned: {value}\n---\n\nbody\n");
            let note = Note::parse(&text).unwrap();
            assert_eq!(note.is_pinned(), pinned, "pinned: {value}");
            assert_eq!(note.render(), text, "pinned: {value}");
        }
    }

    #[test]
    fn parse_tolerates_missing_tags_and_colons_in_titles() {
        let note = Note::parse("---\ntitle: Rust: a tour\n---\n\nhi\n").unwrap();
        assert_eq!(note.title, "Rust: a tour");
        assert!(note.tags.is_empty());
        assert_eq!(note.body, "hi\n");
    }

    #[test]
    fn a_stray_id_field_is_ignored_rather_than_obeyed() {
        let note = Note::parse("---\nid: zzzz\ntitle: Alpha\n---\n\nbody\n").unwrap();
        assert_eq!(note.title, "Alpha");
        assert_eq!(note.extra, ["id: zzzz"]);
        assert!(note.render().contains("id: zzzz"));
    }

    #[test]
    fn fields_noda_does_not_know_survive_a_write_back() {
        let text =
            "---\ntitle: Imported\nsource_id: 4821\nstarred: true\ntags: [work]\n---\n\nbody\n";
        let mut note = Note::parse(text).unwrap();
        assert_eq!(note.extra, ["source_id: 4821", "starred: true"]);

        note.tags.push("q3".into());
        let rewritten = note.render();
        assert!(rewritten.contains("source_id: 4821"), "{rewritten}");
        assert!(rewritten.contains("starred: true"), "{rewritten}");
        assert_eq!(Note::parse(&rewritten).unwrap(), note);
    }

    #[test]
    fn unknown_fields_keep_their_sequence_but_move_below_the_known_ones() {
        let note = Note::parse("---\nzebra: 1\ntitle: Alpha\nalpha: 2\n---\n\nbody\n").unwrap();
        assert_eq!(note.extra, ["zebra: 1", "alpha: 2"]);
        assert_eq!(
            note.render(),
            "---\ntitle: Alpha\nzebra: 1\nalpha: 2\n---\n\nbody\n"
        );
    }

    #[test]
    fn a_time_written_somewhere_else_is_not_restated() {
        let text = "---\ntitle: Imported\ncreated: 2019-03-14T16:21:00+08:00\n---\n\nbody\n";
        let note = Note::parse(text).unwrap();
        assert_eq!(note.created.as_deref(), Some("2019-03-14T16:21:00+08:00"));
        assert!(note.render().contains("created: 2019-03-14T16:21:00+08:00"));
    }

    #[test]
    fn the_time_noda_writes_is_fixed_width_utc() {
        let now = now();
        assert_eq!(now.len(), 20, "{now}");
        assert!(now.ends_with('Z'), "{now}");
        assert!(now.parse::<jiff::Timestamp>().is_ok(), "{now}");
    }

    #[test]
    fn setting_a_field_moves_nothing_else() {
        let text = "---\nzebra: 1\nupdated: old\ntitle: Alpha\n---\n\nbody\n";
        assert_eq!(
            set_field(text, "updated", "new").unwrap(),
            "---\nzebra: 1\nupdated: new\ntitle: Alpha\n---\n\nbody\n"
        );
    }

    #[test]
    fn setting_a_field_that_is_not_there_yet_appends_it() {
        let text = "---\ntitle: Alpha\n---\n\nbody\n";
        assert_eq!(
            set_field(text, "updated", "new").unwrap(),
            "---\ntitle: Alpha\nupdated: new\n---\n\nbody\n"
        );
        assert_eq!(
            set_field("---\n---\n\nbody\n", "updated", "new").unwrap(),
            "---\nupdated: new\n---\n\nbody\n"
        );
        assert_eq!(set_field("no frontmatter\n", "updated", "new"), None);
    }

    #[test]
    fn setting_a_duplicated_field_collapses_it() {
        let text = "---\nupdated: a\ntitle: Alpha\nupdated: b\n---\n\nbody\n";
        assert_eq!(
            set_field(text, "updated", "new").unwrap(),
            "---\nupdated: new\ntitle: Alpha\n---\n\nbody\n"
        );
    }

    #[test]
    fn a_frontmatter_line_without_a_colon_is_carried_as_well() {
        let note = Note::parse("---\ntitle: Alpha\n# a comment\n---\n\nbody\n").unwrap();
        assert_eq!(note.extra, ["# a comment"]);
        assert_eq!(Note::parse(&note.render()).unwrap(), note);
    }

    #[test]
    fn values_that_would_not_survive_the_round_trip_are_refused() {
        assert!(validate_title("Meeting Notes").is_ok());
        let err = validate_title("Meeting\ntitle: other")
            .unwrap_err()
            .to_string();
        assert!(err.contains("one line"), "{err}");
        assert!(validate_title("Meeting\rnotes").is_err());

        assert!(validate_tag("work").is_ok());
        assert!(validate_tag("會議").is_ok());
        for bad in ["", "work, secret", "a]", "[a", "two\nlines"] {
            assert!(validate_tag(bad).is_err(), "{bad} should be refused");
        }
    }

    #[test]
    fn parse_rejects_files_without_frontmatter() {
        assert!(Note::parse("just markdown\n").is_err());
        assert!(Note::parse("# A heading\n\nprose\n").is_err());
        assert!(Note::parse("---\n---\n\nbody\n").is_ok());
    }
}
