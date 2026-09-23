//! The files a note's body points at. Parsed as Markdown, not grepped: a
//! reference-style destination sits at the bottom, a link inside a fence is not
//! a link, and `%20` is a space — each, got wrong, reports a referenced file as
//! an orphan. Only local destinations inside the notebook come back.

use std::collections::BTreeSet;
use std::ops::Range;

use pulldown_cmark::{Event, Parser, Tag};

/// Every notebook-relative path the body links to or embeds, normalised to
/// compare against a directory listing.
pub fn targets(body: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    for event in Parser::new(body) {
        let Event::Start(Tag::Link { dest_url, .. } | Tag::Image { dest_url, .. }) = event else {
            continue;
        };
        if let Some(path) = target(&dest_url) {
            found.insert(path);
        }
    }
    found
}

/// Repoints every destination (inline or reference definition) resolving to
/// `old` at `new`, touching only the destination's bytes. `None` when the body
/// names `old` nowhere.
///
/// A destination written with backslash escapes cannot be located in the
/// source, so the caller re-checks with `targets` — see `cmd::file_mv`.
pub fn rewrite(body: &str, old: &str, new: &str) -> Option<String> {
    let encoded = encode_destination(new);
    let mut edits: Vec<(Range<usize>, &str)> = Vec::new();

    let parser = Parser::new(body);
    // Read before the iterator is consumed.
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
        // A reference-style usage's bytes are in the definition, handled below.
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

/// Where inside `range` the destination's *path* was written; a `#page=2` or
/// `?v=2` suffix stays put.
fn locate(body: &str, range: &Range<usize>, dest: &str) -> Option<Range<usize>> {
    let source = body.get(range.clone())?;
    let at = written_at(source, dest)?;
    let path = dest.find(['#', '?']).unwrap_or(dest.len());
    Some(range.start + at..range.start + at + path)
}

/// `[diagram.png](diagram.png)` writes it twice, so the occurrence where a
/// destination opens wins. If that is ambiguous, give up rather than guess.
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

/// Encodes what would otherwise end a destination or start its title.
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

/// The notebook-relative file a destination names. Also what `web` uses to
/// decide which file it will serve, so there is one "inside the notebook" check.
pub fn target(dest: &str) -> Option<String> {
    if dest.is_empty() || dest.starts_with('#') || has_scheme(dest) {
        return None;
    }
    if dest.starts_with('/') {
        return None;
    }

    // `diagram.png#x` and `diagram.png?x` both name `diagram.png`: neither
    // character survives in a filename synced to Windows.
    let path = dest.split_once('#').map_or(dest, |(before, _)| before);
    let path = path.split_once('?').map_or(path, |(before, _)| before);
    normalize(&percent_decode(path))
}

/// The URL scheme a destination carries (not matched as `://`: `mailto:` has
/// none). The name matters to `web`, which follows `https:` but must never run
/// `javascript:`.
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

/// Collapses `.` and `..`; `None` for a path that climbs out of the notebook.
fn normalize(path: &str) -> Option<String> {
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
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

    #[test]
    fn a_reference_style_link_is_resolved() {
        let body = "See ![the diagram][d] for the shape.\n\n[d]: diagram.png\n";
        assert_eq!(targets_of(body), ["diagram.png"]);
    }

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

    #[test]
    fn rewrite_encodes_a_new_name_that_would_not_survive_verbatim() {
        let out = rewrite("![a](diagram.png)\n", "diagram.png", "my shape (v2).png").unwrap();
        assert_eq!(out, "![a](my%20shape%20%28v2%29.png)\n");
        assert_eq!(
            targets(&out).into_iter().next().unwrap(),
            "my shape (v2).png"
        );
    }

    #[test]
    fn the_link_text_is_not_mistaken_for_the_destination() {
        let out = rewrite("[diagram.png](diagram.png)\n", "diagram.png", "shape.png").unwrap();
        assert_eq!(out, "[diagram.png](shape.png)\n");
    }

    #[test]
    fn a_destination_that_cannot_be_located_is_left_for_the_caller_to_notice() {
        let body = "[a](my\\(file\\).png)\n";
        assert_eq!(targets(body).into_iter().next().unwrap(), "my(file).png");
        assert!(rewrite(body, "my(file).png", "shape.png").is_none());
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
