//! The action items a note's body carries.
//!
//! A todo is a GFM checkbox in a note, not a note and not a file of its own —
//! chosen so anything else reading Markdown renders it as a checkbox, the same
//! bargain attachments make.
//!
//! Parsed rather than grepped, for `link.rs`'s reason: `- [ ]` inside a fence is
//! prose *about* a checkbox, and getting that wrong puts something on a todo
//! list its author never put there.

use std::cmp::Ordering;

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

/// One unticked checkbox, as `noda todo` reports it.
pub struct Item {
    /// Inline markup flattened, `due:` lifted out.
    pub text: String,
    /// `YYYY-MM-DD`, when the item named one.
    pub due: Option<String>,
}

impl Item {
    /// A string comparison, correct only because `YYYY-MM-DD` sorts as text the
    /// way it sorts as a date — the reason `split_due` accepts one spelling.
    ///
    /// `today` is passed in because whose today it is matters: nobody writes
    /// `due:2026-08-10` meaning UTC. All three screens get it from `cmd::today`.
    pub fn overdue(&self, today: &str) -> bool {
        self.due.as_deref().is_some_and(|due| due < today)
    }
}

/// Every unticked checkbox in `body`, in the order they are written. A finished
/// item stays exactly where its author wrote it.
///
/// The text stops at the end of the first paragraph — where a list item stops
/// being a line and starts being a section — and inline markup is flattened, so
/// `[the spec](spec.md)` reads as `the spec`.
pub fn items(body: &str) -> Vec<Item> {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TASKLISTS);

    let mut found = Vec::new();
    // `Some` from the marker until the item's first paragraph ends.
    let mut collecting: Option<String> = None;
    let mut depth = 0usize;

    for event in Parser::new_ext(body, options) {
        match event {
            // Always first in its item, so it closes whatever came before —
            // which is how a nested box ends the text it is nested inside.
            Event::TaskListMarker(ticked) => {
                flush(&mut found, &mut collecting);
                depth = 0;
                if !ticked {
                    collecting = Some(String::new());
                }
            }
            Event::Text(text) | Event::Code(text) => {
                if let Some(item) = &mut collecting {
                    item.push_str(&text);
                }
            }
            // The text is one line by the time it is printed.
            Event::SoftBreak | Event::HardBreak => {
                if let Some(item) = &mut collecting {
                    item.push(' ');
                }
            }
            Event::Start(tag) => {
                if is_inline(&tag) {
                    // Depth, so inline markup does not end the item at its own
                    // closing tag.
                    if collecting.is_some() {
                        depth += 1;
                    }
                } else if !matches!(tag, Tag::Paragraph) {
                    // Any other opening is a new block, and the item's text
                    // stopped before it. A paragraph is exempt because a loose
                    // list wraps the item's own text in one.
                    flush(&mut found, &mut collecting);
                    depth = 0;
                }
            }
            Event::End(
                TagEnd::Emphasis
                | TagEnd::Strong
                | TagEnd::Strikethrough
                | TagEnd::Link
                | TagEnd::Image,
            ) => {
                depth = depth.saturating_sub(1);
            }
            Event::End(TagEnd::Paragraph | TagEnd::Item) if depth == 0 => {
                flush(&mut found, &mut collecting);
            }
            _ => {}
        }
    }
    flush(&mut found, &mut collecting);
    found
}

/// Soonest first, undated last: an item without a date has made no claim about
/// when it happens. Ties fall back to the slug so a listing does not reshuffle.
///
/// Written once because `noda todo` and the browser's todo screen both print
/// this list, and disagreeing would look like a bug in whichever you asked
/// second.
pub fn order((left_slug, left): (&str, &Item), (right_slug, right): (&str, &Item)) -> Ordering {
    match (&left.due, &right.due) {
        (Some(left), Some(right)) => left.cmp(right),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
    .then_with(|| left_slug.cmp(right_slug))
}

/// Markup that lives inside a paragraph rather than replacing it.
fn is_inline(tag: &Tag<'_>) -> bool {
    matches!(
        tag,
        Tag::Emphasis | Tag::Strong | Tag::Strikethrough | Tag::Link { .. } | Tag::Image { .. }
    )
}

fn flush(found: &mut Vec<Item>, collecting: &mut Option<String>) {
    if let Some(item) = collecting.take() {
        push(found, item);
    }
}

/// `- [ ]` on its own is a box nobody has written the task into yet.
fn push(found: &mut Vec<Item>, text: String) {
    let (text, due) = split_due(text.trim());
    if text.is_empty() {
        return;
    }
    found.push(Item { text, due });
}

/// Lifts a `due:YYYY-MM-DD` term out of the text — todo.txt's `key:value`
/// shape, which stays prose to every other renderer. Only what gets printed is
/// affected; the file is never touched.
///
/// The last one wins: two due dates is somebody editing rather than declaring.
fn split_due(text: &str) -> (String, Option<String>) {
    let mut due = None;
    let mut kept: Vec<&str> = Vec::new();
    for word in text.split_whitespace() {
        match word.strip_prefix("due:").filter(|rest| is_date(rest)) {
            Some(date) => due = Some(date.to_string()),
            // `due:tomorrow` included — a term noda cannot read is prose.
            None => kept.push(word),
        }
    }
    (kept.join(" "), due)
}

/// Deliberately not a full date parse: the shape is what makes the column sort
/// as text, and `2026-13-99` is still the author's word about their own
/// deadline. `doctor --times` is where noda argues with a date.
fn is_date(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && [0, 1, 2, 3, 5, 6, 8, 9]
            .iter()
            .all(|at| bytes[*at].is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(body: &str) -> Vec<String> {
        items(body).into_iter().map(|item| item.text).collect()
    }

    #[test]
    fn only_unticked_boxes_come_back() {
        let found = texts("- [ ] open\n- [x] done\n- [ ] also open\n");
        assert_eq!(found, ["open", "also open"]);
    }

    #[test]
    fn a_list_item_without_a_box_is_not_a_todo() {
        assert!(texts("- just a bullet\n\nand a paragraph\n").is_empty());
    }

    /// The reason this parses rather than greps, exactly as in `link.rs`.
    #[test]
    fn a_box_inside_a_code_block_is_prose_about_a_box() {
        assert!(texts("```\n- [ ] not mine\n```\n").is_empty());
        assert!(texts("    - [ ] not mine either\n").is_empty());
    }

    #[test]
    fn every_bullet_and_every_depth_counts() {
        let found = texts("- [ ] dash\n\n* [ ] star\n\n+ [ ] plus\n\n1. [ ] ordered\n");
        assert_eq!(found, ["dash", "star", "plus", "ordered"]);
        assert_eq!(texts("- [ ] outer\n  - [ ] inner\n"), ["outer", "inner"]);
    }

    #[test]
    fn inline_markup_is_flattened() {
        assert_eq!(
            texts("- [ ] read [the spec](spec.md) **today**\n"),
            ["read the spec today"]
        );
        assert_eq!(texts("- [ ] run `cargo test`\n"), ["run cargo test"]);
    }

    #[test]
    fn an_item_stops_at_its_first_paragraph() {
        assert_eq!(texts("- [ ] the task\n\n  a note about it\n"), ["the task"]);
    }

    #[test]
    fn a_line_break_inside_an_item_becomes_a_space() {
        assert_eq!(texts("- [ ] one\n  two\n"), ["one two"]);
    }

    #[test]
    fn an_empty_box_is_not_an_item() {
        assert!(texts("- [ ]\n").is_empty());
    }

    #[test]
    fn a_due_date_is_lifted_out_of_the_text() {
        let found = items("- [ ] send the contract due:2026-08-10 to legal\n");
        assert_eq!(found[0].text, "send the contract to legal");
        assert_eq!(found[0].due.as_deref(), Some("2026-08-10"));
    }

    #[test]
    fn a_term_that_is_not_a_date_stays_in_the_prose() {
        let found = items("- [ ] ask about due:tomorrow\n");
        assert_eq!(found[0].text, "ask about due:tomorrow");
        assert_eq!(found[0].due, None);
    }

    #[test]
    fn the_last_due_date_wins() {
        let found = items("- [ ] due:2026-01-01 moved due:2026-03-01\n");
        assert_eq!(found[0].text, "moved");
        assert_eq!(found[0].due.as_deref(), Some("2026-03-01"));
    }
}
