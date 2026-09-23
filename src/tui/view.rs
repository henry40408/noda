//! Drawing one frame: the body band and the cards; the other bands are
//! `super::frame`.
//!
//! The listing uses `noda ls`'s row, so a note is named the same everywhere. A
//! note is drawn as `noda show` prints it, with search matches picked out as
//! `noda search` does.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, HighlightSpacing, Padding, Paragraph, Row, Scrollbar, ScrollbarOrientation,
    ScrollbarState, Table, Wrap,
};

use super::app::{App, Choice, Mark, Mode, Proposal, SCOPE_KEYS, View, What};
use super::command;
use super::frame::{self, card, plural};
use super::theme;
use crate::cmd::{self, display_width, find_ignoring_case};
use crate::style as palette;

/// The widest key cell on the help card, so the descriptions line up.
const KEY_COLUMN: usize = 22;

/// In front of the id, the same width marked or not, so columns never shift.
const MARK: &str = "• ";
const UNMARKED: &str = "  ";

/// `TITLE_FLOOR` is the title width kept however long the tags.
const COLUMN_GAP: usize = 2;
const TITLE_FLOOR: usize = 10;

const SHORT_COMMIT: u16 = 7;

/// Wider by the unpushed mark, which only the log shows.
const MARKED_COMMIT: u16 = SHORT_COMMIT + 2;

/// The scrollbar's column, reserved even when no bar is drawn so columns do not
/// shift when a list overflows.
const PADDING: u16 = 1;

/// A half block points at the row, not a place in the text. The space keeps it
/// from reading as part of a commit hash.
const CURSOR_BAR: &str = "▌ ";

/// The cursor bar's width, taken off every row measurement.
const GUTTER: usize = 2;

const HEADING_ROWS: u16 = 1;

/// The `?` card: what the header's key grid leaves out or may drop. Kept short
/// enough to fit a 24-row terminal.
const KEYS: &[(&str, &str)] = &[
    ("j / k, ↓ / ↑", "move · scroll"),
    ("ctrl-f / ctrl-b, g / G", "half a screen · first / last"),
    ("enter, esc", "open it · back out of it"),
    ("/", "filter: tag:work OR tag:q3 budget"),
    (":, ctrl-a", "run a command · the list of what it takes"),
    ("space, *, Q", "mark · mark all shown · the queue"),
    // `p` is only here: every column of the header's grid is full.
    ("e, a, p", "edit in $EDITOR · new note · pin, and unpin"),
    ("m, #", "retitle · tags: a box each, tab chooses"),
    ("ctrl-d, T", "delete (after a y) · leave updated alone"),
    ("t, l, b, B", "todo · log · backlinks · blame"),
    (
        "S, R, ctrl-w, 1-9",
        "sort · reverse · wide row · a tag (0 = all)",
    ),
    ("r, ctrl-g, q / ctrl-c", "read again · crumbs · quit"),
    ("while typing", "readline: ctrl-a/e/w/u/k/y, alt-b/f"),
];

pub fn draw(f: &mut Frame, app: &mut App) {
    let area = f.area();
    // Hidden crumbs get zero rows, so the body gets them.
    let [header, title, body, crumbs, status] = ratatui::layout::Layout::vertical([
        Constraint::Length(frame::header_rows(area.height)),
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(u16::from(app.crumbs_shown)),
        Constraint::Length(1),
    ])
    .areas(area);

    // Less the heading row, or a half-screen jump lands a row too far.
    app.set_page(if app.has_rows() {
        body.height.saturating_sub(HEADING_ROWS)
    } else {
        body.height
    });
    frame::draw_header(f, header, app);
    frame::draw_title(f, title, app);
    draw_body(f, body, app);
    if app.crumbs_shown {
        frame::draw_crumbs(f, crumbs, app);
    }
    if let Some(x) = frame::draw_status(f, status, app) {
        f.set_cursor_position((x, status.y));
    }
    // At most one card, matching the mode.
    match app.mode {
        Mode::Help => draw_help(f, area),
        Mode::Commands => draw_commands(f, area, app),
        Mode::Confirm(what) => draw_confirm(f, area, app, what),
        Mode::Queue => draw_queue(f, area, app),
        Mode::Tagging => draw_tagging(f, area, app),
        Mode::Alert => draw_alert(f, area, app),
        _ => {}
    }
}

fn less_the_bar(area: Rect) -> (Rect, Rect) {
    let [content, bar] =
        Layout::horizontal([Constraint::Fill(1), Constraint::Length(PADDING)]).areas(area);
    (content, bar)
}

/// Drawn only on overflow, and without end arrows, which would cost two rows.
fn draw_scrollbar(f: &mut Frame, area: Rect, total: usize, shown: usize, at: usize) {
    if total <= shown || area.height == 0 {
        return;
    }
    let mut state = ScrollbarState::new(total.saturating_sub(shown))
        .viewport_content_length(shown)
        .position(at);
    f.render_stateful_widget(
        Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None)
            .track_symbol(Some("│"))
            .track_style(theme::from(palette::TAGS_PUNCT))
            .thumb_symbol("█"),
        area,
        &mut state,
    );
}

/// The one table shape for every screen of rows. The cursor is a bar and bold
/// rather than reversed, which would invert the id's and tags' colours.
fn sheet<'a>(rows: Vec<Row<'a>>, widths: Vec<Constraint>, headings: &[String]) -> Table<'a> {
    Table::new(rows, widths)
        .header(Row::new(
            headings
                .iter()
                .map(|name| Line::from(Span::styled(name.clone(), theme::from(palette::COLUMN))))
                .collect::<Vec<_>>(),
        ))
        .column_spacing(COLUMN_GAP as u16)
        .row_highlight_style(Style::default().add_modifier(Modifier::BOLD))
        .highlight_symbol(Span::styled(CURSOR_BAR, theme::from(palette::CURSOR)))
        // Always, or the columns shift on a list with no cursor.
        .highlight_spacing(HighlightSpacing::Always)
}

/// A heading indented past the mark, which is inside the cell.
fn under_mark(name: &str) -> String {
    format!("{UNMARKED}{name}")
}

fn headings(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_string()).collect()
}

/// A list with a cursor or a page to scroll, as `app` decided; deciding again
/// here could disagree with what `j` does.
fn draw_body(f: &mut Frame, area: Rect, app: &mut App) {
    match app.view().clone() {
        View::Notes => draw_listing(f, area, app),
        View::Note(_) => draw_note(f, area, app),
        View::Todo => draw_rows(f, area, app, todo_rows(app), "nothing to do"),
        View::Tags => draw_rows(f, area, app, tag_rows(app), "no tags yet"),
        View::Files => draw_rows(
            f,
            area,
            app,
            file_rows(app),
            "this notebook holds nothing but notes",
        ),
        View::Notebooks => draw_rows(f, area, app, notebook_rows(app), "no notebooks"),
        View::Deleted => draw_rows(f, area, app, deleted_rows(app), "nothing has been deleted"),
        View::Backlinks(_) => draw_rows(f, area, app, backlink_rows(app), "nothing links here"),
        View::Log(_) => draw_rows(f, area, app, log_rows(app), "no commits"),
        View::Blame(_) => draw_blame(f, area, app),
        View::Diff => draw_diff(f, area, app),
    }
}

/// An empty screen says why in its own words, not "no results".
fn draw_nothing(f: &mut Frame, area: Rect, said: &str) {
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(said, theme::from(palette::MUTED))))
            // Aligned with where a list's rows start.
            .block(Block::new().padding(Padding::new(GUTTER as u16, PADDING, 0, 0))),
        area,
    );
}

fn draw_listing(f: &mut Frame, area: Rect, app: &mut App) {
    if app.shown() == 0 {
        draw_nothing(
            f,
            area,
            if app.total() == 0 {
                "this notebook has no notes yet"
            } else {
                "nothing matches"
            },
        );
        return;
    }

    let (area, bar) = less_the_bar(area);

    // Taken out so rows can borrow `app` while ratatui writes the offset.
    let mut state = app.take_table();

    let id_width = MARK.chars().count()
        + app
            .rows()
            .map(|file| file.id.chars().count())
            .max()
            .unwrap_or(0);
    // The row's real width, less the cursor bar's gutter (the scrollbar's
    // column is already gone).
    let inner = (area.width as usize).saturating_sub(GUTTER);

    // What `-l` adds.
    let mut extra: Vec<(&'static str, Vec<String>)> = Vec::new();
    if app.long {
        extra.push(("slug", app.rows().map(|file| file.slug.clone()).collect()));
        extra.push((
            "created",
            app.rows()
                .map(|file| stamp(file.note.created.as_ref()))
                .collect(),
        ));
        extra.push((
            "updated",
            app.rows()
                .map(|file| stamp(file.note.updated.as_ref()))
                .collect(),
        ));
    }
    // `-l`'s columns are dropped whole from the right while the title is below
    // its floor: the id and title name a note, the rest is density.
    let mut widths: Vec<usize> = extra
        .iter()
        .map(|(_, values)| values.iter().map(|v| display_width(v)).max().unwrap_or(0))
        .collect();
    // Only a listing holding a pin gets the pin column.
    let pinned_here = app.rows().any(|file| file.note.is_pinned());
    let pin_width = if pinned_here {
        palette::PIN_MARK.chars().count() + COLUMN_GAP
    } else {
        0
    };
    let spent = |widths: &[usize]| {
        widths.iter().sum::<usize>()
            + (widths.len() + 2) * COLUMN_GAP
            + id_width
            + TITLE_FLOOR
            + pin_width
    };
    while !widths.is_empty() && spent(&widths) > inner {
        widths.pop();
        extra.pop();
    }

    // As wide as the longest tag list, but only what the title's floor leaves:
    // an imported tag may be a sentence, and a note is found by its title.
    let room = inner.saturating_sub(spent(&widths));
    let tag_width = app
        .rows()
        .map(|file| tags(&file.note.tags).chars().count())
        .max()
        .unwrap_or(0)
        .min(room);

    let terms = app.terms().to_vec();
    let muted = theme::from(palette::MUTED);
    let rows: Vec<Row> = app
        .rows()
        .enumerate()
        .map(|(at, file)| {
            let mut cells = vec![
                Line::from(vec![
                    Span::styled(
                        if app.marked(&file.id) { MARK } else { UNMARKED },
                        theme::from(palette::MATCH),
                    ),
                    Span::styled(file.id.as_str(), theme::from(palette::ID)),
                ]),
                // Uncoloured, as in `noda ls`: it is where the eye lands.
                marked(&file.note.title, &terms, Style::default()),
            ];
            // `-l` extends the row, in the CLI's order and colours.
            for (which, values) in &extra {
                let style = if *which == "slug" {
                    theme::from(palette::SLUG)
                } else {
                    muted
                };
                cells.push(Line::from(Span::styled(values[at].clone(), style)));
            }
            cells.push(Line::from(
                palette::tag_pieces(&file.note.tags)
                    .into_iter()
                    .map(|(style, text)| Span::styled(text, theme::from(style)))
                    .collect::<Vec<_>>(),
            ));
            if pinned_here {
                cells.push(Line::from(if file.note.is_pinned() {
                    Span::styled(palette::PIN_MARK, theme::from(palette::PIN))
                } else {
                    Span::raw("")
                }));
            }
            Row::new(cells)
        })
        .collect();

    let mut constraints = vec![Constraint::Length(id_width as u16), Constraint::Fill(1)];
    constraints.extend(widths.iter().map(|w| Constraint::Length(*w as u16)));
    constraints.push(Constraint::Length(tag_width as u16));
    if pinned_here {
        constraints.push(Constraint::Length(palette::PIN_MARK.chars().count() as u16));
    }

    // Headings are needed with `-l`: `created` and `updated` look alike.
    let mut names = vec![under_mark("ID"), "TITLE".to_string()];
    names.extend(extra.iter().map(|(which, _)| which.to_uppercase()));
    names.push("TAGS".to_string());
    if pinned_here {
        names.push(palette::PIN_MARK.to_uppercase());
    }

    let rows_shown = rows.len();
    f.render_stateful_widget(sheet(rows, constraints, &names), area, &mut state);
    draw_scrollbar(
        f,
        rows_area(bar),
        rows_shown,
        rows_area(bar).height as usize,
        state.offset(),
    );
    app.put_table(state);
}

/// The bar lines up with the rows, not the heading.
fn rows_area(area: Rect) -> Rect {
    let [_, rows] =
        Layout::vertical([Constraint::Length(HEADING_ROWS), Constraint::Fill(1)]).areas(area);
    rows
}

/// `noda ls -l`'s dash for a missing time, rather than a blank cell.
fn stamp(value: Option<&String>) -> String {
    value.cloned().unwrap_or_else(|| "-".to_string())
}

fn draw_rows(f: &mut Frame, area: Rect, app: &mut App, sheet_of: Sheet, empty: &str) {
    if sheet_of.rows.is_empty() {
        draw_nothing(f, area, empty);
        return;
    }
    let (area, bar) = less_the_bar(area);
    let mut state = app.take_table();
    let total = sheet_of.rows.len();
    f.render_stateful_widget(
        sheet(sheet_of.rows, sheet_of.widths, &sheet_of.names),
        area,
        &mut state,
    );
    draw_scrollbar(
        f,
        rows_area(bar),
        total,
        rows_area(bar).height as usize,
        state.offset(),
    );
    app.put_table(state);
}

struct Sheet {
    names: Vec<String>,
    widths: Vec<Constraint>,
    rows: Vec<Row<'static>>,
}

fn widest(of: impl Iterator<Item = usize>) -> u16 {
    of.max().unwrap_or(0) as u16
}

/// A `Sheet` row outlives the borrow of `app`, so it owns its text.
fn owned(line: Line<'_>) -> Line<'static> {
    Line::from(
        line.spans
            .into_iter()
            .map(|span| Span::styled(span.content.into_owned(), span.style))
            .collect::<Vec<_>>(),
    )
}

/// Only an overdue date is coloured. Never truncated, as in `noda todo`.
fn todo_rows(app: &App) -> Sheet {
    let muted = theme::from(palette::MUTED);
    let notes = |pick: fn(&crate::notebook::NoteFile) -> &str| {
        widest(
            app.tasks()
                .iter()
                .filter_map(|task| app.note_at(task.note))
                .map(|file| display_width(pick(file))),
        )
    };
    let rows = app
        .tasks()
        .iter()
        .filter_map(|task| {
            let file = app.note_at(task.note)?;
            let due = match &task.item.due {
                Some(due) if task.item.overdue(app.today()) => {
                    Span::styled(due.clone(), theme::from(palette::OVERDUE))
                }
                Some(due) => Span::styled(due.clone(), muted),
                None => Span::raw(String::new()),
            };
            Some(Row::new(vec![
                Line::from(Span::styled(file.id.clone(), theme::from(palette::ID))),
                Line::from(Span::styled(file.slug.clone(), theme::from(palette::SLUG))),
                Line::from(due),
                Line::from(Span::raw(task.item.text.clone())),
            ]))
        })
        .collect();
    Sheet {
        names: headings(&["ID", "SLUG", "DUE", "TASK"]),
        widths: vec![
            Constraint::Length(notes(|file| &file.id)),
            Constraint::Length(notes(|file| &file.slug)),
            Constraint::Length(cmd::DATE_WIDTH as u16),
            Constraint::Fill(1),
        ],
        rows,
    }
}

/// Every tag, commonest first, with its note count.
fn tag_rows(app: &App) -> Sheet {
    let muted = theme::from(palette::MUTED);
    let width = widest(app.tallies().iter().map(|t| display_width(&t.tag)));
    let rows = app
        .tallies()
        .iter()
        .enumerate()
        .map(|(at, tally)| {
            // The digit key that filters the listing by this tag.
            let key = if at < SCOPE_KEYS {
                format!("{}", at + 1)
            } else {
                String::new()
            };
            Row::new(vec![
                Line::from(Span::styled(key, theme::from(palette::ID))),
                Line::from(Span::styled(tally.tag.clone(), theme::from(palette::TAGS))),
                Line::from(Span::styled(plural(tally.notes, "note"), muted)),
            ])
        })
        .collect();
    Sheet {
        // No heading fits a one-column cell.
        names: headings(&["", "TAG", "NOTES"]),
        widths: vec![
            Constraint::Length(1),
            Constraint::Length(width),
            Constraint::Fill(1),
        ],
        rows,
    }
}

fn file_rows(app: &App) -> Sheet {
    let rows = app
        .files()
        .iter()
        .map(|name| Row::new(vec![Line::from(Span::raw(name.clone()))]))
        .collect();
    Sheet {
        names: headings(&["FILE"]),
        widths: vec![Constraint::Fill(1)],
        rows,
    }
}

/// The listing's mark on the current notebook.
fn notebook_rows(app: &App) -> Sheet {
    let rows = app
        .notebooks()
        .iter()
        .map(|name| {
            let here = *name == app.notebook;
            Row::new(vec![Line::from(vec![
                Span::styled(
                    if here { MARK } else { UNMARKED },
                    theme::from(palette::MATCH),
                ),
                Span::raw(name.clone()),
            ])])
        })
        .collect();
    Sheet {
        names: vec![under_mark("NOTEBOOK")],
        widths: vec![Constraint::Fill(1)],
        rows,
    }
}

/// The revision shown is the one `restore` needs, the commit before the
/// deletion, as `noda deleted` shows it.
fn deleted_rows(app: &App) -> Sheet {
    let muted = theme::from(palette::MUTED);
    let ids = widest(app.gone().iter().map(|gone| display_width(&gone.id)));
    let slugs = widest(app.gone().iter().map(|gone| display_width(&gone.slug)));
    let rows = app
        .gone()
        .iter()
        .map(|gone| {
            Row::new(vec![
                Line::from(Span::styled(gone.id.clone(), theme::from(palette::ID))),
                Line::from(Span::styled(gone.slug.clone(), theme::from(palette::SLUG))),
                Line::from(Span::styled(
                    cmd::format_time(gone.removed_at, gone.offset_minutes),
                    muted,
                )),
                Line::from(Span::styled(
                    gone.restore_from_short(),
                    theme::from(palette::COMMIT),
                )),
                Line::from(Span::raw(gone.title.clone())),
            ])
        })
        .collect();
    Sheet {
        // `FROM`: it is the commit before the deletion, in seven columns.
        names: headings(&["ID", "SLUG", "DELETED", "FROM", "TITLE"]),
        widths: vec![
            Constraint::Length(ids),
            Constraint::Length(slugs),
            Constraint::Length(cmd::TIME_WIDTH as u16),
            Constraint::Length(SHORT_COMMIT),
            Constraint::Fill(1),
        ],
        rows,
    }
}

/// `noda ls`'s row, since each result is a note.
fn backlink_rows(app: &App) -> Sheet {
    let found = || app.linking().iter().filter_map(|&at| app.note_at(at));
    let ids = widest(found().map(|file| display_width(&file.id)));
    let terms = app.terms().to_vec();
    let rows = found()
        .map(|file| {
            Row::new(vec![
                Line::from(Span::styled(file.id.clone(), theme::from(palette::ID))),
                owned(marked(&file.note.title, &terms, Style::default())),
                Line::from(
                    palette::tag_pieces(&file.note.tags)
                        .into_iter()
                        .map(|(style, text)| Span::styled(text, theme::from(style)))
                        .collect::<Vec<_>>(),
                ),
            ])
        })
        .collect();
    Sheet {
        names: headings(&["ID", "TITLE", "TAGS"]),
        widths: vec![
            Constraint::Length(ids),
            Constraint::Fill(1),
            Constraint::Length(widest(
                found().map(|file| tags(&file.note.tags).chars().count()),
            )),
        ],
        rows,
    }
}

/// Commits, newest first, in `noda log`'s columns.
fn log_rows(app: &App) -> Sheet {
    let muted = theme::from(palette::MUTED);
    let rows = app
        .entries()
        .iter()
        .map(|entry| {
            // Inside the commit column, where `noda log` puts it, rather than a
            // column that is blank on most rows.
            let mark = if app.is_unpushed(entry.id) {
                cmd::UNPUSHED
            } else {
                " "
            };
            Row::new(vec![
                Line::from(vec![
                    Span::styled(format!("{mark} "), muted),
                    Span::styled(entry.short_id(), theme::from(palette::COMMIT)),
                ]),
                Line::from(Span::styled(
                    cmd::format_time(entry.seconds, entry.offset_minutes),
                    muted,
                )),
                Line::from(Span::raw(entry.summary.clone())),
            ])
        })
        .collect();
    Sheet {
        // Indented over the ids, not the arrows.
        names: headings(&["  COMMIT", "WHEN", "SUMMARY"]),
        widths: vec![
            Constraint::Length(MARKED_COMMIT),
            Constraint::Length(cmd::TIME_WIDTH as u16),
            Constraint::Fill(1),
        ],
        rows,
    }
}

/// A page, not a list. Not wrapped, so the commit and time columns line up.
fn draw_blame(f: &mut Frame, area: Rect, app: &App) {
    let muted = theme::from(palette::MUTED);
    let lines: Vec<Line> = app
        .blamed()
        .iter()
        .map(|line| {
            let when = if line.commit.is_some() {
                cmd::format_time(line.seconds, line.offset_minutes)
            } else {
                format!("{:<width$}", "not committed", width = cmd::TIME_WIDTH)
            };
            Line::from(vec![
                Span::styled(line.short_commit(), theme::from(palette::COMMIT)),
                Span::raw("  "),
                Span::styled(when, muted),
                Span::raw("  "),
                Span::raw(line.text.clone()),
            ])
        })
        .collect();
    draw_page(f, area, lines, app.scroll(), false);
}

/// The bar counts source lines, which `j` moves by, not wrapped rows: an
/// approximate bar beats one that disagrees with the key.
fn draw_page(f: &mut Frame, area: Rect, lines: Vec<Line>, scroll: u16, wrap: bool) {
    let (area, bar) = less_the_bar(area);
    let total = lines.len();
    let mut page = Paragraph::new(lines)
        // The gutter, so text starts where rows do.
        .block(Block::new().padding(Padding::new(GUTTER as u16, 0, 0, 0)))
        .scroll((scroll, 0));
    if wrap {
        page = page.wrap(Wrap { trim: false });
    }
    f.render_widget(page, area);
    draw_scrollbar(f, bar, total, bar.height as usize, scroll as usize);
}

/// Coloured by each line's prefix; `fetch` strips `cmd::diff`'s escapes rather
/// than parsing them.
fn draw_diff(f: &mut Frame, area: Rect, app: &App) {
    let Some(patch) = app.text() else {
        f.render_widget(Block::new().padding(Padding::horizontal(PADDING)), area);
        return;
    };
    if patch.trim().is_empty() {
        draw_nothing(f, area, "nothing has changed since the last commit");
        return;
    }
    let lines: Vec<Line> = patch
        .lines()
        .map(|line| {
            let style = if line.starts_with("+++") || line.starts_with("---") {
                theme::from(palette::HEADING)
            } else if line.starts_with('+') {
                theme::from(palette::ADDED)
            } else if line.starts_with('-') {
                theme::from(palette::REMOVED)
            } else if line.starts_with("@@") {
                theme::from(palette::HUNK)
            } else if line.starts_with("diff ") || line.starts_with("index ") {
                theme::from(palette::HEADING)
            } else {
                Style::default()
            };
            Line::from(Span::styled(line, style))
        })
        .collect();
    // Unwrapped: a wrapped `+` line reads as two additions.
    draw_page(f, area, lines, app.scroll(), false);
}

fn draw_note(f: &mut Frame, area: Rect, app: &App) {
    let Some(text) = app.text() else {
        f.render_widget(Block::new(), area);
        return;
    };
    // Wrapped, so prose needs no sideways scrolling.
    draw_page(f, area, lines(text, app.terms()), app.scroll(), true);
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

/// Cut to what the terminal holds, cursor kept in view and the rest counted in
/// the footer, which would otherwise run off the bottom.
fn draw_commands(f: &mut Frame, area: Rect, app: &App) {
    let muted = theme::from(palette::MUTED);
    let shown: Vec<&command::Spec> = command::matching(app.input.text()).collect();
    let width = shown
        .iter()
        .map(|spec| spec.usage().chars().count())
        .max()
        .unwrap_or(0);

    // Less two of border, a blank and the footer. One row per command, which
    // holds only because descriptions are cut rather than wrapped.
    let room = (area.height as usize).saturating_sub(4).max(1);
    let first = app.commands_at().saturating_sub(room.saturating_sub(1));
    let told = (area.width as usize).saturating_sub(2 + width + 2);
    let mut lines: Vec<Line> = if shown.is_empty() {
        vec![Line::from(Span::styled("nothing goes by that", muted))]
    } else {
        shown
            .iter()
            .enumerate()
            .skip(first)
            .take(room)
            .map(|(at, spec)| {
                let usage = if at == app.commands_at() {
                    Style::default().add_modifier(Modifier::REVERSED)
                } else {
                    theme::from(palette::ID)
                };
                Line::from(vec![
                    Span::styled(format!("{:<width$}", spec.usage()), usage),
                    Span::styled(format!("  {}", cut(spec.what, told)), muted),
                ])
            })
            .collect()
    };

    let more = shown.len().saturating_sub(first + room);
    let footer = if more > 0 {
        format!("enter  put it on the prompt       esc  back       {more} more")
    } else {
        "type to narrow       enter  put it on the prompt       esc  back".to_string()
    };
    lines.push(Line::default());
    lines.push(Line::from(Span::styled(footer, muted)));

    let title = if app.input.is_empty() {
        " commands ".to_string()
    } else {
        format!(" commands: {} ", app.input.text())
    };
    card(f, area, &title, lines, muted);
}

/// Asked on screen: here a delete is one chord, and in raw mode a command
/// reading stdin would steal the browser's keystrokes.
fn draw_confirm(f: &mut Frame, area: Rect, app: &App, what: What) {
    let muted = theme::from(palette::MUTED);
    let queued = || {
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
        // Deletions counted apart: they are why the question is asked.
        What::Send => (
            " send the queue? ",
            queued(),
            format!(
                "{} to be deleted — the commit stays, so git revert brings them back",
                plural(app.queued_deletions(), "note")
            ),
            "y  send it       any other key  back to the queue",
        ),
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

/// Each line is the sentence the commit message will use.
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

/// A box per notebook tag, in the tags screen's order. The count is the tag's
/// note total for one note, or how many of the marked set hold it.
fn draw_tagging(f: &mut Frame, area: Rect, app: &App) {
    let muted = theme::from(palette::MUTED);
    let total = app.picking_notes();
    let here = app.tags_at();
    let shown = app.shown_tags();
    let proposal = app.proposal();

    // In terminal columns, not characters, so the counts line up past CJK.
    let width = shown
        .iter()
        .filter_map(|&at| app.choices().get(at))
        .map(|choice| display_width(&choice.tag))
        .chain(match &proposal {
            Some(Proposal::New { tag, .. }) => Some(display_width(tag)),
            _ => None,
        })
        .max()
        .unwrap_or(0);

    let mut rows: Vec<Line> = shown
        .iter()
        .enumerate()
        .filter_map(|(row, &at)| Some((row, app.choices().get(at)?)))
        .map(|(row, choice)| chosen(choice, total, width, row == here))
        .collect();
    if let Some(proposal) = &proposal {
        rows.push(proposed(proposal, width, shown.len() == here));
    }

    // Less two of border, a blank and the footer.
    let room = (area.height as usize).saturating_sub(4).max(1);
    let first = here.saturating_sub(room.saturating_sub(1));
    let mut lines: Vec<Line> = if rows.is_empty() {
        vec![Line::from(Span::styled(
            "no tags yet — type one to make it",
            muted,
        ))]
    } else {
        rows.into_iter().skip(first).take(room).collect()
    };

    // With marks the change is queued, so "apply" would overpromise.
    let doing = if app.marks.is_empty() {
        "apply"
    } else {
        "queue it"
    };
    let more = app.picker_rows().saturating_sub(first + room);
    let footer = if more > 0 {
        format!("tab  choose      enter  {doing}      esc  back      {more} more")
    } else {
        format!("type to narrow      tab  choose      enter  {doing}      esc  back")
    };
    lines.push(Line::default());
    lines.push(Line::from(Span::styled(footer, muted)));

    let about = match app.picking_note() {
        Some(file) => file.slug.clone(),
        None => plural(total, "note"),
    };
    let title = if app.input.is_empty() {
        format!(" tags: {about} ")
    } else {
        format!(" tags: {about} / {} ", app.input.text())
    };
    card(f, area, &title, lines, muted);
}

/// A tag no note carries was just made on this card, so it says `new`, not
/// `0 notes`.
fn chosen(choice: &Choice, total: usize, width: usize, here: bool) -> Line<'static> {
    let count = if choice.notes == 0 {
        "new".to_string()
    } else if total > 1 {
        format!("{} of {total}", choice.held)
    } else {
        plural(choice.notes, "note")
    };
    Line::from(vec![
        Span::styled(format!("{} ", choice.tick(total)), box_style(choice, total)),
        Span::styled(padded(&choice.tag, width), name_style(here)),
        Span::styled(format!("  {count}"), theme::from(palette::MUTED)),
    ])
}

/// The row offering a typed tag the notebook lacks, unticked until chosen.
fn proposed(proposal: &Proposal, width: usize, here: bool) -> Line<'static> {
    match proposal {
        Proposal::New { tag, near } => {
            let mut spans = vec![
                Span::styled("[ ] ", theme::from(palette::MUTED)),
                Span::styled(padded(tag, width), name_style(here)),
                Span::styled("  new", theme::from(palette::MUTED)),
            ];
            // A near-miss of an existing tag is nearly always a misspelling.
            if let Some((near, notes)) = near {
                spans.push(Span::styled(
                    format!(" — close to {near}, {}", plural(*notes, "note")),
                    theme::from(palette::MATCH),
                ));
            }
            Line::from(spans)
        }
        // In `cmd`'s words: shown so `Tab` doing nothing is explained.
        Proposal::Refused(why) => Line::from(Span::styled(
            format!("    {why}"),
            theme::from(palette::INVALID),
        )),
    }
}

/// Padded in terminal columns, not characters.
fn padded(name: &str, width: usize) -> String {
    let pad = " ".repeat(width.saturating_sub(display_width(name)));
    format!("{name}{pad}")
}

fn name_style(here: bool) -> Style {
    if here {
        Style::default().add_modifier(Modifier::REVERSED)
    } else {
        theme::from(palette::TAGS)
    }
}

/// Diff colours for a change, the tags' colour for a tick, muted otherwise.
fn box_style(choice: &Choice, total: usize) -> Style {
    match choice.mark {
        Mark::Add => theme::from(palette::ADDED),
        Mark::Remove => theme::from(palette::REMOVED),
        Mark::Leave if total > 0 && choice.held == total => theme::from(palette::TAGS),
        Mark::Leave => theme::from(palette::MUTED),
    }
}

/// A card for a multi-line answer the status line would cut, such as `bulk`'s
/// list of what it could not do.
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

/// Cut on a character, not a byte, which could panic mid-code-point.
fn cut(text: &str, room: usize) -> String {
    if text.chars().count() <= room {
        return text.to_string();
    }
    let kept: String = text.chars().take(room.saturating_sub(1)).collect();
    format!("{}…", kept.trim_end())
}

/// The uncoloured tag list, built from the same pieces as the row so the
/// column width cannot drift from it.
fn tags(tags: &[String]) -> String {
    palette::tag_pieces(tags)
        .into_iter()
        .map(|(_, text)| text)
        .collect()
}

/// The frontmatter dimmed and search terms marked in the body.
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

/// Judged as `cmd::dim_frontmatter` does: no closed block, nothing dimmed.
fn split_frontmatter(text: &str) -> (&str, &str) {
    let Some(rest) = text.strip_prefix("---\n") else {
        return ("", text);
    };
    let Some(end) = rest.find("\n---\n") else {
        return ("", text);
    };
    text.split_at("---\n".len() + end + "\n---\n".len())
}

/// The earliest match wins an overlap and the search resumes after it, so a
/// line is walked once.
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
    fn the_gutter_is_the_same_width_on_every_screen() {
        assert_eq!(CURSOR_BAR.chars().count(), GUTTER);
        assert_eq!(under_mark("ID"), "  ID");
        assert_eq!(UNMARKED.len(), MARK.chars().count());
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
        // Thirteen rows and a border, inside a 24-row terminal.
        assert!(KEYS.len() + 2 <= 15, "the card has {} rows", KEYS.len() + 2);
        let widest = KEYS.iter().map(|(key, _)| key.chars().count()).max();
        assert_eq!(widest, Some(KEY_COLUMN));
    }

    #[test]
    fn every_key_that_only_the_card_can_teach_is_on_the_card() {
        // The grid may drop these. Both columns are searched, since the
        // readline row names its keys in the description.
        let said = KEYS
            .iter()
            .map(|(key, what)| format!("{key} {what}"))
            .collect::<Vec<_>>()
            .join(" ");
        for key in ["ctrl-f", "S", "R", "ctrl-w", "1-9", "ctrl-g", "readline"] {
            assert!(said.contains(key), "the card does not teach {key}");
        }
    }

    #[test]
    fn a_missing_time_is_a_dash() {
        assert_eq!(stamp(None), "-");
        assert_eq!(
            stamp(Some(&"2026-01-01T00:00:00Z".to_string())),
            "2026-01-01T00:00:00Z"
        );
    }
}
