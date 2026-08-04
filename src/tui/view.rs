//! Drawing one frame.
//!
//! The listing is the same row `noda ls` prints — the id, the title, then the
//! tags — for the reason that row was settled on in the first place: a note is
//! named the same way wherever it is named, so what you read here is what you
//! would have read in a pipe. The flags that extend it are not repeated; a
//! browser has a whole pane in which to show the note itself, which is what
//! `-l`'s extra columns were standing in for.
//!
//! The preview is `noda show`: the frontmatter dimmed, the note's own text left
//! alone. The one thing painted over the prose is the search match, and that is
//! the exception `noda search` already makes when it quotes a hit back.

use ratatui::Frame;
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Row, Table, Wrap};

use super::app::{App, Focus, Mode};
use super::theme;
use crate::cmd::find_ignoring_case;
use crate::style as palette;

/// What the footer says there is to press. Ordered by how soon somebody needs
/// it, not alphabetically.
const HINTS: &str = "j/k move   / search   Tab preview   r reload   ? keys   q quit";

/// How wide the key column on the help card is, so the descriptions line up.
const KEY_COLUMN: usize = 16;

const KEYS: &[(&str, &str)] = &[
    ("j / k, ↓ / ↑", "move"),
    ("Ctrl-d / Ctrl-u", "half a screen"),
    ("g / G", "first / last"),
    ("Tab, h / l", "list ↔ preview"),
    ("/", "search: tag:work OR tag:q3 budget"),
    ("Enter", "read the note · in a query, keep it"),
    ("Esc", "drop the query"),
    ("r", "read the notebook again"),
    ("q, Ctrl-C", "quit"),
];

pub fn draw(frame: &mut Frame, app: &mut App) {
    let [header, body, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(frame.area());
    // The list gets the smaller half: it holds one line per note, and the pane
    // beside it holds a whole note.
    let [left, right] =
        Layout::horizontal([Constraint::Percentage(42), Constraint::Percentage(58)]).areas(body);

    app.set_page(left.height);
    draw_header(frame, header, app);
    draw_list(frame, left, app);
    draw_preview(frame, right, app);
    draw_footer(frame, footer, app);
    if app.mode == Mode::Help {
        draw_help(frame, frame.area());
    }
}

fn draw_header(frame: &mut Frame, area: Rect, app: &App) {
    let muted = theme::from(palette::MUTED);
    let counts = format!(
        "{} {}",
        app.status.notes,
        if app.status.notes == 1 {
            "note"
        } else {
            "notes"
        }
    );
    let mut spans = vec![
        Span::styled(
            app.notebook.clone(),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!("  ({})  ", app.status.branch), muted),
        Span::raw(counts),
    ];
    if app.status.files > 0 {
        spans.push(Span::styled(
            format!(
                "  {} {}",
                app.status.files,
                if app.status.files == 1 {
                    "file"
                } else {
                    "files"
                }
            ),
            muted,
        ));
    }
    // Compact where `noda status` is wordy: this is a strip along the top of a
    // screen somebody is reading notes on, not the answer to "where do I stand".
    let changes = match app.status.uncommitted {
        0 => String::new(),
        1 => "  1 uncommitted".to_string(),
        n => format!("  {n} uncommitted"),
    };
    if !changes.is_empty() {
        spans.push(Span::styled(changes, theme::from(palette::MATCH)));
    }
    if let Some((ahead, behind)) = app.status.drift
        && (ahead > 0 || behind > 0)
    {
        spans.push(Span::styled(format!("  ↑{ahead} ↓{behind}"), muted));
    }
    frame.render_widget(Line::from(spans), area);
}

fn draw_list(frame: &mut Frame, area: Rect, app: &mut App) {
    if app.shown() == 0 {
        let nothing = if app.total() == 0 {
            "this notebook has no notes yet"
        } else {
            "nothing matches"
        };
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                nothing,
                theme::from(palette::MUTED),
            ))),
            area,
        );
        return;
    }

    // Taken out and put back so the rows may borrow the notes while ratatui
    // writes this frame's scroll offset into the state. They are different
    // fields, but the borrow checker only sees `app`.
    let mut state = std::mem::take(&mut app.table);

    let id_width = app
        .rows()
        .map(|file| file.id.chars().count())
        .max()
        .unwrap_or(0);
    let tag_width = app
        .rows()
        .map(|file| tags(&file.note.tags).chars().count())
        .max()
        .unwrap_or(0);

    let rows: Vec<Row> = app
        .rows()
        .map(|file| {
            Row::new(vec![
                Line::from(Span::styled(file.id.as_str(), theme::from(palette::ID))),
                // The title is the column the eye lands on, so it is the one
                // left uncoloured — the same reason `noda ls` leaves it alone.
                marked(&file.note.title, &app.terms, Style::default()),
                Line::from(Span::styled(
                    tags(&file.note.tags),
                    theme::from(palette::TAGS),
                )),
            ])
        })
        .collect();

    let selected = match app.focus {
        Focus::List => Style::default().add_modifier(Modifier::REVERSED),
        // The cursor is still there, just not what the keys are steering.
        Focus::Preview => Style::default().add_modifier(Modifier::BOLD),
    };
    let table = Table::new(
        rows,
        [
            Constraint::Length(id_width as u16),
            Constraint::Fill(1),
            Constraint::Length(tag_width as u16),
        ],
    )
    .column_spacing(2)
    .row_highlight_style(selected)
    .highlight_symbol("");

    frame.render_stateful_widget(table, area, &mut state);
    app.table = state;
}

fn draw_preview(frame: &mut Frame, area: Rect, app: &App) {
    let block = Block::new()
        .borders(Borders::LEFT)
        .border_style(theme::from(palette::MUTED))
        .padding(ratatui::widgets::Padding::horizontal(1));
    let Some(preview) = &app.preview else {
        frame.render_widget(block, area);
        return;
    };
    frame.render_widget(
        Paragraph::new(lines(&preview.text, &app.terms))
            .block(block)
            // Wrapped rather than cut: a note is prose, and a reader who has to
            // scroll sideways to finish a sentence is not reading.
            .wrap(Wrap { trim: false })
            .scroll((app.scroll, 0)),
        area,
    );
}

fn draw_footer(frame: &mut Frame, area: Rect, app: &App) {
    let muted = theme::from(palette::MUTED);
    let count = format!("{}/{}", app.shown(), app.total());

    let (left, cursor) = match app.mode {
        Mode::Search => {
            let typed = Span::raw(app.search.as_str());
            let width = 1 + typed.width() as u16;
            (
                Line::from(vec![Span::styled("/", muted), typed]),
                Some(area.x + width),
            )
        }
        _ if app.error.is_some() || !app.search.is_empty() => (
            Line::from(vec![
                Span::styled("/", muted),
                Span::styled(app.search.as_str(), muted),
            ]),
            None,
        ),
        _ => (Line::from(Span::styled(HINTS, muted)), None),
    };

    // The right-hand end says how much of the notebook is on the left; a query
    // that is not one yet says why instead, because that is the more urgent
    // answer and there is only the one line.
    let right = match &app.error {
        Some(message) => Line::from(Span::styled(message.clone(), theme::from(palette::INVALID))),
        None => Line::from(Span::styled(count, muted)),
    };

    let [left_area, right_area] = Layout::horizontal([
        Constraint::Fill(1),
        Constraint::Length(right.width() as u16),
    ])
    .areas(area);
    frame.render_widget(left, left_area);
    frame.render_widget(right, right_area);
    if let Some(x) = cursor {
        frame.set_cursor_position((x, area.y));
    }
}

fn draw_help(frame: &mut Frame, area: Rect) {
    let keys: Vec<Line> = KEYS
        .iter()
        .map(|(key, what)| {
            Line::from(vec![
                Span::styled(format!("{key:>KEY_COLUMN$}  "), theme::from(palette::ID)),
                Span::raw(*what),
            ])
        })
        .collect();

    // Measured rather than guessed, and measured after the lines are built: a
    // card that cut the search example off would be teaching the one thing on
    // it that cannot be worked out from the key alone. `Line::width` counts
    // what a terminal will show, so the arrows in the key column count once
    // each and not three times.
    let width = 2 + keys.iter().map(Line::width).max().unwrap_or(0) as u16;
    let height = keys.len() as u16 + 2;
    let [area] = Layout::horizontal([Constraint::Length(width)])
        .flex(Flex::Center)
        .areas(area);
    let [area] = Layout::vertical([Constraint::Length(height)])
        .flex(Flex::Center)
        .areas(area);

    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(keys).block(
            Block::bordered()
                .title(" keys ")
                .border_style(theme::from(palette::MUTED)),
        ),
        area,
    );
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

/// The file as the preview shows it: the frontmatter pushed into the background
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
/// preview cannot read this way is one the reader should see as it stands.
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
}
