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

/// How wide git abbreviates an object id to, which is what the commit columns
/// hold.
const SHORT_COMMIT: u16 = 7;

/// The log's commit column, which carries the unpushed mark and the space after
/// it in front of the id. Only the log's: `deleted` names a commit too, and a
/// commit that a note was restored from is not something the remote can be
/// waiting for.
const MARKED_COMMIT: u16 = SHORT_COMMIT + 2;

/// How far the body is held off either edge, so nothing on it is written into
/// the corner of the terminal.
///
/// Both columns are spoken for now and neither is padding any more: the left one
/// is where the cursor's bar goes and the right one is where a scrollbar goes.
/// They are taken whether or not there is anything to draw in them, which is the
/// point — a bar that appeared when a list grew past the bottom of the screen
/// would move every column on it by one at the moment the list got longer.
const PADDING: u16 = 1;

/// The bar down the left of the row the cursor is on, and the column of air
/// that holds it off what it is pointing at.
///
/// A half block rather than an arrow or a `>`: it is the row being pointed at
/// and not a place in the text, and a solid edge says so without being read as
/// a character. The space is not decoration — the screens whose first column is
/// an id have nothing else between the bar and the id, and a bar written against
/// a commit hash reads as part of it.
const CURSOR_BAR: &str = "▌ ";

/// How many columns that takes, which is what every measurement of the row has
/// to be made against.
const GUTTER: usize = 2;

/// The row every table spends on the names of its columns.
const HEADING_ROWS: u16 = 1;

/// What the card has to say that the band along the top does not.
///
/// The keys for the screen you are on are up there, named and always visible, so
/// this is the rest: how to move, what the filter takes, the two keys that open
/// and close a screen, and the keymap the fields answer. Thirteen rows and a
/// border, which is what fits on a terminal short enough to have made the point
/// once already.
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
    // One row per group of keys rather than one per key. The card has to stay
    // inside a twenty-four row terminal, which it has already failed to do
    // twice — once when the write keys arrived and once when the screens did.
    ("t, l, b, B", "todo · log · backlinks · blame"),
    (
        "S, R, ctrl-w, 1-9",
        "sort · reverse · wide row · a tag (0 = all)",
    ),
    ("r, ctrl-g, q / ctrl-c", "read again · crumbs · quit"),
    // One row for a whole keymap, because it is a keymap nobody has to read:
    // anybody who wants these keys already knows them, and the row is here to
    // say they are answered rather than to teach them.
    ("while typing", "readline: ctrl-a/e/w/u/k/y, alt-b/f"),
];

pub fn draw(f: &mut Frame, app: &mut App) {
    let area = f.area();
    // The crumb band is a row the reader may want back. Given no rows rather
    // than drawn empty, so the notes get it — a band that is only invisible is
    // a band that still costs what it did.
    let [header, title, body, crumbs, status] = ratatui::layout::Layout::vertical([
        Constraint::Length(frame::header_rows(area.height)),
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(u16::from(app.crumbs_shown)),
        Constraint::Length(1),
    ])
    .areas(area);

    // A screen with a heading row has one row fewer to move a cursor through,
    // and a half-screen jump measured against the whole body would land a row
    // past what was on it.
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
    // Over the top of all of it, and only ever one of them: a card is what the
    // keyboard is doing, so there is nothing else it could be doing at the time.
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

/// The body, less the column down its right-hand edge a scrollbar goes in.
///
/// Split off whether or not one is drawn there. Taken only when the list
/// overflowed, the columns would all shift by one at whatever moment the list
/// got long enough — and the moment a list gets longer is exactly the moment a
/// reader is looking at it.
fn less_the_bar(area: Rect) -> (Rect, Rect) {
    let [content, bar] =
        Layout::horizontal([Constraint::Fill(1), Constraint::Length(PADDING)]).areas(area);
    (content, bar)
}

/// Where in a screenful of `total` you are, drawn down the right-hand edge.
///
/// Nothing is drawn when everything is on screen: a bar that is always full says
/// only that the list ends where the reader can see it ending. The two ends are
/// left off for the same reason a heading is not a row — an arrow at each end
/// costs two of the rows the bar has to say anything with, and on a body twelve
/// rows tall that is a sixth of the answer spent on decoration.
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

/// The table every screen's rows are drawn in.
///
/// One builder rather than one per screen, because they are one thing: the
/// notebook answering a different question each time, in the same
/// rows-and-a-cursor. What differs is the columns, and each screen says its own.
///
/// The cursor is a bar and a bolder row rather than a reversed one. Reversing
/// inverts the id's yellow and the tags' cyan along with the rest, so the row
/// being looked at is the one row whose columns have stopped being told apart by
/// colour — and the bar is in the column that used to be padding, so the row
/// under the cursor sits where every other row sits.
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
        // Always, or the columns move sideways by the width of the bar on the
        // one screen that has no cursor to draw it for — an empty list, and a
        // list that has just been emptied by a query is the commonest thing on
        // screen while a query is being typed.
        .highlight_spacing(HighlightSpacing::Always)
}

/// A column's name, indented past the mark that goes in front of the first
/// value. The mark is part of the cell rather than a column of its own, so a
/// heading that started where the cell does would sit over the mark.
fn under_mark(name: &str) -> String {
    format!("{UNMARKED}{name}")
}

/// The names along the top of a screen's table, as one row of headings.
fn headings(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_string()).collect()
}

/// Whichever screen is on top.
///
/// Two shapes between them and no more: a list with a cursor in it, or a page of
/// text to scroll. Which one a screen is, is the state's answer and not decided
/// again here — a screen that was a list to the keys and a page to the drawing
/// would be a screen whose `j` did nothing.
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

/// The empty message a screen shows in place of rows it has none of.
///
/// Said in the notebook's own words rather than "no results": an empty todo
/// list is a state worth recognising, and "0 rows" is a spreadsheet talking.
fn draw_nothing(f: &mut Frame, area: Rect, said: &str) {
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(said, theme::from(palette::MUTED))))
            // Held off the edge by as much as a row would have been, so a list
            // and the sentence standing in for one start in the same column.
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
    // scrollbar's column (already off `area`) and the cursor bar's gutter.
    // Counting either as usable is how the title ends up short of the floor it
    // was promised.
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

    // The names of the columns `-l` adds, in the order it added them — which is
    // the whole reason this row is here: `created` and `updated` are the same
    // twenty characters twice, and which is which is not a thing to work out
    // from two timestamps a second apart.
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

/// The part of a table's area its rows are drawn in, which is what a scrollbar
/// down the side of them has to line up with: the heading row is not one of
/// them, and a bar starting a row above the first row would be a bar that is
/// never quite where it says it is.
fn rows_area(area: Rect) -> Rect {
    let [_, rows] =
        Layout::vertical([Constraint::Length(HEADING_ROWS), Constraint::Fill(1)]).areas(area);
    rows
}

/// A timestamp as the long row prints it, with `noda ls -l`'s dash for a note
/// that has none — nothing invents one, and a hole is a thing the eye has to
/// measure.
fn stamp(value: Option<&String>) -> String {
    value.cloned().unwrap_or_else(|| "-".to_string())
}

/// Any of the other lists.
///
/// The same table the listing draws, down to the padding and the reversed row
/// under the cursor: these are the notebook answering different questions, not
/// different programs. Only the columns change, and each screen says what its
/// own are.
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

/// One screen's worth of table: what its columns are called, how wide they are,
/// and the rows themselves. Each screen builds its own and the drawing is the
/// same for all of them.
struct Sheet {
    names: Vec<String>,
    widths: Vec<Constraint>,
    rows: Vec<Row<'static>>,
}

/// How wide a column of these has to be. Measured rather than fixed, because
/// every one of them holds something somebody else chose the length of.
fn widest(of: impl Iterator<Item = usize>) -> u16 {
    of.max().unwrap_or(0) as u16
}

/// A line that has stopped borrowing what it was built from.
///
/// A row of a table outlives the borrow of the session it was measured against,
/// so the text has to come with it. Only the screens whose rows are built one at
/// a time need this; the listing hands ratatui borrowed spans and is the one
/// that can afford to.
fn owned(line: Line<'_>) -> Line<'static> {
    Line::from(
        line.spans
            .into_iter()
            .map(|span| Span::styled(span.content.into_owned(), span.style))
            .collect::<Vec<_>>(),
    )
}

/// The unticked boxes: which note, when it is due, and what it says.
///
/// The date is the only thing coloured, and only when it has been missed —
/// which is the one thing on the row that has changed since it was written.
/// Never truncated, as `noda todo` never truncates it: a real action item is a
/// sentence, and a list that cuts the sentence off is a list you have to open
/// the note to read.
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
            // The first nine are numbered, because those nine digits are the
            // keys that reach them from anywhere. A key you can only find in the
            // help is a key nobody has; the number beside the tag is where
            // somebody will look for it.
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
        // The digit column is not named. It holds the key that reaches the tag
        // beside it, and there is no word for that column which is shorter than
        // the column is wide.
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

/// Every notebook, with a mark against the one this session is in.
///
/// The same mark the listing puts in front of a marked note, and as wide when
/// there is nothing to show: a list that shifted sideways would be a list you
/// have to re-find your place in.
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

/// The notes history holds that the notebook no longer does.
///
/// The revision shown is the one `restore` has to be given — the commit *before*
/// the deletion, not the deletion itself. Naming the deletion and leaving the
/// `~1` to be worked out would be reporting a problem without its remedy, which
/// is the same call `noda deleted` makes.
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
        // `FROM` and not `COMMIT`: the revision in that column is the one
        // *before* the deletion, because that is what `restore` has to be
        // given. Naming the column after what it holds would name the commit
        // that removed the note, and it is not that one — and the word has to
        // fit inside the seven columns git abbreviates an object id to.
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

/// What links here: the same row `noda ls` prints, for the same reason `search`
/// and `backlinks` both print it — what comes back is a note, and there is one
/// shape for naming a note.
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
            // The mark sits inside the commit column rather than in one of its
            // own. A column would cost two characters of heading and a width on
            // every row to say nothing on most of them, and this way the arrow
            // lands where `noda log` puts it — at the far left, one character
            // wide whether or not the row carries it, so the ids stay in line.
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
        // Indented to sit over the ids rather than over the margin in front of
        // them: a heading that named the column from two characters to its left
        // would be pointing at the arrows.
        names: headings(&["  COMMIT", "WHEN", "SUMMARY"]),
        widths: vec![
            Constraint::Length(MARKED_COMMIT),
            Constraint::Length(cmd::TIME_WIDTH as u16),
            Constraint::Fill(1),
        ],
        rows,
    }
}

/// Which commit put each line of a note where it is.
///
/// A page rather than a list: the rows are the note's own lines, and a cursor on
/// one of them would be a cursor on a line of prose. Not wrapped either, for the
/// reason a patch is not — the two columns down the left only line up while
/// every line is one row.
fn draw_blame(f: &mut Frame, area: Rect, app: &App) {
    let muted = theme::from(palette::MUTED);
    let lines: Vec<Line> = app
        .blamed()
        .iter()
        .map(|line| {
            let when = if line.commit.is_some() {
                cmd::format_time(line.seconds, line.offset_minutes)
            } else {
                // Padded to the width of a time so the prose stays in one
                // column, exactly as `noda blame` pads it.
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

/// A screen that is text rather than rows, with the bar down its side saying how
/// much of it there is.
///
/// The bar is measured in the note's own lines, which is what `j` moves by and
/// what the scroll is clamped against. On a wrapped note that is not the number
/// of rows drawn — but a bar that disagreed with the key would be worse than one
/// that is approximate, and the alternative is laying the text out twice.
fn draw_page(f: &mut Frame, area: Rect, lines: Vec<Line>, scroll: u16, wrap: bool) {
    let (area, bar) = less_the_bar(area);
    let total = lines.len();
    let mut page = Paragraph::new(lines)
        // The left columns are the cursor bar's gutter on every other screen,
        // so the text starts where the rows do; the right one has gone to the
        // scrollbar.
        .block(Block::new().padding(Padding::new(GUTTER as u16, 0, 0, 0)))
        .scroll((scroll, 0));
    if wrap {
        page = page.wrap(Wrap { trim: false });
    }
    f.render_widget(page, area);
    draw_scrollbar(f, bar, total, bar.height as usize, scroll as usize);
}

/// What is uncommitted, or what the last commit did.
///
/// Coloured by what each line is rather than by an escape sequence carried over
/// from the command: `cmd::diff` paints for a pipe, and a browser reading its
/// own colours back out of the text would be parsing its own output. The patch
/// is the part written down once; the colour is the drawing's, here as it is for
/// every other listing on screen.
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
    // Not wrapped: a patch is a grid, and a wrapped `+` line reads as a line
    // that was added twice.
    draw_page(f, area, lines, app.scroll(), false);
}

fn draw_note(f: &mut Frame, area: Rect, app: &App) {
    let Some(text) = app.text() else {
        f.render_widget(Block::new(), area);
        return;
    };
    // Wrapped rather than cut: a note is prose, and a reader who has to scroll
    // sideways to finish a sentence is not reading.
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

/// What the prompt accepts, narrowed as you type.
///
/// Searched rather than paged, and searched by what a command *does* as well as
/// by its name: the list exists for somebody who knows they want their notes on
/// the remote and not that it is spelled `push`.
///
/// Cut to what the terminal can hold, with the cursor kept in view and the rest
/// counted on the last line. A card that ran off the bottom would take its own
/// footer with it, which is the mistake the help card made once already.
fn draw_commands(f: &mut Frame, area: Rect, app: &App) {
    let muted = theme::from(palette::MUTED);
    let shown: Vec<&command::Spec> = command::matching(app.input.text()).collect();
    let width = shown
        .iter()
        .map(|spec| spec.usage().chars().count())
        .max()
        .unwrap_or(0);

    // Two of border, one blank and one footer: what is left is for the list.
    //
    // One row per command, which is only true because the description is cut to
    // fit rather than wrapped. Let it wrap and every row becomes two, the budget
    // below is wrong by a factor of two, and the footer is pushed off the bottom
    // of its own card — which is what a card that has stopped saying how to
    // leave it looks like.
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

/// Which tags the notes picked out should end up with.
///
/// A box per tag rather than a line to write `+work -q3` on. The list is every
/// tag the notebook has, commonest first — the tags screen's own order, because
/// it is the tags screen's own list — so the tag being reached for is nearly
/// always already on the card and reaching it is a keystroke rather than a
/// spelling.
///
/// The number down the right-hand side answers the question the boxes cannot.
/// Over one note the box says everything about that note, so the number says how
/// established the tag is; over a marked set the box cannot say that twelve of
/// forty carry it, so the number does.
fn draw_tagging(f: &mut Frame, area: Rect, app: &App) {
    let muted = theme::from(palette::MUTED);
    let total = app.picking_notes();
    let here = app.tags_at();
    let shown = app.shown_tags();
    let proposal = app.proposal();

    // As wide as the widest name on it, so the counts line up in a column of
    // their own. Measured in what a terminal will show and not in characters: a
    // tag is as likely to be Chinese as the note it is on.
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

    // What Enter does, said in the words for what it will actually do: with a
    // set marked the change goes into the queue, and a card that said "apply"
    // would be promising something that does not happen until the queue is sent.
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

/// The box wears the colour of what it is doing: a diff's green and red for the
/// two that change something, the tags' own colour for a tick, and nothing at
/// all for a box that is empty.
fn box_style(choice: &Choice, total: usize) -> Style {
    match choice.mark {
        Mark::Add => theme::from(palette::ADDED),
        Mark::Remove => theme::from(palette::REMOVED),
        Mark::Leave if total > 0 && choice.held == total => theme::from(palette::TAGS),
        Mark::Leave => theme::from(palette::MUTED),
    }
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

/// As much of a line as there is room for, with an ellipsis where it was cut.
///
/// Cut on a character and not a byte: the descriptions are English today and the
/// notebook is not, and a slice through the middle of a code point is a panic
/// rather than a short line.
fn cut(text: &str, room: usize) -> String {
    if text.chars().count() <= room {
        return text.to_string();
    }
    let kept: String = text.chars().take(room.saturating_sub(1)).collect();
    format!("{}…", kept.trim_end())
}

/// A note's tags as the listing writes them, with no colour on them: what the
/// column has to be wide enough to hold. Built from the same pieces the row is
/// drawn from, so the width cannot drift from what lands in it.
///
/// Tags are the one thing a note may not have, which is why they are the last
/// column: an empty cell here shifts nothing.
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
    fn the_gutter_is_the_same_width_on_every_screen() {
        // The bar takes as many columns as everything measured against it
        // assumes — the title's floor among them, which is what goes short when
        // this drifts.
        assert_eq!(CURSOR_BAR.chars().count(), GUTTER);
        // And the mark lives inside the row rather than in the gutter, so the
        // screens that have no mark column start their first value where the
        // ones that do start theirs. Written against the bar it once was, a
        // commit hash read as part of it.
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
        // the header partly so it would not again. Thirteen rows and two of
        // border — the last of them spent on the field keys, which are worth a
        // row precisely because they are the ones nobody thinks to look up.
        assert!(KEYS.len() + 2 <= 15, "the card has {} rows", KEYS.len() + 2);
        // And the column is as wide as the widest set of keys on it, or the
        // descriptions stop lining up.
        let widest = KEYS.iter().map(|(key, _)| key.chars().count()).max();
        assert_eq!(widest, Some(KEY_COLUMN));
    }

    #[test]
    fn every_key_that_only_the_card_can_teach_is_on_the_card() {
        // The grid drops columns from the right, so a key out there has to have
        // a second way of being found. For these it is the card and nothing
        // else — no `:` name, no letter in a column that survives eighty
        // columns — which makes this the check that they are on it.
        // Both columns, because one row's keys are named in its description:
        // `readline` is the name of that keymap and the keys themselves are the
        // gloss on it, not the other way round.
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
        // The default row answers "which note is this", and the title is the
        // answer — the slug is the same words with the spaces taken out. It
        // arrives only with `-l`, which is a density and not a selection.
        assert_eq!(stamp(None), "-");
        assert_eq!(
            stamp(Some(&"2026-01-01T00:00:00Z".to_string())),
            "2026-01-01T00:00:00Z"
        );
    }
}
