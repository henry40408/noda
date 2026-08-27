//! Reading a `TiddlyWiki` 5 export.
//!
//! Two shapes arrive: the JSON array "export all" produces, and a saved
//! single-file wiki carrying that same array in a `<script>`. Nothing here
//! converts anything — the body goes through as the wiki held it, because the
//! import commits it that way first.
//!
//! What the fields mean is `TiddlyWiki`'s to settle:
//!
//! - `created` / `modified` are `YYYYMMDDhhmmssXXX` in **UTC**, milliseconds
//!   included, and become RFC 3339.
//! - `tags` is a space-separated title list, `[[double brackets]]` around any
//!   tag containing a space.
//! - every other field is the wiki's own and is carried through untouched.

use std::collections::BTreeSet;

use serde_json::Value;

use super::Incoming;
use crate::{Error, Result};

/// The fields noda has an opinion about. Everything else is carried through.
const CLAIMED: [&str; 5] = ["title", "text", "tags", "created", "modified"];

/// Fields that describe the wiki's own bookkeeping rather than the note.
const BOOKKEEPING: [&str; 2] = ["revision", "bag"];

/// What a read of an export found.
pub struct Export {
    pub notes: Vec<Incoming>,
    /// Deliberately not imported, and why. Reported rather than dropped.
    pub skipped: Vec<(String, String)>,
}

/// Reads an export, from either a `.json` file or a saved single-file wiki.
pub fn read(text: &str) -> Result<Export> {
    let json = if text.trim_start().starts_with('[') {
        text.trim()
    } else {
        store(text)?
    };
    let Value::Array(tiddlers) = serde_json::from_str(json).map_err(|e| {
        Error::msg(format!(
            "this does not look like a TiddlyWiki export: {e}\n\
             expected the JSON array that `export all` writes, or a saved wiki"
        ))
    })?
    else {
        return Err(Error::msg(
            "a TiddlyWiki export is a JSON array of tiddlers",
        ));
    };

    let mut notes = Vec::new();
    let mut skipped = Vec::new();
    for tiddler in &tiddlers {
        let title = string(tiddler, "title").unwrap_or_default();
        match incoming(tiddler, &title) {
            Ok(note) => notes.push(note),
            Err(why) => skipped.push((title, why)),
        }
    }
    Ok(Export { notes, skipped })
}

/// Script content is raw text and must not be unescaped: doing so rewrites an
/// `&amp;` a tiddler legitimately contains and breaks the JSON around it. A
/// saved wiki holds more than one store, and the biggest is the wiki.
fn store(text: &str) -> Result<&str> {
    const OPEN: &str = r#"<script class="tiddlywiki-tiddler-store" type="application/json">"#;
    let mut best = "";
    let mut rest = text;
    while let Some(at) = rest.find(OPEN) {
        let from = &rest[at + OPEN.len()..];
        let Some(end) = from.find("</script>") else {
            break;
        };
        if from[..end].len() > best.len() {
            best = from[..end].trim();
        }
        rest = &from[end..];
    }
    if best.is_empty() {
        return Err(Error::msg(
            "no tiddler store in this file\n\
             a saved TiddlyWiki holds one; a rendered copy of a wiki does not",
        ));
    }
    Ok(best)
}

/// One tiddler, or the reason it is not a note.
fn incoming(tiddler: &Value, title: &str) -> std::result::Result<Incoming, String> {
    if title.is_empty() {
        return Err("no title".to_string());
    }
    // `TiddlyWiki`'s own namespace: none of it is a note.
    if title.starts_with("$:/") {
        return Err("system tiddler".to_string());
    }
    let kind = string(tiddler, "type").unwrap_or_default();
    if !matches!(
        kind.as_str(),
        "" | "text/vnd.tiddlywiki" | "text/plain" | "text/markdown" | "text/x-markdown"
    ) {
        return Err(format!("not text ({kind})"));
    }
    let body = string(tiddler, "text").unwrap_or_default();
    if body.trim().is_empty() {
        return Err("empty".to_string());
    }

    let mut extra: Vec<String> = Vec::new();
    if let Value::Object(fields) = tiddler {
        for (name, value) in fields {
            if CLAIMED.contains(&name.as_str())
                || BOOKKEEPING.contains(&name.as_str())
                || name == "type"
            {
                continue;
            }
            let Value::String(value) = value else {
                continue;
            };
            // Frontmatter is one line per field, so this cannot be carried
            // silently.
            if value.contains(['\n', '\r']) {
                continue;
            }
            extra.push(format!("{name}: {value}"));
        }
    }

    Ok(Incoming {
        title: title.to_string(),
        body: body.trim_start_matches('\n').to_string(),
        tags: tags(&string(tiddler, "tags").unwrap_or_default()),
        created: string(tiddler, "created").as_deref().and_then(stamp),
        updated: string(tiddler, "modified").as_deref().and_then(stamp),
        extra,
        key: title.to_string(),
    })
}

fn string(tiddler: &Value, field: &str) -> Option<String> {
    match tiddler.get(field) {
        Some(Value::String(s)) => Some(s.clone()),
        _ => None,
    }
}

/// Space-separated, `[[double brackets]]` around anything with a space in it.
/// Deduplicated, because a tiddler may carry the same tag twice.
pub fn tags(field: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut seen = BTreeSet::new();
    let mut rest = field.trim();
    while !rest.is_empty() {
        let (tag, next) = match rest.strip_prefix("[[") {
            Some(after) => match after.split_once("]]") {
                Some((tag, next)) => (tag, next),
                // The rest of the field: a tag nobody can see is worse than an
                // odd one somebody can.
                None => (after, ""),
            },
            None => match rest.split_once(' ') {
                Some((tag, next)) => (tag, next),
                None => (rest, ""),
            },
        };
        let tag = tag.trim();
        if !tag.is_empty() && seen.insert(tag.to_string()) {
            found.push(tag.to_string());
        }
        rest = next.trim_start();
    }
    found
}

/// `YYYYMMDDhhmmssXXX` in UTC to RFC 3339. The milliseconds are kept: noda never
/// restates a stamp, so three digits dropped here are dropped for good.
pub fn stamp(value: &str) -> Option<String> {
    let digits = value.trim();
    if digits.len() < 14 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let at = |from: usize, to: usize| &digits[from..to];
    let millis = if digits.len() >= 17 {
        format!(".{}", at(14, 17))
    } else {
        String::new()
    };
    Some(format!(
        "{}-{}-{}T{}:{}:{}{millis}Z",
        at(0, 4),
        at(4, 6),
        at(6, 8),
        at(8, 10),
        at(10, 12),
        at(12, 14),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_timestamp_becomes_rfc_3339_with_its_milliseconds() {
        assert_eq!(
            stamp("20240515103310243").as_deref(),
            Some("2024-05-15T10:33:10.243Z")
        );
        // Fourteen digits is the same instant without them.
        assert_eq!(
            stamp("20240515103310").as_deref(),
            Some("2024-05-15T10:33:10Z")
        );
        assert_eq!(stamp("").as_deref(), None);
        assert_eq!(stamp("last tuesday").as_deref(), None);
    }

    #[test]
    fn a_title_list_keeps_the_spaces_inside_double_brackets() {
        assert_eq!(tags("one two"), ["one", "two"]);
        assert_eq!(
            tags("[[26.04 Occam's razor]] simple"),
            ["26.04 Occam's razor", "simple"]
        );
        assert_eq!(tags(""), Vec::<String>::new());
        assert_eq!(tags("same same"), ["same"], "said once");
    }

    #[test]
    fn the_fields_noda_does_not_claim_are_carried_through() {
        let export = read(
            r#"[{"title":"A","text":"body","creator":"henry","modifier":"henry",
                 "revision":"0","bag":"default","type":"text/vnd.tiddlywiki"}]"#,
        )
        .unwrap();
        let note = &export.notes[0];
        assert_eq!(note.extra, ["creator: henry", "modifier: henry"]);
        assert!(
            !note.extra.iter().any(|line| line.starts_with("revision")),
            "the wiki's bookkeeping is not the note's"
        );
    }

    #[test]
    fn what_is_not_a_note_is_reported_rather_than_dropped() {
        let export = read(
            r#"[{"title":"$:/config/x","text":"a"},
                {"title":"pic","text":"aGk=","type":"image/png"},
                {"title":"blank","text":"  "},
                {"title":"real","text":"body"}]"#,
        )
        .unwrap();
        assert_eq!(export.notes.len(), 1);
        let reasons: Vec<&str> = export.skipped.iter().map(|(_, why)| why.as_str()).collect();
        assert_eq!(reasons, ["system tiddler", "not text (image/png)", "empty"]);
    }

    #[test]
    fn a_saved_wiki_is_read_out_of_its_store() {
        let wiki = concat!(
            "<html><script class=\"tiddlywiki-tiddler-store\" type=\"application/json\">",
            r#"[{"title":"Small","text":"x"}]"#,
            "</script>\n",
            "<script class=\"tiddlywiki-tiddler-store\" type=\"application/json\">",
            r#"[{"title":"Big","text":"a body that makes this the larger store"}]"#,
            "</script></html>",
        );
        let export = read(wiki).unwrap();
        assert_eq!(export.notes.len(), 1);
        assert_eq!(export.notes[0].title, "Big");
    }

    #[test]
    fn a_file_that_is_not_an_export_says_so() {
        let err = read("# just some markdown\n")
            .err()
            .expect("a markdown file is not an export")
            .to_string();
        assert!(err.contains("no tiddler store"), "{err}");
    }
}
