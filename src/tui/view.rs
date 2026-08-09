//! Drawing one frame.
//!
//! Every screen is the same five bands — the header, what this screen is of, the
//! screen itself, how deep you are, and what was last said — and only the middle
//! one is drawn here. The rest is [`super::frame`], which is what makes a screen
//! added later look like the ones already there rather than like itself.
//!
//! The listing is the same row `noda ls` prints — the id, the title, then the
//! tags — for the reason that row was settled on in the first place: a note is
//! named the same way wherever it is named, so what you read here is what you
//! would have read in a pipe. It gets the whole width now, which is the width
//! the row was designed for.
//!
//! A note is `noda show`: the frontmatter dimmed, the note's own text left
//! alone. The one thing painted over the prose is the search match, and that is
//! the exception `noda search` already makes when it quotes a hit back.

use ratatui::Frame;
use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Padding, Paragraph, Row, Table, Wrap};

use super::app::{App, Mode, View, What};
use super::frame::{self, card, plural};
use super::theme;
use crate::cmd::find_ignoring_case;
use crate::style as palette;

/// How wide the key column on the help card is, so the descriptions line up: the
/// longest set of keys on one row, which is what the column is for.
const KEY_COLUMN: usize = 22;

/// What a marked note carries in front of its id, and what an unmarked one
/// carries in its place. The same width, so nothing moves.
const MARK: &str = "• ";
const UNMARKED: &str = "  ";

/// The gap between the listing's columns, and how much of the title is kept
/// readable however long the tags get.
const COLUMN_GAP: usize = 2;
const TITLE_FLOOR: usize = 10;

/// How far the body is held off either edge, so nothing on it is written into
/// the corner of the terminal.
const PADDING: u16 = 1;

/// What the card has to say that the band along the top does not.
///
/// The keys for the screen you are on are up there, named and always visible, so
/// this is the rest: how to move, what the filter takes, and the two keys that
/// open and close a screen. Ten rows and a border, which is what fits on a
/// terminal short enough to have made the point once already.
const KEYS: &[(&str, &str)] = &[
    ("j / k, ↓ / ↑", "move · scroll"),
    ("ctrl-f / ctrl-b, g / G", "half a screen · first / last"),
    ("enter, esc", "open it · back out of it"),
    ("/", "filter: tag:work OR tag:q3 budget"),
    ("space, *", "mark this one · mark all the filter shows"),
    ("e, a", "edit in $EDITOR · new note"),
    ("m, #", "retitle · tags: +work -\"two words\""),
    ("ctrl-d, T", "delete (after a y) · leave updated alone"),
    ("Q", "the queue: what is waiting · send it"),
    ("r, q / ctrl-c", "read the notebook again · quit"),
];

pub fn draw(f: &mut Frame, app: &mut App) {
    let area = f.area();
    let [header, title, body, crumbs, status] = ratatui::layout::Layout::vertical([
        Constraint::Length(frame::header_rows(area.height)),
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(area);

    app.set_page(body.height);
    frame::draw_header(f, header, app);
    frame::draw_title(f, title, app);
    if matches!(app.view(), View::Notes) {
        draw_listing(f, body, app);
    } else {
        draw_note(f, body, app);
    }
    frame::draw_crumbs(f, crumbs, app);
    if let Some(x) = frame::draw_status(f, status, app) {
        f.set_cursor_position((x, status.y));
    }
    // Over the top of all of it, and only ever one of them: a card is what the
    // keyboard is doing, so there is nothing else it could be doing at the time.
    match app.mode {
        Mode::Help => draw_help(f, area),
        Mode::Confirm(what) => draw_confirm(f, area, app, what),
        Mode::Queue => draw_queue(f, area, app),
        Mode::Alert => draw_alert(f, area, app),
        _ => {}
    }
}

fn draw_listing(f: &mut Frame, area: Rect, app: &mut App) {
    if app.shown() == 0 {
        let nothing = if app.total() == 0 {
            "this notebook has no notes yet"
        } else {
            "nothing matches"
        };
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                nothing,
                theme::from(palette::MUTED),
            )))
            .block(Block::new().padding(Padding::horizontal(PADDING))),
            area,
        );
        return;
    }

    // Taken out and put back so the rows may borrow the notes while ratatui
    // writes this frame's scroll offset into the state. They are different
    // fields, but the borrow checker only sees `app`.
    let mut state = app.take_table();

    // The mark lives in front of the id rather than in a column of its own, and
    // it is as wide when there is nothing to show as when there is: a listing
    // that shifted sideways the moment you marked something would make the
    // marking harder to read than the mark is worth.
    let id_width = MARK.chars().count()
        + app
            .rows()
            .map(|file| file.id.chars().count())
            .max()
            .unwrap_or(0);
    // As wide as the longest tag list, unless that would starve the title.
    //
    // A tag may be a sentence — `24.04 Dark patterns` is what an import leaves
    // behind — and a column sized to the longest one can take a narrow screen
    // whole, leaving the title nothing at all. So the title is given a floor
    // first and the tags get what is left: a note is found by its title, and a
    // cut tag list still says there are tags. Short tag lists, which is nearly
    // all of them, are not affected by this at all.
    // Measured against what the row actually gets, which is the width less the
    // padding on either side of it. Counting the padding as usable is how the
    // title ends up one column short of the floor it was promised.
    let inner = (area.width as usize).saturating_sub(2 * PADDING as usize);
    let room = inner.saturating_sub(id_width + 2 * COLUMN_GAP + TITLE_FLOOR);
    let tag_width = app
        .rows()
        .map(|file| tags(&file.note.tags).chars().count())
        .max()
        .unwrap_or(0)
        .min(room);

    let terms = app.terms().to_vec();
    let rows: Vec<Row> = app
        .rows()
        .map(|file| {
            Row::new(vec![
                Line::from(vec![
                    Span::styled(
                        if app.marked(&file.id) { MARK } else { UNMARKED },
                        theme::from(palette::MATCH),
                    ),
                    Span::styled(file.id.as_str(), theme::from(palette::ID)),
                ]),
                // The title is the column the eye lands on, so it is the one
                // left uncoloured — the same reason `noda ls` leaves it alone.
                marked(&file.note.title, &terms, Style::default()),
                Line::from(Span::styled(
                    tags(&file.note.tags),
                    theme::from(palette::TAGS),
                )),
            ])
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Length(id_width as u16),
            Constraint::Fill(1),
            Constraint::Length(tag_width as u16),
        ],
    )
    .block(Block::new().padding(Padding::horizontal(PADDING)))
    .column_spacing(COLUMN_GAP as u16)
    .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED))
    .highlight_symbol("");

    f.render_stateful_widget(table, area, &mut state);
    app.put_table(state);
}

fn draw_note(f: &mut Frame, area: Rect, app: &App) {
    let block = Block::new().padding(Padding::horizontal(PADDING));
    let Some(reading) = &app.reading else {
        f.render_widget(block, area);
        return;
    };
    f.render_widget(
        Paragraph::new(lines(&reading.text, app.terms()))
            .block(block)
            // Wrapped rather than cut: a note is prose, and a reader who has to
            // scroll sideways to finish a sentence is not reading.
            .wrap(Wrap { trim: false })
            .scroll((app.scroll(), 0)),
        area,
    );
}

fn draw_help(f: &mut Frame, area: Rect) {
    let keys: Vec<Line> = KEYS
        .iter()
        .map(|(key, what)| {
            Line::from(vec![
                Span::styled(format!("{key:>KEY_COLUMN$}  "), theme::from(palette::ID)),
                Span::raw(*what),
            ])
        })
        .collect();
    card(f, area, " keys ", keys, theme::from(palette::MUTED));
}

/// The question `noda rm` does not ask.
///
/// At a prompt a delete is a command you typed on purpose; here it is one chord,
/// so it is asked for. It cannot be asked for the way the rest of noda asks —
/// the terminal is in raw mode, and a command reading a line from stdin would be
/// reading the keystrokes out from under the browser.
fn draw_confirm(f: &mut Frame, area: Rect, app: &App, what: What) {
    let muted = theme::from(palette::MUTED);
    let queued = || {
        // The queue is described by what it will do, not by how many keys were
        // pressed to build it.
        Line::from(format!(
            "{} over {}",
            plural(app.queue.len(), "change"),
            plural(app.queued_notes(), "note")
        ))
    };
    let (title, subject, aside, keys) = match what {
        What::Delete => {
            let Some(file) = app.selected() else {
                return;
            };
            (
                " delete this note? ",
                Line::from(vec![
                    Span::styled(file.id.as_str(), theme::from(palette::ID)),
                    Span::raw("  "),
                    Span::raw(file.note.title.as_str()),
                ]),
                "the commit that removes it stays, so git revert brings it back".to_string(),
                "y  delete       any other key  keep it",
            )
        }
        // The deletions are counted out on their own, because they are the
        // reason the question is being asked at all.
        What::Send => (
            " send the queue? ",
            queued(),
            format!(
                "{} to be deleted — the commit stays, so git revert brings them back",
                plural(app.queued_deletions(), "note")
            ),
            "y  send it       any other key  back to the queue",
        ),
        // Not a warning about a change: a warning about work that has not been
        // written down anywhere and will not survive the process.
        What::Quit => (
            " leave the queue behind? ",
            queued(),
            "none of it has happened, and none of it is written down anywhere".to_string(),
            "y  quit anyway       any other key  stay",
        ),
    };
    let lines = vec![
        subject,
        Line::default(),
        Line::from(Span::styled(aside, muted)),
        Line::default(),
        Line::from(Span::styled(keys, muted)),
    ];
    card(f, area, title, lines, muted);
}

/// What is waiting to be sent, and what can be done about it.
///
/// Each line is the same sentence the commit message will use, so what is read
/// before sending is what the history says afterwards.
fn draw_queue(f: &mut Frame, area: Rect, app: &App) {
    let muted = theme::from(palette::MUTED);
    let mut lines: Vec<Line> = if app.queue.is_empty() {
        vec![Line::from(Span::styled(
            "nothing queued — mark some notes, then # or ctrl-d",
            muted,
        ))]
    } else {
        app.queue
            .iter()
            .enumerate()
            .map(|(at, step)| {
                let style = if at == app.queue_at() {
                    Style::default().add_modifier(Modifier::REVERSED)
                } else {
                    Style::default()
                };
                Line::from(Span::styled(step.describe(), style))
            })
            .collect()
    };
    lines.push(Line::default());
    lines.push(Line::from(Span::styled(
        "enter  send, in one commit       d  drop this one       esc  back",
        muted,
    )));
    card(f, area, " queued ", lines, muted);
}

/// Why a command would not do what it was asked, or what it had to say that one
/// line could not hold.
///
/// A card rather than the status bar, because the part worth reading is the part
/// that does not fit: `edit` says where it left a file whose frontmatter no
/// longer parses, and `bulk` says what it could not do underneath what it did.
fn draw_alert(f: &mut Frame, area: Rect, app: &App) {
    let Some(said) = &app.message else {
        return;
    };
    let lines: Vec<Line> = said.text.lines().map(Line::raw).collect();
    let (title, border) = if said.failed {
        (" no ", theme::from(palette::INVALID))
    } else {
        (" done ", theme::from(palette::MUTED))
    };
    card(f, area, title, lines, border);
}

/// A note's tags as the listing writes them, or nothing at all. Tags are the one
/// thing a note may not have, which is why they are the last column: an empty
/// cell here shifts nothing.
fn tags(tags: &[String]) -> String {
    if tags.is_empty() {
        String::new()
    } else {
        format!("[{}]", tags.join(", "))
    }
}

/// The file as the screen shows it: the frontmatter pushed into the background
/// so the note reads first, and the search terms picked out of the prose.
fn lines<'a>(text: &'a str, terms: &[String]) -> Vec<Line<'a>> {
    let muted = theme::from(palette::MUTED);
    let (frontmatter, body) = split_frontmatter(text);
    let mut out: Vec<Line> = frontmatter
        .lines()
        .map(|line| Line::from(Span::styled(line, muted)))
        .collect();
    out.extend(
        body.lines()
            .map(|line| marked(line, terms, Style::default())),
    );
    out
}

/// Splits the file after the closing `---`. Nothing is dimmed when the block is
/// not there to dim: `dim_frontmatter` makes the same judgement, and a file the
/// screen cannot read this way is one the reader should see as it stands.
fn split_frontmatter(text: &str) -> (&str, &str) {
    let Some(rest) = text.strip_prefix("---\n") else {
        return ("", text);
    };
    let Some(end) = rest.find("\n---\n") else {
        return ("", text);
    };
    text.split_at("---\n".len() + end + "\n---\n".len())
}

/// One line with every occurrence of a term picked out.
///
/// The earliest match wins where two terms overlap, and the search resumes after
/// it — so a line is walked once no matter how many terms are in the query.
fn marked<'a>(text: &'a str, terms: &[String], base: Style) -> Line<'a> {
    let mut spans: Vec<Span<'a>> = Vec::new();
    let mut at = 0;
    while at < text.len() {
        let hit = terms
            .iter()
            .filter_map(|term| find_ignoring_case(&text[at..], term))
            .min_by_key(|(start, _)| *start);
        let Some((start, end)) = hit else { break };
        let (start, end) = (at + start, at + end);
        if start > at {
            spans.push(Span::styled(&text[at..start], base));
        }
        spans.push(Span::styled(&text[start..end], theme::from(palette::MATCH)));
        at = end;
    }
    if at < text.len() || spans.is_empty() {
        spans.push(Span::styled(&text[at..], base));
    }
    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn terms(text: &str) -> Vec<String> {
        text.split(' ').map(str::to_string).collect()
    }

    #[test]
    fn a_line_with_nothing_to_mark_is_one_span() {
        let line = marked("Meeting notes", &[], Style::default());
        assert_eq!(line.spans.len(), 1);
        assert_eq!(line.to_string(), "Meeting notes");
    }

    #[test]
    fn the_match_is_picked_out_of_the_line_it_sits_in() {
        let line = marked("the q3 budget is late", &terms("budget"), Style::default());
        let marked_spans: Vec<&str> = line
            .spans
            .iter()
            .filter(|span| span.style == theme::from(palette::MATCH))
            .map(|span| span.content.as_ref())
            .collect();
        assert_eq!(marked_spans, vec!["budget"]);
        assert_eq!(line.to_string(), "the q3 budget is late");
    }

    #[test]
    fn every_occurrence_is_marked_and_the_line_survives_whole() {
        let line = marked("budget, budget, budget", &terms("budget"), Style::default());
        let hits = line
            .spans
            .iter()
            .filter(|span| span.style == theme::from(palette::MATCH))
            .count();
        assert_eq!(hits, 3);
        assert_eq!(line.to_string(), "budget, budget, budget");
    }

    #[test]
    fn a_match_is_found_whatever_case_it_was_written_in() {
        let line = marked("The Q3 Budget", &terms("q3"), Style::default());
        let marked_spans: Vec<&str> = line
            .spans
            .iter()
            .filter(|span| span.style == theme::from(palette::MATCH))
            .map(|span| span.content.as_ref())
            .collect();
        // Marked as written, not as searched: the note's own text is what is on
        // screen, and only its colour changes.
        assert_eq!(marked_spans, vec!["Q3"]);
    }

    #[test]
    fn a_match_in_a_language_without_spaces_keeps_its_boundaries() {
        let line = marked("這是會議紀錄", &terms("會議"), Style::default());
        assert_eq!(line.to_string(), "這是會議紀錄");
        let marked_spans: Vec<&str> = line
            .spans
            .iter()
            .filter(|span| span.style == theme::from(palette::MATCH))
            .map(|span| span.content.as_ref())
            .collect();
        assert_eq!(marked_spans, vec!["會議"]);
    }

    #[test]
    fn the_frontmatter_is_dimmed_and_the_note_is_not() {
        let file = "---\ntitle: Meeting notes\ntags: [work]\n---\n\n# Agenda\nbudget\n";
        let rendered = lines(file, &[]);
        let muted = theme::from(palette::MUTED);
        assert_eq!(rendered[0].to_string(), "---");
        assert!(rendered[0].spans.iter().all(|span| span.style == muted));
        let agenda = rendered
            .iter()
            .find(|line| line.to_string() == "# Agenda")
            .expect("the body is there");
        assert!(agenda.spans.iter().all(|span| span.style != muted));
    }

    #[test]
    fn a_file_with_no_frontmatter_is_shown_as_it_stands() {
        let (front, body) = split_frontmatter("just prose\n");
        assert_eq!(front, "");
        assert_eq!(body, "just prose\n");

        // An opening fence that never closes is not a block either.
        let (front, body) = split_frontmatter("---\ntitle: unfinished\n");
        assert_eq!(front, "");
        assert_eq!(body, "---\ntitle: unfinished\n");
    }

    #[test]
    fn tags_are_written_the_way_the_listing_writes_them() {
        assert_eq!(tags(&[]), "");
        assert_eq!(tags(&["work".to_string(), "q3".to_string()]), "[work, q3]");
    }

    #[test]
    fn the_help_card_still_fits_a_short_terminal() {
        // The card outgrew a twenty-four row terminal once; the keys moved into
        // the header partly so it would not again. Ten rows and two of border.
        assert!(KEYS.len() + 2 <= 14, "the card has {} rows", KEYS.len() + 2);
        // And the column is as wide as the widest set of keys on it, or the
        // descriptions stop lining up.
        let widest = KEYS.iter().map(|(key, _)| key.chars().count()).max();
        assert_eq!(widest, Some(KEY_COLUMN));
    }
}
