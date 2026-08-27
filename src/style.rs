//! The palette. Colour marks what a line *is*, never what it means, and noda
//! never colours a note's own text — that is the user's file.
//!
//! Nothing here decides whether colour is wanted: `anstream` strips the escapes
//! off a terminal, so commands style unconditionally and a piped `noda show`
//! emits exactly the bytes on disk.

use anstyle::{AnsiColor, Style};

pub const COMMIT: Style = AnsiColor::Yellow.on_default();
/// Deliberately [`COMMIT`]'s yellow: both are the short string you copy out of
/// a listing and hand to the next command.
pub const ID: Style = COMMIT;
/// [`ID`] a step down, because the two columns side by side are the note's
/// filename — `<id>-<slug>.md` — and reading them as one thing is the point.
pub const SLUG: Style = AnsiColor::Yellow.on_default().dimmed();
pub const TAGS: Style = AnsiColor::Cyan.on_default();
/// The `[`, `,` and `]` around and between the tags.
///
/// Grey rather than [`TAGS`] dimmed, which is what this was first: `dim` is a
/// request a terminal may answer with nothing at all, and where it does, a
/// dimmed cyan *is* cyan — the distinction disappears on exactly the terminals
/// that cannot show it. [`SLUG`] survives the same loss because it sits beside
/// the id it belongs to; here the two halves are interleaved.
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
/// Why what has been typed is not a query yet, in `noda tui`'s search line.
///
/// No command needs this — a query the CLI cannot parse ends the command. But
/// half a query is what every query looks like on the way to being one, so
/// filtering as you type has to put the reason somewhere on the screen.
pub const INVALID: Style = AnsiColor::Red.on_default();
/// A due date that has passed, in `todo`.
///
/// The one exception to the rule above, and worth naming as one: it colours a
/// row for what it *means*. It earns that by being the only thing anybody scans
/// a todo list for. Nothing else may follow without the same argument.
pub const OVERDUE: Style = AnsiColor::Red.on_default();
/// The names along the top of a table, in `noda tui`.
///
/// [`INVALID`]'s shape of reason: a piped listing is read once by somebody who
/// named those columns a moment earlier, but a browser is sat in front of, and
/// `-l`'s two timestamps are the same twenty characters twice.
///
/// Grey and bold is [`TAGS_PUNCT`]'s argument one band up — it steps back by
/// hue rather than by an effect a terminal may decline, and the bold keeps it
/// from reading as another row of dimmed data.
pub const COLUMN: Style = AnsiColor::BrightBlack.on_default().bold();
/// The bar down the left of the row the cursor is on, in `noda tui`.
///
/// [`ID`]'s yellow, because the row it marks is a note. The row itself is only
/// emboldened: reversing it — which is what this replaced — inverts the id's
/// yellow and the tags' cyan too, so the one row you are looking at is the one
/// row whose columns have stopped being told apart by colour.
pub const CURSOR: Style = AnsiColor::Yellow.on_default();

/// Wraps `text` in `style`. The `:#` form writes the reset sequence.
pub fn paint(style: Style, text: &str) -> String {
    format!("{style}{text}{style:#}")
}

/// A tag list cut into the pieces that are coloured differently. Empty tags give
/// no pieces at all — a note without tags writes nothing, not `[]`.
///
/// Here rather than in either caller because two writers emit this column — `ls`
/// emits escape sequences, `tui` builds ratatui spans — and what they must agree
/// on is *where the cuts fall*.
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

        // The colouring changed, the text did not.
        let plain: String = pieces.into_iter().map(|(_, text)| text).collect();
        assert_eq!(plain, "[work, q3]");
    }
}
