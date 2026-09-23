//! The HTML, and nothing else.
//!
//! Every function takes what a page is about and returns a string; nothing opens
//! a repository, binds a socket or knows what a request is, so a page is tested
//! without either — `tui/app.rs`'s rule.
//!
//! Machinery — ids, filenames, tags, stamps — is set in monospace and the
//! reader's own words in a text face: the line `style.rs` draws with colour.
//!
//! No JavaScript required: the search field is a form and every row is a link.
//! Filtering as you type is an enhancement, never a replacement.

use std::fmt::Write;

use crate::cmd::Sort;
use crate::notebook::NoteFile;
use crate::style;
use crate::web::asset::Asset;
use crate::web::encoded;

/// `noda status` compressed to a row. Every field but `last` is already in
/// `Status`, and `last` is one commit read.
pub struct Book {
    pub name: String,
    pub notes: usize,
    /// Shown only when non-zero, as `noda status` prints it.
    pub files: usize,
    pub uncommitted: usize,
    /// Already in words. `None` means no remote, and the cell is then not a link.
    pub drift: Option<String>,
    /// The one `noda notebook ls` marks with `*`.
    pub active: bool,
    /// The day of its last commit, already rendered by `cmd::format_time`.
    pub last: String,
}

/// `ls -l`'s row in `ls -l`'s order — id, title, day, tags — minus the slug and
/// the created stamp, which are on the note's own page. The id is always written
/// and shown where the layout has room.
pub struct Row {
    pub id: String,
    pub title: String,
    pub tags: Vec<String>,
    /// The stamp the listing is sorted by (`created` for `created`, otherwise
    /// `updated`); a column of days in no order beside a sorted list reads as a
    /// broken sort. Not called `updated` because it is sometimes the other one.
    pub stamp: Option<String>,
    /// Rows a query excludes arrive `hidden` rather than left out, because a
    /// script cannot put back a row the server never sent. `hidden` is the
    /// attribute browsers already hide, so the scriptless page needs no CSS.
    pub shown: bool,
    /// Explains a row above one with a newer stamp.
    pub pinned: bool,
}

/// A file the notebook holds that is not a note, as the files page lists it.
pub struct Held {
    pub name: String,
    pub size: u64,
    /// The same type the download is served with.
    pub kind: String,
    /// How many notes link to it. Zero is what `doctor --links` calls an orphan.
    pub used: usize,
}

/// One tag, and how many notes carry it.
pub struct Tally {
    pub tag: String,
    pub notes: usize,
}

/// A todo item, named by its note's title; the id is the address.
pub struct Task {
    pub id: String,
    pub title: String,
    /// The item's words with the `due:` term already lifted out.
    pub text: String,
    pub due: Option<String>,
    /// Against the reader's day, which only the server knows.
    pub overdue: bool,
}

/// What a backlinks page is about: a note, or one of the notebook's files.
pub struct Subject {
    /// A note's title or a file's name.
    pub what: String,
    /// Where it is, for the way back.
    pub at: String,
    /// A filename is set in monospace, a title never is.
    pub mono: bool,
}

/// A note, as its own page shows it.
pub struct Reading {
    pub id: String,
    pub slug: String,
    pub title: String,
    pub tags: Vec<String>,
    pub created: Option<String>,
    pub updated: Option<String>,
    /// **Already HTML**, from `web::render::body` — the one string these pages
    /// write out unescaped, hence not called `body`.
    pub rendered: String,
    /// Which way the bar's pin item goes.
    pub pinned: bool,
}

impl Row {
    /// `by` decides only [`Row::stamp`]. A page that is not a listing passes
    /// `Sort::default()` and gets `updated`.
    pub fn of(file: &NoteFile, by: Sort) -> Row {
        Row {
            id: file.id.clone(),
            title: file.note.title.clone(),
            tags: file.note.tags.clone(),
            stamp: match by {
                Sort::Created => file.note.created.clone(),
                Sort::Slug | Sort::Updated | Sort::Title => file.note.updated.clone(),
            },
            shown: true,
            pinned: file.note.is_pinned(),
        }
    }
}

/// Hand-written: five characters, so the one place escaping happens reads in
/// one screen.
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

/// The day a stamp names: `2026-08-15`; anything not a date comes back as is.
///
/// Not the minute: stamps are UTC, and a time with the `Z` cut off to save room
/// looks local and is wrong, while converting would put the server's zone into
/// the note. The full stamp is on the note's own page.
fn day(value: &str) -> String {
    let bytes = value.as_bytes();
    let dated = bytes.len() >= 10 && bytes[4] == b'-' && bytes[7] == b'-';
    if dated {
        value[..10].to_string()
    } else {
        value.to_string()
    }
}

/// `text`, escaped, with every run matching one of `terms` in `<mark>`.
///
/// Matched before escaping and escaped as the pieces are cut: escaping first
/// would search `&amp;` for `&`.
fn highlight(text: &str, terms: &[String]) -> String {
    if terms.is_empty() {
        return escape(text);
    }
    let haystack = text.to_lowercase();
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    while at < text.len() {
        // Earliest match, then longest, so overlapping terms mark one run
        // rather than nesting.
        let next = terms
            .iter()
            .filter(|term| !term.is_empty())
            .filter_map(|term| {
                haystack[at..]
                    .find(term.as_str())
                    .map(|at| (at, term.len()))
            })
            .min_by_key(|&(found, length)| (found, usize::MAX - length));
        let Some((found, length)) = next else { break };
        let start = at + found;
        out.push_str(&escape(&text[at..start]));
        out.push_str("<mark>");
        out.push_str(&escape(&text[start..start + length]));
        out.push_str("</mark>");
        at = start + length;
    }
    out.push_str(&escape(&text[at..]));
    out
}

/// A tag list, comma-joined as the CLI prints it but without the brackets: a
/// page separates things by position.
fn tag_line(tags: &[String]) -> String {
    if tags.is_empty() {
        return String::new();
    }
    format!("<span class=\"tags\">{}</span>", escape(&tags.join(", ")))
}

/// What every page is wrapped in. The stylesheet is linked, for the reason
/// `asset.rs` gives.
///
/// `app` is the layout's classes — `root`, `split`, `at-list`/`at-note`,
/// `indexed` — each a fact about the route, decided by its handler rather than
/// worked out again from the markup.
fn shell(title: &str, app: &str, body: &str) -> String {
    dressed(title, app, None, &[], body)
}

/// A page links the scripts it uses and no others. They are `defer`red: they
/// read the rows, so they run after parsing but download during it.
fn scripted(title: &str, app: &str, scripts: &[Asset], body: &str) -> String {
    dressed(title, app, None, scripts, body)
}

/// The shell, plus an optional `<meta http-equiv="refresh">` — how the
/// scriptless network screen comes back for news.
///
/// The `referrer` meta keeps an address (somebody's note id) from leaving:
/// `web::html` sends the same as a header a proxy may strip, and this copy also
/// covers images a note embeds from elsewhere. `same-origin` rather than
/// `no-referrer`, which would null a form post's `Origin` that `web::guard` needs.
fn dressed(title: &str, app: &str, again_in: Option<u32>, scripts: &[Asset], body: &str) -> String {
    let refresh = refresh(again_in);
    let enhancement = scripts.iter().map(|asset| asset.tag()).collect::<String>();
    let classes = if app.is_empty() {
        String::from("app")
    } else {
        format!("app {app}")
    };
    format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n\
         <meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
         <meta name=\"referrer\" content=\"same-origin\">\n\
         {refresh}{}\n{}{enhancement}\n</head>\n<body>\n\
         <div class=\"{classes}\">\n{}</div>\n</body>\n</html>\n",
        titled(title),
        Asset::Style.tag(),
        body
    )
}

/// Shared by [`dressed`] and the fragments: a swap renames the tab and the
/// polling screen steers by the refresh, so a fragment leads with both. A parser
/// puts a leading `<title>` in the head, where the script looks on a whole page.
fn titled(title: &str) -> String {
    format!("<title>{}</title>", escape(title))
}

fn refresh(again_in: Option<u32>) -> String {
    again_in.map_or_else(String::new, |seconds| {
        format!("<meta http-equiv=\"refresh\" content=\"{seconds}\">\n")
    })
}

/// Kept as typed, so a refused write hands the reader's words back unchanged.
#[derive(Default)]
pub struct Draft {
    pub title: String,
    pub tags: String,
    pub body: String,
}

/// Which note a form is about.
pub struct About {
    pub id: String,
    pub slug: String,
    pub title: String,
}

impl About {
    pub fn of(id: &str, slug: &str, title: &str) -> About {
        About {
            id: id.to_string(),
            slug: slug.to_string(),
            title: title.to_string(),
        }
    }

    fn at(&self, book: &str) -> String {
        format!("/nb/{}/n/{}", escape(book), escape(&self.id))
    }
}

/// One enum rather than two flags, which would allow "here" and "danger" at once.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mark {
    Plain,
    /// The screen the bar is being drawn on.
    Here,
    /// The one item that cannot be undone by doing it again.
    Danger,
}

impl Mark {
    fn at(here: bool) -> Mark {
        if here { Mark::Here } else { Mark::Plain }
    }
}

/// What an item on the bar does when it is pressed.
///
/// A write is a `POST`, never a link: `guard.rs` has nothing to check when
/// another site loads a link of ours as an image. The form sits beside the
/// button, tied by `form=`, so the button stays the bar's flex item.
enum Act {
    Go(String),
    Post { to: String, id: &'static str },
}

fn action_bar(items: &[(&str, &str, Act, Mark)]) -> String {
    let mut out = String::from("<nav class=\"actionbar\">");
    for (icon, label, act, mark) in items {
        let (open, close) = match act {
            Act::Go(href) => (format!("<a href=\"{}\"", escape(href)), "</a>"),
            Act::Post { to, id } => (
                format!(
                    "<form method=\"post\" action=\"{}\" id=\"{id}\" hidden></form>\
                     <button type=\"submit\" form=\"{id}\"",
                    escape(to)
                ),
                "</button>",
            ),
        };
        let _ = write!(
            out,
            "{open}{}><svg viewBox=\"0 0 24 24\" aria-hidden=\"true\">{icon}</svg>\
             <span>{label}</span>{close}",
            // `aria-current` so a screen reader says "current page"; danger
            // is only a class, as the word Delete already says it.
            match mark {
                Mark::Plain => "",
                Mark::Here => " aria-current=\"page\"",
                Mark::Danger => " class=\"danger\"",
            }
        );
    }
    out.push_str("</nav>");
    out
}

const NEW: &str = "<path d=\"M12 4.5v15M4.5 12h15\"/>";
/// A page with a folded corner: a notebook is a directory of files.
const NOTES: &str = "<path d=\"M5.5 4.5h9L19 9v10.5h-13.5z\"/><path d=\"M14.5 4.5V9H19\"/>\
<path d=\"M9 13h6\"/><path d=\"M9 16.5h4\"/>";
const EDIT: &str = "<path d=\"M4 20h4L19 9l-4-4L4 16z\"/>";
const TAGS: &str = "<path d=\"M4 4h7l9 9-7 7-9-9z\"/><circle cx=\"8\" cy=\"8\" r=\"1.4\"/>";
const RENAME: &str = "<path d=\"M4 7V5h16v2\"/><path d=\"M12 5v14\"/><path d=\"M9 19h6\"/>";
const TODO: &str = "<rect x=\"4\" y=\"4\" width=\"16\" height=\"16\" rx=\"3.5\"/><path d=\"M8.5 12.2l2.6 2.6 4.6-5.4\"/>";
const FILES: &str = "<path d=\"M18.5 10.5 11 18a4 4 0 0 1-5.7-5.7l7.8-7.8a2.6 2.6 0 0 1 3.7 3.7\
 l-7.7 7.7a1.2 1.2 0 0 1-1.7-1.7l7.1-7.1\"/>";
/// An arrow arriving at a line: what points at the note.
const LINKS: &str = "<path d=\"M19 5v14\"/><path d=\"M4 12h11\"/><path d=\"M11 8l4 4-4 4\"/>";
/// A thumbtack, for both Pin and Unpin: the label says which way.
const PIN: &str = "<path d=\"M9 3h6l-1 5.5 3.5 3v1.5h-11V11.5l3.5-3z\"/><path d=\"M12 13v8\"/>";
/// Same stroke as the rest: the warning is the colour alone.
const TRASH: &str = "<path d=\"M5 7h14\"/><path d=\"M9.5 7V4.5h5V7\"/>\
<path d=\"M6.5 7l1 12.5h9L17.5 7\"/><path d=\"M10 10.5v6\"/><path d=\"M14 10.5v6\"/>";
/// Two arrows passing, not a cloud: a remote is often the reader's own machine.
const SYNC: &str = "<path d=\"M7 9l5-5 5 5\"/><path d=\"M12 4v10\"/>\
<path d=\"M17 15l-5 5-5-5\"/><path d=\"M12 20V10\"/>";

/// A glyph rather than the word `order`, which costs 55px: four chips fit the
/// index column instead of three and a wrap.
const ORDER: &str = "<path d=\"M4 7h13\"/><path d=\"M4 12h9\"/><path d=\"M4 17h5\"/>";

/// Down is `--sort`'s order, up is `-r`. Not ascending/descending: `updated`
/// runs newest-first and `title` A-to-Z.
const DOWNWARDS: &str = "<path d=\"M12 5v14\"/><path d=\"M6 13l6 6 6-6\"/>";
const UPWARDS: &str = "<path d=\"M12 19V5\"/><path d=\"M6 11l6-6 6 6\"/>";

/// The way to the network screen, showing its answer. The link around the pill
/// is the 48px touch target; the label repeats the words because a narrow
/// screen may ellipsize the text.
fn drift_chip(book: &str, drift: &str) -> String {
    format!(
        "<a class=\"drift\" href=\"/nb/{}/status\" aria-label=\"Status: {}\">\
         <span class=\"pill\"><svg viewBox=\"0 0 24 24\" aria-hidden=\"true\">{SYNC}</svg>\
         <span>{}</span></span></a>",
        escape(book),
        escape(drift),
        escape(drift)
    )
}

/// Inline SVG rather than `‹`, whose weight and position the reader's font
/// would decide.
const BACK: &str =
    "<svg viewBox=\"0 0 24 24\" aria-hidden=\"true\"><path d=\"M15 4.5 7.5 12 15 19.5\"/></svg>";

/// Which of the notebook's screens is being drawn, so the bar can say so.
#[derive(Clone, Copy, PartialEq, Eq)]
enum At {
    Notes,
    Tags,
    Todo,
    Files,
    /// Not on the bar — reached from the drift chip — so no item is marked.
    Status,
}

/// Four places on the bar, the same on every screen, and one action, New, apart
/// from it as a floating button.
fn notebook_bar(book: &str, here: At) -> String {
    let at = escape(book);
    let mark = |screen: At| Mark::at(here == screen);
    // The wrapper sticks, not the bar inside it: on a short page the bar sits
    // under the last row, and a button pinned to the window would float below.
    format!(
        "<div class=\"foot\">{}<a class=\"fab\" href=\"/nb/{at}/new\" aria-label=\"New note\">\
         <svg viewBox=\"0 0 24 24\" aria-hidden=\"true\">{NEW}</svg></a></div>",
        action_bar(&[
            (
                NOTES,
                "Notes",
                Act::Go(format!("/nb/{at}")),
                mark(At::Notes)
            ),
            (
                TAGS,
                "Tags",
                Act::Go(format!("/nb/{at}/tags")),
                mark(At::Tags)
            ),
            (
                TODO,
                "Todo",
                Act::Go(format!("/nb/{at}/todo")),
                mark(At::Todo)
            ),
            (
                FILES,
                "Files",
                Act::Go(format!("/nb/{at}/files")),
                mark(At::Files)
            ),
        ])
    )
}

fn back(href: &str, label: &str) -> String {
    format!(
        "<a class=\"back\" href=\"{}\" aria-label=\"Back to {}\">{BACK}</a>",
        escape(href),
        escape(label)
    )
}

/// The one screen not inside a notebook, so it has no rail or bar; `root`
/// stops the layout reserving a column for the rail.
pub fn notebooks(books: &[Book]) -> String {
    let rows = if books.is_empty() {
        "<div class=\"empty\"><b>No notebooks yet</b>Run <code>noda init</code> in a terminal to make the first one.</div>".to_string()
    } else {
        books.iter().map(book_row).collect()
    };
    shell(
        "noda",
        "root",
        &format!(
            "<section class=\"pane\">\
             <header class=\"topbar\"><span class=\"here lead\">noda</span>\
             <span class=\"count\">{}</span></header>\
             <main class=\"rows books\">{rows}</main></section>",
            tally(books)
        ),
    )
}

/// The second clause is in a `.more` span the stylesheet drops on a phone: the
/// server does not know the screen's width.
fn tally(books: &[Book]) -> String {
    let notes = books.iter().map(|book| book.notes).sum();
    format!(
        "{}<span class=\"more\"> · {}</span>",
        plural(books.len(), "notebook"),
        plural(notes, "note")
    )
}

/// Two links side by side rather than nested: the row goes to the listing, the
/// chip to the network screen. With no remote the chip is plain words.
fn book_row(book: &Book) -> String {
    let at = escape(&book.name);
    // A dot for `noda notebook ls`'s `*`, with words for a screen reader.
    let mark = if book.active {
        "<span class=\"mark\" aria-hidden=\"true\"></span><span class=\"sr\">Active — </span>"
    } else {
        ""
    };

    // `.holds`, not `.when`: a test helper reads stamps by `.when`.
    let mut facts = vec![format!(
        "<span class=\"holds\">{}</span>",
        plural(book.notes, "note")
    )];
    if book.files > 0 {
        // Classed so a phone can drop it: three facts do not fit beside the
        // chip at 390px.
        facts.push(format!(
            "<span class=\"holds files\">{}</span>",
            plural(book.files, "file")
        ));
    }
    if book.uncommitted > 0 {
        facts.push(format!(
            "<span class=\"holds\">{} uncommitted</span>",
            book.uncommitted
        ));
    }

    let aside = match &book.drift {
        Some(drift) => format!(
            "<a class=\"aside\" href=\"/nb/{at}/status\" aria-label=\"Status: {}\">\
             <span class=\"pill\"><svg viewBox=\"0 0 24 24\" aria-hidden=\"true\">{SYNC}</svg>\
             <span>{}</span></span></a>",
            escape(drift),
            escape(drift)
        ),
        None => "<span class=\"aside\">no remote</span>".to_string(),
    };

    format!(
        "<div class=\"row split book\">\
         <a class=\"most\" href=\"/nb/{at}\">\
         <span class=\"name\">{mark}{}</span>\
         <div class=\"under\">{}</div>\
         <span class=\"stamp\">{}</span></a>{aside}</div>",
        escape(&book.name),
        facts.join("<span class=\"sep\">·</span>"),
        escape(&book.last)
    )
}

/// The line in the search field, and everything the page has to say about it.
/// One struct because the four are only ever right together.
pub struct Asked<'a> {
    /// The line exactly as it arrived, which is what goes back in the field.
    pub typed: &'a str,
    /// `query::Query::grouping`; empty when the line is not a query yet.
    pub grouping: &'a [Vec<String>],
    /// What is worth marking in a title.
    pub terms: &'a [String],
    /// Why the line is not a query yet. It is said and the notes are left
    /// alone: half a query is how every query looks while being typed.
    pub problem: Option<&'a str>,
}

impl Asked<'_> {
    /// An empty field, as a note route sends it.
    pub fn nothing() -> Self {
        Self {
            typed: "",
            grouping: &[],
            terms: &[],
            problem: None,
        }
    }
}

/// An order and a reversal applied after it, as `--sort` and `-r` are at the
/// prompt, rather than one enum of eight.
#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
pub struct Order {
    pub sort: Sort,
    pub reversed: bool,
}

impl Order {
    /// The query string this order is asked for by, `q` carried through it.
    /// The default order writes nothing, so the default listing has one address.
    fn asked(self, typed: &str) -> String {
        let mut parts = Vec::new();
        if !typed.is_empty() {
            parts.push(format!("q={}", encoded(typed)));
        }
        if self.sort != Sort::default() {
            parts.push(format!("sort={}", self.sort.name()));
        }
        if self.reversed {
            parts.push("r=1".to_string());
        }
        if parts.is_empty() {
            String::new()
        } else {
            format!("?{}", parts.join("&"))
        }
    }
}

/// A notebook's notes, narrowed by whatever was typed. `drift` is the chip
/// linking to the network screen.
pub fn listing(
    book: &str,
    rows: &[Row],
    asked: &Asked<'_>,
    order: Order,
    drift: &str,
    front: Option<&str>,
) -> String {
    // `indexed`: the rows are in the markup, as the script marks a note route
    // once it has fetched them.
    scripted(
        &listing_title(book),
        "split at-list indexed",
        // `Beside` is for the note a picked row swaps into the reading pane.
        &[Asset::Listing, Asset::Panes, Asset::Beside, Asset::Stamps],
        &format!(
            "{}{}",
            listing_panes(book, rows, asked, order, drift, front),
            notebook_bar(book, At::Notes)
        ),
    )
}

/// Both of the listing's panes and its title, for a screen going back to it
/// from a note in one round trip. Narrowing a search asks for
/// [`listing_pane`] alone and leaves the note pane and the tab be.
pub fn listing_screen(
    book: &str,
    rows: &[Row],
    asked: &Asked<'_>,
    order: Order,
    drift: &str,
    front: Option<&str>,
) -> String {
    format!(
        "{}{}",
        titled(&listing_title(book)),
        listing_panes(book, rows, asked, order, drift, front)
    )
}

fn listing_title(book: &str) -> String {
    format!("{book} — noda")
}

fn listing_panes(
    book: &str,
    rows: &[Row],
    asked: &Asked<'_>,
    order: Order,
    drift: &str,
    front: Option<&str>,
) -> String {
    format!(
        "{}{}",
        listing_pane(book, rows, asked, order, drift),
        front_pane(book, front)
    )
}

/// The index pane with rows. [`note`] sends it empty, since a phone never draws
/// it, and `script::PANES` asks for this where it will be drawn.
pub fn listing_pane(
    book: &str,
    rows: &[Row],
    asked: &Asked<'_>,
    order: Order,
    drift: &str,
) -> String {
    let total = rows.len();
    let shown = rows.iter().filter(|row| row.shown).count();

    let mut body = String::new();
    for row in rows {
        // The pin mark goes last, after the tags.
        let mark = if row.pinned {
            format!("<span class=\"pin\">{}</span>", style::PIN_MARK)
        } else {
            String::new()
        };
        let under = [when(row.stamp.as_deref()), tag_line(&row.tags), mark]
            .into_iter()
            .filter(|piece| !piece.is_empty())
            .collect::<Vec<_>>()
            .join("<span class=\"sep\">·</span>");
        let _ = write!(
            body,
            "<a class=\"row\"{} href=\"/nb/{}/n/{}\">\
             <div class=\"ident\"><span class=\"id\">{}</span>{}</div>\
             <div class=\"title\">{}</div>\
             <div class=\"under\">{under}</div></a>",
            if row.shown { "" } else { " hidden" },
            escape(book),
            escape(&row.id),
            escape(&row.id),
            // The day is written twice — here beside the id and in `under` —
            // and the stylesheet shows one, as the server does not know the
            // screen's width.
            row.stamp
                .as_deref()
                .map_or_else(String::new, |value| format!(
                    "<span class=\"day\">{}</span>",
                    escape(&day(value))
                )),
            // A hidden row's title is unmarked, as the script would leave it.
            if row.shown {
                highlight(&row.title, asked.terms)
            } else {
                escape(&row.title)
            }
        );
    }

    // Only the no-match sentence is ever hidden; typing cannot fill an empty
    // notebook.
    if total == 0 {
        body.push_str(
            "<div class=\"empty\"><b>No notes yet</b>Run <code>noda add \"First note\"</code> \
             in a terminal to start one.</div>",
        );
    } else {
        let _ = write!(
            body,
            "<div class=\"empty\"{}><b>No notes match <span class=\"asked\">{}</span></b>\
             This notebook holds {}. <a href=\"/nb/{}\">Clear the search</a> to see them.</div>",
            if shown > 0 { " hidden" } else { "" },
            escape(asked.typed),
            plural(total, "note"),
            escape(book)
        );
    }

    let counted = if asked.typed.is_empty() {
        total.to_string()
    } else {
        format!("{shown} of {total}")
    };

    index_pane(book, asked, Some(order), &counted, drift, &body)
}

/// The index pane's frame, the same on both routes, with a `GET` search form a
/// scriptless reader can submit from a note page. A note route sends `rows`
/// and `counted` empty for `script::PANES` to fill.
fn index_pane(
    book: &str,
    asked: &Asked<'_>,
    order: Option<Order>,
    counted: &str,
    drift: &str,
    rows: &str,
) -> String {
    format!(
        "<section class=\"pane index\">\
         <header class=\"topbar\">{}<span class=\"here\">{}</span>{}\
         <span class=\"count\">{counted}</span></header>\
         <form class=\"searchbar\" method=\"get\" action=\"/nb/{}\">\
         <input type=\"search\" name=\"q\" value=\"{}\" \
         placeholder=\"tag:work OR tag:q3 budget\" \
         autocomplete=\"off\" autocapitalize=\"off\" spellcheck=\"false\" \
         enterkeyhint=\"search\" aria-label=\"Search this notebook\">{}{}{}{}{}{}</form>\
         <main class=\"rows\">{rows}</main></section>",
        back("/", "the notebooks"),
        escape(book),
        drift_chip(book, drift),
        escape(book),
        escape(asked.typed),
        // The order rides in the form, which would otherwise submit only
        // `?q=…` and drop it. Only a non-default order is written.
        held(
            "sort",
            order
                .filter(|order| order.sort != Sort::default())
                .map(|order| order.sort.name()),
        ),
        held("r", order.filter(|order| order.reversed).map(|_| "1")),
        // Written by the server and only unhidden by the script, so the
        // sentence is testable.
        hint(),
        grouping(asked.grouping),
        asked.problem.map_or_else(String::new, |why| format!(
            "<p class=\"problem\">{}</p>",
            escape(why)
        )),
        // Last, so it does not split the query from the three answers above.
        order.map_or_else(String::new, |order| sortbar(book, asked, order)),
    )
}

/// `None` writes nothing: a hidden input carrying a default would put it in
/// the address on every search.
fn held(name: &str, value: Option<&str>) -> String {
    value.map_or_else(String::new, |value| {
        format!(
            "<input type=\"hidden\" name=\"{}\" value=\"{}\">",
            escape(name),
            escape(value)
        )
    })
}

/// The four orders `--sort` names as four links, no menu, so it works without
/// the script. Pressing the order in force toggles `-r`, like a sortable
/// heading, and its chip carries the direction arrow. Choosing a different
/// order drops the reversal.
fn sortbar(book: &str, asked: &Asked<'_>, order: Order) -> String {
    let mut out = format!(
        "<nav class=\"sortbar\" aria-label=\"Order\">\
         <span class=\"lab\"><svg viewBox=\"0 0 24 24\" aria-hidden=\"true\">{ORDER}</svg></span>"
    );
    for sort in Sort::ALL {
        let here = sort == order.sort;
        let next = Order {
            sort,
            reversed: here && !order.reversed,
        };
        let _ = write!(
            out,
            "<a href=\"/nb/{}{}\"{} aria-label=\"{}\"><span class=\"pill\">{}{}</span></a>",
            escape(book),
            escape(&next.asked(asked.typed)),
            if here { " aria-current=\"true\"" } else { "" },
            // The arrow is `aria-hidden`, so direction and effect are said
            // in words.
            escape(&if here {
                format!(
                    "Ordered by {}{}. Press to {}.",
                    sort.name(),
                    if order.reversed { ", reversed" } else { "" },
                    if order.reversed { "undo" } else { "reverse" }
                )
            } else {
                format!("Order by {}", sort.name())
            }),
            escape(sort.name()),
            if here {
                format!(
                    "<svg viewBox=\"0 0 24 24\" aria-hidden=\"true\">{}</svg>",
                    if order.reversed { UPWARDS } else { DOWNWARDS }
                )
            } else {
                String::new()
            }
        );
    }
    out.push_str("</nav>");
    out
}

/// What was typed, as noda grouped it: each pill a group joined by `or`, the
/// pills joined by `and`, in the reader's own tokens.
///
/// It exists because `a OR b c` is `(a OR b) AND c` — `OR` binding tighter than
/// a space is backwards from other search boxes, and neither reading looks wrong
/// from a list of notes. Written hidden when there is no grouping, for
/// [`hint`]'s reason.
fn grouping(groups: &[Vec<String>]) -> String {
    if groups.is_empty() {
        return "<div class=\"parse\" hidden></div>".to_string();
    }
    let mut out = String::from("<div class=\"parse\">");
    for (at, group) in groups.iter().enumerate() {
        if at > 0 {
            out.push_str("<span class=\"and\">and</span>");
        }
        out.push_str("<span class=\"g\">");
        for (at, term) in group.iter().enumerate() {
            if at > 0 {
                out.push_str("<i>or</i>");
            }
            // Only a `tag:` term is coloured, in the tag's colour.
            let _ = write!(
                out,
                "<b{}>{}</b>",
                if term.starts_with("tag:") || term.starts_with("-tag:") {
                    " class=\"t\""
                } else {
                    ""
                },
                escape(term)
            );
        }
        out.push_str("</span>");
    }
    out.push_str("</div>");
    out
}

/// The reading pane with no note picked, which only a two-pane screen sees:
/// the notebook's `README.md` if it has one, otherwise an invitation.
fn front_pane(book: &str, front: Option<&str>) -> String {
    match front {
        Some(rendered) => format!(
            "<section class=\"pane read\"><header class=\"topbar\">\
             <span class=\"here lead\">{}</span>\
             <span class=\"count mono\">README.md</span></header>\
             <main class=\"note\"><div class=\"body\">{rendered}</div></main></section>",
            escape(book)
        ),
        None => "<section class=\"pane read\"><header class=\"topbar\">\
             <span class=\"here lead\">Reading</span></header>\
             <main><div class=\"empty\"><b>Pick a note</b>Its text opens here. \
             Narrow the list with a search, or press + to write a new one.\
             </div></main></section>"
            .to_string(),
    }
}

/// Unhidden by the script only when its answer and the server's can differ: a
/// bare word or `text:` reads the body, which is not on the page.
fn hint() -> String {
    "<p class=\"hint\" hidden>Filtered by title and tag — press ⏎ to search the text.</p>"
        .to_string()
}

/// One row per file: size, type and how many notes point at it (zero being
/// `doctor --links`' orphan).
pub fn files(book: &str, held: &[Held]) -> String {
    // Columns (`.cols`) only when there are rows: the empty sentence poured
    // into columns would arrive as fragments with a rule through them.
    let (laid, body) = if held.is_empty() {
        (
            "rows",
            "<div class=\"empty\"><b>No files yet</b>Run <code>noda file add diagram.png</code> \
             in a terminal to put one here.</div>"
                .to_string(),
        )
    } else {
        let mut out = String::new();
        for file in held {
            let under = [size(file.size), escape(&file.kind)].join("<span class=\"sep\">·</span>");
            // Side by side, since links cannot nest: the row goes to the file,
            // the count to its backlinks. Zero is not a link.
            let asks = match file.used {
                0 => "<span class=\"aside\">nothing links to it</span>".to_string(),
                used => format!(
                    "<a class=\"aside\" href=\"/nb/{}/f/{}/backlinks\">in {}</a>",
                    escape(book),
                    escape(&file.name),
                    plural(used, "note")
                ),
            };
            let _ = write!(
                out,
                "<div class=\"row split\">\
                 <a class=\"most\" href=\"/nb/{}/f/{}\"><div class=\"title mono\">{}</div>\
                 <div class=\"under\">{under}</div></a>{asks}</div>",
                escape(book),
                escape(&file.name),
                escape(&file.name)
            );
        }
        ("rows cols wide", out)
    };

    shell(
        &format!("Files — {book} — noda"),
        "",
        &format!(
            "<section class=\"pane\">\
             <header class=\"topbar\">{}<span class=\"here\">Files</span>\
             <span class=\"count\">{}</span></header>\
             <main class=\"{laid}\">{body}</main></section>{}",
            back(&format!("/nb/{}", escape(book)), book),
            held.len(),
            notebook_bar(book, At::Files)
        ),
    )
}

/// Powers of two and one decimal place, as file managers show it.
fn size(bytes: u64) -> String {
    #[expect(
        clippy::cast_precision_loss,
        reason = "a rounded size is the point; a notebook holds no file where the lost bits show"
    )]
    let value = bytes as f64;
    match bytes {
        0..1024 => format!("{bytes} B"),
        1024..1_048_576 => format!("{:.1} KB", value / 1024.0),
        1_048_576..1_073_741_824 => format!("{:.1} MB", value / 1_048_576.0),
        _ => format!("{:.1} GB", value / 1_073_741_824.0),
    }
}

/// Tags commonest first, alphabetical within a count (`notebook::tag_tally`
/// orders them), each linking to the listing narrowed to it. `query::scoped` writes the query, because a tag with a space has to
/// arrive quoted.
pub fn tags(book: &str, tallies: &[Tally]) -> String {
    // Columns only when there are rows, as in [`files`].
    let (laid, body) = if tallies.is_empty() {
        (
            "rows",
            "<div class=\"empty\"><b>No tags yet</b>Tags come from a note's frontmatter. \
             Open a note and press Tags to add one.</div>"
                .to_string(),
        )
    } else {
        let mut out = String::new();
        for tally in tallies {
            let _ = write!(
                out,
                "<a class=\"row\" href=\"/nb/{}?q={}\"><div class=\"name tags\">{}</div>\
                 <div class=\"under\"><span class=\"when\">{}</span></div></a>",
                escape(book),
                escape(&crate::web::encoded(&crate::query::scoped(&tally.tag))),
                escape(&tally.tag),
                plural(tally.notes, "note")
            );
        }
        ("rows cols", out)
    };

    shell(
        &format!("Tags — {book} — noda"),
        "",
        &format!(
            "<section class=\"pane\">\
             <header class=\"topbar\">{}<span class=\"here\">Tags</span>\
             <span class=\"count\">{}</span></header>\
             <main class=\"{laid}\">{body}</main></section>{}",
            back(&format!("/nb/{}", escape(book)), book),
            tallies.len(),
            notebook_bar(book, At::Tags)
        ),
    )
}

/// Everything in the notebook that is not done, in `todo::order` like
/// `noda todo`.
///
/// No box can be ticked here: an item has no stable address (line numbers move,
/// text collides) and giving it an id would make the file noda-only. The row
/// goes to the note instead.
pub fn todo(book: &str, tasks: &[Task]) -> String {
    let body = if tasks.is_empty() {
        "<div class=\"empty\"><b>Nothing to do</b>An unticked <code>- [ ]</code> in any note \
         turns up here, with a <code>due:2026-08-20</code> if you give it one.</div>"
            .to_string()
    } else {
        let mut out = String::new();
        for task in tasks {
            let due = match &task.due {
                Some(due) if task.overdue => {
                    format!("<span class=\"overdue\">{}</span>", escape(due))
                }
                Some(due) => format!("<span class=\"when\">{}</span>", escape(due)),
                None => String::new(),
            };
            let under = [
                due,
                format!("<span class=\"in\">{}</span>", escape(&task.title)),
            ]
            .into_iter()
            .filter(|piece| !piece.is_empty())
            .collect::<Vec<_>>()
            .join("<span class=\"sep\">·</span>");
            let _ = write!(
                out,
                "<a class=\"row\" href=\"/nb/{}/n/{}\"><div class=\"title\">{}</div>\
                 <div class=\"under\">{under}</div></a>",
                escape(book),
                escape(&task.id),
                escape(&task.text)
            );
        }
        out
    };

    shell(
        &format!("Todo — {book} — noda"),
        "",
        &format!(
            "<section class=\"pane\">\
             <header class=\"topbar\">{}<span class=\"here\">Todo</span>\
             <span class=\"count\">{}</span></header>\
             <main class=\"rows\">{body}</main></section>{}",
            back(&format!("/nb/{}", escape(book)), book),
            tasks.len(),
            notebook_bar(book, At::Todo)
        ),
    )
}

/// What links to a note, or to one of the notebook's files. Inbound only: what
/// a note points at is already in the note.
pub fn backlinks(book: &str, subject: &Subject, rows: &[Row]) -> String {
    shell(
        &format!("Links to {} — noda", subject.what),
        "",
        &format!(
            "<section class=\"pane\">\
             <header class=\"topbar\">{}<span class=\"here\">Backlinks</span>\
             <span class=\"count\">{}</span></header>{}</section>",
            back(&subject.at, &subject.what),
            rows.len(),
            backlinks_rows(book, subject, rows),
        ),
    )
}

/// The part of [`backlinks`] `script::BESIDE` fetches, per note read on a wide
/// screen, to rebuild the rows in the margin.
pub fn backlinks_rows(book: &str, subject: &Subject, rows: &[Row]) -> String {
    let body = if rows.is_empty() {
        format!(
            "<div class=\"empty\"><b>Nothing links here</b>\
             No note in {} points at {}.</div>",
            escape(book),
            escape(&subject.what)
        )
    } else {
        let mut out = String::new();
        for row in rows {
            let under = tag_line(&row.tags);
            let _ = write!(
                out,
                "<a class=\"row\" href=\"/nb/{}/n/{}\"><div class=\"title\">{}</div>\
                 <div class=\"under\">{under}</div></a>",
                escape(book),
                escape(&row.id),
                escape(&row.title)
            );
        }
        out
    };

    format!(
        "<main class=\"rows\">\
         <p class=\"said\">What links to <span class=\"{}\">{}</span></p>{body}</main>",
        if subject.mono { "mono" } else { "subject" },
        escape(&subject.what)
    )
}

/// One note, and — on a wide enough screen — the listing it came from. `drift`
/// is for the index pane's bar.
pub fn note(book: &str, reading: &Reading, drift: &str) -> String {
    // No `indexed`: the index pane is sent empty, since a listing is ~290
    // bytes a note (half a megabyte at two thousand) and none of it is drawn
    // below 1024px. `script::PANES` fetches it where the column shows.
    scripted(
        &note_title(reading),
        "split at-note",
        &[Asset::Panes, Asset::Beside, Asset::Stamps],
        &format!(
            "{}{}{}",
            // No order either: 450 bytes of a control over no rows.
            // `script::PANES` brings the rows and the order together.
            index_pane(book, &Asked::nothing(), None, "", drift, ""),
            read_pane(book, reading),
            notebook_bar(book, At::Notes),
        ),
    )
}

/// The same note, for a reader who already has the page around it: all a swap
/// uses, the rest (48 of 52 KB, measured) being on screen already. No `drift`,
/// as the swap keeps the index pane, so this costs one file read and no refs.
pub fn note_pane(book: &str, reading: &Reading) -> String {
    format!(
        "{}{}",
        titled(&note_title(reading)),
        read_pane(book, reading)
    )
}

fn note_title(reading: &Reading) -> String {
    format!("{} — noda", reading.title)
}

/// Shared by [`note`] and [`note_pane`], so the swap is part of the page.
fn read_pane(book: &str, reading: &Reading) -> String {
    let at = format!("/nb/{}/n/{}", escape(book), escape(&reading.id));
    let meta = [
        tag_line(&reading.tags),
        stamp("created", reading.created.as_deref()),
        stamp("updated", reading.updated.as_deref()),
    ]
    .into_iter()
    .filter(|piece| !piece.is_empty())
    .collect::<Vec<_>>()
    .join("<span class=\"sep\">·</span>");

    // The box, not the answer: backlinks walk every note (+8% on `ls`,
    // measured) for a column no screen under 1440px draws. `script::BESIDE`
    // fills it where it shows; scriptless readers have the Links button.
    let beside = "<aside class=\"beside\" hidden><div class=\"pane-head\">Backlinks</div>\
                  <div class=\"answer\"></div></aside>";
    // Things to do to the note, so none is marked current. Delete is safe on
    // the bar because `/delete` is a confirmation page. Pin is labelled with
    // what a press does, and is the one item that writes without a page in
    // between: it is undone by pressing it again.
    let (pin_label, pin_to) = if reading.pinned {
        ("Unpin", format!("{at}/unpin"))
    } else {
        ("Pin", format!("{at}/pin"))
    };
    let bar = action_bar(&[
        (EDIT, "Edit", Act::Go(format!("{at}/edit")), Mark::Plain),
        (TAGS, "Tags", Act::Go(format!("{at}/tags")), Mark::Plain),
        (
            RENAME,
            "Rename",
            Act::Go(format!("{at}/rename")),
            Mark::Plain,
        ),
        (
            LINKS,
            "Links",
            Act::Go(format!("{at}/backlinks")),
            Mark::Plain,
        ),
        (
            PIN,
            pin_label,
            Act::Post {
                to: pin_to,
                id: "pin",
            },
            Mark::Plain,
        ),
        (
            TRASH,
            "Delete",
            Act::Go(format!("{at}/delete")),
            Mark::Danger,
        ),
    ]);
    let home = format!("/nb/{}", escape(book));

    format!(
        "<section class=\"pane read\">\
         <header class=\"topbar\">{}<span class=\"here\">{}</span></header>\
         <main class=\"note\">\
         <div class=\"note-head\"><h1>{}</h1>\
         <div class=\"filename\"><span class=\"id\">{}</span>\
         <span class=\"slug\">-{}</span><span class=\"ext\">.md</span></div>\
         <div class=\"note-meta\">{meta}</div></div>\
         <div class=\"body\">{}</div>\
         {beside}</main>{bar}</section>",
        back(&home, book),
        // The note's title: the index pane beside it already names the notebook.
        escape(&reading.title),
        escape(&reading.title),
        escape(&reading.id),
        escape(&reading.slug),
        reading.rendered,
    )
}

/// A bar with a way back, an optional line saying what went wrong, and the form.
/// Both inside `<main>`, which the wide layout caps to a column; outside it a
/// textarea would run the monitor's width.
fn form_page(book: &str, title: &str, back_to: &str, said: &str, form: &str) -> String {
    laid_out(book, title, back_to, said, form, &[])
}

/// A form page that also listens for the note moving under it: the three that
/// carry a fingerprint (the editor and the two answers to a concurrent change).
fn watching_form_page(book: &str, title: &str, back_to: &str, said: &str, form: &str) -> String {
    laid_out(book, title, back_to, said, form, &[Asset::Watching])
}

fn laid_out(
    book: &str,
    title: &str,
    back_to: &str,
    said: &str,
    form: &str,
    scripts: &[Asset],
) -> String {
    scripted(
        &format!("{title} — noda"),
        "",
        scripts,
        &format!(
            "<section class=\"pane\">\
             <header class=\"topbar\">{}<span class=\"here\">{}</span></header>\
             <main>{said}{form}</main></section>",
            back(back_to, book),
            escape(title)
        ),
    )
}

/// Why a change was refused, said where the change was typed.
fn refusal(problem: Option<&str>) -> String {
    problem.map_or_else(String::new, |why| {
        format!("<p class=\"said bad\">{}</p>", escape(why))
    })
}

/// A new note.
pub fn composing(book: &str, draft: &Draft, problem: Option<&str>) -> String {
    form_page(
        book,
        "New note",
        &format!("/nb/{}", escape(book)),
        &refusal(problem),
        &format!(
            "<form class=\"write\" method=\"post\" action=\"/nb/{}/new\">\
             <div><label for=\"t\">Title</label>\
             <input id=\"t\" type=\"text\" name=\"title\" value=\"{}\" \
             placeholder=\"Leave it empty to take the first line\"></div>\
             <div><label for=\"g\">Tags</label>\
             <input id=\"g\" type=\"text\" name=\"tags\" value=\"{}\" placeholder=\"work q3\"></div>\
             <div><label for=\"b\">Note</label>\
             <textarea id=\"b\" name=\"body\" autofocus>{}</textarea></div>\
             <div class=\"buttons\"><button class=\"go\" type=\"submit\">Add note</button>\
             <a class=\"button\" href=\"/nb/{}\">Cancel</a></div></form>",
            escape(book),
            escape(&draft.title),
            escape(&draft.tags),
            escape(&draft.body),
            escape(book)
        ),
    )
}

/// `was` is the fingerprint the file had when this page was drawn, carried
/// through the form so the write can tell whether anything happened since.
pub fn editing(book: &str, about: &About, body: &str, was: &str, problem: Option<&str>) -> String {
    watching_form_page(
        book,
        &about.title,
        &about.at(book),
        &format!(
            "{}<p class=\"said\">Editing the body. The title and the tags have their own screens.</p>",
            refusal(problem)
        ),
        &format!(
            "<form class=\"write\" method=\"post\" action=\"{}/edit\">\
             <input type=\"hidden\" name=\"fingerprint\" value=\"{}\">\
             <div><textarea name=\"body\" autofocus>{}</textarea></div>\
             <div class=\"buttons\"><button class=\"go\" type=\"submit\">Save</button>\
             <a class=\"button\" href=\"{}\">Cancel</a></div></form>",
            about.at(book),
            escape(was),
            escape(body),
            about.at(book)
        ),
    )
}

/// The note changed under the reader and there was no version to merge from
/// (the base is not in the object database, e.g. never committed), so both are
/// handed back whole; otherwise [`conflicted`] shows the merge.
///
/// What is on disk is shown read-only above what they wrote, which stays
/// editable: saving is "keep mine", and editing is combining the two.
pub fn clashed(book: &str, about: &About, theirs: &str, mine: &str, now: &str) -> String {
    watching_form_page(
        book,
        &about.title,
        &about.at(book),
        "<p class=\"said bad\"><b>This note changed while you were writing.</b> \
         Nothing has been overwritten.</p>",
        &format!(
            "<form class=\"write\" method=\"post\" action=\"{}/edit\">\
             <input type=\"hidden\" name=\"fingerprint\" value=\"{}\">\
             <div><label>Saved now</label><pre class=\"theirs\">{}</pre></div>\
             <div><label for=\"b\">What you wrote</label>\
             <textarea id=\"b\" name=\"body\" autofocus>{}</textarea></div>\
             <div class=\"buttons\"><button class=\"go\" type=\"submit\">Save</button>\
             <a class=\"button\" href=\"{}\">Cancel</a></div></form>",
            about.at(book),
            escape(now),
            escape(theirs),
            escape(mine),
            about.at(book)
        ),
    )
}

/// The two edits changed the same lines, and the merge comes back to be
/// settled in one field: everything outside the markers is both edits combined.
pub fn conflicted(book: &str, about: &About, merged: &str, now: &str) -> String {
    watching_form_page(
        book,
        &about.title,
        &about.at(book),
        "<p class=\"said bad\"><b>Someone else saved while you were writing.</b> \
         Both versions are here and nothing has been overwritten. Where the two \
         changed the same lines, they are wrapped in <code>&lt;&lt;&lt;&lt;&lt;&lt;&lt;</code> \
         markers — keep what the note should say and delete the rest.</p>",
        &format!(
            "<form class=\"write\" method=\"post\" action=\"{}/edit\">\
             <input type=\"hidden\" name=\"fingerprint\" value=\"{}\">\
             <div><textarea name=\"body\" autofocus>{}</textarea></div>\
             <div class=\"buttons\"><button class=\"go\" type=\"submit\">Save</button>\
             <a class=\"button\" href=\"{}\">Cancel</a></div></form>",
            about.at(book),
            escape(now),
            escape(merged),
            about.at(book)
        ),
    )
}

/// A new title.
pub fn renaming(book: &str, about: &About, title: &str, problem: Option<&str>) -> String {
    form_page(
        book,
        "Rename",
        &about.at(book),
        &format!(
            "{}<p class=\"said\">The id never moves, so links and bookmarks survive. \
             The filename follows the title.</p>",
            refusal(problem)
        ),
        &format!(
            "<form class=\"write\" method=\"post\" action=\"{}/rename\">\
             <div><label for=\"t\">Title</label>\
             <input id=\"t\" type=\"text\" name=\"title\" value=\"{}\" autofocus></div>\
             <div class=\"buttons\"><button class=\"go\" type=\"submit\">Rename</button>\
             <a class=\"button\" href=\"{}\">Cancel</a></div></form>",
            about.at(book),
            escape(title),
            about.at(book)
        ),
    )
}

/// A ticked box per tag the note has, and a field for new ones; the server
/// works out the `+`s and `-`s. The field is cut by `query::split` — a space
/// separates, a quote holds one together — and the placeholder shows a quoted
/// tag so it does not read as a one-tag field.
///
/// Every tag is sent as `saw`, and again as `keep` if still ticked. An unticked
/// box sends nothing, so without `saw` the server would diff against the file
/// and remove a tag added since. One hidden field per tag, so a tag with a
/// space needs no quoting.
pub fn tagging(book: &str, about: &About, tags: &[String], problem: Option<&str>) -> String {
    let mut boxes = String::new();
    for (n, tag) in tags.iter().enumerate() {
        let _ = write!(
            boxes,
            "<input type=\"hidden\" name=\"saw\" value=\"{}\">\
             <label class=\"tick\" for=\"t{n}\">\
             <input id=\"t{n}\" type=\"checkbox\" name=\"keep\" value=\"{}\" checked>\
             <span>{}</span></label>",
            escape(tag),
            escape(tag),
            escape(tag)
        );
    }
    let held = if tags.is_empty() {
        "<p class=\"said\">This note has no tags yet.</p>".to_string()
    } else {
        format!("<div><label>On this note</label><div class=\"ticks\">{boxes}</div></div>")
    };

    form_page(
        book,
        "Tags",
        &about.at(book),
        &refusal(problem),
        &format!(
            "<form class=\"write\" method=\"post\" action=\"{}/tags\">{held}\
             <div><label for=\"a\">Add tags</label>\
             <input id=\"a\" type=\"text\" name=\"add\" \
             placeholder=\"ops docs &quot;24.04 Dark patterns&quot;\">\
             <small class=\"hint\">Separated by spaces. Quote one that has a space in it.\
             </small></div>\
             <div class=\"buttons\"><button class=\"go\" type=\"submit\">Save</button>\
             <a class=\"button\" href=\"{}\">Cancel</a></div></form>",
            about.at(book),
            about.at(book)
        ),
    )
}

/// Says the note can be brought back, which git makes true: a warning that
/// overstates the danger teaches people to click through warnings.
pub fn deleting(book: &str, about: &About) -> String {
    form_page(
        book,
        "Delete",
        &about.at(book),
        // In the `said` slot, not the form: `.said` has its own padding and
        // rule, which a form would inset.
        &format!(
            "<p class=\"said\"><b>Delete {}?</b> The file goes and the commit that \
             removed it stays, so <code>noda restore</code> brings it back with its id.</p>",
            escape(&about.title)
        ),
        &format!(
            "<form class=\"write\" method=\"post\" action=\"{}/delete\">\
             <div class=\"buttons\"><button class=\"danger\" type=\"submit\">Delete</button>\
             <a class=\"button\" href=\"{}\">Keep it</a></div></form>",
            about.at(book),
            about.at(book)
        ),
    )
}

/// `noda status`'s facts, already worked out by the caller.
pub struct Standing {
    pub branch: String,
    pub notes: usize,
    pub files: usize,
    /// Files differing from `HEAD`: what a sync would commit before it pulls.
    pub uncommitted: usize,
    pub remote: Option<String>,
    /// How far apart the two are, in words: `2 to push, 1 to pull`.
    pub drift: String,
    /// One line per kind, as `doctor` would name them.
    pub problems: Vec<String>,
}

/// An errand as the page needs to say it: what it is, and how it went.
pub struct Errand<'a> {
    /// `Syncing` / `Pulling` / `Pushing`, for while it is happening.
    pub doing: &'a str,
    /// Chosen by the caller alongside `failed`.
    pub done: &'a str,
    /// What it printed, or what went wrong. `None` while it is still going.
    pub said: Option<&'a str>,
    pub failed: bool,
    pub seconds: u64,
}

/// Where the notebook stands, and the three ways to move it.
///
/// A button `POST`s to start the errand and redirects back, rather than holding
/// a request open for as long as the network takes; the page is a `GET`, so a
/// reload asks how it is going. While one runs the page refreshes itself.
pub fn standing(book: &str, standing: &Standing, errand: Option<&Errand>) -> String {
    let at = escape(book);
    dressed(
        &format!("Status — {book} — noda"),
        "",
        working(errand).then_some(AGAIN_IN),
        &[Asset::Standing],
        &format!(
            "<section class=\"pane\">\
             <header class=\"topbar\">{}<span class=\"here\">Status</span>\
             <span class=\"count\">{at}</span></header>{}</section>{}",
            back(&format!("/nb/{at}"), book),
            network_main(book, standing, errand),
            notebook_bar(book, At::Status),
        ),
    )
}

/// What `script::STANDING` polls every `AGAIN_IN` seconds while an errand
/// runs: the `<main>`, led by the `refresh` meta. The script polls again only
/// if the meta is there, so stopping stays the server's decision.
pub fn standing_main(book: &str, standing: &Standing, errand: Option<&Errand>) -> String {
    format!(
        "{}{}",
        refresh(working(errand).then_some(AGAIN_IN)),
        network_main(book, standing, errand)
    )
}

/// Read by both the refresh meta and the script's poll.
const AGAIN_IN: u32 = 2;

fn working(errand: Option<&Errand>) -> bool {
    errand.is_some_and(|errand| errand.said.is_none())
}

fn network_main(book: &str, standing: &Standing, errand: Option<&Errand>) -> String {
    let mut rows = String::new();
    let mut row = |name: &str, value: &str, mono: bool| {
        let _ = write!(
            rows,
            "<div class=\"row\"><div class=\"name\">{}</div>\
             <div class=\"under\"><span class=\"{}\">{}</span></div></div>",
            escape(name),
            if mono { "mono" } else { "when" },
            escape(value)
        );
    };
    row("Branch", &standing.branch, true);
    row(
        "Holds",
        &if standing.files > 0 {
            format!(
                "{}, {}",
                plural(standing.notes, "note"),
                plural(standing.files, "file")
            )
        } else {
            plural(standing.notes, "note")
        },
        false,
    );
    row(
        "Changes",
        // `noda status`'s words.
        &match standing.uncommitted {
            0 => "clean".to_string(),
            1 => "1 file uncommitted".to_string(),
            n => format!("{n} files uncommitted"),
        },
        false,
    );
    match &standing.remote {
        Some(url) => {
            row("Remote", url, true);
            row("Drift", &standing.drift, false);
        }
        // The remedy, since no screen here sets a remote.
        None => row(
            "Remote",
            "none — set one with `noda remote set <url>`",
            false,
        ),
    }
    for problem in &standing.problems {
        row("Problem", problem, false);
    }
    // Which build is answering, for bug reports against `noda web`.
    row("Version", crate::VERSION, true);

    let said = match errand {
        None => String::new(),
        Some(errand) => match errand.said {
            // Seconds rather than a bar: nothing knows how long a fetch takes.
            None => format!(
                "<p class=\"said working\"><b>{}…</b> {}</p>",
                escape(errand.doing),
                plural(errand.seconds as usize, "second")
            ),
            // Whole: what `sync` printed says what it did.
            Some(said) => format!(
                "<p class=\"said{}\"><b>{}</b><span class=\"outcome\">{}</span></p>",
                if errand.failed { " bad" } else { "" },
                escape(errand.done),
                escape(said)
            ),
        },
    };

    // The server already refuses a second press; disabling shows the first
    // landed.
    let busy = working(errand);
    let at = escape(book);
    let mut buttons = String::new();
    for (errand, label) in [("sync", "Sync"), ("pull", "Pull"), ("push", "Push")] {
        let _ = write!(
            buttons,
            "<form method=\"post\" action=\"/nb/{at}/status/{errand}\">\
             <button class=\"{}\" type=\"submit\"{}>{label}</button></form>",
            // Sync is accented by colour, so all three stay one row on a phone.
            if errand == "sync" { "go" } else { "" },
            if busy { " disabled" } else { "" }
        );
    }

    format!(
        "<main>{said}<div class=\"rows facts\">{rows}</div>\
         <div class=\"abreast\">{buttons}</div></main>"
    )
}

/// What failed and why, with a way back so it is not a dead end.
pub fn failure(heading: &str, detail: &str) -> String {
    shell(
        &format!("{heading} — noda"),
        "",
        &format!(
            "<section class=\"pane\">\
             <header class=\"topbar\">{}<span class=\"here\">noda</span></header>\
             <main><div class=\"empty\"><b>{}</b>{}</div></main></section>",
            back("/", "the notebooks"),
            escape(heading),
            escape(detail)
        ),
    )
}

/// A listing's stamp: the day, and nothing that could be read as a clock.
fn when(updated: Option<&str>) -> String {
    updated.map_or_else(String::new, |value| {
        format!(
            "<time class=\"when\" datetime=\"{}\">{}</time>",
            escape(value),
            escape(&day(value))
        )
    })
}

/// A note page's stamp, written whole with its offset so it cannot be misread;
/// `data-clock` tells `script::STAMPS` it has room for a time of day, which a
/// listing's does not.
fn stamp(what: &str, value: Option<&str>) -> String {
    value.map_or_else(String::new, |value| {
        let value = escape(value);
        format!("<span class=\"when\">{what} <time datetime=\"{value}\" data-clock>{value}</time></span>")
    })
}

/// `1 note` / `2 notes`. English only, like every other string noda prints.
fn plural(count: usize, thing: &str) -> String {
    if count == 1 {
        format!("{count} {thing}")
    } else {
        format!("{count} {thing}s")
    }
}

/// Exposed for tests, among them that `script.rs`'s copy of the split
/// breakpoint agrees with the sheet.
pub(crate) fn stylesheet() -> &'static str {
    CSS
}

/// Mobile first. `--tap` is the smallest any control may be, and inputs are
/// 16px because below that iOS Safari zooms on focus.
const CSS: &str = "\
*{box-sizing:border-box}\
/* The user-agent `[hidden]{display:none}` loses to any author `display` \
   rule such as `.row{display:block}`, which drew hidden rows. */\
[hidden]{display:none!important}\
:root{--tap:48px;\
/* The rail's fixed width above 640px, so a pane can be sized against the rest. */\
--rail:76px;\
--mono:ui-monospace,SFMono-Regular,'SF Mono',Menlo,'Cascadia Mono',Consolas,monospace;\
--read:ui-serif,Charter,'Iowan Old Style',Georgia,'Songti TC','Noto Serif CJK TC',serif}\
html{-webkit-text-size-adjust:100%}\
body{margin:0;background:var(--bg);color:var(--text);font-family:var(--mono);font-size:14px;line-height:1.6}\
svg{fill:none;stroke:currentColor;stroke-width:2;stroke-linecap:round;stroke-linejoin:round}\
/* A phone stacks; wider screens are a grid, declared at their width. \
   `min-width:0` stops one long unbroken line widening a column past the screen. \
   The pane grows to fill the screen because a sticky bar is bounded by its \
   containing block: otherwise a short note leaves it floating mid-screen. */\
.app{display:flex;flex-direction:column;min-height:100dvh}\
.pane{display:block;min-width:0;flex:1 1 auto}\
.foot{flex:0 0 auto}\
/* The same for the note's own bar: the prose takes the slack. */\
.read{display:flex;flex-direction:column}\
.read main{flex:1 1 auto}\
.topbar{display:flex;align-items:center;gap:4px;min-height:56px;padding-right:16px;\
border-bottom:1px solid var(--rule);position:sticky;top:0;background:var(--bg);z-index:1}\
.topbar .back{min-width:var(--tap);min-height:var(--tap);display:inline-flex;align-items:center;\
justify-content:center;color:var(--muted);flex:0 0 auto;-webkit-tap-highlight-color:transparent}\
.topbar .back:active{background:var(--press)}\
.topbar .back svg{width:24px;height:24px}\
.topbar .lead{padding-left:16px}\
.topbar .here{font-size:15px;font-weight:600;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}\
.topbar .count{margin-left:auto;color:var(--punct);font-size:13px;flex:0 0 auto;padding-left:12px}\
.searchbar{padding:10px 12px;border-bottom:1px solid var(--rule)}\
.searchbar input{width:100%;height:var(--tap);padding:0 14px;border-radius:10px;\
border:1px solid var(--rule);background:var(--bg-sunk);color:var(--text);\
font-family:var(--mono);font-size:16px}\
.searchbar input::placeholder{color:var(--punct)}\
.problem{margin:8px 2px 0;font-size:12.5px;color:var(--alert)}\
/* Grey rather than a hue: nothing in this palette colours a state. */\
.searchbar .hint{margin:8px 2px 0;font-size:12.5px;color:var(--muted)}\
/* ------------------------------------------- SIGNATURE: the query parse */\
/* See `page::grouping`. Not on a phone, which is short of room. */\
.parse{display:none;margin:9px 2px 1px;flex-wrap:wrap;align-items:center;gap:7px;font-size:12px}\
/* A group is a pill: people do not read brackets. */\
.parse .g{display:inline-flex;align-items:center;gap:6px;padding:2px 9px;\
border:1px solid var(--rule);border-radius:999px;background:var(--bg-sunk)}\
.parse .g b{font-weight:400;color:var(--text)}\
.parse .g b.t{color:var(--tag)}\
.parse .g i{font-style:normal;color:var(--punct);font-size:11px;letter-spacing:.06em}\
.parse .and{color:var(--punct);font-size:11px;letter-spacing:.09em;text-transform:uppercase}\
/* ------------------------------------------------ SIGNATURE: the order */\
/* `.drift .pill`'s chip: a 32px pill in a 48px press. It wraps rather than \
   scrolling, so no chip is out of sight; the split index is sized to hold \
   the four in a line (see the desktop grid). */\
.sortbar{display:flex;flex-wrap:wrap;align-items:center;gap:6px;margin:2px 2px 0}\
.sortbar .lab{flex:0 0 auto;display:inline-flex;align-items:center;\
color:var(--punct);padding-right:2px}\
.sortbar .lab svg{width:15px;height:15px}\
.sortbar a{flex:0 0 auto;display:inline-flex;align-items:center;min-height:var(--tap);\
color:var(--muted);-webkit-tap-highlight-color:transparent}\
/* 8px, not `.drift .pill`'s 11px: 24px less for the index column to find. */\
.sortbar a .pill{display:inline-flex;align-items:center;gap:5px;min-height:32px;\
padding:0 8px;border:1px solid var(--rule);border-radius:999px;\
background:var(--bg-sunk);font-size:12px;white-space:nowrap}\
.sortbar a:active .pill{background:var(--press)}\
/* The one in force loses its fill and takes the id's hue on its edge, \
   rather than coloured text: nothing in this palette colours a state. */\
.sortbar a[aria-current] .pill{background:transparent;border-color:var(--id);color:var(--text)}\
.sortbar a svg{width:12px;height:12px;flex:0 0 auto;color:var(--id)}\
a{color:inherit;text-decoration:none}\
.row{display:block;min-height:64px;padding:12px 16px;border-bottom:1px solid var(--rule);\
-webkit-tap-highlight-color:transparent}\
.row:active{background:var(--press)}\
/* The note the reading pane shows, where both panes are; a press's grey. */\
.row.here{background:var(--press)}\
.row .title{font-family:var(--read);font-size:17px;line-height:1.32}\
.row .title.mono{font-family:var(--mono);font-size:15px}\
.row .name{font-size:15px}\
.row .under{margin-top:4px;font-size:12.5px;display:flex;gap:8px;align-items:baseline;flex-wrap:wrap}\
.row .under:empty{display:none}\
/* ------------------------------------------------ SIGNATURE: the id column */\
/* Written on every row, shown only where there is a column to spare: not on \
   a phone (see 640 and 1024 below). Mono and `--id`, as `noda ls` prints it. */\
.row .ident{display:none;font-family:var(--mono);font-size:12px;color:var(--id)}\
.tags{color:var(--tag)}\
.pin{color:var(--pin)}\
.sep{color:var(--punct)}\
.when{color:var(--muted)}\
/* The one colour marking a state rather than a kind; `style.rs` says why. */\
.overdue{color:var(--overdue)}\
.row .in{color:var(--muted)}\
/* A row with two destinations; the aside is a tap target, so row-tall. */\
.row.split{display:flex;align-items:stretch;gap:12px;padding:0}\
.row.split .most{flex:1 1 auto;min-width:0;padding:12px 0 12px 16px}\
.row.split .aside{flex:0 0 auto;display:flex;align-items:center;padding:0 16px;\
color:var(--muted);font-size:12.5px;-webkit-tap-highlight-color:transparent}\
.row.split .aside:active{background:var(--press)}\
.subject{font-family:var(--read)}\
.mono{font-family:var(--mono)}\
mark{background:var(--mark);color:inherit;border-radius:2px;padding:0 1px}\
.note-head{padding:18px 16px 16px;border-bottom:1px solid var(--rule)}\
.note-head h1{font-family:var(--read);font-size:24px;line-height:1.24;margin:0 0 10px;\
font-weight:600;letter-spacing:-0.01em}\
.filename{font-size:13px}\
.filename .id{color:var(--id)}\
.filename .slug{color:var(--id-dim)}\
.filename .ext{color:var(--punct)}\
.note-meta{margin-top:8px;font-size:12.5px;display:flex;gap:8px;align-items:baseline;flex-wrap:wrap}\
.body{padding:16px;font-family:var(--read);font-size:17px;line-height:1.62;\
overflow-wrap:break-word}\
/* A rendered note. The measure is set on the wide screen, below. */\
.body>:first-child{margin-top:0}\
.body>:last-child{margin-bottom:0}\
.body h1,.body h2,.body h3,.body h4{font-family:var(--read);font-weight:600;\
line-height:1.25;letter-spacing:-0.01em;margin:1.5em 0 .5em}\
.body h1{font-size:1.24em}\
.body h2{font-size:1.12em}\
.body h3{font-size:1em}\
.body h4{font-size:.94em;color:var(--muted)}\
.body p{margin:0 0 .9em}\
.body ul,.body ol{margin:0 0 .9em;padding-left:1.4em}\
.body li{margin:.25em 0}\
.body li p{margin:0}\
/* Disabled: ticking one has to be a commit, as the CLI has no `todo done`. */\
.body li input[type=checkbox]{width:16px;height:16px;margin:0 .45em 0 0;\
accent-color:var(--tag);vertical-align:-2px}\
.body a{color:var(--tag);text-decoration:underline;text-underline-offset:3px;\
text-decoration-thickness:1px}\
/* A link in the id's colour stays in the notebook; the tag's colour leaves. */\
.body a.note{color:var(--id)}\
.body code{font-family:var(--mono);font-size:.85em;background:var(--bg-sunk);\
border:1px solid var(--rule);border-radius:5px;padding:1px 5px}\
.body pre{background:var(--bg-sunk);border:1px solid var(--rule);border-radius:10px;\
padding:12px 14px;margin:0 0 .9em;overflow-x:auto}\
.body pre code{background:none;border:0;padding:0;font-size:14px;line-height:1.55}\
.body blockquote{margin:0 0 .9em;padding:.1em 0 .1em 14px;\
border-left:3px solid var(--rule);color:var(--muted)}\
.body blockquote p:last-child{margin:0}\
.body img{max-width:100%;height:auto;display:block;border-radius:10px;margin:0 0 .9em}\
.body hr{border:0;border-top:1px solid var(--rule);margin:1.5em 0}\
/* A table scrolls inside itself rather than widening the whole note. */\
.body table{display:block;width:fit-content;max-width:100%;overflow-x:auto;\
border-collapse:collapse;margin:0 0 .9em;font-size:.92em}\
.body th,.body td{border-bottom:1px solid var(--rule);padding:8px 12px 8px 0;\
text-align:left;vertical-align:top}\
.body th{font-family:var(--mono);font-size:.8em;font-weight:600;color:var(--muted);\
letter-spacing:.03em;text-transform:uppercase}\
.empty{padding:28px 18px;color:var(--muted)}\
.empty b{display:block;font-family:var(--read);font-size:19px;color:var(--text);\
font-weight:600;margin-bottom:8px}\
.empty a{color:var(--tag);display:inline-flex;align-items:center;min-height:var(--tap);\
text-decoration:underline;text-underline-offset:3px}\
.empty code{color:var(--id)}\
:focus-visible{outline:2px solid var(--tag);outline-offset:-2px}\
.actionbar{position:sticky;bottom:0;display:flex;border-top:1px solid var(--rule);\
background:var(--bg-sunk);padding-bottom:env(safe-area-inset-bottom,0px)}\
/* A write is a POST, so a button, styled here to look like the links. */\
.actionbar a,.actionbar button{flex:1;min-height:64px;padding:9px 0 10px;display:flex;\
flex-direction:column;align-items:center;justify-content:center;gap:4px;color:var(--muted);\
-webkit-tap-highlight-color:transparent}\
.actionbar button{background:none;border:0;font:inherit;cursor:pointer}\
.actionbar a:active,.actionbar button:active{background:var(--press)}\
.actionbar svg{width:26px;height:26px}\
.actionbar span{font-size:11.5px;letter-spacing:0.02em}\
/* Brighter rather than another hue: the palette does not colour a state. */\
.actionbar a[aria-current]{color:var(--text)}\
/* The only use of the alert colour other than a refusal. */\
.actionbar a.danger{color:var(--alert)}\
/* The one action, above the bar at the right where a thumb is, and the only \
   round thing here so it cannot be mistaken for a row. */\
.foot{position:sticky;bottom:0;z-index:2}\
.foot .actionbar{position:static}\
.fab{position:absolute;right:16px;bottom:calc(100% + 16px);\
width:56px;height:56px;border-radius:28px;background:var(--id);color:var(--bg);\
display:flex;align-items:center;justify-content:center;\
/* A ring of background, not a shadow: this sheet never uses depth. */\
box-shadow:0 0 0 5px var(--bg);-webkit-tap-highlight-color:transparent}\
.fab svg{width:26px;height:26px;stroke-width:2.4}\
.fab:active{background:var(--id-dim)}\
/* Room under the last row for the 56px button standing 16px clear. */\
body:has(.fab) main{padding-bottom:76px}\
form.write{padding:16px;display:flex;flex-direction:column;gap:16px}\
form.write label{font-size:12.5px;color:var(--punct);display:block;margin-bottom:6px}\
form.write .hint{display:block;margin-top:6px;font-size:12.5px;color:var(--muted);\
line-height:1.45}\
form.write input[type=text],form.write textarea{width:100%;padding:12px 14px;border-radius:10px;\
border:1px solid var(--rule);background:var(--bg-sunk);color:var(--text);font-size:16px}\
form.write input[type=text]{font-family:var(--mono);min-height:var(--tap)}\
form.write textarea{font-family:var(--read);font-size:17px;line-height:1.6;min-height:300px;\
resize:vertical}\
.buttons{display:flex;gap:10px}\
button,.button{min-height:var(--tap);padding:0 20px;border-radius:10px;\
border:1px solid var(--rule);background:var(--bg-sunk);color:var(--text);\
font-family:var(--mono);font-size:15px;display:inline-flex;align-items:center;\
justify-content:center}\
button.go{background:var(--id);border-color:var(--id);color:var(--bg);font-weight:600;flex:1}\
button.danger{background:var(--alert);border-color:var(--alert);color:var(--bg);\
font-weight:600;flex:1}\
.ticks{display:flex;flex-direction:column}\
/* The whole row is the label, so it is the tap target. `form.write` in the \
   selector to outrank `form.write label` above. */\
form.write label.tick{display:flex;align-items:center;gap:12px;min-height:var(--tap);\
color:var(--tag);font-size:16px;margin:0;width:100%;padding:0 2px;\
border-bottom:1px solid var(--rule)}\
form.write label.tick:last-child{border-bottom:0}\
.tick input{width:22px;height:22px;flex:none;margin:0;accent-color:var(--tag)}\
.said{padding:12px 16px;border-bottom:1px solid var(--rule);color:var(--muted);font-size:13px}\
.said b{color:var(--text);font-weight:600}\
.said code{font-family:var(--mono)}\
.said.bad{color:var(--alert)}\
.said.bad b{color:var(--alert)}\
/* Drift against the remote: uncoloured, as the palette marks no state, and \
   a link, so target-sized. */\
.topbar .drift{margin-left:auto;flex:0 1 auto;min-width:0;display:inline-flex;\
align-items:center;min-height:var(--tap);color:var(--text);\
-webkit-tap-highlight-color:transparent}\
.topbar .drift .pill{display:inline-flex;align-items:center;gap:6px;min-width:0;\
min-height:32px;padding:0 11px;border:1px solid var(--rule);border-radius:999px;\
font-size:12px}\
.topbar .drift .pill span{overflow:hidden;text-overflow:ellipsis;white-space:nowrap}\
.topbar .drift svg{width:14px;height:14px;flex:0 0 auto;color:var(--muted)}\
.topbar .drift:active .pill{background:var(--press)}\
/* `.topbar` to beat `.topbar .count` on specificity, not source order. */\
.topbar .drift+.count{margin-left:10px;padding-left:0}\
.abreast{display:flex;gap:10px;padding:18px 16px 22px}\
.abreast form{flex:1;display:flex}\
.abreast button{flex:1;padding:0 12px}\
button[disabled]{opacity:.45}\
/* What the command printed, line for line. */\
.outcome{display:block;margin-top:6px;font-family:var(--mono);font-size:12.5px;\
line-height:1.5;white-space:pre-line;overflow-wrap:anywhere;color:var(--muted)}\
.said.bad .outcome{color:var(--alert)}\
/* The one thing that moves: a self-reloading page must show it is working. */\
.working b::after{content:\"\";display:inline-block;width:7px;height:7px;\
border-radius:4px;background:var(--id);margin-left:9px;vertical-align:1px;\
animation:breathe 1.4s ease-in-out infinite}\
@keyframes breathe{0%,100%{opacity:.2}50%{opacity:1}}\
@media (prefers-reduced-motion:reduce){.working b::after{animation:none;opacity:.8}}\
.theirs{margin:0;padding:14px;border:1px solid var(--rule);border-radius:10px;\
background:var(--bg-sunk);font-family:var(--read);font-size:16px;line-height:1.55;\
white-space:pre-wrap;overflow-wrap:break-word}\
/* ------------------------------------------------------------ label role */\
/* Names a region; never inside content. */\
.pane-head{font-size:11px;letter-spacing:.09em;text-transform:uppercase;\
color:var(--punct);padding:0 0 8px}\
/* --------------------------------------------------------- the margin note */\
/* Hidden until it holds something: the `display:block` below is on \
   `:not([hidden])`. Without a script it stays hidden for good. */\
.beside{display:none}\
.beside .mini{display:block;padding:8px 0;border-bottom:1px solid var(--rule);\
font-family:var(--read);font-size:14px;line-height:1.35;color:var(--text)}\
.beside .mini:last-child{border-bottom:0}\
.beside .mini:hover{color:var(--tag)}\
.beside .mini span{display:block;font-family:var(--mono);font-size:11px;\
color:var(--id);margin-top:3px}\
.beside .none{margin:0;color:var(--muted);font-size:13px}\
/* Waiting: the status screen's breathing dot, without its bold. */\
.beside .said{margin:0;padding:0;border-bottom:0;font-size:13px}\
.beside .said b{font-weight:400;color:var(--muted)}\
/* Below two-pane width one pane shows, by route. Its own query, so no two \
   `display` rules fight on source order. */\
@media (max-width:1023px){.app.split.at-list .read{display:none}}\
/* A note page is sent without its listing (`script::PANES`). `.indexed` says \
   the pane has rows: set by the server on the listing route and by the script \
   on a note route before first paint, so an empty pane is never a column. */\
.app.split.at-note .index{display:none}\
/* A phone reading a note gets one bar, the note's; the rest is up the chevron. */\
@media (max-width:639px){\
.app.split.at-note .foot{display:none}\
.app.split.at-note main{padding-bottom:16px}}\
/* ================================================================= TABLET */\
/* The bottom bar becomes a rail. It is last in the markup, so a phone's \
   sticks to the foot, and placed first here. */\
@media (min-width:640px){\
.app{display:grid;grid-template-columns:var(--rail) minmax(0,1fr);height:100dvh;overflow:hidden}\
.foot{grid-column:1;grid-row:1/-1;position:static;display:flex;flex-direction:column;\
align-items:stretch;background:var(--bg-sunk);border-right:1px solid var(--rule)}\
.foot .actionbar{flex-direction:column;border-top:0;background:transparent;padding:0}\
.foot .actionbar a{flex:0 0 auto;min-height:62px}\
/* The action first in the rail, and squarish to sit in a column of them. */\
.fab{order:-1;position:static;width:44px;height:44px;border-radius:13px;\
margin:14px auto 12px;box-shadow:none}\
.fab svg{width:22px;height:22px}\
/* Each pane scrolls on its own, so a listing keeps its place. */\
.pane{grid-column:2;min-height:0;overflow-y:auto}\
.app:has(.fab) main{padding-bottom:24px}\
/* One pane hangs off the rail, uncentred, capped at a readable width. */\
.app:not(.split) .topbar,.app:not(.split) main{max-width:80em}\
.topbar,.searchbar{padding-left:24px;padding-right:24px}\
.topbar .back{margin-left:-12px}\
.topbar .lead{padding-left:0}\
.parse{display:flex}\
/* The row extends into `ls -l`'s columns: id, title, day, tags. Tags last, \
   as the one column a note may lack would shift the rest. */\
.rows .row{display:flex;align-items:baseline;gap:20px;min-height:0;padding:13px 24px}\
.rows .row .ident{display:block;flex:0 0 auto}\
/* The day is at the right, where `-l` prints it; the copy by the id is for \
   the narrow index pane below. */\
.rows .row .ident .day{display:none}\
.rows .row .title,.rows .row .name{flex:1 1 auto}\
.rows .row .under{margin:0;flex:0 0 auto;justify-content:flex-end}\
.rows .row.split{display:flex}\
.rows .row.split .most{display:flex;align-items:baseline;gap:20px;padding:13px 0 13px 24px}\
.rows .row.split .aside{padding:0 24px}\
/* Short rows in columns; `column-width` rather than a grid for the \
   hairline `column-rule`. */\
.rows.cols{column-width:270px;column-gap:0;column-rule:1px solid var(--rule)}\
.rows.cols .row{break-inside:avoid;display:block;padding:13px 24px}\
.rows.cols .row .under{margin-top:3px;justify-content:flex-start}\
.rows.cols.wide{column-width:400px}\
.rows.cols .row.split{display:flex;padding:0}\
.rows.cols .row.split .most{display:block;padding:13px 0 13px 24px}\
.rows.cols .row.split .aside{padding:0 24px}\
.rows.facts .row{display:grid;grid-template-columns:150px minmax(0,1fr);gap:0}\
.rows.facts .row .under{justify-content:flex-start}\
.note-head{padding:26px 32px 20px}\
.note-head h1{font-size:28px}\
/* The measure is on the body, in its own font: on `main` the `em` would be \
   the chrome's monospace. */\
.body{font-size:18px;line-height:1.65;max-width:34em;padding:24px 32px 8px}\
.said{padding:14px 32px}\
.empty{padding:34px 32px}\
form.write{padding:24px 32px;max-width:52em}\
form.write textarea{min-height:min(56vh,560px)}\
.buttons{justify-content:flex-start}\
button.go,button.danger{flex:0 0 auto;min-width:190px}\
.abreast{padding:20px 32px 28px;max-width:52em}\
/* The note's bar becomes a toolbar at the head, by `order`: one element. */\
.read{display:flex;flex-direction:column}\
.read .topbar{order:-2}\
.read .actionbar{order:-1;position:static;background:var(--bg);border-top:0;\
border-bottom:1px solid var(--rule);justify-content:flex-start;gap:2px;padding:7px 20px}\
.read .actionbar a,.read .actionbar button{flex:0 0 auto;flex-direction:row;gap:8px;\
min-height:38px;padding:0 13px;border-radius:9px;font-size:13px}\
.read .actionbar a:hover,.read .actionbar button:hover{background:var(--press);\
color:var(--text)}\
/* Keeps Delete's colour against the hover rule above. */\
.read .actionbar a.danger:hover{color:var(--alert)}\
.read .actionbar svg{width:17px;height:17px}\
.read main{order:0;flex:1 1 auto}}\
/* ================================================================ DESKTOP */\
/* The index stays beside the note. Three columns only with `.indexed`; \
   without a script it is the tablet's two. \
   The 356px floor is measured: 288px of order chips, 40px padding, 2px bar \
   and 24px for a wider monospace. At 300px the fourth chip wrapped. */\
@media (min-width:1024px){\
.app.split.indexed{grid-template-columns:var(--rail) clamp(356px,26vw,380px) minmax(0,1fr)}\
.app.split.indexed .index{grid-column:2;border-right:1px solid var(--rule)}\
.app.split.indexed .read{grid-column:3}\
.app.split.indexed.at-note .index{display:block}\
.app.split.at-list .read{display:flex}\
/* In the narrow column the row stacks again, tighter than a phone's. */\
.app.split .index .rows .row{display:block;padding:11px 20px;min-height:0}\
/* The id line takes the day at its other end; the copy under the title goes. */\
.app.split .index .rows .row .ident{display:flex;justify-content:space-between;gap:10px}\
.app.split .index .rows .row .ident .day{display:block;color:var(--muted)}\
.app.split .index .rows .row .title{font-size:15.5px;line-height:1.34;margin-top:2px}\
.app.split .index .rows .row .under{margin-top:2px;justify-content:flex-start;font-size:12px}\
.app.split .index .rows .row .under .when,.app.split .index .rows .row .under .sep{display:none}\
.app.split .index .searchbar,.app.split .index .topbar{padding-left:20px;padding-right:20px}\
.app.split .index .empty{padding:26px 20px}\
/* Head and body share one column, so the title's rule spans the prose. */\
.app.split .read main.note{width:100%;max-width:44em;margin-inline:auto}\
.app.split .read .body{max-width:none}\
/* The note's chevron goes when the listing is beside it. */\
.app.split.indexed .read .topbar .back{display:none}}\
/* =================================================================== WIDE */\
/* Backlinks in the margin. `margined`, like `indexed`, is set by the script \
   before first paint, so the column is reserved and the prose laid out once; \
   without a script the note keeps its centred measure. */\
@media (min-width:1440px){\
/* `align-content:start` is load-bearing: the default stretches spare height \
   among the rows, and a short note's body jumped 40px when the margin landed. */\
.app.split.at-note.margined .read main.note{max-width:none;margin-inline:0;\
display:grid;grid-template-columns:minmax(0,38em) 236px;column-gap:48px;\
align-items:start;align-content:start;justify-content:center}\
/* Head and body share the track, without padding. */\
.app.split.at-note.margined .read .note-head,\
.app.split.at-note.margined .read .body{grid-column:1;padding-left:0;padding-right:0}\
.app.split.at-note.margined .read .beside:not([hidden]){display:block;grid-column:2;\
grid-row:1/span 2;position:sticky;top:24px;padding:26px 0 0}}\
/* Extra width goes to the index and the margin, never the measure. */\
@media (min-width:1800px){\
.app.split.indexed{grid-template-columns:var(--rail) clamp(340px,22vw,470px) minmax(0,1fr)}\
.app.split.at-note.margined .read main.note{grid-template-columns:minmax(0,40em) 290px;\
column-gap:64px}}\
/* ============================================ SIGNATURE: a row is a notebook */\
/* Last on purpose: some rules below beat earlier ones of equal weight by \
   source order. The front page has no rail, so no grid (it left an empty 76px \
   column), and no `max-width:80em`: its rows are a table, not prose. */\
@media (min-width:640px){\
.app.root{display:block;height:auto;overflow:visible}\
/* Not its own scroll container, or the sticky bar never sticks. */\
.app.root .pane{overflow:visible;min-height:0}\
.app.root .topbar,.app.root main{max-width:none}}\
/* `.books`, or `.rows .row`'s padding at 640 wins and the row is indented twice. */\
.books .row.split{position:relative;padding:0}\
.books .row.split .most{padding:13px 0 13px 28px}\
.books .most{min-width:0}\
/* A directory name, so mono. `display:block` because `text-overflow` does \
   nothing to an inline box; a tight line keeps it by the facts under it. */\
.books .name{display:block;font-family:var(--mono);font-size:16px;line-height:1.3;\
color:var(--text);overflow:hidden;text-overflow:ellipsis;white-space:nowrap}\
/* `noda notebook ls`'s uncoloured `*`. Absolute, so every name starts at the \
   same x; `top` is padding plus half a line, not `50%`, which slides when the \
   facts wrap. */\
.books .mark{position:absolute;left:11px;top:23px;transform:translateY(-50%);\
width:7px;height:7px;border-radius:4px;background:var(--text)}\
.books .sr{position:absolute;width:1px;height:1px;overflow:hidden;clip-path:inset(50%)}\
/* `.rows` to beat `.rows .row .under`'s `flex-end`. */\
.rows.books .row .under{justify-content:flex-start;line-height:1.45;row-gap:0}\
.books .holds{color:var(--muted)}\
/* At 390px three facts need 250px of about 215 and wrap into loose bands, so \
   a phone drops the file count, and `:has` its separator. */\
@media (max-width:639px){\
.books .under .files,.books .under .sep:has(+ .files){display:none}}\
.books .aside .pill{display:inline-flex;align-items:center;gap:6px;min-height:32px;\
padding:0 11px;border:1px solid var(--rule);border-radius:999px;font-size:12px;\
white-space:nowrap}\
.books .aside svg{width:14px;height:14px;flex:0 0 auto;color:var(--muted)}\
.books a.aside:active .pill{background:var(--press)}\
.books .row .aside{align-items:center}\
.count .more,.books .stamp{display:none}\
@media (min-width:640px){\
.count .more{display:inline}\
/* The name column has a floor and a ceiling, for short and long names. */\
.books .row.split .most{display:grid;grid-template-columns:minmax(9em,15em) minmax(0,1fr);\
gap:20px;align-items:baseline;padding:15px 0 15px 36px}\
.books .mark{left:19px;top:25px}\
/* A floor under the chip, so `.most` is one width in every row. \
   `align-self:stretch` keeps the chip row-tall (a 48px target): at this width \
   `.rows .row{align-items:baseline}` outranks `.row.split`'s `stretch`. */\
.books .row.split .aside{min-width:15em;justify-content:flex-end;padding:0 24px;\
align-self:stretch}}\
/* The last commit's day, at the right where `-l` prints a date, where it fits. */\
@media (min-width:1024px){\
.books .row.split .most{grid-template-columns:minmax(9em,16em) minmax(0,1fr) auto}\
.books .stamp{display:block;color:var(--muted);font-size:12.5px;justify-self:end}}\
";

#[cfg(test)]
mod tests {
    use super::*;

    /// `.rows.cols` flowed the empty-state sentence across four columns.
    #[test]
    fn an_empty_screen_is_not_poured_into_columns() {
        let no_tags = tags("work", &[]);
        assert!(no_tags.contains("No tags yet"), "{no_tags}");
        assert!(no_tags.contains("<main class=\"rows\">"), "{no_tags}");

        let no_files = files("work", &[]);
        assert!(no_files.contains("No files yet"), "{no_files}");
        assert!(no_files.contains("<main class=\"rows\">"), "{no_files}");

        let some_tags = tags(
            "work",
            &[Tally {
                tag: "ops".into(),
                notes: 2,
            }],
        );
        assert!(
            some_tags.contains("<main class=\"rows cols\">"),
            "{some_tags}"
        );

        let some_files = files(
            "work",
            &[Held {
                name: "rack.png".into(),
                size: 4096,
                kind: "image/png".into(),
                used: 1,
            }],
        );
        assert!(
            some_files.contains("<main class=\"rows cols wide\">"),
            "{some_files}"
        );
    }

    #[test]
    fn a_note_cannot_write_markup_into_the_page() {
        let out = escape("<script>alert('x')</script> & \"quoted\"");
        assert!(!out.contains('<'), "{out}");
        assert!(!out.contains('>'), "{out}");
        assert_eq!(
            out,
            "&lt;script&gt;alert(&#39;x&#39;)&lt;/script&gt; &amp; &quot;quoted&quot;"
        );
    }

    /// A note's raw HTML is `render`'s to neutralise, and tested there.
    #[test]
    fn the_rendered_body_is_written_out_as_it_stands() {
        let page = note(
            "work",
            &Reading {
                id: "k3f9".into(),
                slug: "notes".into(),
                title: "Notes".into(),
                tags: vec![],
                created: None,
                updated: None,
                rendered: "<p>a <em>rendered</em> note</p>".into(),
                pinned: false,
            },
            "in sync",
        );
        assert!(page.contains("<p>a <em>rendered</em> note</p>"), "{page}");
    }

    #[test]
    fn a_listing_shows_the_day_and_never_a_clock() {
        assert_eq!(day("2019-03-14T16:21:00+08:00"), "2019-03-14");
        assert_eq!(day("2026-08-15T08:59:03Z"), "2026-08-15");
        // Not a shape noda wrote, so left as it is.
        assert_eq!(day("last tuesday"), "last tuesday");
        assert_eq!(day(""), "");
    }

    /// Both stamps whole, zone and all. Converting to the reader's zone is
    /// `script::STAMPS`'s job; this page owes it `datetime` and `data-clock`.
    #[test]
    fn the_note_page_prints_both_stamps_as_the_file_holds_them() {
        let page = note(
            "work",
            &Reading {
                id: "k3f9".into(),
                slug: "notes".into(),
                title: "Notes".into(),
                tags: vec![],
                created: Some("2026-08-12T08:03:00Z".into()),
                updated: Some("2026-08-15T09:54:23Z".into()),
                rendered: String::new(),
                pinned: false,
            },
            "in sync",
        );
        for (what, value) in [
            ("created", "2026-08-12T08:03:00Z"),
            ("updated", "2026-08-15T09:54:23Z"),
        ] {
            assert!(
                page.contains(&format!(
                    "{what} <time datetime=\"{value}\" data-clock>{value}</time>"
                )),
                "{page}"
            );
        }
        // The label is outside, so the script overwrites only the stamp.
        assert!(!page.contains(">created 20"), "{page}");
    }

    #[test]
    fn the_matched_run_is_marked_and_the_rest_is_escaped() {
        let out = highlight("Budget <review>", &["budget".to_string()]);
        assert_eq!(out, "<mark>Budget</mark> &lt;review&gt;");
        assert_eq!(highlight("a & b", &[]), "a &amp; b");
    }

    /// Nested `<mark>` would draw the overlap twice as dark.
    #[test]
    fn overlapping_terms_mark_one_run() {
        let out = highlight(
            "budgeting",
            &["budget".to_string(), "budgeting".to_string()],
        );
        assert_eq!(out, "<mark>budgeting</mark>");
    }

    fn row(id: &str, title: &str, shown: bool) -> Row {
        Row {
            id: id.into(),
            title: title.into(),
            tags: vec!["work".into()],
            stamp: Some("2026-08-12T08:03:00Z".into()),
            shown,
            pinned: false,
        }
    }

    /// One address for the default listing, so bookmarks keep working.
    #[test]
    fn the_default_order_writes_no_parameter_and_the_rest_write_one() {
        let page = listing(
            "work",
            &[row("k3f9", "Budget review", true)],
            &Asked::nothing(),
            Order::default(),
            "in sync",
            None,
        );
        assert!(page.contains("<nav class=\"sortbar\""), "{page}");
        // Pressing the one in force reverses it.
        assert!(page.contains("href=\"/nb/work?r=1\""), "{page}");
        assert!(page.contains("href=\"/nb/work?sort=created\""), "{page}");
        assert!(page.contains("href=\"/nb/work?sort=updated\""), "{page}");
        assert!(page.contains("href=\"/nb/work?sort=title\""), "{page}");
        assert!(!page.contains("name=\"sort\""), "{page}");
        assert!(!page.contains("name=\"r\""), "{page}");
    }

    /// `Sort::ALL` rather than four literals, so an order added to the CLI
    /// cannot quietly miss the browser.
    #[test]
    fn every_order_the_command_names_is_on_the_screen() {
        let page = listing(
            "work",
            &[],
            &Asked::nothing(),
            Order::default(),
            "in sync",
            None,
        );
        for sort in Sort::ALL {
            assert!(
                page.contains(&format!("<span class=\"pill\">{}", sort.name())),
                "the browser stopped offering {}: {page}",
                sort.name()
            );
        }
    }

    /// Pressing another order starts it unreversed: landing on the opposite of
    /// `--sort updated` is not what the chip looks like it will do.
    #[test]
    fn the_order_in_force_reverses_and_the_others_start_the_right_way_up() {
        let order = Order {
            sort: Sort::Updated,
            reversed: false,
        };
        let page = listing("work", &[], &Asked::nothing(), order, "in sync", None);
        assert!(
            page.contains("href=\"/nb/work?sort=updated&amp;r=1\""),
            "{page}"
        );
        assert!(page.contains("href=\"/nb/work?sort=title\""), "{page}");
        assert!(page.contains("href=\"/nb/work\""), "{page}");

        let back = listing(
            "work",
            &[],
            &Asked::nothing(),
            Order {
                sort: Sort::Updated,
                reversed: true,
            },
            "in sync",
            None,
        );
        assert!(back.contains("href=\"/nb/work?sort=updated\""), "{back}");
        assert!(!back.contains("sort=title&r=1"), "{back}");
        assert!(back.contains(UPWARDS), "{back}");
        assert!(!back.contains(DOWNWARDS), "{back}");
    }

    /// The chips carry what was typed and the form carries the order, since a
    /// `GET` form sends only its own fields.
    #[test]
    fn an_order_and_a_search_survive_each_other() {
        let order = Order {
            sort: Sort::Created,
            reversed: true,
        };
        let page = listing(
            "work",
            &[],
            &Asked {
                typed: "tag:q3 budget",
                ..Asked::nothing()
            },
            order,
            "in sync",
            None,
        );
        // Encoded as an address, not as markup: a query may hold an `&`.
        assert!(
            page.contains("href=\"/nb/work?q=tag%3Aq3%20budget&amp;sort=title\""),
            "{page}"
        );
        assert!(
            page.contains("<input type=\"hidden\" name=\"sort\" value=\"created\">"),
            "{page}"
        );
        assert!(
            page.contains("<input type=\"hidden\" name=\"r\" value=\"1\">"),
            "{page}"
        );
    }

    /// See [`Row::stamp`].
    #[test]
    fn a_listing_ordered_by_created_prints_created() {
        let file = NoteFile {
            id: "k3f9".into(),
            slug: "budget-review".into(),
            note: crate::note::Note {
                title: "Budget review".into(),
                tags: vec![],
                created: Some("2019-03-14T16:21:00+08:00".into()),
                updated: Some("2026-08-12T08:03:00Z".into()),
                pinned: None,
                extra: vec![],
                body: String::new(),
            },
        };
        assert_eq!(
            Row::of(&file, Sort::Created).stamp.as_deref(),
            Some("2019-03-14T16:21:00+08:00")
        );
        for sort in [Sort::Slug, Sort::Updated, Sort::Title] {
            assert_eq!(
                Row::of(&file, sort).stamp.as_deref(),
                Some("2026-08-12T08:03:00Z"),
                "{} stopped printing updated",
                sort.name()
            );
        }
    }

    /// The column is sent empty, so the order over it is left out too;
    /// `script::PANES` fetches both.
    #[test]
    fn a_note_page_is_sent_without_an_order_over_its_empty_column() {
        let reading = Reading {
            id: "em0xvn4e".into(),
            slug: "budget-review".into(),
            title: "Budget review".into(),
            tags: vec![],
            created: None,
            updated: None,
            rendered: "<p>late</p>".into(),
            pinned: false,
        };
        let page = note("work", &reading, "in sync");
        // The search field stays, as the scriptless way back to the listing.
        assert!(page.contains("<form class=\"searchbar\""), "{page}");
        assert!(!page.contains("class=\"sortbar\""), "{page}");
        assert!(!page.contains("name=\"sort\""), "{page}");

        let column = listing_pane(
            "work",
            &[row("k3f9", "Budget review", true)],
            &Asked::nothing(),
            Order {
                sort: Sort::Title,
                reversed: false,
            },
            "in sync",
        );
        assert!(column.contains("class=\"sortbar\""), "{column}");
        assert!(column.contains("name=\"sort\""), "{column}");
    }

    #[test]
    fn a_filtered_listing_says_what_it_is_hiding() {
        let rows = (0..12)
            .map(|n| row(&format!("k3f{n}"), "Budget review", false))
            .collect::<Vec<_>>();
        let page = listing(
            "work",
            &rows,
            &Asked {
                typed: "tag:ghost",
                ..Asked::nothing()
            },
            Order::default(),
            "in sync",
            None,
        );
        assert!(
            page.contains("No notes match <span class=\"asked\">tag:ghost"),
            "{page}"
        );
        assert!(page.contains("12 notes"), "{page}");
        assert!(page.contains("href=\"/nb/work\""), "{page}");
    }

    /// See [`Row::shown`].
    #[test]
    fn an_excluded_row_rides_along_hidden_and_unmarked() {
        let rows = [
            row("k3f9", "Budget review", true),
            row("em0x", "Reading list", false),
        ];
        let terms = ["budget".to_string()];
        let page = listing(
            "work",
            &rows,
            &Asked {
                typed: "budget",
                terms: &terms,
                ..Asked::nothing()
            },
            Order::default(),
            "in sync",
            None,
        );
        assert!(
            page.contains("<a class=\"row\" href=\"/nb/work/n/k3f9\""),
            "{page}"
        );
        assert!(
            page.contains("<a class=\"row\" hidden href=\"/nb/work/n/em0x\""),
            "{page}"
        );
        assert!(page.contains("<mark>Budget</mark>"), "{page}");
        assert!(page.contains(">Reading list</div>"), "{page}");
        assert!(page.contains(">1 of 2<"), "{page}");
    }

    #[test]
    fn an_empty_notebook_says_what_to_do_instead_of_nothing() {
        let page = listing(
            "work",
            &[],
            &Asked::nothing(),
            Order::default(),
            "in sync",
            None,
        );
        assert!(page.contains("No notes yet"), "{page}");
        assert!(page.contains("noda add"), "{page}");
        assert!(!page.contains("No notes match"), "{page}");
    }

    /// The script decides when the sentence applies, never what it says.
    #[test]
    fn the_hint_is_written_by_the_page_and_hidden_by_it() {
        let page = listing(
            "work",
            &[row("k3f9", "Budget review", true)],
            &Asked::nothing(),
            Order::default(),
            "in sync",
            None,
        );
        assert!(page.contains("<p class=\"hint\" hidden>"), "{page}");
        assert!(page.contains("press ⏎ to search the text"), "{page}");
        assert!(page.contains("<script src=\"/a/listing."), "{page}");
    }

    #[test]
    fn the_note_page_names_the_file() {
        let page = note(
            "work",
            &Reading {
                id: "em0xvn4e".into(),
                slug: "budget-review".into(),
                title: "Budget review".into(),
                tags: vec!["work".into()],
                created: Some("2026-08-12T08:03:00Z".into()),
                updated: Some("2026-08-15T16:59:00Z".into()),
                rendered: "late".into(),
                pinned: false,
            },
            "in sync",
        );
        assert!(page.contains(">em0xvn4e</span>"), "{page}");
        assert!(page.contains(">-budget-review</span>"), "{page}");
        assert!(page.contains(">.md</span>"), "{page}");
    }

    #[test]
    fn a_row_without_tags_prints_no_separator() {
        let rows = [Row {
            id: "k3f9".into(),
            title: "Reading list".into(),
            tags: vec![],
            stamp: Some("2026-08-12T08:03:00Z".into()),
            shown: true,
            pinned: false,
        }];
        let page = listing(
            "work",
            &rows,
            &Asked::nothing(),
            Order::default(),
            "in sync",
            None,
        );
        assert!(!page.contains("·"), "{page}");
        assert!(page.contains("2026-08-12"), "{page}");
        // The time of day is in the `datetime` attribute for the script, but
        // never in shown text.
        for shown in page
            .split('>')
            .skip(1)
            .filter_map(|rest| rest.split_once('<'))
        {
            assert!(!shown.0.contains("08:03"), "a clock is on the page: {page}");
        }
    }

    /// The day is written twice because only one of its places is ever shown.
    #[test]
    fn a_row_prints_the_columns_ls_dash_l_prints_in_that_order() {
        let page = listing(
            "work",
            &[row("em0xvn4e", "Budget review", true)],
            &Asked::nothing(),
            Order::default(),
            "in sync",
            None,
        );
        assert!(
            page.contains(
                "<div class=\"ident\"><span class=\"id\">em0xvn4e</span>\
                 <span class=\"day\">2026-08-12</span></div>\
                 <div class=\"title\">Budget review</div>"
            ),
            "{page}"
        );
        let under = page.split("<div class=\"under\">").nth(1).unwrap();
        assert!(
            under.starts_with(
                "<time class=\"when\" datetime=\"2026-08-12T08:03:00Z\">2026-08-12</time>\
                 <span class=\"sep\">·</span><span class=\"tags\">work</span>"
            ),
            "{under}"
        );
    }

    /// `a OR b c` is `(a OR b) AND c`, in the reader's own words.
    #[test]
    fn the_field_draws_the_grouping_it_arrived_at() {
        let grouping = [
            vec!["tag:work".to_string(), "tag:q3".to_string()],
            vec!["budget".to_string()],
        ];
        let page = listing(
            "work",
            &[row("k3f9", "Budget review", true)],
            &Asked {
                typed: "tag:work OR tag:q3 budget",
                grouping: &grouping,
                ..Asked::nothing()
            },
            Order::default(),
            "in sync",
            None,
        );
        assert!(
            page.contains(
                "<div class=\"parse\">\
                 <span class=\"g\"><b class=\"t\">tag:work</b><i>or</i><b class=\"t\">tag:q3</b></span>\
                 <span class=\"and\">and</span>\
                 <span class=\"g\"><b>budget</b></span></div>"
            ),
            "{page}"
        );
    }

    /// The box is written anyway, for the hint's reason.
    #[test]
    fn a_line_that_is_not_a_query_yet_is_grouped_into_nothing() {
        let page = listing(
            "work",
            &[row("k3f9", "Budget review", true)],
            &Asked {
                typed: "budget OR",
                problem: Some("`OR` needs a term on both sides"),
                ..Asked::nothing()
            },
            Order::default(),
            "in sync",
            None,
        );
        assert!(
            page.contains("<div class=\"parse\" hidden></div>"),
            "{page}"
        );
        assert!(
            page.contains("<p class=\"problem\">`OR` needs a term on both sides</p>"),
            "{page}"
        );
    }

    /// The box is there for the script; the count is not, as no rows are sent.
    #[test]
    fn a_note_page_sends_the_search_field_with_nothing_asked_of_it() {
        let page = note(
            "work",
            &Reading {
                id: "em0xvn4e".into(),
                slug: "budget-review".into(),
                title: "Budget review".into(),
                tags: vec![],
                created: None,
                updated: None,
                rendered: String::new(),
                pinned: false,
            },
            "in sync",
        );
        assert!(
            page.contains("<div class=\"parse\" hidden></div>"),
            "{page}"
        );
        assert!(page.contains("<span class=\"count\"></span>"), "{page}");
    }

    #[test]
    fn a_grouping_cannot_write_markup_into_the_page() {
        let grouping = [vec!["<script>x</script>".to_string()]];
        let page = listing(
            "work",
            &[],
            &Asked {
                typed: "<script>x</script>",
                grouping: &grouping,
                ..Asked::nothing()
            },
            Order::default(),
            "in sync",
            None,
        );
        assert!(page.contains("&lt;script&gt;x&lt;/script&gt;"), "{page}");
        assert!(!page.contains("<script>x"), "{page}");
    }

    #[test]
    fn every_page_names_the_viewport_and_links_the_sheet_that_holds_both_themes() {
        let page = listing(
            "work",
            &[],
            &Asked::nothing(),
            Order::default(),
            "in sync",
            None,
        );
        assert!(page.contains("width=device-width"), "{page}");
        assert!(
            page.contains("<link rel=\"stylesheet\" href=\"/a/style."),
            "{page}"
        );
        let sheet = format!("{}{}", crate::web::theme::stylesheet(), stylesheet());
        assert!(sheet.contains("prefers-color-scheme:dark"), "{sheet}");
        assert!(sheet.contains("--tap:48px"), "{sheet}");
    }

    fn reading() -> Reading {
        Reading {
            id: "em0xvn4e".into(),
            slug: "budget-review".into(),
            title: "Budget review".into(),
            tags: vec![],
            created: None,
            updated: None,
            rendered: "late".into(),
            pinned: false,
        }
    }

    /// The fragment contract: each part is contained in the page it was cut
    /// from, so the two cannot disagree.
    #[test]
    fn a_fragment_is_a_piece_of_the_page_it_came_from() {
        let (title, pane) = note_pane("work", &reading())
            .split_once("</title>")
            .map(|(title, pane)| (format!("{title}</title>"), pane.to_string()))
            .expect("the reading fragment names the tab");
        let page = note("work", &reading(), "in sync");
        assert!(page.contains(&title), "{page}");
        assert!(page.contains(&pane), "{page}");

        let rows = [Row {
            id: "em0xvn4e".into(),
            title: "Budget review".into(),
            tags: vec!["work".into()],
            stamp: None,
            shown: true,
            pinned: false,
        }];
        let column = listing_pane(
            "work",
            &rows,
            &Asked::nothing(),
            Order::default(),
            "in sync",
        );
        assert!(
            listing(
                "work",
                &rows,
                &Asked::nothing(),
                Order::default(),
                "in sync",
                None
            )
            .contains(&column),
            "{column}"
        );

        let subject = Subject {
            what: "Budget review".into(),
            at: "/nb/work/n/em0xvn4e".into(),
            mono: false,
        };
        let answer = backlinks_rows("work", &subject, &rows);
        assert!(
            backlinks("work", &subject, &rows).contains(&answer),
            "{answer}"
        );

        let news = standing_main("work", &still(), None);
        assert!(standing("work", &still(), None).contains(&news), "{news}");

        let both = listing_screen(
            "work",
            &rows,
            &Asked::nothing(),
            Order::default(),
            "in sync",
            Some("<p>Read me</p>"),
        );
        let (title, panes) = both
            .split_once("</title>")
            .map(|(title, panes)| (format!("{title}</title>"), panes.to_string()))
            .expect("the listing screen names the tab");
        let page = listing(
            "work",
            &rows,
            &Asked::nothing(),
            Order::default(),
            "in sync",
            Some("<p>Read me</p>"),
        );
        assert!(page.contains(&title), "{page}");
        assert!(page.contains(&panes), "{page}");
        assert!(panes.contains("class=\"pane index\""), "{panes}");
        assert!(panes.contains("class=\"pane read\""), "{panes}");
        assert!(panes.contains("Read me"), "{panes}");
    }

    #[test]
    fn a_fragment_carries_none_of_the_page_around_it() {
        let fragment = note_pane("work", &reading());
        for absent in [
            "<!doctype",
            "<link rel=\"stylesheet\"",
            "<script",
            "class=\"pane index\"",
            "class=\"notebooks\"",
        ] {
            assert!(!fragment.contains(absent), "the fragment carries {absent}");
        }
        assert!(fragment.contains("class=\"pane read\""), "{fragment}");

        let page = note("work", &reading(), "in sync");
        assert!(fragment.len() < page.len(), "{fragment}");
        // A declaration only the stylesheet holds.
        assert!(
            !page.contains("--tap:48px"),
            "the stylesheet is back inside the page"
        );
        assert!(
            page.contains("<link rel=\"stylesheet\" href=\"/a/style."),
            "{page}"
        );
    }

    /// Whether to poll again is the server's decision, carried in the fragment
    /// by the same `<meta>` the whole page has.
    #[test]
    fn the_news_says_whether_to_come_back_the_way_the_page_does() {
        let running = Errand {
            doing: "Syncing",
            done: "Synced",
            said: None,
            failed: false,
            seconds: 3,
        };
        let news = standing_main("work", &still(), Some(&running));
        assert!(news.starts_with("<meta http-equiv=\"refresh\""), "{news}");
        assert!(
            standing("work", &still(), Some(&running)).contains("<meta http-equiv=\"refresh\""),
            "the whole page stopped asking to come back"
        );

        let quiet = standing_main("work", &still(), None);
        assert!(!quiet.contains("http-equiv"), "{quiet}");
        assert!(quiet.starts_with("<main>"), "{quiet}");
    }

    /// Inside the `<main>`, so it survives the poll's swap.
    #[test]
    fn the_status_screen_names_the_build_answering() {
        let expected = format!(
            "<div class=\"name\">Version</div>\
             <div class=\"under\"><span class=\"mono\">{}</span></div>",
            escape(crate::VERSION)
        );
        let page = standing("work", &still(), None);
        assert!(page.contains(&expected), "{page}");
        let news = standing_main("work", &still(), None);
        assert!(news.contains(&expected), "{news}");
    }

    fn still() -> Standing {
        Standing {
            branch: "main".into(),
            notes: 5,
            files: 0,
            uncommitted: 0,
            remote: Some("https://example.com/notes.git".into()),
            drift: "in sync".into(),
            problems: vec![],
        }
    }

    /// Filling it walks every note, so that happens only where it is drawn.
    #[test]
    fn the_note_page_sends_the_margin_note_empty_and_hidden() {
        let page = note("work", &reading(), "in sync");
        assert!(page.contains("<aside class=\"beside\" hidden>"), "{page}");
        assert!(page.contains("<div class=\"answer\"></div>"), "{page}");
        assert!(page.contains(">Backlinks</div>"), "{page}");
    }

    /// `display:block` on a class chain outranks `hidden`, so the rule must
    /// exempt a closed margin.
    #[test]
    fn the_margin_note_is_drawn_only_when_it_holds_something() {
        let sheet = stylesheet();
        assert!(sheet.contains(".beside{display:none}"), "{sheet}");
        assert!(
            sheet.contains(".read .beside:not([hidden]){display:block"),
            "the margin note can now out-order its own hidden attribute"
        );
    }

    #[test]
    fn the_note_bar_carries_delete_and_marks_only_that() {
        let page = note("work", &reading(), "in sync");
        assert!(
            page.contains("/n/em0xvn4e/delete\" class=\"danger\""),
            "{page}"
        );
        assert_eq!(page.matches("class=\"danger\"").count(), 1, "{page}");
        assert!(page.contains("<span>Delete</span>"), "{page}");
        // The old link below the prose is gone.
        assert!(!page.contains("perilous"), "{page}");
    }

    /// Inside the form, `.said` was inset twice.
    #[test]
    fn the_delete_page_says_its_piece_from_outside_the_form() {
        let about = About::of("em0xvn4e", "budget-review", "Budget review");
        let page = deleting("work", &about);
        let said = page.find("class=\"said\"").expect("the page says nothing");
        let form = page.find("<form").expect("the page has no form");
        assert!(said < form, "the paragraph is back inside the form: {page}");
    }

    /// The README stands in the same pane with no margin, and would be shoved
    /// 236px off centre.
    #[test]
    fn the_wide_grid_asks_for_a_note_before_it_reserves_a_margin() {
        let sheet = stylesheet();
        assert!(sheet.contains("@media (min-width:1440px)"), "{sheet}");
        for rule in sheet.split('}') {
            if rule.contains("main.note") && rule.contains("grid-template-columns") {
                assert!(
                    rule.contains("at-note.margined"),
                    "the wide grid applies to a pane that may hold the README: {rule}"
                );
            }
        }
    }

    /// The listing too, because picking a row turns it into a note page.
    #[test]
    fn both_routes_that_can_show_a_note_link_the_script_that_fills_its_margin() {
        let hook = Asset::Beside.href();
        assert!(note("work", &reading(), "in sync").contains(hook), "note");
        assert!(
            listing(
                "work",
                &[],
                &Asked::nothing(),
                Order::default(),
                "in sync",
                None
            )
            .contains(hook),
            "listing"
        );
    }

    /// Not the tags screen: its `.when` is a count of notes.
    #[test]
    fn the_screens_that_show_an_instant_link_the_script_that_converts_it() {
        let hook = Asset::Stamps.href();
        assert!(note("work", &reading(), "in sync").contains(hook), "note");
        assert!(
            listing(
                "work",
                &[],
                &Asked::nothing(),
                Order::default(),
                "in sync",
                None
            )
            .contains(hook),
            "listing"
        );
        assert!(!tags("work", &[]).contains(hook), "tags");
    }

    /// A `due:` is a calendar day, and converting it could move it a day.
    #[test]
    fn a_due_date_is_not_an_instant_and_is_not_marked_as_one() {
        let page = todo(
            "work",
            &[Task {
                id: "em0xvn4e".into(),
                title: "Budget review".into(),
                text: "send the revised contract".into(),
                due: Some("2026-08-10".into()),
                overdue: false,
            }],
        );
        assert!(page.contains("2026-08-10"), "{page}");
        assert!(!page.contains("<time"), "{page}");
        assert!(!page.contains(Asset::Stamps.href()), "{page}");
    }

    fn book(name: &str) -> Book {
        Book {
            name: name.to_string(),
            notes: 10,
            files: 2,
            uncommitted: 1,
            drift: Some("2 to push".to_string()),
            active: false,
            last: "2026-08-18".to_string(),
        }
    }

    #[test]
    fn a_notebook_row_says_what_status_says_and_leads_two_ways() {
        let page = notebooks(&[book("work")]);
        assert!(page.contains("href=\"/nb/work\""), "{page}");
        assert!(page.contains("href=\"/nb/work/status\""), "{page}");
        for fact in [
            "10 notes",
            "2 files",
            "1 uncommitted",
            "2026-08-18",
            "2 to push",
        ] {
            assert!(page.contains(fact), "{fact} is missing:\n{page}");
        }
    }

    #[test]
    fn a_notebook_row_leaves_out_what_it_holds_none_of() {
        let page = notebooks(&[Book {
            files: 0,
            uncommitted: 0,
            ..book("work")
        }]);
        assert!(page.contains("10 notes"), "{page}");
        assert!(!page.contains("0 files"), "{page}");
        assert!(!page.contains("uncommitted"), "{page}");
    }

    #[test]
    fn a_notebook_with_no_remote_keeps_the_words_and_loses_the_link() {
        let page = notebooks(&[Book {
            drift: None,
            ..book("work")
        }]);
        assert!(page.contains("no remote"), "{page}");
        assert!(!page.contains("/status"), "{page}");
    }

    #[test]
    fn the_active_notebook_is_marked_and_the_others_are_not() {
        let page = notebooks(&[
            book("journal"),
            Book {
                active: true,
                ..book("work")
            },
        ]);
        assert_eq!(page.matches("class=\"mark\"").count(), 1, "{page}");
        assert!(page.contains("<span class=\"sr\">Active"), "{page}");
    }

    #[test]
    fn the_front_page_is_the_one_screen_with_neither_rail_nor_bar() {
        let page = notebooks(&[book("work")]);
        assert!(page.contains("class=\"app root\""), "{page}");
        assert!(!page.contains("<nav class=\"actionbar\""), "{page}");
        assert!(!page.contains("class=\"fab\""), "{page}");
    }

    #[test]
    fn the_corner_counts_the_notebooks_and_leaves_the_rest_to_the_stylesheet() {
        let page = notebooks(&[book("journal"), book("work")]);
        assert!(
            page.contains("2 notebooks<span class=\"more\"> · 20 notes</span>"),
            "{page}"
        );
    }
}
