//! The bands every screen shares (header, title, crumbs, status) and the card
//! drawn over them, kept here so no screen's furniture can move.
//!
//! The current screen's keys are in the header rather than behind `?`, which
//! also keeps the help card short enough for a small terminal.

use ratatui::Frame;
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Paragraph, Wrap};

use super::app::{App, Mode, View};
use super::theme;
use crate::cmd::{Sort, Touch};
use crate::style as palette;

pub const INFO_ROWS: u16 = 5;

/// Below this terminal height the header is a single line.
const ROOM_FOR_INFO: u16 = 20;

const MENU_ROWS: usize = 5;

/// One wider than `Notebook:`, so the longest label still gets a gap.
const LABEL: usize = 10;

const MENU_GAP: usize = 2;

const GAP: &str = "  ";

const KEY_GAP: usize = 2;

type Column = [(&'static str, &'static str); MENU_ROWS];

/// Column-major: read down each column of five.
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
    // Listing-only view keys, after the others because columns drop from the
    // right and each of these can also be found on the `?` card.
    [
        ("S", "sort"),
        ("R", "reverse"),
        ("ctrl-w", "wide"),
        ("1-9", "tag, 0 all"),
        ("ctrl-g", "crumbs"),
    ],
    VIEW_KEYS,
];

/// `:` comes first and `g`/`G` is left off: columns drop from the right, and
/// `:` cannot be looked up while `G` is on the card.
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

/// Pads a column out to five. Nothing is drawn.
const BLANK: (&str, &str) = ("", "");

/// Last on every screen, being the first to drop: each is also a `:` command.
const VIEW_KEYS: Column = [
    ("t", "todo"),
    ("l", "log"),
    ("b", "backlinks"),
    ("B", "blame"),
    BLANK,
];

/// The keys of a screen of rows, labelled with what `enter` does there. `None`
/// where `enter` does nothing: the notebook's log has no one note to restore.
fn rows_keys(enter: Option<&'static str>) -> Vec<Column> {
    // The first column is never dropped, so it holds what cannot be looked up.
    // `ctrl-f/b` is left off: its width would push out the note-changing keys.
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

/// A screen whose rows are notes also gets the note-changing keys, ahead of the
/// view keys, which have `:` names to fall back on.
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
        // Short, because this shape has four columns to fit.
        View::Todo | View::Backlinks(_) => {
            let mut keys = rows_keys(Some("read it"));
            keys.push(changing);
            keys
        }
        View::Tags => rows_keys(Some("filter by it")),
        View::Files => rows_keys(Some("what links here")),
        View::Notebooks => rows_keys(Some("switch to it")),
        // The ellipsis: `enter` writes the command rather than running it.
        View::Deleted | View::Log(Some(_)) => rows_keys(Some("restore it…")),
        View::Log(None) => rows_keys(None),
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

pub fn header_rows(height: u16) -> u16 {
    if height >= ROOM_FOR_INFO {
        INFO_ROWS
    } else {
        1
    }
}

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

/// Always the same five labels in the same order, so each stays where the eye
/// learned it.
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

/// Keyed-in state that changes what the next key does. On the title band, not
/// the header, since in the header its width pushed out the key grid.
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

fn info_width(app: &App) -> u16 {
    let widest = info(app).iter().map(Line::width).max().unwrap_or(0);
    (widest + MENU_GAP) as u16
}

/// Columns are dropped from the right rather than squeezed until words are cut.
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
                // A blank still takes its width, or later columns on this row
                // slide left.
                let (key, what) = &columns[at][row];
                if key.is_empty() {
                    spans.push(Span::raw(" ".repeat(*wanted)));
                    continue;
                }
                let named = format!("<{key}>");
                // Padding after the description, or it sits against the next
                // key and reads as that key's.
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

/// The name only; no version, whose columns the key grid needs more.
fn wordmark() -> Vec<Line<'static>> {
    vec![
        Line::from(Span::styled(
            "noda",
            Style::default().add_modifier(Modifier::BOLD),
        ))
        .right_aligned(),
    ]
}

/// The header on a short terminal: the facts on one line, no keys.
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
    spans
}

/// The `--sort`, `-r` and `-l` settings, when not the defaults: those keys
/// rearrange rows and otherwise leave no trace.
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

/// A listing says its filter and count; a note its id then title, as `noda ls`.
pub fn draw_title(frame: &mut Frame, area: Rect, app: &App) {
    let muted = theme::from(palette::MUTED);
    let bold = Style::default().add_modifier(Modifier::BOLD);
    // Name, scope, count in that order on every screen, split by gaps.
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
        // The id is here rather than in the crumbs, which would grow too wide.
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
    // Ruled to the far end, so the band reads as the top of the body.
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

/// Shown even with one crumb, so how far `esc` goes back is never a guess.
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

/// The one line for typing or for the last command's answer; returns where
/// the cursor belongs.
pub fn draw_status(frame: &mut Frame, area: Rect, app: &App) -> Option<u16> {
    let muted = theme::from(palette::MUTED);
    // Drawn on a frame of its own, before the slow action starts.
    if let Some(waiting) = app.working {
        frame.render_widget(Line::from(Span::styled(waiting, muted)), area);
        return None;
    }
    // Measured in columns, not characters (CJK is two wide), over the text left
    // of the cursor.
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
        // Cleared by the next key. A failure uses the invalid-query colour:
        // both are a sentence that never reached the notebook.
        (Some(said), _) => {
            let style = if said.failed {
                theme::from(palette::INVALID)
            } else {
                Style::default()
            };
            (Line::from(Span::styled(said.line(), style)), None)
        }
        // Kept after typing ends: it is why the listing is short.
        _ if !app.search().is_empty() => (
            Line::from(vec![
                Span::styled("/", muted),
                Span::styled(app.search(), muted),
            ]),
            None,
        ),
        _ => (Line::default(), None),
    };

    // An error first, else a prompt's hint.
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

/// `1 note` / `3 notes`.
pub fn plural(n: usize, thing: &str) -> String {
    if n == 1 {
        format!("1 {thing}")
    } else {
        format!("{n} {thing}s")
    }
}

/// As wide as its widest line in terminal columns, clamped to the screen;
/// what does not fit wraps rather than cuts.
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
                .border_type(BorderType::Rounded)
                // In the border's colour, so the title is quieter than the body.
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

    /// Restated, since what is checked is that `LABEL` fits them.
    const LABELS: [&str; 5] = ["Notebook", "Branch", "Remote", "Notes", "Changes"];

    #[test]
    fn the_longest_label_still_gets_a_gap_after_its_colon() {
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
        assert_eq!(header_rows(14), 1);
    }

    /// A screen's keys in grid order, blanks left out.
    fn named(view: &View) -> Vec<&'static str> {
        keys_for(view)
            .into_iter()
            .flatten()
            .map(|(key, _)| key)
            .filter(|key| !key.is_empty())
            .collect()
    }

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

        let note = named(&View::Note("aaaa1111".to_string()));
        assert!(!note.contains(&"space"));
        assert!(!note.contains(&"/"));
        for key in ["e", "m", "#", "ctrl-d", "T"] {
            assert!(listing.contains(&key), "the listing lost {key}");
            assert!(note.contains(&key), "the note lost {key}");
        }

        // Only screens whose rows are notes.
        assert!(named(&View::Todo).contains(&"e"));
        assert!(!named(&View::Tags).contains(&"e"));
        assert!(!named(&View::Notebooks).contains(&"ctrl-d"));
    }

    #[test]
    fn no_key_is_listed_twice_on_one_screen() {
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
        // The first column is never dropped, and these cannot be looked up.
        for view in every_screen() {
            let first: Vec<&str> = keys_for(&view)[0].iter().map(|(key, _)| *key).collect();
            for key in [":", "?"] {
                assert!(
                    first.contains(&key),
                    "{} does not show {key} in its first column",
                    view.crumb()
                );
            }
            // The listing is the bottom of the stack: nothing to back out of.
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
