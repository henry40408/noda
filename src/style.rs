//! The palette. Deliberately small: colour marks what a line *is*, never what
//! it means, and noda never colours a note's own text — that is the user's file.
//!
//! Nothing here decides whether colour is wanted. `anstream` strips the escapes
//! when the output is not a terminal, so commands can style unconditionally and
//! a piped `noda show` still emits exactly the bytes on disk.

use anstyle::{AnsiColor, Style};

/// Commit ids, in `log`.
pub const COMMIT: Style = AnsiColor::Yellow.on_default();
/// A note's id, in `ls`. The same yellow as a commit id on purpose: both are
/// the short string you copy out of a listing and hand to the next command.
pub const ID: Style = COMMIT;
/// A note's slug, in `ls -l`. [`ID`]'s colour a step down, because the two
/// columns side by side are the note's filename — `<id>-<slug>.md` — and
/// reading them as one thing is the point.
pub const SLUG: Style = AnsiColor::Yellow.on_default().dimmed();
/// A note's tags, in `ls`. The one column that groups notes rather than
/// naming one, so it gets a hue of its own.
pub const TAGS: Style = AnsiColor::Cyan.on_default();
/// The `[`, `,` and `]` around and between those tags: the brackets are how you
/// know a tag list is a tag list, but they are not the part you read, so they
/// step back and leave the tags the only [`TAGS`]-coloured thing in the column.
///
/// Grey rather than [`TAGS`] dimmed, which is what this was first. A colour and
/// not an effect: `dim` is a request a terminal may answer with a half-blend, a
/// different palette entry, or nothing at all, and where it answers with nothing
/// a dimmed cyan *is* cyan — the distinction this exists to draw disappears on
/// exactly the terminals that cannot show it. [`SLUG`] is dimmed and stays that
/// way; it sits beside the id it belongs to, so if the dim is dropped the pair
/// still reads correctly. Here the two halves are interleaved, and losing the
/// difference loses the point.
pub const TAGS_PUNCT: Style = AnsiColor::BrightBlack.on_default();
/// Timestamps and other supporting detail.
pub const MUTED: Style = Style::new().dimmed();
/// The `+` side of a diff.
pub const ADDED: Style = AnsiColor::Green.on_default();
/// The `-` side of a diff.
pub const REMOVED: Style = AnsiColor::Red.on_default();
/// `@@` hunk headers.
pub const HUNK: Style = AnsiColor::Cyan.on_default();
/// File headers in a diff.
pub const HEADING: Style = Style::new().bold();
/// The part of a search result that matched.
pub const MATCH: Style = AnsiColor::Yellow.on_default().bold();
/// Why what has been typed is not a query yet, in `noda tui`'s search line.
///
/// No command needs this: a query the CLI cannot parse ends the command, and
/// `main` prints the reason as `noda: …`. Filtering as you type has no such
/// moment — half a query is what every query looks like on the way to being one
/// — so the reason has to sit somewhere on the screen instead.
pub const INVALID: Style = AnsiColor::Red.on_default();
/// A due date that has passed, in `todo`.
///
/// The one exception to the rule above, and it is worth naming as one: this
/// colours a row for what it *means*, not for what it is. It earns the
/// exception by being the only thing anybody scans a todo list for — a list
/// that does not distinguish late from not-yet is a list you have to read
/// rather than glance at. Nothing else may follow it without the same argument.
pub const OVERDUE: Style = AnsiColor::Red.on_default();
/// The names along the top of a table, in `noda tui`.
///
/// The second thing here no command needs, and for the same shape of reason as
/// [`INVALID`]: a listing printed into a pipe is read once, by somebody who
/// asked for those columns by name a moment earlier, and `noda ls -l` has never
/// wanted a heading. A browser is stood in front of for a sitting, and the two
/// timestamps `-l` adds are the same twenty characters twice — which one is
/// `created` is not a thing to work out from the values.
///
/// Grey and bold, which is [`TAGS_PUNCT`]'s argument one band up: a heading is
/// not the part you read, so it steps back by hue rather than by an effect a
/// terminal may decline, and the bold is what keeps it from reading as another
/// row of dimmed data.
pub const COLUMN: Style = AnsiColor::BrightBlack.on_default().bold();
/// The bar down the left of the row the cursor is on, in `noda tui`.
///
/// [`ID`]'s yellow, because the row it marks is a note and the id is how a note
/// is pointed at everywhere else. The row itself is only emboldened: reversing
/// it — which is what this replaced — inverts the id's yellow and the tags' cyan
/// along with everything else, so the one row you are looking at is the one row
/// whose columns have stopped being told apart by colour.
pub const CURSOR: Style = AnsiColor::Yellow.on_default();

/// Wraps `text` in `style`. The `:#` form writes the reset sequence.
pub fn paint(style: Style, text: &str) -> String {
    format!("{style}{text}{style:#}")
}

/// A tag list cut into the pieces that are coloured differently: the brackets
/// and separators in [`TAGS_PUNCT`], each tag in [`TAGS`]. Empty tags give no
/// pieces at all — a note without tags writes nothing, not `[]`.
///
/// Here rather than in either caller because there are two writers of this one
/// column — `ls` emits escape sequences, `tui` builds ratatui spans — and the
/// thing they must agree on is *where the cuts fall*. Splitting it twice is how
/// the browser's column and the listing's stop being the same string.
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

        // Read back without the escapes, it is still the row `ls` has always
        // printed — the colouring changed, the text did not.
        let plain: String = pieces.into_iter().map(|(_, text)| text).collect();
        assert_eq!(plain, "[work, q3]");
    }
}
