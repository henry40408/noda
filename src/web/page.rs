//! The HTML, and nothing else.
//!
//! Every function takes what a page is about and returns a string. Nothing opens
//! a repository, binds a socket or knows what a request is — `tui/app.rs`'s rule,
//! for its reason: what an interface puts on the screen is worth testing without
//! one.
//!
//! **The type system is the palette's argument, said in type instead of
//! colour.** The machinery — ids, filenames, tags, stamps — is set in the
//! monospace a terminal would have used, and what is the reader's own is set to
//! be read. `style.rs` draws the line in exactly the same place; a terminal has
//! one face and cannot show it.
//!
//! No JavaScript: the search field is a form and every row is a link. Filtering
//! as you type is an enhancement over that, never a replacement for it.

use std::fmt::Write;

use crate::cmd::Sort;
use crate::notebook::NoteFile;
use crate::web::asset::Asset;
use crate::web::encoded;

/// `noda status` compressed to a row, the way a row is `ls -l` compressed.
///
/// Every field but `last` is already in `Status`, and `last` is one commit read
/// — which is the only reason a page that already walks every notebook can
/// afford to say more about each.
pub struct Book {
    pub name: String,
    pub notes: usize,
    /// Shown only when there are any, as `noda status` prints the line.
    pub files: usize,
    /// The one fact on the row about something to do rather than something
    /// held.
    pub uncommitted: usize,
    /// Already in words: a count is not what anybody wants to be told about a
    /// remote they have not set up. `None` means no remote — the one field whose
    /// cell is not a link.
    pub drift: Option<String>,
    /// The one `noda notebook ls` puts a `*` beside.
    pub active: bool,
    /// The day of its last commit, already rendered by `cmd::format_time`.
    pub last: String,
}

/// `ls -l`'s row minus the slug and the created stamp, both of which are one
/// press away on the note's own page. What is left is id, title, day and tags,
/// **in that order**, because that is `ls -l`'s order.
///
/// The id was left out until a screen turned up with room: on a phone the next
/// thing you do is press the row rather than type the id, but that is an
/// argument about space and a monitor has it. What it buys back is the
/// notebook's own vocabulary — the id on screen is the one you say to
/// `noda show`. So it is written on every row and shown where it fits, which is
/// the layout's habit in one element.
pub struct Row {
    pub id: String,
    pub title: String,
    pub tags: Vec<String>,
    /// The one stamp the row has room for, and **which one follows the order the
    /// listing is in**.
    ///
    /// A `created` listing still printing `updated` is a column of days in no
    /// discernible order beside a list claiming to be sorted, and a reader
    /// cannot tell that from a broken sort. So the stamp shown is the stamp
    /// sorted by; `slug` and `title` keep `updated`.
    ///
    /// Not called `updated` for that reason: it should not be possible to read
    /// this as the note's `updated` now that it is sometimes the other one.
    pub stamp: Option<String>,
    /// **Every row of the notebook is on the listing whatever is typed** — the
    /// excluded ones arrive `hidden` rather than left out, because a script
    /// cannot put back a row the server never sent, and a second copy to filter
    /// *from* is the copy that goes stale.
    ///
    /// `hidden` is not a class but the attribute every browser's own stylesheet
    /// already hides, so the scriptless page needs nothing of noda's.
    pub shown: bool,
}

/// A file the notebook holds that is not a note, as the files page lists it.
pub struct Held {
    pub name: String,
    pub size: u64,
    /// The same answer the download carries, so the page cannot promise one
    /// thing and the file be another.
    pub kind: String,
    /// How many notes link to it. Zero is what `doctor --links` calls an orphan.
    pub used: usize,
}

/// One tag, and how many notes carry it.
pub struct Tally {
    pub tag: String,
    pub notes: usize,
}

/// Named by title rather than filename: this is a list of things to do, and
/// which file a task is in is a second question. The id is still the address.
pub struct Task {
    pub id: String,
    pub title: String,
    /// The item's own words, with the `due:` term already lifted out of them.
    pub text: String,
    pub due: Option<String>,
    /// Against the reader's own day, which only the server knows.
    pub overdue: bool,
}

/// What a backlinks page is about: a note, or one of the notebook's files.
pub struct Subject {
    /// What to call it — a note's title, a file's name.
    pub what: String,
    /// Where it is, for the way back.
    pub at: String,
    /// A filename is monospace wherever it appears, and a title never is.
    pub mono: bool,
}

/// A note, as its own page shows it.
pub struct Reading {
    pub id: String,
    pub slug: String,
    pub title: String,
    pub tags: Vec<String>,
    /// `ls -l` has never printed it — a listing is about what changed — but a
    /// note's own page has room for "how long has this been here".
    pub created: Option<String>,
    pub updated: Option<String>,
    /// **Already HTML**, from `web::render::body` — the one string on these
    /// pages written out as it stands. Not called `body` for that reason:
    /// `escape(&reading.body)` was right while a note was shown as text.
    pub rendered: String,
}

impl Row {
    /// `by` decides one thing, [`Row::stamp`]. A page that is not a listing
    /// passes `Sort::default()` and gets `updated`, which is also the truth: a
    /// backlinks answer comes back in slug order.
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
        }
    }
}

/// Hand-written: five characters, reached by arbitrary text on every page, so
/// the one place it happens should be readable in one screen.
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

/// The day a stamp names: `2026-08-15`.
///
/// **The day and not the minute, because the minute cannot be shown without
/// lying.** noda writes UTC and a rendering uses the recorded offset, so a note
/// written at six in the evening shows as ten in the morning — and with the `Z`
/// cut off to save room it shows as ten in the morning *and looks local*.
/// Converting would put the server's zone into a fact about the note.
///
/// A day has none of that, and is what a listing wants: a row answers "when did
/// I last touch this". The minute is on the note's own page, `Z` and all.
/// Anything that is not a date comes back untouched.
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
/// would search `&amp;` for `&`, and marking without escaping would put a note's
/// own angle brackets into the markup.
fn highlight(text: &str, terms: &[String]) -> String {
    if terms.is_empty() {
        return escape(text);
    }
    let haystack = text.to_lowercase();
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    while at < text.len() {
        // The earliest match wins, and the longest of the ones starting there,
        // so two terms overlapping mark one run rather than nesting.
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

/// A tag list, in the shape and the colours a listing gives it.
///
/// `style::tag_pieces` decides where the cuts fall — the brackets are not the
/// part you read, the tags are. That is the same judgement here, so it is the
/// same function: the brackets are dropped on a page that has room to separate
/// things by position, and the join keeps the comma the CLI uses.
fn tag_line(tags: &[String]) -> String {
    if tags.is_empty() {
        return String::new();
    }
    format!("<span class=\"tags\">{}</span>", escape(&tags.join(", ")))
}

/// What every page is wrapped in.
///
/// The stylesheet is linked rather than carried, and `asset.rs` is where that
/// decision is argued — it used to be argued here, the other way round.
///
/// `app` is the classes the layout hangs off, and there are only ever three of
/// them: `split` for the two screens made of panes, `at-list`/`at-note` for
/// which of the two is being shown, and `indexed` for whether the index pane
/// arrived with rows in it. Every one is a fact about the route, decided by the
/// handler that knows it and never worked out again from the markup.
fn shell(title: &str, app: &str, body: &str) -> String {
    dressed(title, app, None, &[], body)
}

/// A page links the scripts it uses and no others — the inline version's rule,
/// kept now that they are addresses.
///
/// `defer` is what an address buys that inline could not: they read the rows, so
/// they must run after parsing, and a deferred script downloads while parsing is
/// still going.
fn scripted(title: &str, app: &str, scripts: &[Asset], body: &str) -> String {
    dressed(title, app, None, scripts, body)
}

/// The shell, plus the one thing a page may ask the browser to do on its own.
///
/// `<meta http-equiv="refresh">` is how a scriptless page comes back for news,
/// and the network screen is the only page with any. A few hundred bytes
/// reloaded, and the same reload a reader would do by hand.
///
/// The `referrer` meta is the second of three places noda says that an address
/// here is somebody's note id and does not travel: `web::html` says it as a
/// header a proxy may strip, this says it where nothing can, and `web::render`
/// says it on the links. This is also the copy that reaches an image a note
/// embeds from elsewhere, which no attribute on a link would cover.
///
/// `same-origin` rather than `no-referrer`, for the reason `web::html` gives:
/// the stricter one nulls a form post's `Origin`, which `web::guard` needs.
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

/// Written here rather than inline in [`dressed`], because a fragment sends them
/// too.
///
/// **A fragment is a page with the parts the reader already has left out**, and
/// what is left is never only the body: a swap renames the tab, and the polling
/// screen steers by the same refresh. Both live in the `<head>`, so a fragment
/// leads with them — and because an HTML parser puts a leading `<title>` in the
/// head of whatever it parses, the script finds each where it looks on a whole
/// page.
///
/// One function each, called from both sides, so the shorter answer cannot come
/// to differ by a character.
fn titled(title: &str) -> String {
    format!("<title>{}</title>", escape(title))
}

fn refresh(again_in: Option<u32>) -> String {
    again_in.map_or_else(String::new, |seconds| {
        format!("<meta http-equiv=\"refresh\" content=\"{seconds}\">\n")
    })
}

/// Kept as the strings that were typed, because its other job is to be handed
/// back when the write is refused: a reader told a tag is not allowed should
/// find their words where they left them.
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

/// Three states rather than two flags: a pair of booleans makes room for the
/// combination that means nothing — the screen you are standing on, in the
/// colour of the thing that removes it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mark {
    /// Somewhere to go, and nothing further to say about it.
    Plain,
    /// The screen the bar is being drawn on.
    Here,
    /// The one item that cannot be undone by doing it again.
    Danger,
}

impl Mark {
    /// The shape every caller had before there was a third state.
    fn at(here: bool) -> Mark {
        if here { Mark::Here } else { Mark::Plain }
    }
}

/// Shipped only once there was something in every slot: two greyed-out buttons
/// are not a design. A fixed strip at the foot of a page is an extension and not
/// a rearrangement, which is the rule this project applies to a listing's row.
fn action_bar(items: &[(&str, &str, String, Mark)]) -> String {
    let mut out = String::from("<nav class=\"actionbar\">");
    for (icon, label, href, mark) in items {
        let _ = write!(
            out,
            "<a href=\"{}\"{}><svg viewBox=\"0 0 24 24\" aria-hidden=\"true\">{icon}</svg>\
             <span>{label}</span></a>",
            escape(href),
            // `aria-current` and not a class: a screen reader says "current
            // page" and the stylesheet hangs the colour off the same fact.
            //
            // Danger is a class, because there is nothing to announce that the
            // item does not already say — the word is Delete, and the colour is
            // for a reader aiming rather than reading.
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
/// A page with a folded corner. Not a book: a notebook is a directory of files,
/// and this reaches a list of them.
const NOTES: &str = "<path d=\"M5.5 4.5h9L19 9v10.5h-13.5z\"/><path d=\"M14.5 4.5V9H19\"/>\
<path d=\"M9 13h6\"/><path d=\"M9 16.5h4\"/>";
const EDIT: &str = "<path d=\"M4 20h4L19 9l-4-4L4 16z\"/>";
const TAGS: &str = "<path d=\"M4 4h7l9 9-7 7-9-9z\"/><circle cx=\"8\" cy=\"8\" r=\"1.4\"/>";
const RENAME: &str = "<path d=\"M4 7V5h16v2\"/><path d=\"M12 5v14\"/><path d=\"M9 19h6\"/>";
/// The GFM checkbox every other Markdown reader draws.
const TODO: &str = "<rect x=\"4\" y=\"4\" width=\"16\" height=\"16\" rx=\"3.5\"/><path d=\"M8.5 12.2l2.6 2.6 4.6-5.4\"/>";
/// A paperclip: the shape everything on a phone uses for an attachment.
const FILES: &str = "<path d=\"M18.5 10.5 11 18a4 4 0 0 1-5.7-5.7l7.8-7.8a2.6 2.6 0 0 1 3.7 3.7\
 l-7.7 7.7a1.2 1.2 0 0 1-1.7-1.7l7.1-7.1\"/>";
/// An arrow arriving at a line: the line is the note, the arrow is what points
/// at it.
const LINKS: &str = "<path d=\"M19 5v14\"/><path d=\"M4 12h11\"/><path d=\"M11 8l4 4-4 4\"/>";
/// Drawn in the same stroke as the rest: the odd one out by colour, and being
/// odd by weight too would read as a mistake rather than a warning.
const TRASH: &str = "<path d=\"M5 7h14\"/><path d=\"M9.5 7V4.5h5V7\"/>\
<path d=\"M6.5 7l1 12.5h9L17.5 7\"/><path d=\"M10 10.5v6\"/><path d=\"M14 10.5v6\"/>";
/// Two arrows passing. Not a cloud: a notebook syncs with a repository on
/// somebody's machine, and half the time that machine is their own.
const SYNC: &str = "<path d=\"M7 9l5-5 5 5\"/><path d=\"M12 4v10\"/>\
<path d=\"M17 15l-5 5-5-5\"/><path d=\"M12 20V10\"/>";

/// Three lines getting shorter — the glyph every interface with a sort has
/// settled on. The word `order` reads better and costs 55px, which is four chips
/// fitting the index column against three and a wrap.
const ORDER: &str = "<path d=\"M4 7h13\"/><path d=\"M4 12h9\"/><path d=\"M4 17h5\"/>";

/// Down is `--sort`'s order, up is the same under `-r`. Not "ascending" and
/// "descending": `updated` runs newest-first and `title` A-to-Z, so one arrow
/// would have to mean opposite things.
const DOWNWARDS: &str = "<path d=\"M12 5v14\"/><path d=\"M6 13l6 6 6-6\"/>";
const UPWARDS: &str = "<path d=\"M12 19V5\"/><path d=\"M6 11l6-6 6 6\"/>";

/// The way to the network screen, and also the answer that screen exists to
/// give: "is there anything to sync" is worth knowing without pressing anything.
///
/// The pill is what is drawn and the link around it is what is pressed — nothing
/// on a phone may be under 48px, and a 48px pill in a 56px bar would be wedged
/// in. The label repeats the words because a narrow screen may end the text in
/// an ellipsis and a label never does.
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

/// Inline SVG rather than a character like `‹`, whose weight, size and position
/// on the line are whatever the reader's font decides. It is also why this can
/// be given a stroke width.
const BACK: &str =
    "<svg viewBox=\"0 0 24 24\" aria-hidden=\"true\"><path d=\"M15 4.5 7.5 12 15 19.5\"/></svg>";

/// Which of the notebook's screens is being drawn, so the bar can say so.
///
/// All four are on it. An earlier design left `Notes` off, on the argument that
/// the bar held the places you go *from* the listing — but a rail reads as a
/// list of where you can be, and on a wide screen the listing is not somewhere
/// you leave.
#[derive(Clone, Copy, PartialEq, Eq)]
enum At {
    Notes,
    Tags,
    Todo,
    Files,
    /// The one notebook screen not on the bar, reached from the chip because it
    /// is about the notebook as a whole. A variant rather than an absence, so
    /// the bar is told where the reader is on every screen that carries it.
    Status,
}

/// **Four places and one action, told apart by not being in the same row.**
/// Notes, Tags, Todo and Files are somewhere to go; New is something to do, and
/// a row mixing the two is a row you read rather than aim at.
///
/// The same four on every screen, because a bar whose contents changed would be
/// worse than no bar.
fn notebook_bar(book: &str, here: At) -> String {
    let at = escape(book);
    // One of the four is where you are and the rest are somewhere to go. The
    // third state a `Mark` holds is a note's, and none of these is ever it.
    let mark = |screen: At| Mark::at(here == screen);
    // The wrapper sticks, not the bar inside it: on a short page the bar sits
    // under the last row, and a button pinned to the window would float below.
    format!(
        "<div class=\"foot\">{}<a class=\"fab\" href=\"/nb/{at}/new\" aria-label=\"New note\">\
         <svg viewBox=\"0 0 24 24\" aria-hidden=\"true\">{NEW}</svg></a></div>",
        action_bar(&[
            (NOTES, "Notes", format!("/nb/{at}"), mark(At::Notes)),
            (TAGS, "Tags", format!("/nb/{at}/tags"), mark(At::Tags)),
            (TODO, "Todo", format!("/nb/{at}/todo"), mark(At::Todo)),
            (FILES, "Files", format!("/nb/{at}/files"), mark(At::Files)),
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

/// **The one screen not inside a notebook**, which is why it carries neither the
/// rail nor the bar — both hold places inside one. `root` says so to the
/// stylesheet, and stops the layout reserving a column for a rail.
pub fn notebooks(books: &[Book]) -> String {
    let rows = if books.is_empty() {
        // An invitation to act, and the act is at a terminal.
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

/// The second clause rides in a `.more` span the stylesheet drops on a phone —
/// the listing's row does the same with its day. The server does not know how
/// wide the screen is, and asking is worse than a dozen unshown bytes.
fn tally(books: &[Book]) -> String {
    let notes = books.iter().map(|book| book.notes).sum();
    format!(
        "{}<span class=\"more\"> · {}</span>",
        plural(books.len(), "notebook"),
        plural(notes, "note")
    )
}

/// **Two destinations, side by side rather than nested**, as the files page
/// does it: the row goes to the listing, the chip to the network screen. A
/// notebook with no remote keeps the words and loses the link, a press whose
/// answer is what you already read not being worth having.
fn book_row(book: &Book) -> String {
    let at = escape(&book.name);
    // `*` in `noda notebook ls`, a dot here — and words for a screen reader.
    let mark = if book.active {
        "<span class=\"mark\" aria-hidden=\"true\"></span><span class=\"sr\">Active — </span>"
    } else {
        ""
    };

    // `.when` is a timestamp everywhere else, and a test helper reads by it.
    let mut facts = vec![format!(
        "<span class=\"holds\">{}</span>",
        plural(book.notes, "note")
    )];
    if book.files > 0 {
        // Classed, because a phone drops this one: three facts do not fit
        // beside the chip at 390px, and the file count is the one of the three
        // that keeps least. The stylesheet is where that is argued.
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
        // `.row.split .aside` already sets the muted colour and the size, so
        // the words need nothing of their own.
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
///
/// One argument rather than four because they are one thing: four answers to
/// the same line, only ever right together. A page holding the grouping of one
/// query and the complaint about another is not a state worth being able to
/// construct.
pub struct Asked<'a> {
    /// The line exactly as it arrived, which is what goes back in the field.
    pub typed: &'a str,
    /// The grouping it parsed to — `query::Query::grouping`. Empty when the
    /// line is not a query yet, which is the same state as nothing typed as far
    /// as this page is concerned: there is no grouping to show either way.
    pub grouping: &'a [Vec<String>],
    /// What is worth marking in a title, from the query that has all of it.
    pub terms: &'a [String],
    /// Why what has been typed is not a query yet. It is said and the notes are
    /// left alone — the same call `style::INVALID` exists for in the browser:
    /// half a query is what every query looks like on the way to being one, and
    /// emptying the screen over an unfinished thought is not an answer.
    pub problem: Option<&'a str>,
}

impl Asked<'_> {
    /// A page with no search on it. What a note route sends: the field is there
    /// and empty, because a reader who narrows from a note lands on the
    /// listing, and nothing has been asked yet.
    pub fn nothing() -> Self {
        Self {
            typed: "",
            grouping: &[],
            terms: &[],
            problem: None,
        }
    }
}

/// Two fields rather than one enum of eight, because that is what they are at
/// the prompt: an order, and a reversal applied after it. Folding them makes
/// `-r` a property of each order, and the eight-way match is a decision nobody
/// made.
#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
pub struct Order {
    pub sort: Sort,
    pub reversed: bool,
}

impl Order {
    /// The query string this order is asked for by, `q` carried through it.
    ///
    /// **The default order writes nothing**, so `/nb/work` keeps meaning what it
    /// meant and there is one address for the default listing rather than one
    /// plain and one spelled out.
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

/// A notebook's notes, narrowed by whatever was typed.
///
/// `total` matters only when it differs from how many are shown: a reader who
/// filtered to nothing needs telling there is something to go back to.
///
/// `drift` rides in the corner of the bar as a link to the network screen, which
/// is not on the bottom bar because that holds places *inside* the notebook.
pub fn listing(
    book: &str,
    rows: &[Row],
    asked: &Asked<'_>,
    order: Order,
    drift: &str,
    front: Option<&str>,
) -> String {
    // `indexed`: the rows are here in the markup. The same class the script
    // sets on a note route, meaning the same thing.
    scripted(
        &listing_title(book),
        "split at-list indexed",
        // `BESIDE` is for the note this listing turns into: picking a row
        // replaces the reading pane with a note's, aside and all.
        &[Asset::Listing, Asset::Panes, Asset::Beside, Asset::Stamps],
        &format!(
            "{}{}",
            listing_panes(book, rows, asked, order, drift, front),
            notebook_bar(book, At::Notes)
        ),
    )
}

/// Both of the listing's panes, for a screen going back to it.
///
/// What [`listing_pane`] cannot answer alone: backing out of a note has to put
/// the rows *and* the pane the note stood in right, and asking separately is two
/// round trips with a moment on the way showing half of each.
///
/// The first route to send two different parts. Which is asked for is the
/// difference between narrowing a search — which leaves the note pane alone —
/// and going back, which is also the only one that renames the tab.
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

/// The other half of [`note`]'s trade: a note page is sent with this pane empty
/// because a phone never draws it, and `script::PANES` asks for it on a screen
/// that will.
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
        // `ls -l`'s order, tags last for its reason: they are the one column a
        // note may not have, so anything after them shifts from row to row.
        let under = [when(row.stamp.as_deref()), tag_line(&row.tags)]
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
            // Said twice and shown once: a wide row prints it at the right
            // where `-l` does, a narrow column beside the id. The stylesheet
            // chooses, because the server is not told how wide the screen is and
            // asking is a worse page than eleven bytes sent twice.
            row.stamp
                .as_deref()
                .map_or_else(String::new, |value| format!(
                    "<span class=\"day\">{}</span>",
                    escape(&day(value))
                )),
            // A hidden row carries its title unmarked, which is where the
            // script would leave it when a different query lets it back.
            if row.shown {
                highlight(&row.title, asked.terms)
            } else {
                escape(&row.title)
            }
        );
    }

    // Different sentences, and only the second is ever hidden: no amount of
    // typing changes an empty notebook.
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

/// The frame is the same on both routes: which notebook, where it stands, and a
/// search field that is a `GET` form on its own — so a scriptless reader can
/// narrow the listing from a note page by submitting it.
///
/// `rows` differs. A note route leaves it empty for `script::PANES` to fill,
/// because a phone that never shows this column should not be sent it, and
/// `counted` is empty for the same reason.
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
        // **The order, in the form, because the form is what would drop it.** A
        // search field submits `?q=…` and nothing else, so an `updated` listing
        // searched would come back in `slug` order with nothing said. Written
        // only when there is something to carry — see `Order::asked`.
        held(
            "sort",
            order
                .filter(|order| order.sort != Sort::default())
                .map(|order| order.sort.name()),
        ),
        held("r", order.filter(|order| order.reversed).map(|_| "1")),
        // Written and hidden by the server, so the script only decides when it
        // applies: a sentence living inside a script is one nothing can test.
        hint(),
        grouping(asked.grouping),
        asked.problem.map_or_else(String::new, |why| format!(
            "<p class=\"problem\">{}</p>",
            escape(why)
        )),
        // Last. The three above are the server answering the query and come and
        // go with it; the order between question and answer would split them.
        order.map_or_else(String::new, |order| sortbar(book, asked, order)),
    )
}

/// A hidden input carrying a default is a parameter that appears in the address
/// the first time anybody searches, and never leaves.
fn held(name: &str, value: Option<&str>) -> String {
    value.map_or_else(String::new, |value| {
        format!(
            "<input type=\"hidden\" name=\"{}\" value=\"{}\">",
            escape(name),
            escape(value)
        )
    })
}

/// The four orders `--sort` names, one chip apiece, the one in force marked.
///
/// **Four links, and that is the whole of it** — no menu, nothing to press
/// twice, so it works with the script off and puts the vocabulary on the screen
/// rather than behind a press.
///
/// **Pressing the order already in force turns it round**, which is `-r` and
/// where every sortable heading has taught people to look. The arrow is the only
/// place direction is written, so it sits on the chip whose press changes it.
///
/// Choosing a *different* order drops the reversal: each order has a direction
/// it means first, and arriving somewhere other than `--sort updated` is not
/// what pressing `updated` looks like it will do.
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
            // The arrow is `aria-hidden`, so the direction is said in words
            // here — along with what the press does, which differs on the chip
            // in force.
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

/// What was typed, as noda grouped it.
///
/// **`a OR b c` is `(a OR b) AND c`, and that is the one thing about this
/// grammar people get wrong** — `OR` binding tighter than a space is backwards
/// from every search box that has one, and the two readings are not close.
/// Neither answer looks wrong from a list of notes.
///
/// The manual says so and is not on the screen; this is, in the only terms that
/// cannot be misread — the grouping drawn. Each pill is a group joined by `or`,
/// and the pills are joined by `and`.
///
/// Never the parser's words: every token is the reader's own, which makes this a
/// mirror rather than a second opinion.
///
/// Empty for an empty field and for a line that is not a query yet — the field
/// already carries the red line, and grouping what does not parse would invent
/// an answer. Written either way and hidden when empty, for [`hint`]'s reason.
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
            // A `tag:` term wears the tag's colour, as everywhere else. Nothing
            // else is coloured: the point is the shape, and a pill per field
            // would be a legend to learn first.
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

/// The reading pane with no note picked, which only a two-pane screen sees.
///
/// A notebook with a `README.md` has already written the page about the whole of
/// itself, so that stands here rather than an invitation to press something.
/// Without one, the invitation.
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

/// Shown only when the script's answer and the server's can differ — a bare word
/// or `text:` reads the body, and the body is not on the page. Hidden the rest
/// of the time, including on every scriptless page.
fn hint() -> String {
    "<p class=\"hint\" hidden>Filtered by title and tag — press ⏎ to search the text.</p>"
        .to_string()
}

/// One row per file: how big it is, what it will arrive as, and how many notes
/// point at it. The last is the one worth the walk — a file nothing points at is
/// `doctor --links`' orphan, answered here the same way rather than a second
/// opinion.
pub fn files(book: &str, held: &[Held]) -> String {
    // The class goes with the branch that decides it, being one decision.
    // `.rows.cols` pours its contents across `column-width` tracks with a rule
    // between — right for a screenful of short rows, and one sentence poured
    // into four columns arrives as four fragments with a rule through it.
    // Neither screen gains a row after the paint, so the server knows which.
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
            // Side by side rather than nested — a link inside a link is not a
            // thing HTML has. The row goes to the file, the count to what points
            // at it, which is the only way to ask a file that has no page.
            //
            // **Zero is not a link**: a press whose answer is a page saying
            // "nothing links here" tells you what you already read.
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

/// Powers of two and one decimal place, as every file manager shows: 4.2 MB is
/// what this line is for, not 4,404,019 bytes.
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

/// Commonest first: by name alone, the four tags a notebook runs on are buried
/// under every one-off ever typed. Alphabetical within a count, so it does not
/// reshuffle between visits.
///
/// A row links into the listing narrowed to that tag, which makes this a way of
/// getting somewhere rather than a report. `query::scoped` writes the query,
/// because a tag with a space has to arrive quoted.
pub fn tags(book: &str, tallies: &[Tally]) -> String {
    // Columns only where there are rows to put in them — `page::files` says why
    // at length.
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

/// Everything in the notebook that is not done.
///
/// `todo::order` puts the rows in order — soonest first, undated last — because
/// `noda todo` and the browser print this same list, and one that came out
/// differently depending on which you asked would read as a bug in whichever you
/// asked second.
///
/// There is no way to tick a box here, and that is not an omission. An item
/// inside a note has no address: line numbers move and text prefixes collide,
/// and giving each one an id would turn the file into a noda-only format, which
/// is the thing choosing GFM checkboxes was meant to avoid. The row goes to the
/// note, where the box is a character you can type an `x` into.
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

/// What links to a note, or to one of the notebook's files.
///
/// Inbound only, which is why it is not called "links". What a note points at is
/// in the note and every Markdown reader renders it; what points *at* the note
/// is the half nothing else could tell you — and on a phone, where there is no
/// `noda backlinks` to type, it is the half that has nowhere else to come from.
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

/// Asked twice for two reasons: a reader pressing Links gets the page, and
/// `script::BESIDE` reads the rows out to rebuild them 236px wide. The second is
/// a fetch per note read on a monitor, so it pays for saying which part.
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

/// One note, and — on a wide enough screen — the listing it came from.
///
/// `drift` is for the index pane's bar, which is the notebook's rather than the
/// note's: two refs compared, what the listing route already pays.
pub fn note(book: &str, reading: &Reading, drift: &str) -> String {
    // No `indexed`, and that pane is sent empty: a listing is about 290 bytes a
    // note — half a megabyte at two thousand — and below 1024px none of it is
    // drawn. The frame goes out and `script::PANES` asks for the rest where the
    // column is on screen. Scriptless, the page is the note and the chevron.
    scripted(
        &note_title(reading),
        "split at-note",
        &[Asset::Panes, Asset::Beside, Asset::Stamps],
        &format!(
            "{}{}{}",
            // The empty `rows`' argument: an order over no rows orders nothing,
            // and on a phone it is 450 bytes of a control nobody sees.
            // `script::PANES` brings the column and the order together.
            index_pane(book, &Asked::nothing(), None, "", drift, ""),
            read_pane(book, reading),
            notebook_bar(book, At::Notes),
        ),
    )
}

/// The same note, for a reader who already has the page around it.
///
/// **The whole of what a swap uses.** Everything else a note page carries is
/// already on the screen it is going into — 48 of the 52 KB, measured — so a
/// request saying it wants this part gets this part, out of the same function
/// the whole page is built from.
///
/// No `drift`: that chip sits in the index pane's bar, which a swap does not
/// replace, so a note asked for this way costs one file read and no refs.
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

/// Written once and sent by both answers above, which is what makes the shorter
/// one a part of the longer rather than a second opinion.
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

    // The box, not the answer in it: what points at a note is a walk of every
    // note — +8% on `ls`, measured — spent on a column no screen under 1440px
    // draws. `script::BESIDE` fills it where it shows; sent `hidden`, so a
    // scriptless reader gets the note and the Links button.
    let beside = "<aside class=\"beside\" hidden><div class=\"pane-head\">Backlinks</div>\
                  <div class=\"answer\"></div></aside>";
    // Nothing marked as current: five things to do *to* the note you are on,
    // with no "here" among them.
    //
    // **Delete is on it, and used to be a line past the end of the prose.** The
    // friction that was meant to buy is already built elsewhere: `/delete` is a
    // confirmation page, so a thumb landing here spends a page and never a note.
    // Hiding the way in only cost a new reader the knowledge that a note can be
    // deleted at all.
    //
    // Last, so the four that were here keep the positions a hand has learned,
    // and the only item on any bar that carries a colour.
    let bar = action_bar(&[
        (EDIT, "Edit", format!("{at}/edit"), Mark::Plain),
        (TAGS, "Tags", format!("{at}/tags"), Mark::Plain),
        (RENAME, "Rename", format!("{at}/rename"), Mark::Plain),
        (LINKS, "Links", format!("{at}/backlinks"), Mark::Plain),
        (TRASH, "Delete", format!("{at}/delete"), Mark::Danger),
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
        // The note, not the notebook: beside an index pane already headed with
        // the notebook's name, repeating it says nothing.
        escape(&reading.title),
        escape(&reading.title),
        escape(&reading.id),
        escape(&reading.slug),
        reading.rendered,
    )
}

/// A bar with a way back, an optional line saying what went wrong, and the form.
///
/// **`<main>` around both, and it is not decoration**: the wide layout caps
/// `main` to make its column, so anything outside runs the whole width of a
/// monitor — which is what every form page did, a topbar neatly in its column
/// with a textarea stretching past it.
fn form_page(book: &str, title: &str, back_to: &str, said: &str, form: &str) -> String {
    laid_out(book, title, back_to, said, form, &[])
}

/// A form page that also listens for the note moving under it.
///
/// The three forms carrying a fingerprint — the editor, and the two answers to
/// a note that changed while it was open — are exactly the three worth telling.
/// Every other form here changes one field and is gone before anything could
/// have happened to it.
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

/// The note changed under the reader and there was no version to merge from, so
/// both are handed back whole.
///
/// Reached only when the version the edit began from is not in the object
/// database — a note written by hand and never committed. Where it is,
/// `conflicted` shows the merge instead and this shows nothing.
///
/// What is on disk is shown first and cannot be typed into; what they wrote is
/// underneath and still can be. That needs no "keep mine" button — saving *is*
/// keeping theirs — and leaves room for the one thing a program cannot do:
/// decide what the two versions together should say.
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
/// settled.
///
/// One field where `clashed` has two, because a merge has already done the part
/// a program can do: everything outside the markers is both edits, combined.
/// What is left is the question no program can answer, and it is answered by
/// editing the text rather than by copying between two panes.
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

/// A ticked box per tag it has, and a field for ones it does not. The form says
/// what survives; the `+`s and `-`s are the server's job, `+work -q3` being a
/// notation for somebody with a keyboard.
///
/// **The field takes as many tags as you can type**, being cut by `query::split`
/// — a space separates, a quote holds one together. The placeholder shows a
/// quoted tag rather than describing one: while the label was singular the field
/// read as a one-tag field, which hid something the server always did.
///
/// **Every tag is sent twice: once as `saw`, and again as `keep` if its box is
/// still ticked.** An unticked box sends nothing, so without `saw` the only
/// record of what the page offered is what came back ticked, and the server has
/// to read the difference against the file — where a tag added since would look
/// unticked and be removed. `saw` is that record, and it is one hidden field per
/// tag rather than a list in one, so a tag with a space in it needs no quoting.
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

/// It says what git makes true — the note can be brought back — because a
/// warning that overstates the danger teaches people to click through warnings.
pub fn deleting(book: &str, about: &About) -> String {
    form_page(
        book,
        "Delete",
        &about.at(book),
        // The slot every other form page speaks from; this one had it inside
        // the form. `.said` is a strip with its own padding and a rule reaching
        // both edges, so nesting it inset the words twice and stopped the rule
        // short. The strip was never a thing to put inside a form.
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

/// `noda status`'s facts, already in words. Worked out by the caller, on this
/// file's rule: the page arranges what it is given and decides nothing.
pub struct Standing {
    pub branch: String,
    pub notes: usize,
    pub files: usize,
    /// Files differing from `HEAD`. What a sync would commit before it pulls.
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
    /// Chosen by the caller, being the same choice the failure colour is made
    /// from — one fact read once.
    pub done: &'a str,
    /// What it printed, or what went wrong. `None` while it is still going.
    pub said: Option<&'a str>,
    pub failed: bool,
    pub seconds: u64,
}

/// Where the notebook stands, and the three ways to move it.
///
/// **The buttons are `POST`s and the page is a `GET`, and that is the whole
/// design.** A sync takes as long as the network takes, and a request held open
/// would leave a phone showing nothing — so the press starts the errand and
/// redirects back, and a reload asks how it is going rather than doing it again.
/// While one runs the page comes back for news; when it stops, so does that.
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

/// The only fetch here that repeats: while an errand runs `script::STANDING`
/// asks every two seconds, and a whole answer carried the stylesheet to move one
/// line of text. What it takes is the `<main>` and one fact from the head.
///
/// **Whether to poll again is still the server's decision, said the way the
/// scriptless page hears it**: the script reads the same `refresh` meta rather
/// than deciding, so dropping it would be the script inventing a stop
/// condition.
pub fn standing_main(book: &str, standing: &Standing, errand: Option<&Errand>) -> String {
    format!(
        "{}{}",
        refresh(working(errand).then_some(AGAIN_IN)),
        network_main(book, standing, errand)
    )
}

/// One number, read by the meta the browser obeys and the poll replacing it.
const AGAIN_IN: u32 = 2;

/// The whole of what "come back for more" means here.
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
        // The same three answers `noda status` gives, in the same words.
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
        // The remedy, not just the fact: the command that sets a remote is not
        // on any screen here.
        None => row(
            "Remote",
            "none — set one with `noda remote set <url>`",
            false,
        ),
    }
    for problem in &standing.problems {
        row("Problem", problem, false);
    }

    let said = match errand {
        None => String::new(),
        Some(errand) => match errand.said {
            // Seconds rather than a bar: nothing knows how long a fetch takes,
            // and a bar that guessed would be the one untrue thing here.
            None => format!(
                "<p class=\"said working\"><b>{}…</b> {}</p>",
                escape(errand.doing),
                plural(errand.seconds as usize, "second")
            ),
            // Whole, not summarised into a tick: the three lines `sync` prints
            // are the difference between "it worked" and what it did.
            Some(said) => format!(
                "<p class=\"said{}\"><b>{}</b><span class=\"outcome\">{}</span></p>",
                if errand.failed { " bad" } else { "" },
                escape(errand.done),
                escape(said)
            ),
        },
    };

    // Honesty rather than safety: the server already refuses a second press, so
    // the greying prevents the belief that the first did not land.
    let busy = working(errand);
    let at = escape(book);
    let mut buttons = String::new();
    for (errand, label) in [("sync", "Sync"), ("pull", "Pull"), ("push", "Push")] {
        let _ = write!(
            buttons,
            "<form method=\"post\" action=\"/nb/{at}/status/{errand}\">\
             <button class=\"{}\" type=\"submit\"{}>{label}</button></form>",
            // The accent on the one nearly always right, said in colour rather
            // than layout so all three stay one row on a phone.
            if errand == "sync" { "go" } else { "" },
            if busy { " disabled" } else { "" }
        );
    }

    format!(
        "<main>{said}<div class=\"rows facts\">{rows}</div>\
         <div class=\"abreast\">{buttons}</div></main>"
    )
}

/// No apology and no blame: what was asked for, why it could not be answered,
/// and the way back — which on a page with no navigation of its own is what
/// makes it not a dead end.
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

/// The `Z` or `+08:00` comes with it, which is the point: the one place with
/// room for the whole thing, and the whole thing is the only version that cannot
/// be misread. It is also what a scriptless reader keeps.
///
/// `data-clock` says this stamp has room for a time of day and a listing's has
/// not. It rides here because a note page sends two and a listing sends one per
/// note.
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

/// `script.rs` writes the split breakpoint a second time, and the two numbers
/// have to agree. Exposing the sheet is how that is checked.
pub(crate) fn stylesheet() -> &'static str {
    CSS
}

/// Mobile first, because that is what this exists for. Two numbers run through
/// it: `--tap`, which no control may be smaller than, and the 16px on the search
/// field — below that, iOS Safari zooms on focus.
const CSS: &str = "\
*{box-sizing:border-box}\
/* The browser's own `[hidden]{display:none}` loses to any author rule that \
   sets `display` — `.row{display:block}` is the same specificity and an author \
   rule beats a user-agent one, so a listing's excluded rows were `hidden` in \
   the DOM and drawn on the screen. This is not a preference about how to hide \
   things; it is the one declaration that makes the attribute mean what it \
   says, wherever it is put and whatever else styles the element. */\
[hidden]{display:none!important}\
:root{--tap:48px;\
/* The rail's width above 640px. Two numbers run through this sheet and this \
   is the second: nothing may be smaller than `--tap`, and the navigation is \
   always exactly this wide, so a pane can be sized against what is left. */\
--rail:76px;\
--mono:ui-monospace,SFMono-Regular,'SF Mono',Menlo,'Cascadia Mono',Consolas,monospace;\
--read:ui-serif,Charter,'Iowan Old Style',Georgia,'Songti TC','Noto Serif CJK TC',serif}\
html{-webkit-text-size-adjust:100%}\
body{margin:0;background:var(--bg);color:var(--text);font-family:var(--mono);font-size:14px;line-height:1.6}\
svg{fill:none;stroke:currentColor;stroke-width:2;stroke-linecap:round;stroke-linejoin:round}\
/* A phone stacks; everything wider is a grid, and the grid is declared where \
   the width is known. `min-width:0` because a grid item's default is `auto`, \
   which lets one long unbroken line in a note push a whole column wider than \
   the screen. */\
/* A column that fills the screen, and it is not decoration: a bar is \
   `position:sticky;bottom:0`, and sticky is bounded by its containing block. \
   With the pane only as tall as its content, a short note left the bar \
   floating halfway down the screen wherever the prose happened to stop. The \
   pane grows into the space instead, so the bar has a foot to sit at. */\
.app{display:flex;flex-direction:column;min-height:100dvh}\
.pane{display:block;min-width:0;flex:1 1 auto}\
.foot{flex:0 0 auto}\
/* And the same one floor down, for the note's own bar. `position:sticky` holds \
   a thing in view while the page scrolls past it; it will not push one down a \
   page that does not scroll, so a short note used to leave the bar wherever \
   the prose stopped. The prose takes the slack instead. */\
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
/* What the script is answering while the server has not been asked. It sits \
   where a complaint would, because it is the same kind of remark about the \
   same field — and it is the chrome's grey rather than a hue, since nothing \
   in this palette colours a state. */\
.searchbar .hint{margin:8px 2px 0;font-size:12.5px;color:var(--muted)}\
/* ------------------------------------------- SIGNATURE: the query parse */\
/* The grouping, drawn under the field it is about. See `page::grouping` for \
   why it is worth the room; this is only where it is spent. Not on a phone: \
   the field there is one line with one remark under it, and a second remark \
   about the same line would be the taller half of a screen already short of \
   room. The reading of `a OR b c` a phone gets wrong is the reading a monitor \
   gets wrong, but a monitor can afford to be told. */\
.parse{display:none;margin:9px 2px 1px;flex-wrap:wrap;align-items:center;gap:7px;font-size:12px}\
/* A group is a pill, so the boundary is a shape rather than a word — the \
   thing being said is where the brackets fall, and brackets are what people \
   were not reading. */\
.parse .g{display:inline-flex;align-items:center;gap:6px;padding:2px 9px;\
border:1px solid var(--rule);border-radius:999px;background:var(--bg-sunk)}\
.parse .g b{font-weight:400;color:var(--text)}\
.parse .g b.t{color:var(--tag)}\
.parse .g i{font-style:normal;color:var(--punct);font-size:11px;letter-spacing:.06em}\
.parse .and{color:var(--punct);font-size:11px;letter-spacing:.09em;text-transform:uppercase}\
/* ------------------------------------------------ SIGNATURE: the order */\
/* Four chips, and they are `.drift .pill`'s chip: a 32px pill inside a 48px \
   press, which is how this sheet has drawn every small control since the \
   first one. Nothing new is being invented for the fourth thing that is a \
   row of pills. \
   It wraps rather than scrolling. A chip that has slid out of sight is a \
   chip a reader does not know exists, and there are only four of them — in \
   the index column of a split screen the last one takes a line of its own, \
   which costs 48px and hides nothing. */\
.sortbar{display:flex;flex-wrap:wrap;align-items:center;gap:6px;margin:2px 2px 0}\
/* The glyph, because the word `order` costs 55px and that is the difference \
   between four chips on one line in a split view's index column and three. */\
.sortbar .lab{flex:0 0 auto;display:inline-flex;align-items:center;\
color:var(--punct);padding-right:2px}\
.sortbar .lab svg{width:15px;height:15px}\
.sortbar a{flex:0 0 auto;display:inline-flex;align-items:center;min-height:var(--tap);\
color:var(--muted);-webkit-tap-highlight-color:transparent}\
.sortbar a .pill{display:inline-flex;align-items:center;gap:5px;min-height:32px;\
padding:0 11px;border:1px solid var(--rule);border-radius:999px;\
background:var(--bg-sunk);font-size:12px;white-space:nowrap}\
.sortbar a:active .pill{background:var(--press)}\
/* The one in force steps forward by losing its fill and taking the id's hue \
   on its edge — the same move `.parse .g` makes in reverse. It is not a \
   different colour of text: nothing in this palette colours a state. */\
.sortbar a[aria-current] .pill{background:transparent;border-color:var(--id);color:var(--text)}\
.sortbar a svg{width:12px;height:12px;flex:0 0 auto;color:var(--id)}\
a{color:inherit;text-decoration:none}\
.row{display:block;min-height:64px;padding:12px 16px;border-bottom:1px solid var(--rule);\
-webkit-tap-highlight-color:transparent}\
.row:active{background:var(--press)}\
/* The note the reading pane is showing. Only ever seen where both panes are, \
   because it is the answer to a question only two panes can ask. The same \
   grey a press leaves: it is the row you are on, not a row that is special. */\
.row.here{background:var(--press)}\
.row .title{font-family:var(--read);font-size:17px;line-height:1.32}\
/* A filename is the machine's, not the reader's — the same rule the note page \
   follows when it sets the id and the slug in monospace. */\
.row .title.mono{font-family:var(--mono);font-size:15px}\
.row .name{font-size:15px}\
.row .under{margin-top:4px;font-size:12.5px;display:flex;gap:8px;align-items:baseline;flex-wrap:wrap}\
.row .under:empty{display:none}\
/* ------------------------------------------------ SIGNATURE: the id column */\
/* The note's own name, in the notebook's own vocabulary — `noda show` takes \
   it, and it is the first half of the filename in the repository. Written on \
   every listing row and shown only where a row has a column to spare for it, \
   which is nowhere on a phone: see the rules at 640 and 1024 below. \
   Monospace and `--id`, the same as `noda ls` prints and the same as the \
   filename at the head of a note page, because it is the same thing being \
   named three times. */\
.row .ident{display:none;font-family:var(--mono);font-size:12px;color:var(--id)}\
.tags{color:var(--tag)}\
.sep{color:var(--punct)}\
.when{color:var(--muted)}\
/* A due date that has gone by. The one colour in noda that marks what a thing \
   means rather than what it is, and `style.rs` argues for the exception where \
   the colour is chosen. */\
.overdue{color:var(--overdue)}\
/* Which note a task is written in — the answer to a second question, so it \
   steps back from the task itself. */\
.row .in{color:var(--muted)}\
/* A row with two destinations: the thing, and one question about it. The \
   question is a tap target in its own right, which is why it is as tall as the \
   row rather than as tall as its words. */\
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
/* A rendered note. One size ladder, one rhythm, and every block the same \
   distance from the next — a page of prose has one job and the styling of it \
   should be boring. The measure is set further down, on the wide screen, and \
   on the element that is actually read. */\
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
/* `noda todo` reads these boxes across the whole notebook, so they are boxes \
   here too. Disabled, because ticking one has to be a commit — the same reason \
   the CLI has no `todo done`. */\
.body li input[type=checkbox]{width:16px;height:16px;margin:0 .45em 0 0;\
accent-color:var(--tag);vertical-align:-2px}\
.body a{color:var(--tag);text-decoration:underline;text-underline-offset:3px;\
text-decoration-thickness:1px}\
/* The one distinction the body draws, and it is the palette's own rule: colour \
   marks what a thing is. A link in the id's colour stays inside the notebook; \
   a link in the tag's colour leaves it. Same face, same size — a reader who \
   never notices loses nothing, and one who does can tell before pressing. */\
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
/* The table scrolls inside itself rather than widening the page. A phone is \
   narrower than three columns of anything, and the alternative is a note whose \
   every paragraph is cut off because one table was wide. */\
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
.actionbar a{flex:1;min-height:64px;padding:9px 0 10px;display:flex;flex-direction:column;\
align-items:center;justify-content:center;gap:4px;color:var(--muted);\
-webkit-tap-highlight-color:transparent}\
.actionbar a:active{background:var(--press)}\
.actionbar svg{width:26px;height:26px}\
.actionbar span{font-size:11.5px;letter-spacing:0.02em}\
/* Where you are, said by the attribute a screen reader reads for the same \
   fact. Brighter rather than another hue: the palette's colours mark what a \
   thing is, and this marks which one of them you are standing on. */\
.actionbar a[aria-current]{color:var(--text)}\
/* The one item on any bar here that removes something, and the only place the \
   alert colour is used for anything other than a refusal. Colour in this \
   palette says what a thing *is*, and what this one is is the action that \
   cannot be undone by doing it again. */\
.actionbar a.danger{color:var(--alert)}\
/* The one action, lifted off the row of places. It sits above the bar and to \
   the right, where a thumb already is, and it is the only round thing on any \
   of these pages — a shape nothing else uses cannot be mistaken for a row. */\
.foot{position:sticky;bottom:0;z-index:2}\
.foot .actionbar{position:static}\
.fab{position:absolute;right:16px;bottom:calc(100% + 16px);\
width:56px;height:56px;border-radius:28px;background:var(--id);color:var(--bg);\
display:flex;align-items:center;justify-content:center;\
/* Not a shadow: this stylesheet separates things with a 1px rule and never \
   with depth. A ring of the page's own background is the same idea said in \
   the one language the rest of the page speaks. */\
box-shadow:0 0 0 5px var(--bg);-webkit-tap-highlight-color:transparent}\
.fab svg{width:26px;height:26px;stroke-width:2.4}\
.fab:active{background:var(--id-dim)}\
/* Room under the last row, so the button never covers the end of a list. The \
   button is 56px tall and stands 16px clear of the bar, so 72 is what it \
   occupies and 76 is that with a hair to spare. */\
body:has(.fab) main{padding-bottom:76px}\
form.write{padding:16px;display:flex;flex-direction:column;gap:16px}\
form.write label{font-size:12.5px;color:var(--punct);display:block;margin-bottom:6px}\
/* Under the field, not above it: what a field takes is read after the reader \
   has seen the field, and a label that tried to hold this would stop being a \
   name for the thing. */\
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
/* The whole row, not the words. A label is what a thumb presses to toggle the \
   box inside it, so it is the label that has to be a target — and on a phone \
   the row is the shape a list of choices takes. The selector names the form as \
   well as the class because `form.write label` above is the more specific of \
   the two otherwise, and a row left as a block puts the box and its tag on a \
   shared baseline instead of a common centre. */\
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
/* Where the notebook stands against its remote, on the way to the screen that \
   says the rest. Nothing in the palette marks a state — colour here says what a \
   thing *is* — so the chip is the chrome's own grey and the words do the work. \
   It is a link and not a badge, which is why it is the size of a target. */\
.topbar .drift{margin-left:auto;flex:0 1 auto;min-width:0;display:inline-flex;\
align-items:center;min-height:var(--tap);color:var(--text);\
-webkit-tap-highlight-color:transparent}\
.topbar .drift .pill{display:inline-flex;align-items:center;gap:6px;min-width:0;\
min-height:32px;padding:0 11px;border:1px solid var(--rule);border-radius:999px;\
font-size:12px}\
.topbar .drift .pill span{overflow:hidden;text-overflow:ellipsis;white-space:nowrap}\
.topbar .drift svg{width:14px;height:14px;flex:0 0 auto;color:var(--muted)}\
.topbar .drift:active .pill{background:var(--press)}\
/* Named with the ancestor as well, so it beats `.topbar .count` on specificity \
   rather than on which of them was written last. */\
.topbar .drift+.count{margin-left:10px;padding-left:0}\
/* The three errands, abreast. Sync carries the accent because it is the one \
   that is nearly always right — commit, pull and push, in the order that keeps \
   local work from being left behind. Pull and Push are for somebody who knows \
   which half they want, and they are the same size because they are the same \
   kind of thing. */\
.abreast{display:flex;gap:10px;padding:18px 16px 22px}\
.abreast form{flex:1;display:flex}\
.abreast button{flex:1;padding:0 12px}\
button[disabled]{opacity:.45}\
/* What the command printed, in the face a command prints in — and one line per \
   line, because `sync` says three things and a paragraph of them run together \
   is a paragraph nobody reads. */\
.outcome{display:block;margin-top:6px;font-family:var(--mono);font-size:12.5px;\
line-height:1.5;white-space:pre-line;overflow-wrap:anywhere;color:var(--muted)}\
.said.bad .outcome{color:var(--alert)}\
/* The one thing on any of these pages that moves. It moves because a page which \
   reloads itself every two seconds has to say whether it is doing anything \
   before the reader reaches for the button again. */\
.working b::after{content:\"\";display:inline-block;width:7px;height:7px;\
border-radius:4px;background:var(--id);margin-left:9px;vertical-align:1px;\
animation:breathe 1.4s ease-in-out infinite}\
@keyframes breathe{0%,100%{opacity:.2}50%{opacity:1}}\
/* Somebody who has asked their system not to see movement is asking about this. */\
@media (prefers-reduced-motion:reduce){.working b::after{animation:none;opacity:.8}}\
.theirs{margin:0;padding:14px;border:1px solid var(--rule);border-radius:10px;\
background:var(--bg-sunk);font-family:var(--read);font-size:16px;line-height:1.55;\
white-space:pre-wrap;overflow-wrap:break-word}\
/* ------------------------------------------------------------ label role */\
/* Mono, small, uppercase, tracked — a terminal's header line. It names a \
   region and never appears inside content. */\
.pane-head{font-size:11px;letter-spacing:.09em;text-transform:uppercase;\
color:var(--punct);padding:0 0 8px}\
/* --------------------------------------------------------- the margin note */\
/* Hidden until it holds something, and there is no width at which that is not \
   true: the `display:block` below is on `:not([hidden])` so it cannot out-order \
   the attribute. A reader with no script keeps the attribute for good, and what \
   they get is the note and the Links button — nothing stuck half-loaded, \
   because nothing was promised. */\
.beside{display:none}\
.beside .mini{display:block;padding:8px 0;border-bottom:1px solid var(--rule);\
font-family:var(--read);font-size:14px;line-height:1.35;color:var(--text)}\
.beside .mini:last-child{border-bottom:0}\
.beside .mini:hover{color:var(--tag)}\
.beside .mini span{display:block;font-family:var(--mono);font-size:11px;\
color:var(--id);margin-top:3px}\
/* An answer of none is still an answer, and worth the column it arrived in. */\
.beside .none{margin:0;color:var(--muted);font-size:13px}\
/* Waiting for the walk. It borrows the status screen's breathing dot rather \
   than inventing a second way of saying \"working\", but not that screen's bold: \
   there it is the page's answer, here it is a margin note keeping its place. */\
.beside .said{margin:0;padding:0;border-bottom:0;font-size:13px}\
.beside .said b{font-weight:400;color:var(--muted)}\
/* Below the width that holds two panes, exactly one is on screen, and which \
   one is the route's answer. Said as its own query rather than as a rule the \
   wide ones have to out-order: two `display` declarations fighting on source \
   position is how a pane goes missing. */\
@media (max-width:1023px){.app.split.at-list .read{display:none}}\
/* A note page is sent without its listing — see `script::PANES`. `.indexed` \
   is what says the pane has one, and it has exactly two writers: the server, \
   on the listing route, where the rows are in the markup; and the script, on \
   a note route, before the first paint. Nothing else turns it on, so a pane \
   that would have nothing in it is never a column. */\
.app.split.at-note .index{display:none}\
/* And a phone reading a note gets one bar, not two: the note's own actions. \
   The rail holds the notebook's four places, and they are one press away up \
   the chevron. */\
@media (max-width:639px){\
.app.split.at-note .foot{display:none}\
.app.split.at-note main{padding-bottom:16px}}\
/* ================================================================= TABLET */\
/* The bottom bar stands up and becomes a rail, and the content takes \
   everything left over. A bar pinned to the foot of a 1100px-tall screen is a \
   phone idiom stranded; a rail is where the hand already is. \
   The rail is written last in the markup — a phone needs it at the foot of \
   the document to stick to the foot of the screen — and placed first here. */\
@media (min-width:640px){\
.app{display:grid;grid-template-columns:var(--rail) minmax(0,1fr);height:100dvh;overflow:hidden}\
.foot{grid-column:1;grid-row:1/-1;position:static;display:flex;flex-direction:column;\
align-items:stretch;background:var(--bg-sunk);border-right:1px solid var(--rule)}\
.foot .actionbar{flex-direction:column;border-top:0;background:transparent;padding:0}\
.foot .actionbar a{flex:0 0 auto;min-height:62px}\
/* The one action, first: a rail reads top to bottom, and what you came to do \
   goes above where you might go. Square-ish rather than round, because at this \
   size it sits in a column of things and a circle in a column of rectangles is \
   a circle asking to be looked at. */\
.fab{order:-1;position:static;width:44px;height:44px;border-radius:13px;\
margin:14px auto 12px;box-shadow:none}\
.fab svg{width:22px;height:22px}\
/* Each pane scrolls on its own, which is what makes two of them worth having: \
   a listing keeps its place while a note is read past its end. */\
.pane{grid-column:2;min-height:0;overflow-y:auto}\
.app:has(.fab) main{padding-bottom:24px}\
/* A screen with one pane hangs its content off the rail and stops at a width \
   a row can still be read across. Not centred — a column with a gutter on \
   both sides is what a monitor looked like before this. */\
.app:not(.split) .topbar,.app:not(.split) main{max-width:80em}\
.topbar,.searchbar{padding-left:24px;padding-right:24px}\
.topbar .back{margin-left:-12px}\
.topbar .lead{padding-left:0}\
.parse{display:flex}\
/* The row extends rather than stacking, and what it extends into is `ls -l`'s \
   own columns: id, title, updated, tags. The slug and the created stamp are \
   the two `-l` prints that are not here, and both are one press away on the \
   note's own page. \
   Tags stay last for the reason `-l` gives: they are the one column a note \
   may not have, and anywhere but the end their absence would shift every \
   column behind them from row to row. */\
.rows .row{display:flex;align-items:baseline;gap:20px;min-height:0;padding:13px 24px}\
.rows .row .ident{display:block;flex:0 0 auto}\
/* The day is at the right of a wide row, where `-l` prints it. The copy \
   beside the id is for the narrow index pane further down, which has no \
   right-hand side to print it at. */\
.rows .row .ident .day{display:none}\
.rows .row .title,.rows .row .name{flex:1 1 auto}\
.rows .row .under{margin:0;flex:0 0 auto;justify-content:flex-end}\
.rows .row.split{display:flex}\
.rows .row.split .most{display:flex;align-items:baseline;gap:20px;padding:13px 0 13px 24px}\
.rows .row.split .aside{padding:0 24px}\
/* Short rows go in columns rather than down one long strip, so eight tags on \
   a monitor are eight tags and not eight tags and a field of nothing. \
   `column-width` and not a grid, because the divider wanted is a hairline and \
   `column-rule` draws exactly that. */\
.rows.cols{column-width:270px;column-gap:0;column-rule:1px solid var(--rule)}\
.rows.cols .row{break-inside:avoid;display:block;padding:13px 24px}\
.rows.cols .row .under{margin-top:3px;justify-content:flex-start}\
.rows.cols.wide{column-width:400px}\
.rows.cols .row.split{display:flex;padding:0}\
.rows.cols .row.split .most{display:block;padding:13px 0 13px 24px}\
.rows.cols .row.split .aside{padding:0 24px}\
/* A fact is a name and a value, so it is two columns and not the row's three. */\
.rows.facts .row{display:grid;grid-template-columns:150px minmax(0,1fr);gap:0}\
.rows.facts .row .under{justify-content:flex-start}\
.note-head{padding:26px 32px 20px}\
.note-head h1{font-size:28px}\
/* A measure, and measured in the font it is set in. `ch` and `em` are relative \
   to the element's own type — putting the reading measure on `main`, which is \
   set in the monospace the chrome uses, sized a column of prose by a font the \
   prose is not in. It is the body that is read, so it is the body that is \
   capped. */\
.body{font-size:18px;line-height:1.65;max-width:34em;padding:24px 32px 8px}\
.said{padding:14px 32px}\
.empty{padding:34px 32px}\
/* A form on a screen with a rail has vertical room the phone never had, and \
   the text field is the one thing here that can use all of it. */\
form.write{padding:24px 32px;max-width:52em}\
form.write textarea{min-height:min(56vh,560px)}\
.buttons{justify-content:flex-start}\
button.go,button.danger{flex:0 0 auto;min-width:190px}\
.abreast{padding:20px 32px 28px;max-width:52em}\
/* A note's actions become a toolbar at the head of the reading pane. `order` \
   moves it there without moving it in the markup, so the phone's bottom bar \
   and the desktop's toolbar are one element said twice. */\
.read{display:flex;flex-direction:column}\
.read .topbar{order:-2}\
.read .actionbar{order:-1;position:static;background:var(--bg);border-top:0;\
border-bottom:1px solid var(--rule);justify-content:flex-start;gap:2px;padding:7px 20px}\
.read .actionbar a{flex:0 0 auto;flex-direction:row;gap:8px;min-height:38px;\
padding:0 13px;border-radius:9px;font-size:13px}\
.read .actionbar a:hover{background:var(--press);color:var(--text)}\
/* The rule above lifts every item to `--text` under the pointer, which would \
   take the colour off the one item whose colour is the point. */\
.read .actionbar a.danger:hover{color:var(--alert)}\
.read .actionbar svg{width:17px;height:17px}\
.read main{order:0;flex:1 1 auto}}\
/* ================================================================ DESKTOP */\
/* The index stays on screen while a note is read. That is the one thing a \
   phone cannot do, and the reason a wide screen is worth having. Every other \
   screen keeps one pane and spends the width on columns instead. \
   Three columns only where there is a third thing: without the class the grid \
   is the tablet's two, which is what a reader with no script gets — the note, \
   whole, and the chevron back to the listing. Nothing is stuck half-loaded, \
   because nothing was promised. */\
@media (min-width:1024px){\
.app.split.indexed{grid-template-columns:var(--rail) clamp(300px,26vw,380px) minmax(0,1fr)}\
.app.split.indexed .index{grid-column:2;border-right:1px solid var(--rule)}\
.app.split.indexed .read{grid-column:3}\
.app.split.indexed.at-note .index{display:block}\
.app.split.at-list .read{display:flex}\
/* In a column this narrow the row stacks again — but tighter than a phone's, \
   because a dense list is the point of keeping it on screen. */\
.app.split .index .rows .row{display:block;padding:11px 20px;min-height:0}\
/* Stacked, `-l`'s four columns become three lines, and the id line takes the \
   day with it: a column 300px wide has no right-hand side to print a date at, \
   but it does have two ends, and the two things that are not the title are \
   exactly what belongs at them. The copy under the title goes, so the day is \
   still said once. */\
.app.split .index .rows .row .ident{display:flex;justify-content:space-between;gap:10px}\
.app.split .index .rows .row .ident .day{display:block;color:var(--muted)}\
.app.split .index .rows .row .title{font-size:15.5px;line-height:1.34;margin-top:2px}\
.app.split .index .rows .row .under{margin-top:2px;justify-content:flex-start;font-size:12px}\
.app.split .index .rows .row .under .when,.app.split .index .rows .row .under .sep{display:none}\
.app.split .index .searchbar,.app.split .index .topbar{padding-left:20px;padding-right:20px}\
.app.split .index .empty{padding:26px 20px}\
/* Head and body share one column so the rule under the title spans exactly \
   what the prose does. The pane keeps the slack. */\
.app.split .read main.note{width:100%;max-width:44em;margin-inline:auto}\
.app.split .read .body{max-width:none}\
/* The note's chevron points at the listing, and here the listing is already \
   on the screen beside it. The index pane keeps its own, which points \
   somewhere you cannot see: the notebooks. Only when the index is actually \
   there — without the script this is the one pane, and the way back with it. */\
.app.split.indexed .read .topbar .back{display:none}}\
/* =================================================================== WIDE */\
/* At 1440px what points at this note comes out from behind a press and sits \
   beside it. It is the half of a note's links nothing else could tell you, and \
   on a screen this wide there is room to just show it. \
   `margined` is `indexed`'s twin and is set the same way — by the script, \
   synchronously, before the first paint. It says the column has been asked \
   for, which is why the layout may reserve it: the prose is then laid out once \
   and the answer lands into space already kept for it. Without a script the \
   class never appears and the note keeps the centred measure it has at \
   1024px, rather than sitting off to one side of a column nothing will fill. */\
@media (min-width:1440px){\
/* `align-content:start` is load-bearing, not tidiness. `main` fills the pane, \
   so a note shorter than the screen leaves the grid with room to spare — and \
   the default (`normal`, resolving to `stretch`) hands that room out among the \
   auto rows, which moves everything below the first row down by a distance \
   that depends on nothing but how short the note is. It was found as a 40px \
   jump in the body of a short note the moment the margin note landed. Packing \
   the rows at the start leaves the spare room at the bottom, where it belongs, \
   and how tall a note is stops being something the layout spends on gaps. */\
.app.split.at-note.margined .read main.note{max-width:none;margin-inline:0;\
display:grid;grid-template-columns:minmax(0,38em) 236px;column-gap:48px;\
align-items:start;align-content:start;justify-content:center}\
/* Head and body share one column, so the rule under the title spans exactly \
   what the prose does and the measure is the track rather than the track less \
   two paddings. */\
.app.split.at-note.margined .read .note-head,\
.app.split.at-note.margined .read .body{grid-column:1;padding-left:0;padding-right:0}\
.app.split.at-note.margined .read .beside:not([hidden]){display:block;grid-column:2;\
grid-row:1/span 2;position:sticky;top:24px;padding:26px 0 0}}\
/* A monitor wider than a laptop spends the extra on the index and the margin \
   note, never on the measure: a line of prose has a right length and it is not \
   \"however wide the window is\". */\
@media (min-width:1800px){\
.app.split.indexed{grid-template-columns:var(--rail) clamp(340px,22vw,470px) minmax(0,1fr)}\
.app.split.at-note.margined .read main.note{grid-template-columns:minmax(0,40em) 290px;\
column-gap:64px}}\
/* ============================================ SIGNATURE: a row is a notebook */\
/* Written last on purpose. Two of the rules below have to beat something the \
   sheet already says at the same weight, and at the same weight the later rule \
   wins — putting the front page's own section anywhere else would mean raising \
   selectors until they won, which is how a stylesheet stops being readable. */\
/* The front page has no rail and no bar, so it does not want the grid that \
   holds one. With no `.foot` in its markup the rail's column was there and \
   empty: 76px of nothing down the left of every screen wider than a phone, \
   which is most of what made this page look broken. \
   `.app:not(.split)`'s `max-width:80em` goes with it. That cap is for one pane \
   hanging off the rail with prose in it; here there is no rail, and a row is a \
   line of a table rather than a line of text, so stopping at 1120px only leaves \
   the rest of a monitor empty. */\
@media (min-width:640px){\
.app.root{display:block;height:auto;overflow:visible}\
/* And the pane stops being its own scroll container, or the bar above it is \
   sticky inside a box that never scrolls — which is the same as not sticky. */\
.app.root .pane{overflow:visible;min-height:0}\
.app.root .topbar,.app.root main{max-width:none}}\
/* `.books` as well as `.row`, because at 640 `.rows .row{padding:13px 24px}` is \
   the same weight as `.row.split{padding:0}` and comes after it: the split row \
   keeps that padding, `.most` adds its own, and the whole row is indented \
   twice. */\
.books .row.split{position:relative;padding:0}\
.books .row.split .most{padding:13px 0 13px 28px}\
.books .most{min-width:0}\
/* A notebook's name is a directory's name, so it is set in the machine's face \
   like every other filename here. `display:block` is what makes the ellipsis \
   work at all — `text-overflow` does nothing to an inline box, so a long name \
   used to run across the row rather than end in one. The line is tight because \
   this is one word and not prose: at 1.6 it carries five pixels of air above \
   and below, and the name floats away from the facts under it. */\
.books .name{display:block;font-family:var(--mono);font-size:16px;line-height:1.3;\
color:var(--text);overflow:hidden;text-overflow:ellipsis;white-space:nowrap}\
/* `noda notebook ls` marks the active notebook with an uncoloured `*` in the \
   margin, and this is that mark in that margin. Contrast and not hue: the \
   colours in this palette say what a thing *is*, and which notebook a terminal \
   is pointed at is not that. \
   Absolute, so an inactive row reserves nothing and every name starts at the \
   same x. Its `top` is the row's padding plus half a line rather than `50%`, \
   which would slide down the row the moment the facts wrapped. */\
.books .mark{position:absolute;left:11px;top:23px;transform:translateY(-50%);\
width:7px;height:7px;border-radius:4px;background:var(--text)}\
.books .sr{position:absolute;width:1px;height:1px;overflow:hidden;clip-path:inset(50%)}\
/* The facts run from a common x, which is what makes a column of them a column. \
   `.rows` is in the selector to beat `.rows .row .under`'s `flex-end`: that \
   rule puts a note's day at the right of its row, and this is not that. */\
.rows.books .row .under{justify-content:flex-start;line-height:1.45;row-gap:0}\
.books .holds{color:var(--muted)}\
/* One fact fewer on a phone. At 390px the facts have about 215px once the row's \
   padding and the chip are out of it, and three of them want 250 — so they \
   wrapped, and a wrapped `.under` sets its 8px gap between the two lines, which \
   is wider than the 4px between the name and the first of them. The row read as \
   three loose bands rather than one row. \
   Dropping the file count is the argument the listing row already makes for \
   dropping the id on a phone: it is about the space, not about the count. \
   `:has` takes the separator out with it, or the line keeps a separator with \
   nothing left on one side of it. (Nothing here writes that separator out as a \
   character: a listing test asserts a page holding no tags holds no separator, \
   and this sheet rides inside every page.) */\
@media (max-width:639px){\
.books .under .files,.books .under .sep:has(+ .files){display:none}}\
/* Where it stands, in the pill the listing already wears in its corner. */\
.books .aside .pill{display:inline-flex;align-items:center;gap:6px;min-height:32px;\
padding:0 11px;border:1px solid var(--rule);border-radius:999px;font-size:12px;\
white-space:nowrap}\
.books .aside svg{width:14px;height:14px;flex:0 0 auto;color:var(--muted)}\
.books a.aside:active .pill{background:var(--press)}\
.books .row .aside{align-items:center}\
/* The rest of the tally, and the day: both are what a wider screen buys. */\
.count .more,.books .stamp{display:none}\
@media (min-width:640px){\
.count .more{display:inline}\
/* The row extends into the columns it was always made of. The name column has \
   a floor and a ceiling both: a notebook called `q` should not leave the facts \
   at the left margin, and one with a long name should not push them off the \
   screen. */\
.books .row.split .most{display:grid;grid-template-columns:minmax(9em,15em) minmax(0,1fr);\
gap:20px;align-items:baseline;padding:15px 0 15px 36px}\
.books .mark{left:19px;top:25px}\
/* A floor under the chip's column, so `.most` is the same width in every row. \
   Each row is its own grid, and a pill and a bare `no remote` are not the same \
   width — without this the day above sits at a different x in every row. \
   `align-self:stretch` is the tap target, and it is needed only here: at this \
   width `.rows .row{align-items:baseline}` outranks `.row.split`'s `stretch`, \
   so the chip stopped being as tall as the row and became as tall as its own \
   pill — 32px, under the 48 nothing here may go below. Below 640 it already \
   stretches, which is why only a laid-out page above it shows this. */\
.books .row.split .aside{min-width:15em;justify-content:flex-end;padding:0 24px;\
align-self:stretch}}\
/* The day it was last committed to, at the right of the row, where `-l` prints \
   a date — and only where there is a column to spare for it, which is the \
   bargain the listing's id makes too. */\
@media (min-width:1024px){\
.books .row.split .most{grid-template-columns:minmax(9em,16em) minmax(0,1fr) auto}\
.books .stamp{display:block;color:var(--muted);font-size:12.5px;justify-self:end}}\
";

#[cfg(test)]
mod tests {
    use super::*;

    /// `.rows.cols` flows whatever it holds, including an empty state that is
    /// one sentence of prose — which came out as "No tags yet" alone in the
    /// first column and the invitation broken across the next three.
    ///
    /// Asserted on the class, which is the whole of the decision: no script adds
    /// or removes it.
    #[test]
    fn an_empty_screen_is_not_poured_into_columns() {
        let no_tags = tags("work", &[]);
        assert!(no_tags.contains("No tags yet"), "{no_tags}");
        assert!(no_tags.contains("<main class=\"rows\">"), "{no_tags}");

        let no_files = files("work", &[]);
        assert!(no_files.contains("No files yet"), "{no_files}");
        assert!(no_files.contains("<main class=\"rows\">"), "{no_files}");

        // And still there the moment there is something to put in them.
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

    /// Already rendered and written out as it stands: this page escapes
    /// everything else and must not escape this. What keeps a note's raw HTML
    /// from becoming markup is `render`, which is where those tests are.
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
            },
            "in sync",
        );
        assert!(page.contains("<p>a <em>rendered</em> note</p>"), "{page}");
    }

    /// The minute would be either a UTC clock reading as a local one, or a
    /// conversion into a zone the note was not written in.
    #[test]
    fn a_listing_shows_the_day_and_never_a_clock() {
        assert_eq!(day("2019-03-14T16:21:00+08:00"), "2019-03-14");
        assert_eq!(day("2026-08-15T08:59:03Z"), "2026-08-15");
        // Not a shape noda wrote. It is still the only copy of what it says.
        assert_eq!(day("last tuesday"), "last tuesday");
        assert_eq!(day(""), "");
    }

    /// Both stamps whole, `Z` and all, as `ls -l` prints them: the minute can be
    /// read correctly here because the zone came with it.
    ///
    /// **What is asserted is the page a reader with no script gets.** The
    /// reader's own zone is `script::STAMPS`'s job and cannot be done from here
    /// — nothing in a request says where the reader is. What this page owes that
    /// script is the `datetime` to convert from and the `data-clock` saying this
    /// stamp has room for a time of day.
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
        // No terms is the ordinary case — a listing nobody has filtered.
        assert_eq!(highlight("a & b", &[]), "a &amp; b");
    }

    /// Nested `<mark>` would draw the overlap twice as dark, reading as a
    /// third kind of match.
    #[test]
    fn overlapping_terms_mark_one_run() {
        let out = highlight(
            "budgeting",
            &["budget".to_string(), "budgeting".to_string()],
        );
        assert_eq!(out, "<mark>budgeting</mark>");
    }

    /// A row, as a listing test needs one.
    fn row(id: &str, title: &str, shown: bool) -> Row {
        Row {
            id: id.into(),
            title: title.into(),
            tags: vec!["work".into()],
            stamp: Some("2026-08-12T08:03:00Z".into()),
            shown,
        }
    }

    /// **The listing's address does not change until somebody asks it to**, so
    /// every bookmark still names the page it named — and there is one address
    /// for the default listing rather than a plain and a spelled-out one.
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
        // The one in force is the bare address, and pressing it turns it round
        // — the only thing on this row that writes `r=1`.
        assert!(page.contains("href=\"/nb/work?r=1\""), "{page}");
        assert!(page.contains("href=\"/nb/work?sort=created\""), "{page}");
        assert!(page.contains("href=\"/nb/work?sort=updated\""), "{page}");
        assert!(page.contains("href=\"/nb/work?sort=title\""), "{page}");
        // A hidden field holding the default appears in the address the first
        // time ⏎ is pressed and never leaves.
        assert!(!page.contains("name=\"sort\""), "{page}");
        assert!(!page.contains("name=\"r\""), "{page}");
    }

    /// Every order `--sort` accepts, under the name it is accepted by.
    ///
    /// `Sort::ALL` rather than four literals, so an order added to the command
    /// cannot quietly fail to reach the browser — the direction this goes wrong
    /// in, the CLI being where an order gets added.
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

    /// **Pressing the order in force turns it round; pressing another starts it
    /// the way that order means first.**
    ///
    /// The second half is the one worth a test: `updated` means newest-first and
    /// `title` A-to-Z, and landing on the opposite of `--sort updated` is not
    /// what the chip looks like it will do.
    #[test]
    fn the_order_in_force_reverses_and_the_others_start_the_right_way_up() {
        let order = Order {
            sort: Sort::Updated,
            reversed: false,
        };
        let page = listing("work", &[], &Asked::nothing(), order, "in sync", None);
        // Itself, turned round.
        assert!(
            page.contains("href=\"/nb/work?sort=updated&amp;r=1\""),
            "{page}"
        );
        // The others, forwards.
        assert!(page.contains("href=\"/nb/work?sort=title\""), "{page}");
        assert!(page.contains("href=\"/nb/work\""), "{page}");

        // The press that reversed it undoes it, back to the same address.
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
        // The arrow is the only place direction is written, so it turns too.
        assert!(back.contains(UPWARDS), "{back}");
        assert!(!back.contains(DOWNWARDS), "{back}");
    }

    /// **A search and an order survive each other**: the chips carry what was
    /// typed and the form carries the order.
    ///
    /// The second half has no other way to work — a `GET` form sends its own
    /// fields and nothing else, so an ordered listing searched would come back
    /// in `slug` order with nothing on screen to say why.
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
        // And in the form, for the press that would otherwise drop it.
        assert!(
            page.contains("<input type=\"hidden\" name=\"sort\" value=\"created\">"),
            "{page}"
        );
        assert!(
            page.contains("<input type=\"hidden\" name=\"r\" value=\"1\">"),
            "{page}"
        );
    }

    /// **The stamp a row prints is the stamp the listing is ordered by.**
    ///
    /// `created` order printing `updated` is a column of days in no order beside
    /// a list claiming to be sorted, which cannot be told from a broken sort.
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
                extra: vec![],
                body: String::new(),
            },
        };
        assert_eq!(
            Row::of(&file, Sort::Created).stamp.as_deref(),
            Some("2019-03-14T16:21:00+08:00")
        );
        // The two orders that are not about a time keep `updated`.
        for sort in [Sort::Slug, Sort::Updated, Sort::Title] {
            assert_eq!(
                Row::of(&file, sort).stamp.as_deref(),
                Some("2026-08-12T08:03:00Z"),
                "{} stopped printing updated",
                sort.name()
            );
        }
    }

    /// **A note page's index pane is a frame, and a frame has no order in it.**
    ///
    /// The column is sent empty — about 290 bytes a row, none of it drawn below
    /// 1024px — and the order follows: it orders no rows. `script::PANES`
    /// fetches both from the route that decides them.
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
        };
        let page = note("work", &reading, "in sync");
        // The field is on a note page at every width, and is the way back.
        assert!(page.contains("<form class=\"searchbar\""), "{page}");
        assert!(!page.contains("class=\"sortbar\""), "{page}");
        assert!(!page.contains("name=\"sort\""), "{page}");

        // And the fragment that fills that column carries both.
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

    /// **The rows the query excluded are still on the page.** A listing the
    /// script could only narrow would need a second copy to filter from, and
    /// that copy goes stale. `hidden` is the browser's own attribute.
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
        // The shown one says why; the hidden one carries its title as it
        // stands, where the script would leave it anyway.
        assert!(page.contains("<mark>Budget</mark>"), "{page}");
        assert!(page.contains(">Reading list</div>"), "{page}");
        // The count is of what is on screen, out of what is on the page.
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
        // A notebook with nothing in it is not a query that found nothing.
        assert!(!page.contains("No notes match"), "{page}");
    }

    /// The page carries the sentence, switched off. The script decides *when*
    /// it applies — never what it says, a sentence living inside a script being
    /// one nothing can test.
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

    /// The signature: the id and the slug drawn as the one filename they are.
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
            },
            "in sync",
        );
        assert!(page.contains(">em0xvn4e</span>"), "{page}");
        assert!(page.contains(">-budget-review</span>"), "{page}");
        assert!(page.contains(">.md</span>"), "{page}");
    }

    /// Tags are the one thing a note may not have, which is also why they go
    /// last everywhere else.
    #[test]
    fn a_row_without_tags_prints_no_separator() {
        let rows = [Row {
            id: "k3f9".into(),
            title: "Reading list".into(),
            tags: vec![],
            stamp: Some("2026-08-12T08:03:00Z".into()),
            shown: true,
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
        // **The clock is not shown in a listing, and now it is in one.** An
        // instant cannot be moved into another zone without the time of day, so
        // the rule is that no reader is *shown* it, not that the bytes lack it.
        // A UTC clock with its `Z` cut off reads as a local one, wrong by the
        // reader's offset and in a way nothing on the page admits to. In an
        // attribute it keeps its `Z` and is read by the one thing that knows
        // where the reader is.
        for shown in page
            .split('>')
            .skip(1)
            .filter_map(|rest| rest.split_once('<'))
        {
            assert!(!shown.0.contains("08:03"), "a clock is on the page: {page}");
        }
    }

    /// A row is `ls -l`'s row in its order: the id leads, tags come last, and
    /// the day is written twice because only one of its places is ever on
    /// screen.
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
        // Tags last: the column a note may not have.
        let under = page.split("<div class=\"under\">").nth(1).unwrap();
        assert!(
            under.starts_with(
                "<time class=\"when\" datetime=\"2026-08-12T08:03:00Z\">2026-08-12</time>\
                 <span class=\"sep\">·</span><span class=\"tags\">work</span>"
            ),
            "{under}"
        );
    }

    /// `a OR b c` is `(a OR b) AND c`, drawn rather than explained, in the
    /// reader's own words.
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

    /// The box is written anyway, as the hint is: the script decides when it
    /// applies, never what it says.
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

    /// The frame with the field in it and nothing asked: the box is there for
    /// the script, the count is not, a count of rows nobody has being no fact.
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
            },
            "in sync",
        );
        assert!(
            page.contains("<div class=\"parse\" hidden></div>"),
            "{page}"
        );
        assert!(page.contains("<span class=\"count\"></span>"), "{page}");
    }

    /// Everything in the grouping came off the query line, so it is the query
    /// line that must not be able to write markup.
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

    /// The viewport is the page's own and the themes are in the sheet it links.
    /// The pair is the claim: a page says how wide it is read at, and where to
    /// get the rest.
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
        }
    }

    /// **The whole of the fragment contract, said as four assertions.** A part
    /// is what the page was cut out of, so the two cannot come to disagree
    /// whatever either grows into.
    ///
    /// Containment is the check because containment is the claim: nothing here
    /// compares two renderings for looking alike.
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

        // Two parts off one route, the wider carrying the tab's name.
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
        // Both panes: going back has to put the reading side right too.
        assert!(panes.contains("class=\"pane index\""), "{panes}");
        assert!(panes.contains("class=\"pane read\""), "{panes}");
        assert!(panes.contains("Read me"), "{panes}");
    }

    /// What it leaves out is the reason for it: none of that is on the screen
    /// the fragment goes into.
    ///
    /// It used to say how much — 48 of a note page's 52 KB — and cannot now that
    /// `asset.rs` took the same 46 KB off the page. What is left is the shape:
    /// the pane and nothing around it, against four addresses.
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
        // The needle is a declaration only the stylesheet holds, so finding it
        // on a page means the sheet was written back into one.
        assert!(
            !page.contains("--tap:48px"),
            "the stylesheet is back inside the page"
        );
        assert!(
            page.contains("<link rel=\"stylesheet\" href=\"/a/style."),
            "{page}"
        );
    }

    /// The one fact carried out of the head, and the one that would be a
    /// decision if the script made it. The server's own `<meta>`, from the same
    /// function the whole page uses.
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

        // Nothing to wait for, and neither says to.
        let quiet = standing_main("work", &still(), None);
        assert!(!quiet.contains("http-equiv"), "{quiet}");
        assert!(quiet.starts_with("<main>"), "{quiet}");
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

    /// Sent empty and closed: what goes in it is a walk of every note, and a
    /// note page reads one file — so the walk happens where the column is drawn
    /// or not at all.
    #[test]
    fn the_note_page_sends_the_margin_note_empty_and_hidden() {
        let page = note("work", &reading(), "in sync");
        assert!(page.contains("<aside class=\"beside\" hidden>"), "{page}");
        assert!(page.contains("<div class=\"answer\"></div>"), "{page}");
        assert!(page.contains(">Backlinks</div>"), "{page}");
    }

    /// `display:block` on a class chain out-ranks the `hidden` attribute, so the
    /// rule drawing the column has to exempt a closed one — or a scriptless
    /// reader gets a heading over an empty column forever.
    #[test]
    fn the_margin_note_is_drawn_only_when_it_holds_something() {
        let sheet = stylesheet();
        assert!(sheet.contains(".beside{display:none}"), "{sheet}");
        assert!(
            sheet.contains(".read .beside:not([hidden]){display:block"),
            "the margin note can now out-order its own hidden attribute"
        );
    }

    /// The two things argued over rather than the markup: that Delete is
    /// *there*, and that it is the only item wearing the colour — a second
    /// `danger` would mean the mark had stopped saying anything.
    #[test]
    fn the_note_bar_carries_delete_and_marks_only_that() {
        let page = note("work", &reading(), "in sync");
        assert!(
            page.contains("/n/em0xvn4e/delete\" class=\"danger\""),
            "{page}"
        );
        assert_eq!(page.matches("class=\"danger\"").count(), 1, "{page}");
        assert!(page.contains("<span>Delete</span>"), "{page}");
        // Gone rather than doubled up with the bar: one action reached two
        // ways is a page you have to read.
        assert!(!page.contains("perilous"), "{page}");
    }

    /// The strip sits above the form, where its padding is the pane's and its
    /// rule reaches both edges. The delete page had one *inside* the form, inset
    /// twice and 16px right of the button it was about.
    #[test]
    fn the_delete_page_says_its_piece_from_outside_the_form() {
        let about = About::of("em0xvn4e", "budget-review", "Budget review");
        let page = deleting("work", &about);
        let said = page.find("class=\"said\"").expect("the page says nothing");
        let form = page.find("<form").expect("the page has no form");
        assert!(said < form, "the paragraph is back inside the form: {page}");
    }

    /// The README stands in the same `main.note` and has no aside, but the wide
    /// grid reserves a column for one — so it asks `at-note`, or the README is
    /// shoved off centre by 236px of nothing.
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

    /// The listing route links the script because picking a row turns that page
    /// into a note page without a reload. The address rather than the source:
    /// what a page must get right is which scripts it names.
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

    /// The two screens showing an instant link the script that restates it where
    /// the reader is. The tags screen does not, deliberately: what it draws in
    /// the same class is a count of notes.
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

    /// A `due:` is a calendar day somebody typed, not an instant. Converting it
    /// would move an item due today into tomorrow for a reader in another zone,
    /// so it must not carry the `datetime` the script converts.
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

    /// A row is `noda status` in a line, and its two destinations are two links:
    /// the notebook, and where it stands.
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

    /// `noda status` prints the files line only when there are files.
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

    /// The one case that is not a link, for the files page's reason.
    #[test]
    fn a_notebook_with_no_remote_keeps_the_words_and_loses_the_link() {
        let page = notebooks(&[Book {
            drift: None,
            ..book("work")
        }]);
        assert!(page.contains("no remote"), "{page}");
        assert!(!page.contains("/status"), "{page}");
    }

    /// `noda notebook ls`'s `*`, in the same margin — and in words, a dot not
    /// being one.
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

    /// No rail, no bar, and the class that stops the layout keeping a column.
    #[test]
    fn the_front_page_is_the_one_screen_with_neither_rail_nor_bar() {
        let page = notebooks(&[book("work")]);
        assert!(page.contains("class=\"app root\""), "{page}");
        // Not the bare word: `actionbar` is in the stylesheet.
        assert!(!page.contains("<nav class=\"actionbar\""), "{page}");
        assert!(!page.contains("class=\"fab\""), "{page}");
    }

    /// Always the count, and what they hold when there is room — the listing
    /// row's bargain with the day.
    #[test]
    fn the_corner_counts_the_notebooks_and_leaves_the_rest_to_the_stylesheet() {
        let page = notebooks(&[book("journal"), book("work")]);
        assert!(
            page.contains("2 notebooks<span class=\"more\"> · 20 notes</span>"),
            "{page}"
        );
    }
}
