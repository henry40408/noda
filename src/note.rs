//! A note: the Markdown file that carries it, and the identity its filename
//! spells out.
//!
//! The id lives in the filename. git forbids two entries sharing a path in one
//! tree, so uniqueness is structural rather than policed — two machines each
//! adding a note write two filenames that merge without a conflict.
//!
//! The frontmatter carries the title (a slug is lossy) and the tags, and its
//! *presence* is what marks a file as a note at all — see `notebook::Scan`.
//! noda interprets those fields and no others, but does not own the block: any
//! other field survives a write-back untouched.

use std::collections::HashSet;
use std::fmt::Write as _;

use crate::{Error, Result};

/// Crockford base32 — `i`, `l`, `o` and `u` are absent, so an id can't be misread.
const CROCKFORD: &[u8; 32] = b"0123456789abcdefghjkmnpqrstvwxyz";

/// 40 bits, minted against what the notebook holds — long enough that a
/// collision takes deliberate effort, short enough to read in a filename.
pub const ID_LEN: usize = 8;

/// How many base32 characters one draw of randomness can supply.
const CHARS_PER_DRAW: usize = 12;

/// Fallback slug for a title that contains nothing sluggable.
const FALLBACK_SLUG: &str = "note";

/// The id is not here — it is the filename.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    pub title: String,
    pub tags: Vec<String>,
    /// RFC 3339, exactly as found in the file. `Option` because a note may
    /// predate the fields, and never respelled: an imported
    /// `2019-03-14T16:21:00+08:00` keeps its offset, or `noda show` would stop
    /// matching the file.
    pub created: Option<String>,
    pub updated: Option<String>,
    /// Frontmatter lines noda does not interpret, in the order they were read.
    /// A note from elsewhere carries fields noda has never heard of, and losing
    /// them on the first `tag add` loses the only copy.
    ///
    /// None can be `---`: such a line would have closed the block instead of
    /// landing here, which is what keeps `render` from writing what `parse`
    /// cannot read.
    pub extra: Vec<String>,
    pub body: String,
}

impl Note {
    /// The block is always written, even empty: it still says "this file is a
    /// note", which a bare `.md` dropped into the notebook cannot.
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
        // After noda's own fields, never interleaved: their order among
        // themselves is preserved, their position relative to `title` is not.
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
        let mut extra = Vec::new();
        for line in frontmatter.lines() {
            // A line noda cannot even split into a field is still somebody's.
            let Some((key, value)) = line.split_once(':') else {
                extra.push(line.to_string());
                continue;
            };
            let value = value.trim();
            match key.trim() {
                "title" => title = Some(value.to_string()),
                "tags" => tags = parse_tags(value),
                // Kept as written: refusing to read the note would put a typo
                // between somebody and their own prose. `doctor --times`
                // reports it instead.
                "created" => created = Some(value.to_string()),
                "updated" => updated = Some(value.to_string()),
                // Every other field belongs to whoever wrote it.
                _ => extra.push(line.to_string()),
            }
        }

        Ok(Note {
            title: title.unwrap_or_default(),
            tags,
            created,
            updated,
            extra,
            body: body.trim_start_matches('\n').to_string(),
        })
    }
}

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

/// RFC 3339, UTC, whole seconds. UTC because one note must not claim two times
/// depending on which machine opened it; whole seconds because nothing consumes
/// finer. Together they make it fixed-width, so it sorts as text.
pub fn now() -> String {
    jiff::Timestamp::now()
        .strftime("%Y-%m-%dT%H:%M:%SZ")
        .to_string()
}

/// Sets one frontmatter field, leaving every other byte alone.
///
/// `render` would also set it, but it rebuilds the block and so moves
/// uninterpreted fields below noda's own — fine in `tag` and `mv`, which rewrite
/// the frontmatter anyway, and not in `edit`, where it would reorder a block
/// somebody just arranged by hand as the price of recording that they did.
pub fn set_field(text: &str, key: &str, value: &str) -> Option<String> {
    let (frontmatter, body) = split_frontmatter(text)?;

    let mut out = String::from("---\n");
    let mut written = false;
    for line in frontmatter.lines() {
        if line.split_once(':').is_some_and(|(k, _)| k.trim() == key) {
            // `parse` reads one value, so a duplicate gets no second one.
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

/// The body replaced, the frontmatter block left exactly as found.
///
/// [`set_field`]'s sibling, for its reason: rebuilding from a parsed [`Note`] is
/// right for a note noda made and wrong for one another program wrote, whose
/// arrangement is the only copy of itself.
///
/// Line endings are normalised because the one caller holding a body rather than
/// a file is a browser, and the HTML specification says a form submits `CRLF` —
/// writing those through would put a carriage return on every line of every note
/// edited from a phone, invisibly, for ever.
pub fn set_body(text: &str, body: &str) -> Option<String> {
    let (frontmatter, _) = split_frontmatter(text)?;
    let mut out = String::from("---\n");
    out.push_str(frontmatter);
    // `render`'s shape, so a reader cannot tell which of the two wrote it.
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

/// The filename a note with this id and slug lives under.
pub fn file_name(id: &str, slug: &str) -> String {
    format!("{id}-{slug}.md")
}

/// The id alphabet has no `-`, so the first one is the boundary however long
/// ids grow. The length floor stops an ordinary slug being read as one: without
/// it `c-vs-rust` is the id `c` and every hand-written note claims an identity.
pub fn split_stem(stem: &str) -> Option<(&str, &str)> {
    let (id, slug) = stem.split_once('-')?;
    if slug.is_empty() || !is_id_shaped(id) {
        return None;
    }
    Some((id, slug))
}

/// `.md` alone does not decide it: a notebook is one flat directory holding
/// Markdown that is not a note, `README.md` above all. This is the test
/// `Notebook::inventory` applies, so anything needing the same answer from a
/// name alone asks here rather than re-deciding.
pub fn names_a_note(name: &str) -> bool {
    name.strip_suffix(".md").and_then(split_stem).is_some()
}

/// Whether a string could be an id noda minted: long enough, and drawn entirely
/// from the alphabet ids use.
pub fn is_id_shaped(text: &str) -> bool {
    text.len() >= ID_LEN
        && text
            .bytes()
            .all(|b| CROCKFORD.contains(&b.to_ascii_lowercase()))
}

/// A title goes into the frontmatter verbatim, so a second line in it becomes a
/// field. Refusing it keeps `render` and `parse` inverse without an escaping
/// syntax every hand-edited note would have to speak.
pub fn validate_title(title: &str) -> Result<()> {
    if title.contains(['\n', '\r']) {
        return Err(Error::msg("a title has to fit on one line"));
    }
    Ok(())
}

/// Tags share the frontmatter's punctuation, so a tag carrying `,` or `[]`
/// reads back as something else. `validate_title`'s reasoning.
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
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'i' | 'l' => '1',
            'o' => '0',
            other => other,
        })
        .collect()
}

/// Mints an id that is not already `taken`, widening the id space if it fills up.
/// `taken` holds folded ids, the way `resolve` compares them.
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
        // One draw carries 60 usable bits. Past that the shift would be feeding
        // in zeros, and every character after the twelfth would be `0`.
        if n % CHARS_PER_DRAW == 0 {
            bits = random_bits();
        }
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

    /// Byte for byte, or a note from another program is quietly rewritten the
    /// first time a browser edits it.
    #[test]
    fn set_body_keeps_the_frontmatter_exactly() {
        let text = "---\nweird: yes\ntitle: Kept\nsomebody-elses: field\n---\n\nold body\n";
        let out = set_body(text, "new body").unwrap();
        assert_eq!(
            out,
            "---\nweird: yes\ntitle: Kept\nsomebody-elses: field\n---\n\nnew body\n"
        );
    }

    /// The HTML specification says a form submits `CRLF` — every browser.
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

    /// A body emptied on purpose is a body.
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

    /// One draw runs out after twelve characters, and a longer id used to be
    /// padded with the zeros the shift kept supplying.
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

    /// The floor that stops an ordinary slug claiming to be an id.
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
        // `u` is not in the alphabet, so this eight-character word is not one.
        assert_eq!(split_stem("untitled-thing"), None);
    }

    /// A filename can look exactly like a note's without being one.
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
    fn parse_tolerates_missing_tags_and_colons_in_titles() {
        let note = Note::parse("---\ntitle: Rust: a tour\n---\n\nhi\n").unwrap();
        assert_eq!(note.title, "Rust: a tour");
        assert!(note.tags.is_empty());
        assert_eq!(note.body, "hi\n");
    }

    /// A leftover `id:` is another field with no authority over identity — it
    /// survives the round trip, it just never gets obeyed.
    #[test]
    fn a_stray_id_field_is_ignored_rather_than_obeyed() {
        let note = Note::parse("---\nid: zzzz\ntitle: Alpha\n---\n\nbody\n").unwrap();
        assert_eq!(note.title, "Alpha");
        assert_eq!(note.extra, ["id: zzzz"]);
        assert!(note.render().contains("id: zzzz"));
    }

    /// Why `extra` exists: `tag`/`mv`/`add` all write back through `render`,
    /// and dropping an unknown field there destroys the only copy.
    #[test]
    fn fields_noda_does_not_know_survive_a_write_back() {
        let text =
            "---\ntitle: Imported\nsource_id: 4821\nstarred: true\ntags: [work]\n---\n\nbody\n";
        let mut note = Note::parse(text).unwrap();
        assert_eq!(note.extra, ["source_id: 4821", "starred: true"]);

        // What `noda tag add` does to a note it read.
        note.tags.push("q3".into());
        let rewritten = note.render();
        assert!(rewritten.contains("source_id: 4821"), "{rewritten}");
        assert!(rewritten.contains("starred: true"), "{rewritten}");
        assert_eq!(Note::parse(&rewritten).unwrap(), note);
    }

    /// Order among themselves is kept; position relative to `title` is not,
    /// which is a one-off reordering rather than a loss.
    #[test]
    fn unknown_fields_keep_their_sequence_but_move_below_the_known_ones() {
        let note = Note::parse("---\nzebra: 1\ntitle: Alpha\nalpha: 2\n---\n\nbody\n").unwrap();
        assert_eq!(note.extra, ["zebra: 1", "alpha: 2"]);
        assert_eq!(
            note.render(),
            "---\ntitle: Alpha\nzebra: 1\nalpha: 2\n---\n\nbody\n"
        );
    }

    /// noda writes UTC but never restates what it reads — `noda show` has to
    /// keep matching the file byte for byte.
    #[test]
    fn a_time_written_somewhere_else_is_not_restated() {
        let text = "---\ntitle: Imported\ncreated: 2019-03-14T16:21:00+08:00\n---\n\nbody\n";
        let note = Note::parse(text).unwrap();
        assert_eq!(note.created.as_deref(), Some("2019-03-14T16:21:00+08:00"));
        assert!(note.render().contains("created: 2019-03-14T16:21:00+08:00"));
    }

    /// Fixed width, so it sorts as text without being parsed.
    #[test]
    fn the_time_noda_writes_is_fixed_width_utc() {
        let now = now();
        assert_eq!(now.len(), 20, "{now}");
        assert!(now.ends_with('Z'), "{now}");
        assert!(now.parse::<jiff::Timestamp>().is_ok(), "{now}");
    }

    /// `edit` records a change without rearranging what somebody just arranged.
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
        // An empty block is still a block, and still gets the field.
        assert_eq!(
            set_field("---\n---\n\nbody\n", "updated", "new").unwrap(),
            "---\nupdated: new\n---\n\nbody\n"
        );
        assert_eq!(set_field("no frontmatter\n", "updated", "new"), None);
    }

    /// `parse` reads one value, so no second one may be left behind.
    #[test]
    fn setting_a_duplicated_field_collapses_it() {
        let text = "---\nupdated: a\ntitle: Alpha\nupdated: b\n---\n\nbody\n";
        assert_eq!(
            set_field(text, "updated", "new").unwrap(),
            "---\nupdated: new\ntitle: Alpha\n---\n\nbody\n"
        );
    }

    /// A line noda cannot split into a field is still somebody's.
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

    /// The block is the declaration "this file is a note".
    #[test]
    fn parse_rejects_files_without_frontmatter() {
        assert!(Note::parse("just markdown\n").is_err());
        assert!(Note::parse("# A heading\n\nprose\n").is_err());
        // An empty block still declares it.
        assert!(Note::parse("---\n---\n\nbody\n").is_ok());
    }
}
