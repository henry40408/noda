//! The chrome every screen wears, and the card that goes over the top of it.
//!
//! A screen is the same five bands whatever it is showing: where the notebook
//! stands, what the keys do here, what this screen is of, how deep you are, and
//! what the last command said. Only the middle changes — which is the point of
//! keeping it here rather than in the drawing of each view. A browser whose
//! furniture moved between screens would have to be re-read on every one.
//!
//! The keys live in the band along the top rather than behind `?`. They are what
//! is different about the screen you are on, and a screen that only tells you
//! what it can do once you ask is a screen you have to ask about every time.
//! That also takes the pressure off the help card, which had grown to the point
//! of not fitting on a short terminal.

use ratatui::Frame;
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph, Wrap};

use super::app::{App, Mode, View};
use super::theme;
use crate::cmd::Touch;
use crate::style as palette;

/// How many rows the standing information wants when there is room for it.
pub const INFO_ROWS: u16 = 5;

/// Below this many rows the header is a single line instead. The card that
/// outgrew a twenty-four row terminal is the reason this is checked at all: a
/// browser has to keep working on the terminal it is given, and five rows of
/// header on a screen that only has fourteen is five rows of notes it does not
/// have.
const ROOM_FOR_INFO: u16 = 20;

/// How many rows of keys the header shows, and therefore how the list of them is
/// cut into columns.
const MENU_ROWS: usize = 5;

/// The width of the labels down the left of the header, so their values line up.
const LABEL: usize = 9;

/// The gap between one column of keys and the next.
const MENU_GAP: usize = 2;

/// The gap between a key and what it does, which is what makes the two read as
/// one thing rather than as two columns.
const KEY_GAP: usize = 2;

/// What the keys do on the listing. Read down each column of five, not across:
/// moving, then changing, then picking out.
const LISTING_KEYS: &[(&str, &str)] = &[
    ("enter", "read"),
    ("/", "filter"),
    (":", "command"),
    ("ctrl-a", "commands"),
    ("?", "keys"),
    ("e", "edit"),
    ("a", "new"),
    ("m", "retitle"),
    ("#", "tags"),
    ("ctrl-d", "delete"),
    ("space", "mark"),
    ("*", "mark shown"),
    ("Q", "queue"),
    ("T", "keep updated"),
    ("q", "quit"),
];

/// What the keys do on a note. The same words for the same keys — a key that
/// changes a note is the same key here, aimed at the note you are reading rather
/// than at the row under a cursor.
/// The way to everything else comes first, and `g` / `G` is not here at all: the
/// columns are dropped from the right when they do not fit, so what goes in the
/// last one has to be what can afford to go. A reader who does not know `:`
/// exists cannot look it up; one who does not know `G` has both `j` and the
/// card.
const NOTE_KEYS: &[(&str, &str)] = &[
    ("esc", "back"),
    ("j/k", "scroll"),
    (":", "command"),
    ("ctrl-a", "commands"),
    ("?", "keys"),
    ("e", "edit"),
    ("m", "retitle"),
    ("#", "tags"),
    ("ctrl-d", "delete"),
    ("T", "keep updated"),
    ("r", "reload"),
    ("q", "quit"),
];

/// The keys the screen in front of you answers to.
pub fn keys_for(view: &View) -> &'static [(&'static str, &'static str)] {
    match view {
        View::Notes => LISTING_KEYS,
        View::Note(_) => NOTE_KEYS,
    }
}

/// How tall the header may be on a terminal this size.
pub fn header_rows(height: u16) -> u16 {
    if height >= ROOM_FOR_INFO {
        INFO_ROWS
    } else {
        1
    }
}

/// Where the notebook stands, what the keys do, and whose browser this is.
pub fn draw_header(frame: &mut Frame, area: Rect, app: &App) {
    if area.height < INFO_ROWS {
        frame.render_widget(Line::from(compact(app)), area);
        return;
    }
    let mark = wordmark();
    let width = mark.iter().map(Line::width).max().unwrap_or(0) as u16;
    let [left, keys, right] = Layout::horizontal([
        Constraint::Length(info_width(app)),
        Constraint::Fill(1),
        Constraint::Length(width),
    ])
    .areas(area);

    frame.render_widget(Paragraph::new(info(app)), left);
    frame.render_widget(Paragraph::new(menu(app, keys.width)), keys);
    frame.render_widget(Paragraph::new(mark), right);
}

/// The standing information, one fact to a line.
///
/// The same five labels every time, in the same order, whatever the values are.
/// A block that dropped a line when a value was uninteresting would move the
/// four below it, and the whole reason for putting these in a fixed block is
/// that the eye learns where each one is.
fn info(app: &App) -> Vec<Line<'_>> {
    let muted = theme::from(palette::MUTED);
    let mut branch = vec![Span::raw(app.status.branch.as_str())];
    if let Some((ahead, behind)) = app.status.drift
        && (ahead > 0 || behind > 0)
    {
        branch.push(Span::styled(format!("  ↑{ahead} ↓{behind}"), muted));
    }

    let mut counts = vec![Span::raw(plural(app.total(), "note"))];
    if app.status.files > 0 {
        counts.push(Span::styled(
            format!("  {}", plural(app.status.files, "file")),
            muted,
        ));
    }

    let changes = match app.status.uncommitted {
        0 => Span::styled("none", muted),
        n => Span::styled(format!("{n} uncommitted"), theme::from(palette::MATCH)),
    };

    let remote = match &app.status.remote {
        Some(url) => Span::raw(url.as_str()),
        None => Span::styled("none", muted),
    };

    vec![
        labelled(
            "Notebook",
            vec![Span::styled(
                app.notebook.as_str(),
                Style::default().add_modifier(Modifier::BOLD),
            )],
        ),
        labelled("Branch", branch),
        labelled("Remote", vec![remote]),
        labelled("Notes", counts),
        labelled("Changes", vec![changes]),
    ]
}

/// What this session is doing that the notebook on disk knows nothing about.
///
/// All three are state you can only have got into by pressing something, and all
/// three change what the next keystroke will do — which is exactly the sort of
/// thing a browser must not keep to itself.
///
/// Said on the title band rather than among the standing facts, because they are
/// the only ones that change while you sit there. In the block they widened it,
/// and a block that widens pushes the keys along and drops the rightmost column
/// off the end — so marking a note was what hid the keys about marking.
fn session(app: &App) -> Vec<Span<'static>> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut add = |text: String, style: Style| {
        if !spans.is_empty() {
            spans.push(Span::styled("  ", theme::from(palette::MUTED)));
        }
        spans.push(Span::styled(text, style));
    };
    if app.touch == Touch::Keep {
        add("keeping updated".to_string(), theme::from(palette::MATCH));
    }
    if !app.marks.is_empty() {
        add(plural(app.marks.len(), "mark"), theme::from(palette::MATCH));
    }
    if !app.queue.is_empty() {
        add(
            format!("{} queued", app.queue.len()),
            theme::from(palette::TAGS),
        );
    }
    spans
}

fn labelled<'a>(label: &'a str, mut value: Vec<Span<'a>>) -> Line<'a> {
    let mut spans = vec![Span::styled(
        format!("{:<LABEL$}", format!("{label}:")),
        theme::from(palette::MUTED),
    )];
    spans.append(&mut value);
    Line::from(spans)
}

/// How wide the block of standing information needs to be, measured rather than
/// guessed: a notebook may be called anything, and a branch name is somebody
/// else's decision.
fn info_width(app: &App) -> u16 {
    let widest = info(app).iter().map(Line::width).max().unwrap_or(0);
    (widest + MENU_GAP) as u16
}

/// The keys, in as many columns of five as there is room for.
///
/// Columns are dropped from the right rather than squeezed. A key list that has
/// been narrowed until its words are cut is a key list that has stopped saying
/// what the keys do, and the ones that go are the ones furthest from the hand.
fn menu(app: &App, width: u16) -> Vec<Line<'static>> {
    let keys = keys_for(app.view());
    let columns: Vec<&[(&str, &str)]> = keys.chunks(MENU_ROWS).collect();

    let mut widths = Vec::new();
    let mut room = width as usize;
    for column in &columns {
        let wanted = column
            .iter()
            .map(|(key, what)| key.chars().count() + 2 + KEY_GAP + what.chars().count())
            .max()
            .unwrap_or(0)
            + MENU_GAP;
        if wanted > room {
            break;
        }
        room -= wanted;
        widths.push(wanted);
    }

    (0..MENU_ROWS)
        .map(|row| {
            let mut spans = Vec::new();
            for (at, wanted) in widths.iter().enumerate() {
                let Some((key, what)) = columns[at].get(row) else {
                    continue;
                };
                let named = format!("<{key}>");
                // The padding goes after what the key does, not between the key
                // and its description. Put it in front and the description is
                // pushed across the column to sit against the next key, which
                // reads as though it belonged to that one.
                let used = named.chars().count() + KEY_GAP + what.chars().count();
                spans.push(Span::styled(named, theme::from(palette::ID)));
                spans.push(Span::raw(format!(
                    "{:KEY_GAP$}{what}{:pad$}",
                    "",
                    "",
                    pad = wanted.saturating_sub(used)
                )));
            }
            Line::from(spans)
        })
        .collect()
}

/// Whose browser this is, in the corner the eye does not need.
///
/// The name and nothing else. The version was here and is not: it is two columns
/// the key grid wants more, and `noda --version` is where you would look for it
/// anyway.
fn wordmark() -> Vec<Line<'static>> {
    vec![
        Line::from(Span::styled(
            "noda",
            Style::default().add_modifier(Modifier::BOLD),
        ))
        .right_aligned(),
    ]
}

/// The header for a terminal with no room for one: the same facts, run together
/// on a line, and no keys — the space goes to the notes instead.
fn compact(app: &App) -> Vec<Span<'_>> {
    let muted = theme::from(palette::MUTED);
    let mut spans = vec![
        Span::styled(
            app.notebook.as_str(),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!("  ({})  ", app.status.branch), muted),
        Span::raw(plural(app.total(), "note")),
    ];
    if app.status.uncommitted > 0 {
        spans.push(Span::styled(
            format!("  {} uncommitted", app.status.uncommitted),
            theme::from(palette::MATCH),
        ));
    }
    // What the session is holding is not repeated here: the title band carries
    // it at whatever height the terminal is, and this line has the least room of
    // anything on the screen.
    spans
}

/// What this screen is of, and how much of it there is.
///
/// The rule follows the notebook's own naming: a listing says what it is
/// narrowed to and how many that leaves, and a note says its id and then its
/// title — the same two things in the same order as every row of `noda ls`.
pub fn draw_title(frame: &mut Frame, area: Rect, app: &App) {
    let muted = theme::from(palette::MUTED);
    let mut spans = match app.view() {
        View::Notes => {
            let scope = if app.search().is_empty() {
                "all".to_string()
            } else {
                app.search().to_string()
            };
            vec![
                Span::styled("Notes", Style::default().add_modifier(Modifier::BOLD)),
                Span::styled(format!("({scope})"), muted),
                Span::styled(format!("[{}]", app.shown()), muted),
            ]
        }
        View::Note(id) => {
            let title = app.selected().map(|file| file.note.title.as_str());
            vec![
                Span::styled("Note", Style::default().add_modifier(Modifier::BOLD)),
                Span::styled(format!("({id})"), muted),
                Span::raw("  "),
                Span::raw(title.unwrap_or_default()),
            ]
        }
    };
    // Ruled out to the far end, so the band reads as the top of the body rather
    // than as another line of header — and what this session is holding is put
    // at that end, next to what the screen is of.
    let mut held = session(app);
    let used = spans.iter().map(Span::width).sum::<usize>();
    let mut wanted = held.iter().map(Span::width).sum::<usize>();
    if wanted > 0 {
        wanted += 1;
    }
    let rule = (area.width as usize).saturating_sub(used + wanted + 1);
    if rule > 0 {
        spans.push(Span::styled(format!(" {}", "─".repeat(rule)), muted));
    }
    if !held.is_empty() {
        spans.push(Span::raw(" "));
        spans.append(&mut held);
    }
    frame.render_widget(Line::from(spans), area);
}

/// How far down you are, outermost first.
///
/// Worth a line of its own even when there is only one crumb on it: a stack you
/// cannot see the depth of is a stack whose Escape key you have to guess at.
pub fn draw_crumbs(frame: &mut Frame, area: Rect, app: &App) {
    let muted = theme::from(palette::MUTED);
    let here = Style::default().add_modifier(Modifier::REVERSED);
    let depth = app.depth();
    let spans: Vec<Span> = app
        .crumbs()
        .enumerate()
        .flat_map(|(at, crumb)| {
            let style = if at + 1 == depth { here } else { muted };
            [Span::styled(format!(" {crumb} "), style), Span::raw(" ")]
        })
        .collect();
    frame.render_widget(Line::from(spans), area);
}

/// What is being typed, or what the last command said. Returns where the cursor
/// belongs, when there is a field for it to be in.
///
/// One line and one field. The prompt and the query take the same place because
/// only one of them can be open at a time, and a browser with two places to type
/// would be one you had to look at to find out where your keystrokes were going.
pub fn draw_status(frame: &mut Frame, area: Rect, app: &App) -> Option<u16> {
    let muted = theme::from(palette::MUTED);
    // What is being waited for wins the line outright: it is drawn on a frame of
    // its own, before the thing it is waiting for has begun, and nothing else on
    // the screen is going to change until it is over.
    if let Some(waiting) = app.working {
        frame.render_widget(Line::from(Span::styled(waiting, muted)), area);
        return None;
    }
    let (left, cursor) = match (&app.message, app.mode) {
        (_, Mode::Command) => {
            let typed = Span::raw(app.input.as_str());
            let width = 1 + typed.width() as u16;
            (
                Line::from(vec![Span::styled(":", muted), typed]),
                Some(area.x + width),
            )
        }
        (_, Mode::Search) => {
            let typed = Span::raw(app.search());
            let width = 1 + typed.width() as u16;
            (
                Line::from(vec![Span::styled("/", muted), typed]),
                Some(area.x + width),
            )
        }
        (_, Mode::Ask(what)) => {
            let label = Span::styled(format!("{}  ", what.prompt()), muted);
            let typed = Span::raw(app.input.as_str());
            let width = label.width() as u16 + typed.width() as u16;
            (Line::from(vec![label, typed]), Some(area.x + width))
        }
        // What the last command said, in its own words. The next key takes it
        // away again — and a line that says why something did *not* run is
        // coloured like the query that is not a query yet, because it is the
        // same class of thing: not a refusal by the notebook, a sentence that
        // never reached it.
        (Some(said), _) => {
            let style = if said.failed {
                theme::from(palette::INVALID)
            } else {
                Style::default()
            };
            (Line::from(Span::styled(said.line(), style)), None)
        }
        // A query that has been left in force is worth showing even when the
        // keyboard has moved on from it, because it is why the listing is short.
        _ if !app.search().is_empty() => (
            Line::from(vec![
                Span::styled("/", muted),
                Span::styled(app.search(), muted),
            ]),
            None,
        ),
        _ => (Line::default(), None),
    };

    // The right-hand end says why a query is not one yet, which is the more
    // urgent of the two things this line can carry — and the hint for a prompt
    // whose name does not say everything about what it takes.
    let right = match (app.error(), app.mode) {
        (Some(message), _) => Line::from(Span::styled(
            message.to_string(),
            theme::from(palette::INVALID),
        )),
        (None, Mode::Ask(what)) if !what.hint().is_empty() => {
            Line::from(Span::styled(what.hint(), muted))
        }
        _ => Line::default(),
    };

    let [left_area, right_area] = Layout::horizontal([
        Constraint::Fill(1),
        Constraint::Length(right.width() as u16),
    ])
    .areas(area);
    frame.render_widget(left, left_area);
    frame.render_widget(right, right_area);
    cursor
}

/// `1 note` / `3 notes`, the way `cmd` says it.
pub fn plural(n: usize, thing: &str) -> String {
    if n == 1 {
        format!("1 {thing}")
    } else {
        format!("{n} {thing}s")
    }
}

/// A card in the middle of the screen, as wide as what is on it.
///
/// Measured rather than guessed, and measured after the lines are built: a card
/// that cut the help's search example off would be losing the one thing on it
/// that cannot be worked out from the key beside it. `Line::width` counts what a
/// terminal will show, so an arrow counts once and not three times.
///
/// Clamped to the screen, and what does not fit is wrapped rather than cut — the
/// same bargain a note makes, and for the same reason: it is prose.
pub fn card(frame: &mut Frame, area: Rect, title: &str, lines: Vec<Line>, border: Style) {
    let width = (2 + lines.iter().map(Line::width).max().unwrap_or(0) as u16)
        .max(title.chars().count() as u16 + 2)
        .min(area.width);
    // Counted after the clamp, so a line the screen was too narrow to hold is
    // given the rows it will wrap onto rather than being pushed off the bottom.
    let inner = width.saturating_sub(2).max(1);
    let rows: u16 = lines
        .iter()
        .map(|line| (line.width() as u16).div_ceil(inner).max(1))
        .sum();
    let height = (rows + 2).min(area.height);
    let [area] = Layout::horizontal([Constraint::Length(width)])
        .flex(Flex::Center)
        .areas(area);
    let [area] = Layout::vertical([Constraint::Length(height)])
        .flex(Flex::Center)
        .areas(area);

    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(Block::bordered().title(title).border_style(border)),
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_short_terminal_gets_a_header_it_can_afford() {
        assert_eq!(header_rows(40), INFO_ROWS);
        assert_eq!(header_rows(24), INFO_ROWS);
        // The size the card once outgrew, and the size a split pane made tight.
        assert_eq!(header_rows(14), 1);
    }

    #[test]
    fn the_keys_are_the_ones_the_screen_answers_to() {
        let listing: Vec<&str> = keys_for(&View::Notes).iter().map(|(key, _)| *key).collect();
        assert!(listing.contains(&"enter"));
        assert!(listing.contains(&"space"));

        // A note has no cursor to mark and nothing to filter, and says so by not
        // offering the keys.
        let note: Vec<&str> = keys_for(&View::Note("aaaa1111".to_string()))
            .iter()
            .map(|(key, _)| *key)
            .collect();
        assert!(!note.contains(&"space"));
        assert!(!note.contains(&"/"));
        // But every key that changes a note is on both, spelled the same way.
        for key in ["e", "m", "#", "ctrl-d", "T"] {
            assert!(listing.contains(&key), "the listing lost {key}");
            assert!(note.contains(&key), "the note lost {key}");
        }
    }

    #[test]
    fn no_key_is_listed_twice_on_one_screen() {
        // The grid is laid out column-major and the last column may be short,
        // which is fine. What is not fine is the same key appearing twice: the
        // second entry is unreachable, and the two say different things about
        // what pressing it does.
        for keys in [LISTING_KEYS, NOTE_KEYS] {
            let mut seen = std::collections::BTreeSet::new();
            for (key, _) in keys {
                assert!(seen.insert(key), "`{key}` is on the same grid twice");
            }
        }
    }
}
