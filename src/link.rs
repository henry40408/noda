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
use std::ops::Range;

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
        if let Some(path) = target(&dest_url) {
            found.insert(path);
        }
    }
    found
}

/// Rewrites every destination that resolves to `old` so that it resolves to
/// `new`, and returns the new body — or `None` when the body names `old`
/// nowhere and there is nothing to change.
///
/// Only the destination's own bytes are replaced, so the link text, the title
/// and the surrounding prose are left exactly as they were. Both spellings a
/// destination can have are covered: written inline, and written once at the
/// bottom as a reference definition.
///
/// This does not promise to have caught everything. A destination that survives
/// backslash escapes or character references does not appear literally in the
/// source and cannot be located, so the caller checks the result with `targets`
/// rather than trusting it — see `cmd::file_mv`.
pub fn rewrite(body: &str, old: &str, new: &str) -> Option<String> {
    let encoded = encode_destination(new);
    let mut edits: Vec<(Range<usize>, &str)> = Vec::new();

    let parser = Parser::new(body);
    // Taken before the iterator is consumed: the definitions are collected as
    // the document is parsed, and this borrows the table they land in.
    let definitions: Vec<(String, Range<usize>)> = parser
        .reference_definitions()
        .iter()
        .map(|(_, def)| (def.dest.to_string(), def.span.clone()))
        .collect();

    for (event, range) in parser.into_offset_iter() {
        let Event::Start(Tag::Link { dest_url, .. } | Tag::Image { dest_url, .. }) = event else {
            continue;
        };
        if target(&dest_url).as_deref() != Some(old) {
            continue;
        }
        // A reference-style usage carries the destination it resolved to, but
        // the bytes it was written from are in the definition, not here. Nothing
        // to do at the usage: the definition below is the thing to rewrite.
        if let Some(span) = locate(body, &range, &dest_url) {
            edits.push((span, encoded.as_str()));
        }
    }

    for (dest, span) in &definitions {
        if target(dest).as_deref() == Some(old)
            && let Some(span) = locate(body, span, dest)
        {
            edits.push((span, encoded.as_str()));
        }
    }

    if edits.is_empty() {
        return None;
    }

    // Applied back to front so an earlier edit cannot move a later one's bytes.
    edits.sort_by_key(|(span, _)| span.start);
    edits.dedup_by_key(|(span, _)| span.start);
    let mut out = body.to_string();
    for (span, replacement) in edits.into_iter().rev() {
        out.replace_range(span, replacement);
    }
    Some(out)
}

/// Where inside `range` the destination's *path* was written.
///
/// Only the path: a `#page=2` or `?v=2` after it says how to open the file
/// rather than which file it is, so it is left where it was.
fn locate(body: &str, range: &Range<usize>, dest: &str) -> Option<Range<usize>> {
    let source = body.get(range.clone())?;
    let at = written_at(source, dest)?;
    let path = dest.find(['#', '?']).unwrap_or(dest.len());
    Some(range.start + at..range.start + at + path)
}

/// Which occurrence of `dest` inside `source` is the destination.
///
/// `[diagram.png](diagram.png)` writes it twice and only the second is the
/// destination, so an occurrence sitting where a destination opens is preferred
/// over one that merely reads like it. When nothing distinguishes them, this
/// gives up rather than guessing — `targets` is what notices the miss.
fn written_at(source: &str, dest: &str) -> Option<usize> {
    let mut all = Vec::new();
    let mut from = 0;
    while let Some(at) = source[from..].find(dest) {
        all.push(from + at);
        from += at + 1;
    }

    let mut opens = all.iter().copied().filter(|&at| {
        let before = &source[..at];
        before.ends_with("](") || before.ends_with('<') || before.ends_with(": ")
    });
    match (opens.next(), opens.next()) {
        (Some(only), None) => Some(only),
        (None, _) => match all.as_slice() {
            [only] => Some(*only),
            _ => None,
        },
        _ => None,
    }
}

/// Percent-encodes the characters that would otherwise end a destination or
/// start its title, so a filename with a space in it survives being written
/// into a link.
fn encode_destination(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for ch in name.chars() {
        match ch {
            '%' => out.push_str("%25"),
            ' ' => out.push_str("%20"),
            '(' => out.push_str("%28"),
            ')' => out.push_str("%29"),
            '<' => out.push_str("%3C"),
            '>' => out.push_str("%3E"),
            '"' => out.push_str("%22"),
            other => out.push(other),
        }
    }
    out
}

/// The notebook-relative file a destination names, or `None` when it does not
/// name one.
///
/// Public because `web` asks the same question one destination at a time, and
/// for a stricter reason than `targets` has: what comes back is the only thing
/// the server will open on a reader's behalf. A second implementation of "is
/// this path inside the notebook" is exactly the kind of thing that gets one of
/// the two wrong — and the one that would be wrong is the one facing the
/// network.
pub fn target(dest: &str) -> Option<String> {
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

/// The URL scheme a destination carries — `https`, `mailto`, `javascript` — or
/// `None` when it carries none.
///
/// Read out rather than looked for as `://`, because `mailto:` and `tel:` carry
/// no slashes and are just as much somebody else's.
///
/// What the name is, and not merely that there is one, because that is the
/// question the web pages have to ask: `https:` is a link to follow and
/// `javascript:` is a script to run, and telling them apart is the whole of
/// what stops a note from being able to execute anything.
pub fn scheme(dest: &str) -> Option<&str> {
    let mut chars = dest.char_indices();
    match chars.next() {
        Some((_, first)) if first.is_ascii_alphabetic() => {}
        _ => return None,
    }
    for (index, ch) in chars {
        match ch {
            ':' if index > 0 => return Some(&dest[..index]),
            c if c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.' => {}
            _ => return None,
        }
    }
    None
}

fn has_scheme(dest: &str) -> bool {
    scheme(dest).is_some()
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
    fn rewrite_replaces_only_the_destination() {
        let body = "See ![diagram.png](diagram.png \"the shape\") and [x](other.png)\n";
        let out = rewrite(body, "diagram.png", "shape.png").unwrap();
        assert_eq!(
            out, "See ![diagram.png](shape.png \"the shape\") and [x](other.png)\n",
            "the link text, the title and the other link are untouched"
        );
    }

    #[test]
    fn rewrite_reaches_a_reference_definition_at_the_bottom() {
        let body = "See ![the diagram][d].\n\n[d]: diagram.png\n";
        let out = rewrite(body, "diagram.png", "shape.png").unwrap();
        assert_eq!(out, "See ![the diagram][d].\n\n[d]: shape.png\n");
        assert!(targets(&out).contains("shape.png"));
        assert!(!targets(&out).contains("diagram.png"));
    }

    #[test]
    fn rewrite_finds_every_spelling_of_the_same_file() {
        let body = "![a](diagram.png) [b](./diagram.png) [c](diagram.png#page=2)\n";
        let out = rewrite(body, "diagram.png", "shape.png").unwrap();
        assert_eq!(targets(&out), targets("[x](shape.png)"), "{out}");
        assert!(out.contains("#page=2"), "the fragment survives: {out}");
    }

    #[test]
    fn rewrite_leaves_a_body_that_never_named_it_alone() {
        assert!(rewrite("![a](other.png)\n", "diagram.png", "shape.png").is_none());
        assert!(
            rewrite("```\n![a](diagram.png)\n```\n", "diagram.png", "shape.png").is_none(),
            "a link inside a fence is not a link here either"
        );
    }

    /// A name that needs escaping to survive being written into a destination
    /// gets it, so the rewritten link still resolves to the file.
    #[test]
    fn rewrite_encodes_a_new_name_that_would_not_survive_verbatim() {
        let out = rewrite("![a](diagram.png)\n", "diagram.png", "my shape (v2).png").unwrap();
        assert_eq!(out, "![a](my%20shape%20%28v2%29.png)\n");
        assert_eq!(
            targets(&out).into_iter().next().unwrap(),
            "my shape (v2).png"
        );
    }

    /// The link text can read exactly like the destination. Only the one that
    /// sits where a destination opens is the destination.
    #[test]
    fn the_link_text_is_not_mistaken_for_the_destination() {
        let out = rewrite("[diagram.png](diagram.png)\n", "diagram.png", "shape.png").unwrap();
        assert_eq!(out, "[diagram.png](shape.png)\n");
    }

    /// A destination written with backslash escapes does not appear literally in
    /// the source, so it cannot be located. `rewrite` leaves it rather than
    /// guessing, and `targets` is what tells the caller it was missed.
    #[test]
    fn a_destination_that_cannot_be_located_is_left_for_the_caller_to_notice() {
        let body = "[a](my\\(file\\).png)\n";
        assert_eq!(targets(body).into_iter().next().unwrap(), "my(file).png");
        assert!(rewrite(body, "my(file).png", "shape.png").is_none());
        // Which is why the caller re-reads the body instead of trusting it.
        assert!(targets(body).contains("my(file).png"));
    }

    #[test]
    fn a_link_to_another_note_is_a_target_like_any_other() {
        assert_eq!(
            targets_of("[see](k3f9m2p1-other.md)\n"),
            ["k3f9m2p1-other.md"]
        );
    }
}
