//! The palette. Colour marks what a line *is*, never what it means, and never a
//! note's own text.
//!
//! Whether colour is wanted is not decided here: `anstream` strips it off a
//! terminal, so a piped `noda show` emits exactly the bytes on disk.

use anstyle::{AnsiColor, Style};

pub const COMMIT: Style = AnsiColor::Yellow.on_default();
/// [`COMMIT`]'s yellow: both are the short string you copy into the next
/// command.
pub const ID: Style = COMMIT;
/// [`ID`] a step down: side by side they are the filename `<id>-<slug>.md`.
pub const SLUG: Style = AnsiColor::Yellow.on_default().dimmed();
pub const TAGS: Style = AnsiColor::Cyan.on_default();
/// The `[`, `,` and `]` around and between the tags.
///
/// Grey, not [`TAGS`] dimmed: a terminal may ignore `dim`, and interleaved with
/// the tags the difference would vanish. ([`SLUG`] can risk it, being beside
/// the id rather than mixed in.)
pub const TAGS_PUNCT: Style = AnsiColor::BrightBlack.on_default();
/// Timestamps and other supporting detail.
pub const MUTED: Style = Style::new().dimmed();
pub const ADDED: Style = AnsiColor::Green.on_default();
pub const REMOVED: Style = AnsiColor::Red.on_default();
/// `@@` hunk headers.
pub const HUNK: Style = AnsiColor::Cyan.on_default();
/// File headers in a diff.
pub const HEADING: Style = Style::new().bold();
/// The part of a search result that matched.
pub const MATCH: Style = AnsiColor::Yellow.on_default().bold();
/// Why what has been typed is not a query yet, in `noda tui`'s search line
/// (the CLI just fails on a bad query).
pub const INVALID: Style = AnsiColor::Red.on_default();
/// A due date that has passed, in `todo`.
///
/// An exception to the rule above — it colours what a row *means* — because it
/// is what anybody scans a todo list for.
pub const OVERDUE: Style = AnsiColor::Red.on_default();
/// The names along the top of a table, in `noda tui` only: a TUI is sat in
/// front of, and `-l`'s two timestamps look alike without a heading.
///
/// Grey steps back by hue, as [`TAGS_PUNCT`] does; bold keeps it from reading
/// as another row of data.
pub const COLUMN: Style = AnsiColor::BrightBlack.on_default().bold();
/// The mark on a pinned row. [`OVERDUE`]'s exception again: a pin exists to be
/// seen from across the listing.
pub const PIN: Style = AnsiColor::Magenta.on_default();
/// What that mark says, shared by `ls`, `tui` and `web`. A word, not an emoji,
/// whose width depends on the terminal's font and would misalign the row.
pub const PIN_MARK: &str = "pinned";
/// The bar down the left of the row the cursor is on, in `noda tui`, in
/// [`ID`]'s yellow. The row is only emboldened: reverse video would invert the
/// columns' colours on the one row being looked at.
pub const CURSOR: Style = AnsiColor::Yellow.on_default();

/// Wraps `text` in `style`. The `:#` form writes the reset sequence.
pub fn paint(style: Style, text: &str) -> String {
    format!("{style}{text}{style:#}")
}

/// A tag list cut into differently coloured pieces, so `ls` (escapes) and `tui`
/// (spans) cut it the same way. No tags give no pieces, not `[]`.
pub fn tag_pieces(tags: &[String]) -> Vec<(Style, String)> {
    if tags.is_empty() {
        return Vec::new();
    }
    let mut pieces = vec![(TAGS_PUNCT, "[".to_string())];
    for (i, tag) in tags.iter().enumerate() {
        if i > 0 {
            pieces.push((TAGS_PUNCT, ", ".to_string()));
        }
        pieces.push((TAGS, tag.clone()));
    }
    pieces.push((TAGS_PUNCT, "]".to_string()));
    pieces
}

/// The same pieces, painted and joined — the tag list as a listing writes it.
pub fn tags(tags: &[String]) -> String {
    tag_pieces(tags)
        .iter()
        .map(|(style, text)| paint(*style, text))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tag_list_is_cut_between_the_tags_and_the_punctuation() {
        assert!(tag_pieces(&[]).is_empty());

        let pieces = tag_pieces(&["work".to_string(), "q3".to_string()]);
        assert_eq!(
            pieces,
            vec![
                (TAGS_PUNCT, "[".to_string()),
                (TAGS, "work".to_string()),
                (TAGS_PUNCT, ", ".to_string()),
                (TAGS, "q3".to_string()),
                (TAGS_PUNCT, "]".to_string()),
            ]
        );

        let plain: String = pieces.into_iter().map(|(_, text)| text).collect();
        assert_eq!(plain, "[work, q3]");
    }
}
