//! The files a note's body points at.
//!
//! This is the one place noda reads a note as Markdown rather than as text. It
//! has to: a link is not a string that looks like a filename. Reference-style
//! links keep their destination at the bottom of the file, so the paragraph that
//! uses one never contains it; a link inside a code fence is not a link at all;
//! and `%20` in a destination is a space in a filename. Getting any of those
//! wrong reports a file as unreferenced when a note references it perfectly
//! well, which is the one failure a report about orphans cannot afford.
//!
//! Only local destinations come back. Anything carrying a URL scheme is somebody
//! else's file, and anything that climbs out of the notebook is not the
//! notebook's business.

use std::collections::BTreeSet;

use pulldown_cmark::{Event, Parser, Tag};

/// Every notebook-relative path the body links to or embeds, deduplicated.
///
/// Paths come back normalised — `./` dropped, `%20` decoded, any `#fragment` or
/// `?query` cut off — so they can be compared against a directory listing
/// directly.
pub fn targets(body: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    for event in Parser::new(body) {
        // Both tags carry a destination and both mean "this note uses that
        // file": an image is displayed and a link is followed, which is a
        // difference to the reader and none to the file.
        let Event::Start(Tag::Link { dest_url, .. } | Tag::Image { dest_url, .. }) = event else {
            continue;
        };
        if let Some(path) = local_path(&dest_url) {
            found.insert(path);
        }
    }
    found
}

/// The notebook-relative file a destination names, or `None` when it does not
/// name one.
fn local_path(dest: &str) -> Option<String> {
    // A fragment alone points inside this very document, and a scheme points at
    // something noda does not own — neither is a file in the notebook.
    if dest.is_empty() || dest.starts_with('#') || has_scheme(dest) {
        return None;
    }
    // An absolute path may well exist, but it is not a file the notebook holds,
    // so nothing here can be said about it.
    if dest.starts_with('/') {
        return None;
    }

    // `diagram.png#page=2` and `diagram.png?v=2` both name `diagram.png`. A
    // filename may legally contain either character on this platform, but one
    // that does cannot be carried by a notebook that syncs to Windows, so
    // reading them as a destination's punctuation is the safer bet.
    let path = dest.split_once('#').map_or(dest, |(before, _)| before);
    let path = path.split_once('?').map_or(path, |(before, _)| before);
    normalize(&percent_decode(path))
}

/// Whether the destination starts with a URL scheme (`https:`, `mailto:`, …).
///
/// Spelled out rather than looked for as `://`, because `mailto:` and `tel:`
/// carry no slashes and are just as much somebody else's.
fn has_scheme(dest: &str) -> bool {
    let mut chars = dest.char_indices();
    match chars.next() {
        Some((_, first)) if first.is_ascii_alphabetic() => {}
        _ => return false,
    }
    for (index, ch) in chars {
        match ch {
            ':' => return index > 0,
            c if c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.' => {}
            _ => return false,
        }
    }
    false
}

/// Decodes `%XX` escapes. A destination is percent-encoded, so a filename with a
/// space in it arrives as `%20` and would match nothing on disk.
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && let (Some(high), Some(low)) = (hex(bytes[index + 1]), hex(bytes[index + 2]))
        {
            out.push(high * 16 + low);
            index += 3;
            continue;
        }
        out.push(bytes[index]);
        index += 1;
    }
    // A destination that does not decode to UTF-8 is not naming a file noda can
    // compare against a listing, so it stands as it was written.
    String::from_utf8(out).unwrap_or_else(|_| text.to_string())
}

fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// Collapses `.` and `..` so two spellings of one file compare equal, and
/// refuses a path that climbs out of the notebook.
fn normalize(path: &str) -> Option<String> {
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                // Nothing to pop means the path has left the notebook, and a
                // file outside it is not one the notebook can account for.
                parts.pop()?;
            }
            other => parts.push(other),
        }
    }
    if parts.is_empty() {
        return None;
    }
    Some(parts.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn targets_of(body: &str) -> Vec<String> {
        targets(body).into_iter().collect()
    }

    #[test]
    fn inline_links_and_images_both_count() {
        assert_eq!(
            targets_of("![diagram](diagram.png) and [the spec](spec.pdf)\n"),
            ["diagram.png", "spec.pdf"]
        );
    }

    /// The case a regex cannot reach: the paragraph naming the file does not
    /// contain it.
    #[test]
    fn a_reference_style_link_is_resolved() {
        let body = "See ![the diagram][d] for the shape.\n\n[d]: diagram.png\n";
        assert_eq!(targets_of(body), ["diagram.png"]);
    }

    /// The other case a regex cannot reach, and the one that would invent
    /// orphans: prose about a link is not a link.
    #[test]
    fn a_link_inside_a_code_fence_is_not_a_link() {
        let body = "```markdown\n![diagram](diagram.png)\n```\n\nand `[x](y.png)` inline\n";
        assert!(targets_of(body).is_empty(), "{:?}", targets_of(body));
    }

    #[test]
    fn destinations_that_name_no_file_in_the_notebook_are_dropped() {
        let body = "[a](https://example.com/x.png)\n\
                    [b](http://example.com)\n\
                    [c](mailto:me@example.com)\n\
                    [d](#section)\n\
                    [e](/etc/passwd)\n\
                    [f](../outside.png)\n";
        assert!(targets_of(body).is_empty(), "{:?}", targets_of(body));
    }

    #[test]
    fn a_destination_is_normalized_before_it_is_compared() {
        assert_eq!(targets_of("[a](./diagram.png)\n"), ["diagram.png"]);
        assert_eq!(targets_of("[a](my%20file.pdf)\n"), ["my file.pdf"]);
        assert_eq!(targets_of("[a](diagram.png#page=2)\n"), ["diagram.png"]);
        assert_eq!(targets_of("[a](sub/./dir/../f.png)\n"), ["sub/f.png"]);
    }

    /// Two spellings of one file must not report as one referenced and one
    /// orphaned.
    #[test]
    fn two_spellings_of_one_file_collapse_to_one_target() {
        assert_eq!(
            targets_of("[a](./diagram.png) [b](diagram.png) ![c](diagram.png)\n"),
            ["diagram.png"]
        );
    }

    #[test]
    fn a_link_to_another_note_is_a_target_like_any_other() {
        assert_eq!(
            targets_of("[see](k3f9m2p1-other.md)\n"),
            ["k3f9m2p1-other.md"]
        );
    }
}
