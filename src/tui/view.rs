//! Drawing one frame.
//!
//! Every screen is the same five bands and only the middle one is drawn here;
//! the rest is [`super::frame`], which is what makes a screen added later look
//! like the ones already there.
//!
//! The listing is `noda ls`'s row, for the reason that row was settled on: a
//! note is named the same way wherever it is named. A note is `noda show`, the
//! frontmatter dimmed and the prose left alone but for the search match —
//! `noda search`'s own exception.

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

/// The longest set of keys on one row, so the descriptions line up.
const KEY_COLUMN: usize = 22;

/// In front of the id, the same width marked or not, so nothing moves.
const MARK: &str = "• ";
const UNMARKED: &str = "  ";

/// Between the columns, and how much title survives however long the tags.
const COLUMN_GAP: usize = 2;
const TITLE_FLOOR: usize = 10;

/// What git abbreviates an object id to.
const SHORT_COMMIT: u16 = 7;

/// Wider by the unpushed mark. Only the log's: `deleted` names a commit too, and
/// one a note was restored from is not something a remote waits for.
const MARKED_COMMIT: u16 = SHORT_COMMIT + 2;

/// Both columns are spoken for — the cursor's bar on the left, a scrollbar on
/// the right — and both are taken whether or not anything is drawn in them: a
/// bar appearing when a list overflows moves every column at the moment the list
/// gets longer.
const PADDING: u16 = 1;

/// A half block rather than an arrow: it points at the row and not at a place in
/// the text, and a solid edge says so without reading as a character. The space
/// is not decoration — against a commit hash the bar would read as part of it.
const CURSOR_BAR: &str = "▌ ";

/// What every measurement of the row has to be made against.
const GUTTER: usize = 2;

/// The row every table spends on the names of its columns.
const HEADING_ROWS: u16 = 1;

/// The keys for the screen you are on are along the top, so this is the rest.
/// Thirteen rows and a border, which is what fits on a terminal short enough to
/// have made the point once already.
const KEYS: &[(&str, &str)] = &[
    ("j / k, ↓ / ↑", "move · scroll"),
    ("ctrl-f / ctrl-b, g / G", "half a screen · first / last"),
    ("enter, esc", "open it · back out of it"),
    ("/", "filter: tag:work OR tag:q3 budget"),
    (":, ctrl-a", "run a command · the list of what it takes"),
    ("space, *, Q", "mark · mark all shown · the queue"),
    ("e, a", "edit in $EDITOR · new note"),
    ("m, #", "retitle · tags: a box each, tab chooses"),
    ("ctrl-d, T", "delete (after a y) · leave updated alone"),
    // One row per group: the card has to stay inside twenty-four rows, which it
    // has already failed to do twice.
    ("t, l, b, B", "todo · log · backlinks · blame"),
    (
        "S, R, ctrl-w, 1-9",
        "sort · reverse · wide row · a tag (0 = all)",
    ),
    ("r, ctrl-g, q / ctrl-c", "read again · crumbs · quit"),
    // A keymap nobody has to read: the row says these keys are answered rather
    // than teaching them.
    ("while typing", "readline: ctrl-a/e/w/u/k/y, alt-b/f"),
];

pub fn draw(f: &mut Frame, app: &mut App) {
    let area = f.area();
    // Given no rows rather than drawn empty, so the notes get it back.
    let [header, title, body, crumbs, status] = ratatui::layout::Layout::vertical([
        Constraint::Length(frame::header_rows(area.height)),
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(u16::from(app.crumbs_shown)),
        Constraint::Length(1),
    ])
    .areas(area);

    // A heading row is one row fewer to move through, and a half-screen jump
    // measured against the whole body lands a row past it.
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
    // Only ever one: a card is what the keyboard is doing.
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

/// Split off whether or not a bar is drawn: taken only on overflow, every column
/// would shift at the moment the list got longer.
fn less_the_bar(area: Rect) -> (Rect, Rect) {
    let [content, bar] =
        Layout::horizontal([Constraint::Fill(1), Constraint::Length(PADDING)]).areas(area);
    (content, bar)
}

/// Nothing is drawn when everything is on screen: an always-full bar says only
/// that the list ends where the reader can see it end. No end arrows either —
/// two of twelve rows is a sixth of the answer spent on decoration.
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

/// One builder rather than one per screen: they are the notebook answering a
/// different question in the same rows-and-a-cursor, and only the columns
/// differ.
///
/// The cursor is a bar and a bolder row rather than a reversed one, which would
/// invert the id's yellow and the tags' cyan along with the rest. The bar sits
/// in what used to be padding, so the row under it sits where every row sits.
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
        // Always, or the columns shift on a list with no cursor — which is what
        // a query being typed produces most often.
        .highlight_spacing(HighlightSpacing::Always)
}

/// Indented past the mark, which is part of the cell rather than a column of its
/// own — so a heading starting where the cell does would sit over it.
fn under_mark(name: &str) -> String {
    format!("{UNMARKED}{name}")
}

/// The names along the top of a screen's table, as one row of headings.
fn headings(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_string()).collect()
}

/// Two shapes and no more: a list with a cursor, or a page to scroll. Which one
/// is the state's answer and not decided again here — a screen that was a list
/// to the keys and a page to the drawing has a `j` that does nothing.
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

/// In the notebook's own words rather than "no results": an empty todo list is a
/// state worth recognising, and "0 rows" is a spreadsheet talking.
fn draw_nothing(f: &mut Frame, area: Rect, said: &str) {
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(said, theme::from(palette::MUTED))))
            // So a list and the sentence standing in for one start alike.
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

    // So the rows may borrow the notes while ratatui writes this frame's offset
    // — different fields, but the borrow checker sees `app`.
    let mut state = app.take_table();

    // In front of the id rather than a column of its own, and as wide either
    // way: a listing that shifted the moment you marked something would make
    // the marking harder to read than the mark is worth.
    let id_width = MARK.chars().count()
        + app
            .rows()
            .map(|file| file.id.chars().count())
            .max()
            .unwrap_or(0);
    // As wide as the longest tag list, unless that would starve the title.
    //
    // A tag may be a sentence, and a column sized to the longest can take a
    // narrow screen whole. So the title gets a floor first and the tags what is
    // left: a note is found by its title, and a cut tag list still says there
    // are tags.
    //
    // Measured against what the row actually gets — the width less the
    // scrollbar's column and the cursor bar's gutter — because counting either
    // as usable is how the title ends up short of its floor.
    let inner = (area.width as usize).saturating_sub(GUTTER);

    // What `-l` adds, measured the same way and dropped from the right when
    // there is no room. A note may have no times at all — nothing invents one,
    // so the column says so rather than leaving a hole the eye has to measure,
    // which is what `noda ls -l` does with it too.
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
    // Dropped from the right, one whole column at a time, while the title still
    // has less than its floor. The id and the title are what name a note; the
    // columns behind them are a density, and a density is the thing to give up.
    let mut widths: Vec<usize> = extra
        .iter()
        .map(|(_, values)| values.iter().map(|v| display_width(v)).max().unwrap_or(0))
        .collect();
    let spent = |widths: &[usize]| {
        widths.iter().sum::<usize>() + (widths.len() + 2) * COLUMN_GAP + id_width + TITLE_FLOOR
    };
    while !widths.is_empty() && spent(&widths) > inner {
        widths.pop();
        extra.pop();
    }

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
                // The title is the column the eye lands on, so it is the one
                // left uncoloured — the same reason `noda ls` leaves it alone.
                marked(&file.note.title, &terms, Style::default()),
            ];
            // `-l` extends the row rather than rearranging it: the id and the
            // title are the first two columns in both, and the tags stay last
            // in both. Same order, same colours, same reasons as the CLI's.
            for (which, values) in &extra {
                let style = if *which == "slug" {
                    theme::from(palette::SLUG)
                } else {
                    muted
                };
                cells.push(Line::from(Span::styled(values[at].clone(), style)));
            }
            cells.push(Line::from(
                // The brackets and the commas grey behind the tags they hold,
                // the same split `noda ls` prints.
                palette::tag_pieces(&file.note.tags)
                    .into_iter()
                    .map(|(style, text)| Span::styled(text, theme::from(style)))
                    .collect::<Vec<_>>(),
            ));
            Row::new(cells)
        })
        .collect();

    let mut constraints = vec![Constraint::Length(id_width as u16), Constraint::Fill(1)];
    constraints.extend(widths.iter().map(|w| Constraint::Length(*w as u16)));
    constraints.push(Constraint::Length(tag_width as u16));

    // In `-l`'s own order, which is why the row is here: `created` and `updated`
    // are the same twenty characters twice.
    let mut names = vec![under_mark("ID"), "TITLE".to_string()];
    names.extend(extra.iter().map(|(which, _)| which.to_uppercase()));
    names.push("TAGS".to_string());

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

/// What a scrollbar has to line up with: the heading is not a row, and a bar
/// starting above the first one is never quite where it says it is.
fn rows_area(area: Rect) -> Rect {
    let [_, rows] =
        Layout::vertical([Constraint::Length(HEADING_ROWS), Constraint::Fill(1)]).areas(area);
    rows
}

/// With `noda ls -l`'s dash for a note that has none: nothing invents one, and
/// a hole is a thing the eye has to measure.
fn stamp(value: Option<&String>) -> String {
    value.cloned().unwrap_or_else(|| "-".to_string())
}

/// The listing's table down to the padding: these are the notebook answering
/// different questions, not different programs.
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

/// Each screen builds its own; the drawing is the same for all of them.
struct Sheet {
    names: Vec<String>,
    widths: Vec<Constraint>,
    rows: Vec<Row<'static>>,
}

/// Measured rather than fixed: every column holds something somebody else chose
/// the length of.
fn widest(of: impl Iterator<Item = usize>) -> u16 {
    of.max().unwrap_or(0) as u16
}

/// A row outlives the borrow of the session it was measured against, so the text
/// comes with it. Only the screens built a row at a time need this — the listing
/// hands ratatui borrowed spans.
fn owned(line: Line<'_>) -> Line<'static> {
    Line::from(
        line.spans
            .into_iter()
            .map(|span| Span::styled(span.content.into_owned(), span.style))
            .collect::<Vec<_>>(),
    )
}

/// The date is the only thing coloured and only when missed, that being the one
/// thing on the row that has changed since it was written. Never truncated, as
/// `noda todo` never truncates it.
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

/// Every tag, commonest first, and how many notes carry it.
fn tag_rows(app: &App) -> Sheet {
    let muted = theme::from(palette::MUTED);
    let width = widest(app.tallies().iter().map(|t| display_width(&t.tag)));
    let rows = app
        .tallies()
        .iter()
        .enumerate()
        .map(|(at, tally)| {
            // Those nine digits are the keys that reach them from anywhere, and
            // a key you can only find in the help is a key nobody has.
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
        // No word for it is shorter than the column is wide.
        names: headings(&["", "TAG", "NOTES"]),
        widths: vec![
            Constraint::Length(1),
            Constraint::Length(width),
            Constraint::Fill(1),
        ],
        rows,
    }
}

/// What the notebook holds that is not a note.
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

/// The listing's mark, and as wide when there is nothing to show: a list that
/// shifted sideways is one you re-find your place in.
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

/// The revision shown is the one `restore` needs — the commit *before* the
/// deletion. Leaving the `~1` to be worked out reports a problem without its
/// remedy, which is `noda deleted`'s call too.
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
        // `FROM` and not `COMMIT`: the revision is the one *before* the
        // deletion, and the word has to fit in seven columns.
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

/// `noda ls`'s row: what comes back is a note, and there is one shape for
/// naming one.
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

/// Commits, newest first — the same three columns `noda log` prints.
fn log_rows(app: &App) -> Sheet {
    let muted = theme::from(palette::MUTED);
    let rows = app
        .entries()
        .iter()
        .map(|entry| {
            // Inside the commit column: one of its own would cost a heading and
            // a width on every row to say nothing on most. The arrow lands where
            // `noda log` puts it, one character wide either way.
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
        // Over the ids and not the margin, or the heading points at the
        // arrows.
        names: headings(&["  COMMIT", "WHEN", "SUMMARY"]),
        widths: vec![
            Constraint::Length(MARKED_COMMIT),
            Constraint::Length(cmd::TIME_WIDTH as u16),
            Constraint::Fill(1),
        ],
        rows,
    }
}

/// A page rather than a list, the rows being the note's own lines. Not wrapped,
/// for a patch's reason: the two columns down the left only line up while every
/// line is one row.
fn draw_blame(f: &mut Frame, area: Rect, app: &App) {
    let muted = theme::from(palette::MUTED);
    let lines: Vec<Line> = app
        .blamed()
        .iter()
        .map(|line| {
            let when = if line.commit.is_some() {
                cmd::format_time(line.seconds, line.offset_minutes)
            } else {
                // To the width of a time, as `noda blame` pads it.
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

/// The bar is measured in the note's own lines, which is what `j` moves by. On a
/// wrapped note that is not the number of rows drawn — but a bar disagreeing
/// with the key is worse than one that is approximate.
fn draw_page(f: &mut Frame, area: Rect, lines: Vec<Line>, scroll: u16, wrap: bool) {
    let (area, bar) = less_the_bar(area);
    let total = lines.len();
    let mut page = Paragraph::new(lines)
        // The cursor bar's gutter, so the text starts where the rows do.
        .block(Block::new().padding(Padding::new(GUTTER as u16, 0, 0, 0)))
        .scroll((scroll, 0));
    if wrap {
        page = page.wrap(Wrap { trim: false });
    }
    f.render_widget(page, area);
    draw_scrollbar(f, bar, total, bar.height as usize, scroll as usize);
}

/// Coloured by what each line is rather than by escapes carried over: `cmd::diff`
/// paints for a pipe, and reading those back out would be parsing its own
/// output. The patch is written down once; the colour is the drawing's.
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
    // A patch is a grid, and a wrapped `+` line reads as two additions.
    draw_page(f, area, lines, app.scroll(), false);
}

fn draw_note(f: &mut Frame, area: Rect, app: &App) {
    let Some(text) = app.text() else {
        f.render_widget(Block::new(), area);
        return;
    };
    // A reader scrolling sideways to finish a sentence is not reading.
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

/// Searched by what a command *does* as well as by its name: the list is for
/// somebody who knows they want their notes on the remote and not that it is
/// spelled `push`.
///
/// Cut to what the terminal holds, cursor kept in view and the rest counted on
/// the last line — a card that ran off the bottom would take its footer with
/// it, which the help card did once already.
fn draw_commands(f: &mut Frame, area: Rect, app: &App) {
    let muted = theme::from(palette::MUTED);
    let shown: Vec<&command::Spec> = command::matching(app.input.text()).collect();
    let width = shown
        .iter()
        .map(|spec| spec.usage().chars().count())
        .max()
        .unwrap_or(0);

    // Two of border, one blank and one footer; the rest is the list.
    //
    // One row per command, true only because the description is cut rather than
    // wrapped: let it wrap and the budget is out by a factor of two and the
    // footer goes off the bottom of its own card.
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

/// At a prompt a delete is a command you typed on purpose; here it is one chord.
/// Asked on the screen, because the terminal is in raw mode and a command
/// reading stdin would take keystrokes out from under the browser.
fn draw_confirm(f: &mut Frame, area: Rect, app: &App, what: What) {
    let muted = theme::from(palette::MUTED);
    let queued = || {
        // Described by what it will do, not by how it was built.
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
        // Counted on their own: they are why the question is asked.
        What::Send => (
            " send the queue? ",
            queued(),
            format!(
                "{} to be deleted — the commit stays, so git revert brings them back",
                plural(app.queued_deletions(), "note")
            ),
            "y  send it       any other key  back to the queue",
        ),
        // About work written down nowhere that will not survive the process.
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

/// Each line is the sentence the commit message will use, so what is read before
/// sending is what the history says after.
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

/// A box per tag rather than a line to write `+work -q3` on, listing every tag
/// the notebook has in the tags screen's order — so the tag being reached for is
/// a keystroke rather than a spelling.
///
/// The number answers what the boxes cannot: over one note it says how
/// established the tag is, and over a marked set it says twelve of forty.
fn draw_tagging(f: &mut Frame, area: Rect, app: &App) {
    let muted = theme::from(palette::MUTED);
    let total = app.picking_notes();
    let here = app.tags_at();
    let shown = app.shown_tags();
    let proposal = app.proposal();

    // So the counts line up. Measured in what a terminal shows, not characters:
    // a tag is as likely to be Chinese as the note it is on.
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

    // Two of border, one blank and one footer: what is left is for the list.
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

    // Said in the words for what it will do: with a set marked the change is
    // queued, and "apply" would promise something that waits for the send.
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

/// One tag on the picker, with its box and its count.
///
/// A tag no note carries is one made a keystroke ago on this very card, and it
/// goes on saying what it said then. `0 notes` would be true and would read as
/// a tag that had somehow lost all of them.
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

/// The row for what has been typed, when the notebook has no such tag.
///
/// Its box is empty until it is chosen, because it has not been: the row is an
/// offer, and a row that arrived already ticked would be making the decision the
/// keystroke is there to make.
fn proposed(proposal: &Proposal, width: usize, here: bool) -> Line<'static> {
    match proposal {
        Proposal::New { tag, near } => {
            let mut spans = vec![
                Span::styled("[ ] ", theme::from(palette::MUTED)),
                Span::styled(padded(tag, width), name_style(here)),
                Span::styled("  new", theme::from(palette::MUTED)),
            ];
            // The one thing on this card that has to be read rather than
            // glanced at: a tag one keystroke from one the notebook already runs
            // on is nearly always the one it is a misspelling of, and the whole
            // cost of getting it wrong is that both go on existing.
            if let Some((near, notes)) = near {
                spans.push(Span::styled(
                    format!(" — close to {near}, {}", plural(*notes, "note")),
                    theme::from(palette::MATCH),
                ));
            }
            Line::from(spans)
        }
        // In `cmd`'s own words. The row cannot be chosen, and saying why is more
        // use than leaving it off the card and letting `Tab` do nothing.
        Proposal::Refused(why) => Line::from(Span::styled(
            format!("    {why}"),
            theme::from(palette::INVALID),
        )),
    }
}

/// A name padded out to the column's width, in columns and not in characters.
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

/// A diff's green and red for the two that change something, the tags' colour
/// for a tick, nothing for an empty box.
fn box_style(choice: &Choice, total: usize) -> Style {
    match choice.mark {
        Mark::Add => theme::from(palette::ADDED),
        Mark::Remove => theme::from(palette::REMOVED),
        Mark::Leave if total > 0 && choice.held == total => theme::from(palette::TAGS),
        Mark::Leave => theme::from(palette::MUTED),
    }
}

/// A card rather than the status bar, because the part worth reading is the part
/// that does not fit: `edit` says where it left an unparseable file, and `bulk`
/// says what it could not do underneath what it did.
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

/// Cut on a character and not a byte: a slice through the middle of a code point
/// is a panic rather than a short line.
fn cut(text: &str, room: usize) -> String {
    if text.chars().count() <= room {
        return text.to_string();
    }
    let kept: String = text.chars().take(room.saturating_sub(1)).collect();
    format!("{}…", kept.trim_end())
}

/// Uncoloured, which is what the column has to be wide enough to hold. Built
/// from the pieces the row is drawn from, so the width cannot drift from what
/// lands in it. Tags are the last column because an empty cell shifts nothing.
fn tags(tags: &[String]) -> String {
    palette::tag_pieces(tags)
        .into_iter()
        .map(|(_, text)| text)
        .collect()
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

/// Nothing is dimmed when there is no block to dim, as `dim_frontmatter` judges
/// it: a file the screen cannot read this way is one to show as it stands.
fn split_frontmatter(text: &str) -> (&str, &str) {
    let Some(rest) = text.strip_prefix("---\n") else {
        return ("", text);
    };
    let Some(end) = rest.find("\n---\n") else {
        return ("", text);
    };
    text.split_at("---\n".len() + end + "\n---\n".len())
}

/// The earliest match wins where two overlap and the search resumes after it, so
/// a line is walked once however many terms are in the query.
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
        // Everything measured against it assumes this width, the title's floor
        // included.
        assert_eq!(CURSOR_BAR.chars().count(), GUTTER);
        // Inside the row rather than the gutter, so screens with and without a
        // mark column start their first value in the same place.
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
        // The note's own text is on screen; only its colour changes.
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
        // The card outgrew a twenty-four row terminal once. Thirteen rows and
        // two of border, the last spent on the field keys — worth a row because
        // they are the ones nobody thinks to look up.
        assert!(KEYS.len() + 2 <= 15, "the card has {} rows", KEYS.len() + 2);
        // As wide as the widest set of keys, or the descriptions stop lining
        // up.
        let widest = KEYS.iter().map(|(key, _)| key.chars().count()).max();
        assert_eq!(widest, Some(KEY_COLUMN));
    }

    #[test]
    fn every_key_that_only_the_card_can_teach_is_on_the_card() {
        // The grid drops columns from the right, so a key out there needs a
        // second way of being found — and for these it is the card and nothing
        // else. Both columns, because one row names its keys in the description
        // rather than in the key cell.
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
    fn a_note_is_not_named_twice_when_the_row_is_the_short_one() {
        // The default row answers "which note is this" and the title is the
        // answer; the slug arrives only with `-l`.
        assert_eq!(stamp(None), "-");
        assert_eq!(
            stamp(Some(&"2026-01-01T00:00:00Z".to_string())),
            "2026-01-01T00:00:00Z"
        );
    }
}
