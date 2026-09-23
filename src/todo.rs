//! The action items in a note's body: GFM checkboxes, so any other Markdown
//! reader renders them too. Parsed rather than grepped — `- [ ]` inside a fence
//! is not a todo.

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
    /// A string comparison, valid because `YYYY-MM-DD` sorts as text the way it
    /// sorts as a date. `today` is the local date from `cmd::today`, not UTC.
    pub fn overdue(&self, today: &str) -> bool {
        self.due.as_deref().is_some_and(|due| due < today)
    }
}

/// Every unticked checkbox in `body`, in order. The text stops at the end of
/// the item's first paragraph and inline markup is flattened, so
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
            // Always first in its item, so it ends the text of any enclosing item.
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
            Event::SoftBreak | Event::HardBreak => {
                if let Some(item) = &mut collecting {
                    item.push(' ');
                }
            }
            Event::Start(tag) => {
                if is_inline(&tag) {
                    // Depth, so inline markup's closing tag does not end the item.
                    if collecting.is_some() {
                        depth += 1;
                    }
                } else if !matches!(tag, Tag::Paragraph) {
                    // Any other block ends the item's text. A paragraph is
                    // exempt because a loose list wraps the item's text in one.
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

/// Soonest first, undated last, ties by slug so a listing does not reshuffle.
/// Shared so `noda todo` and the browser's todo screen agree.
pub fn order((left_slug, left): (&str, &Item), (right_slug, right): (&str, &Item)) -> Ordering {
    match (&left.due, &right.due) {
        (Some(left), Some(right)) => left.cmp(right),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
    .then_with(|| left_slug.cmp(right_slug))
}

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

/// An empty `- [ ]` is not an item.
fn push(found: &mut Vec<Item>, text: String) {
    let (text, due) = split_due(text.trim());
    if text.is_empty() {
        return;
    }
    found.push(Item { text, due });
}

/// Lifts a todo.txt-style `due:YYYY-MM-DD` term out of the printed text (the
/// file is untouched). The last one wins: two dates means the author moved it.
fn split_due(text: &str) -> (String, Option<String>) {
    let mut due = None;
    let mut kept: Vec<&str> = Vec::new();
    for word in text.split_whitespace() {
        match word.strip_prefix("due:").filter(|rest| is_date(rest)) {
            Some(date) => due = Some(date.to_string()),
            // Includes `due:tomorrow`: a term noda cannot read stays prose.
            None => kept.push(word),
        }
    }
    (kept.join(" "), due)
}

/// Shape only, not a date parse: the shape is what makes it sort as text, and
/// `2026-13-99` is still the author's word.
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
