//! The chrome every screen wears, and the card that goes over the top of it.
//!
//! A screen is the same five bands whatever it shows, and only the middle
//! changes — which is why they are here rather than in each view's drawing: a
//! browser whose furniture moved between screens has to be re-read on every one.
//!
//! The keys are along the top rather than behind `?`, being what is different
//! about the screen you are on. That also takes the pressure off the help card,
//! which had grown past fitting on a short terminal.

use ratatui::Frame;
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Paragraph, Wrap};

use super::app::{App, Mode, View};
use super::theme;
use crate::cmd::{Sort, Touch};
use crate::style as palette;

/// How many rows the standing information wants when there is room for it.
pub const INFO_ROWS: u16 = 5;

/// Below this the header is a single line: five rows of header on a screen with
/// fourteen is five rows of notes it does not have.
const ROOM_FOR_INFO: u16 = 20;

/// How many rows of keys, and so how the list is cut into columns.
const MENU_ROWS: usize = 5;

/// One wider than the longest label with its colon. Sized to the longest
/// exactly, `Notebook:work` comes out with no gap while the rows under it have
/// two or three — the column lines up and still reads as a mistake.
const LABEL: usize = 10;

/// The gap between one column of keys and the next.
const MENU_GAP: usize = 2;

/// What separates one thing from the next along the title band.
const GAP: &str = "  ";

/// What makes a key and its description read as one thing.
const KEY_GAP: usize = 2;

/// One column of the key grid, which is what the list is cut into.
type Column = [(&'static str, &'static str); MENU_ROWS];

/// Read down each column of five, not across.
const LISTING_KEYS: &[Column] = &[
    [
        ("enter", "read"),
        ("/", "filter"),
        (":", "command"),
        ("ctrl-a", "commands"),
        ("?", "keys"),
    ],
    [
        ("e", "edit"),
        ("a", "new"),
        ("m", "retitle"),
        ("#", "tags"),
        ("ctrl-d", "delete"),
    ],
    [
        ("space", "mark"),
        ("*", "mark shown"),
        ("Q", "queue"),
        ("T", "keep updated"),
        ("q", "quit"),
    ],
    // The listing's own density and order, which no other screen has.
    //
    // Behind the other three because each is a way of *looking* rather than of
    // doing, and each has a second way of being found when the column goes: all
    // five are on the `?` card, the order and density say themselves on the
    // title band once used, and the tags screen numbers its first nine rows with
    // the digits that reach them. That is the test a key passes to sit here.
    [
        ("S", "sort"),
        ("R", "reverse"),
        ("ctrl-w", "wide"),
        ("1-9", "tag, 0 all"),
        ("ctrl-g", "crumbs"),
    ],
    VIEW_KEYS,
];

/// The same words for the same keys, aimed at the note you are reading rather
/// than the row under a cursor.
///
/// The way to everything else comes first and `g`/`G` is absent: columns drop
/// from the right, so the last has to hold what can afford to go. A reader who
/// does not know `:` exists cannot look it up; one who does not know `G` has
/// both `j` and the card.
const NOTE_KEYS: &[Column] = &[
    [
        ("esc", "back"),
        ("j/k", "scroll"),
        (":", "command"),
        ("ctrl-a", "commands"),
        ("?", "keys"),
    ],
    [
        ("e", "edit"),
        ("m", "retitle"),
        ("#", "tags"),
        ("ctrl-d", "delete"),
        ("T", "keep updated"),
    ],
    [("r", "reload"), ("q", "quit"), BLANK, BLANK, BLANK],
    VIEW_KEYS,
];

/// Pads a column out to five so the grid stays a grid. Nothing is drawn.
const BLANK: (&str, &str) = ("", "");

/// Last on every screen, because columns drop from the right and these can
/// afford to go: each has a name as well, and `:` is in the first column
/// everywhere. Nothing else on a grid has a second way of being found.
const VIEW_KEYS: Column = [
    ("t", "todo"),
    ("l", "log"),
    ("b", "backlinks"),
    ("B", "blame"),
    BLANK,
];

/// Walking it, leaving it, and whatever `enter` is for on this one.
///
/// One shape for all of them, only the first entry differing: what `enter` does
/// to a tag and what it does to a notebook are not the same act.
///
/// `None` where the rows lead nowhere — the notebook's own log, every commit on
/// which is a version of *some* note with no one note to put a revision
/// against.
fn rows_keys(enter: Option<&'static str>) -> Vec<Column> {
    // The first column is never dropped, so it holds what cannot be looked up
    // when it is not shown.
    //
    // `ctrl-f/b` is on neither: twelve columns for a key `j` already covers come
    // off whatever falls from the right-hand end, which here is the keys that
    // change a note.
    match enter {
        Some(what) => vec![
            [
                ("enter", what),
                ("j/k", "move"),
                ("esc", "back"),
                (":", "command"),
                ("?", "keys"),
            ],
            [
                ("g/G", "first / last"),
                ("ctrl-a", "commands"),
                ("r", "reload"),
                ("q", "quit"),
                BLANK,
            ],
        ],
        None => vec![
            [
                ("j/k", "move"),
                ("g/G", "first / last"),
                ("esc", "back"),
                (":", "command"),
                ("?", "keys"),
            ],
            [
                ("ctrl-a", "commands"),
                ("r", "reload"),
                ("q", "quit"),
                BLANK,
                BLANK,
            ],
        ],
    }
}

/// A screen whose rows are notes gets the keys that change one as well, before
/// the view keys: a key that changes a note has no other way of being found and
/// a view key has a name.
pub fn keys_for(view: &View) -> Vec<Column> {
    let changing: Column = [
        ("e", "edit"),
        ("m", "retitle"),
        ("#", "tags"),
        ("ctrl-d", "delete"),
        ("T", "keep updated"),
    ];
    let mut keys = match view {
        View::Notes => return LISTING_KEYS.to_vec(),
        View::Note(_) => return NOTE_KEYS.to_vec(),
        // `read it` and not `read the note`: this is the one shape with four
        // columns to fit, and the words come out of the column that would
        // otherwise be dropped.
        View::Todo | View::Backlinks(_) => {
            let mut keys = rows_keys(Some("read it"));
            keys.push(changing);
            keys
        }
        View::Tags => rows_keys(Some("filter by it")),
        View::Files => rows_keys(Some("what links here")),
        View::Notebooks => rows_keys(Some("switch to it")),
        // The ellipsis promises the key writes the command rather than running
        // it.
        View::Deleted | View::Log(Some(_)) => rows_keys(Some("restore it…")),
        View::Log(None) => rows_keys(None),
        // A page of text, so there is no row to press `enter` on.
        View::Blame(_) | View::Diff => vec![
            [
                ("j/k", "scroll"),
                ("g/G", "top / end"),
                ("esc", "back"),
                (":", "command"),
                ("?", "keys"),
            ],
            [
                ("ctrl-a", "commands"),
                ("r", "reload"),
                ("q", "quit"),
                BLANK,
                BLANK,
            ],
        ],
    };
    keys.push(VIEW_KEYS);
    keys
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

/// The same five labels in the same order whatever the values are: a block that
/// dropped an uninteresting line would move the four below it, and the point of
/// a fixed block is that the eye learns where each one is.
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

/// All three are state you got into by pressing something, and all three change
/// what the next keystroke does.
///
/// On the title band rather than among the standing facts, because they are the
/// only ones that change while you sit there — and in the block they widened it,
/// so marking a note was what hid the keys about marking.
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

/// Measured rather than guessed: a notebook may be called anything.
fn info_width(app: &App) -> u16 {
    let widest = info(app).iter().map(Line::width).max().unwrap_or(0);
    (widest + MENU_GAP) as u16
}

/// Columns are dropped from the right rather than squeezed: a key list narrowed
/// until its words are cut has stopped saying what the keys do.
fn menu(app: &App, width: u16) -> Vec<Line<'static>> {
    let columns = keys_for(app.view());

    let mut widths = Vec::new();
    let mut room = width as usize;
    for column in &columns {
        let wanted = column
            .iter()
            .filter(|(key, _)| !key.is_empty())
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
                // A blank pads a column out to five so a short one stays a
                // column. It still takes its width: skipped outright, every
                // column after it on that one row slides left, and a key ends up
                // under the heading of the column beside it — which is the same
                // mistake as putting the padding in front of the description,
                // one axis over.
                let (key, what) = &columns[at][row];
                if key.is_empty() {
                    spans.push(Span::raw(" ".repeat(*wanted)));
                    continue;
                }
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

/// The same facts run together on a line, and no keys: the space goes to the
/// notes.
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
    // The title band carries it at any height, and this line has least room.
    spans
}

/// Spelled as `--sort`, `-r` and `-l` spell it, these being the same three
/// settings from the other end. Silent when nothing has changed — the reason for
/// saying it at all is that those keys rearrange rows and leave nothing
/// behind.
fn looking(app: &App) -> Option<String> {
    let mut said = Vec::new();
    if app.sort != Sort::Slug || app.reverse {
        said.push(format!("by {}", app.sort.name()));
    }
    if app.reverse {
        said.push("reversed".to_string());
    }
    if app.long {
        said.push("wide".to_string());
    }
    (!said.is_empty()).then(|| said.join(" "))
}

/// A listing says what it is narrowed to and how many that leaves; a note says
/// its id and then its title, in `noda ls`'s order.
pub fn draw_title(frame: &mut Frame, area: Rect, app: &App) {
    let muted = theme::from(palette::MUTED);
    let bold = Style::default().add_modifier(Modifier::BOLD);
    // The same order on every screen, so the eye learns one place for each.
    //
    // Set apart by a gap rather than brackets: the punctuation was doing the
    // work of a space, which a band with columns to spare has plenty of. The
    // same gap a note's heading uses between its id and title.
    let banner = |name: &'static str, scope: Option<String>, count: Option<usize>| {
        let mut spans = vec![Span::styled(name, bold)];
        if let Some(scope) = scope {
            spans.push(Span::styled(format!("{GAP}{scope}"), muted));
        }
        if let Some(count) = count {
            spans.push(Span::styled(format!("{GAP}{count}"), muted));
        }
        spans
    };
    let titled = |id: &str| {
        app.note_of(id)
            .map(|file| file.note.title.clone())
            .unwrap_or_default()
    };

    let mut spans = match app.view() {
        View::Notes => {
            let mut spans = banner(
                "Notes",
                Some(if app.search().is_empty() {
                    "all".to_string()
                } else {
                    app.search().to_string()
                }),
                Some(app.shown()),
            );
            if let Some(said) = looking(app) {
                spans.push(Span::styled(format!("{GAP}{said}"), muted));
            }
            spans
        }
        View::Note(id) => {
            let mut spans = banner("Note", Some(id.clone()), None);
            spans.push(Span::raw(GAP));
            spans.push(Span::raw(titled(id)));
            spans
        }
        View::Todo => banner("Todo", None, Some(app.tasks().len())),
        View::Tags => banner("Tags", None, Some(app.tallies().len())),
        View::Files => banner("Files", None, Some(app.files().len())),
        View::Notebooks => banner(
            "Notebooks",
            Some(app.notebook.clone()),
            Some(app.notebooks().len()),
        ),
        View::Deleted => banner("Deleted", None, Some(app.gone().len())),
        View::Diff => banner("Diff", None, None),
        // Here rather than in the crumb trail, which names the kind of screen:
        // three crumbs with an id apiece would be as wide as the terminal.
        View::Log(id) => {
            let mut spans = banner(
                "Log",
                Some(id.clone().unwrap_or_else(|| app.notebook.clone())),
                Some(app.entries().len()),
            );
            if let Some(id) = id {
                spans.push(Span::raw(GAP));
                spans.push(Span::raw(titled(id)));
            }
            spans
        }
        View::Backlinks(subject) => banner(
            "Backlinks",
            Some(subject.name().to_string()),
            Some(app.linking().len()),
        ),
        View::Blame(id) => {
            let mut spans = banner("Blame", Some(id.clone()), None);
            spans.push(Span::raw(GAP));
            spans.push(Span::raw(titled(id)));
            spans
        }
    };
    // Ruled to the far end, so the band reads as the top of the body rather than
    // another line of header.
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

/// Worth a line even with one crumb on it: a stack whose depth you cannot see
/// is one whose Escape key you guess at.
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

/// What is being typed, or what the last command said, and where the cursor
/// belongs.
///
/// One line and one field: only one can be open at a time, and two places to
/// type is a browser you look at to find where your keystrokes went.
pub fn draw_status(frame: &mut Frame, area: Rect, app: &App) -> Option<u16> {
    let muted = theme::from(palette::MUTED);
    // Wins the line outright: drawn on a frame of its own, before the thing it
    // waits for has begun.
    if let Some(waiting) = app.working {
        frame.render_widget(Line::from(Span::styled(waiting, muted)), area);
        return None;
    }
    // Measured from what is to the *left* of the cursor: the whole line only
    // agrees while the cursor is at the end. In columns and not characters,
    // because a title in Chinese is two columns a character.
    let (left, cursor) = match (&app.message, app.mode) {
        (_, Mode::Command) => {
            let typed = Span::raw(app.input.text());
            let width = 1 + Span::raw(app.input.before()).width() as u16;
            (
                Line::from(vec![Span::styled(":", muted), typed]),
                Some(area.x + width),
            )
        }
        (_, Mode::Search) => {
            let typed = Span::raw(app.search());
            let width = 1 + Span::raw(app.search_before()).width() as u16;
            (
                Line::from(vec![Span::styled("/", muted), typed]),
                Some(area.x + width),
            )
        }
        (_, Mode::Ask(what)) => {
            let label = Span::styled(format!("{}  ", what.prompt()), muted);
            let typed = Span::raw(app.input.text());
            let width = label.width() as u16 + Span::raw(app.input.before()).width() as u16;
            (Line::from(vec![label, typed]), Some(area.x + width))
        }
        // In its own words, and the next key takes it away. A line saying why
        // something did *not* run is coloured like half a query, being the same
        // class of thing: a sentence that never reached the notebook.
        (Some(said), _) => {
            let style = if said.failed {
                theme::from(palette::INVALID)
            } else {
                Style::default()
            };
            (Line::from(Span::styled(said.line(), style)), None)
        }
        // Worth showing after the keyboard moves on: it is why the listing is
        // short.
        _ if !app.search().is_empty() => (
            Line::from(vec![
                Span::styled("/", muted),
                Span::styled(app.search(), muted),
            ]),
            None,
        ),
        _ => (Line::default(), None),
    };

    // The more urgent of the two things this line can carry, and the hint for a
    // prompt whose name does not say everything.
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

/// As wide as what is on it, measured after the lines are built: a card that cut
/// the help's search example would lose the one thing not worked out from the
/// key beside it. `Line::width` counts what a terminal shows.
///
/// Clamped to the screen, and what does not fit wraps rather than cuts.
pub fn card(frame: &mut Frame, area: Rect, title: &str, lines: Vec<Line>, border: Style) {
    let width = (2 + lines.iter().map(Line::width).max().unwrap_or(0) as u16)
        .max(title.chars().count() as u16 + 2)
        .min(area.width);
    // After the clamp, so a line too wide gets the rows it wraps onto.
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
        Paragraph::new(lines).wrap(Wrap { trim: false }).block(
            Block::bordered()
                // The difference between a card lying over the screen and a box
                // drawn on it: the corner is the part anybody reads.
                .border_type(BorderType::Rounded)
                // In the border's colour: a card whose name is as loud as what
                // is on it is one you read twice.
                .title(Span::styled(title, border.add_modifier(Modifier::BOLD)))
                .border_style(border),
        ),
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::app::Subject;

    /// Restated rather than reached for, because what is checked is that `LABEL`
    /// is wide enough for all of them.
    const LABELS: [&str; 5] = ["Notebook", "Branch", "Remote", "Notes", "Changes"];

    #[test]
    fn the_longest_label_still_gets_a_gap_after_its_colon() {
        // Sized to the longest exactly, that one row's padding is empty:
        // `Notebook:work`, with no space at all, under four rows that have two
        // or three. The column lines up and the entry that set its width is the
        // one that reads as a mistake.
        let widest = LABELS
            .iter()
            .map(|label| label.chars().count() + ":".len())
            .max()
            .expect("there are labels");
        assert!(
            LABEL > widest,
            "the widest label fills its own column: {widest} of {LABEL}"
        );
    }

    #[test]
    fn a_short_terminal_gets_a_header_it_can_afford() {
        assert_eq!(header_rows(40), INFO_ROWS);
        assert_eq!(header_rows(24), INFO_ROWS);
        // The size the card once outgrew, and the size a split pane made tight.
        assert_eq!(header_rows(14), 1);
    }

    /// Every key on a screen's grid, in the order it is laid out, with the
    /// blanks that pad a column out to five left off.
    fn named(view: &View) -> Vec<&'static str> {
        keys_for(view)
            .into_iter()
            .flatten()
            .map(|(key, _)| key)
            .filter(|key| !key.is_empty())
            .collect()
    }

    /// One of each kind of screen there is, which is what the checks below have
    /// to hold for.
    fn every_screen() -> Vec<View> {
        vec![
            View::Notes,
            View::Note("aaaa1111".to_string()),
            View::Todo,
            View::Tags,
            View::Files,
            View::Notebooks,
            View::Deleted,
            View::Diff,
            View::Log(None),
            View::Log(Some("aaaa1111".to_string())),
            View::Backlinks(Subject::Note("aaaa1111".to_string())),
            View::Backlinks(Subject::File("diagram.png".to_string())),
            View::Blame("aaaa1111".to_string()),
        ]
    }

    #[test]
    fn the_keys_are_the_ones_the_screen_answers_to() {
        let listing = named(&View::Notes);
        assert!(listing.contains(&"enter"));
        assert!(listing.contains(&"space"));

        // A note has no cursor to mark and nothing to filter, and says so by not
        // offering the keys.
        let note = named(&View::Note("aaaa1111".to_string()));
        assert!(!note.contains(&"space"));
        assert!(!note.contains(&"/"));
        // But every key that changes a note is on both, spelled the same way.
        for key in ["e", "m", "#", "ctrl-d", "T"] {
            assert!(listing.contains(&key), "the listing lost {key}");
            assert!(note.contains(&key), "the note lost {key}");
        }

        // A screen whose rows are notes offers them too, and one whose rows are
        // not does not: `e` there would have nothing to edit.
        assert!(named(&View::Todo).contains(&"e"));
        assert!(!named(&View::Tags).contains(&"e"));
        assert!(!named(&View::Notebooks).contains(&"ctrl-d"));
    }

    #[test]
    fn no_key_is_listed_twice_on_one_screen() {
        // The grid is laid out column-major and the last column may be short,
        // which is fine. What is not fine is the same key appearing twice: the
        // second entry is unreachable, and the two say different things about
        // what pressing it does.
        for view in every_screen() {
            let mut seen = std::collections::BTreeSet::new();
            for key in named(&view) {
                assert!(
                    seen.insert(key),
                    "`{key}` is on {}'s grid twice",
                    view.crumb()
                );
            }
        }
    }

    #[test]
    fn every_screen_says_how_to_leave_it_and_where_everything_else_is() {
        // The three that cannot be looked up if they are not shown: `:` is how
        // the screens with no key of their own are reached, `?` is the card, and
        // a screen that does not say how to get out of it is a trap. All three
        // are in the first column, which is the one that is never dropped.
        for view in every_screen() {
            let first: Vec<&str> = keys_for(&view)[0].iter().map(|(key, _)| *key).collect();
            for key in [":", "?"] {
                assert!(
                    first.contains(&key),
                    "{} does not show {key} in its first column",
                    view.crumb()
                );
            }
            // The listing is exempt: it is the bottom of the stack, so there is
            // nothing to back out of and `q` is the way out — which lives in a
            // column that may be dropped, as it always has.
            if !matches!(view, View::Notes) {
                assert!(
                    first.contains(&"esc"),
                    "{} does not show esc in its first column",
                    view.crumb()
                );
            }
        }
    }
}
