//! The HTML, and nothing else.
//!
//! Every function here takes what a page is about and returns a string. Nothing
//! opens a repository, binds a socket or knows what a request is — which is the
//! same rule `tui/app.rs` follows for the same reason: the interesting part of
//! an interface is what it puts on the screen, and that is worth being able to
//! test without one.
//!
//! **The type system is the palette's argument, said in type instead of colour.**
//! `style.rs` opens with two rules: colour marks what a line *is*, never what it
//! means; and noda never colours a note's own text, because that is the user's
//! file. Here the machinery — ids, filenames, tags, stamps, the search field —
//! is set in the monospace a terminal would have used, and the parts that are
//! the reader's own — titles, note bodies — are set to be read. The line falls
//! in exactly the same place. A terminal cannot draw that distinction because it
//! has one face; a browser can, so it does.
//!
//! No JavaScript, and not as an achievement to announce: the search field is a
//! form, every row is a link, and there is nothing on any of these pages that
//! needs a script to work. What arrives later (filtering as you type) is an
//! enhancement over this, never a replacement for it.

use std::fmt::Write;

use crate::notebook::NoteFile;
use crate::web::{script, theme};

/// A notebook, as the front page lists it.
pub struct Book {
    pub name: String,
    pub notes: usize,
    /// What `status` says about the remote, already in words: a count is not
    /// what anybody wants to be told about a remote they have not set up.
    pub remote: String,
}

/// A note, as a listing names it. The row `ls` prints, minus the id.
///
/// The id is gone for a reason worth writing down: `ls` shows it because the
/// next thing you do is type it, and here the next thing you do is press it. It
/// is still the address — the link is `/nb/<book>/n/<id>` — it is just not
/// something the reader has to carry any more.
pub struct Row {
    pub id: String,
    pub title: String,
    pub tags: Vec<String>,
    pub updated: Option<String>,
    /// Whether the query lets this row through.
    ///
    /// **Every row of the notebook is on the listing whatever is typed** — the
    /// excluded ones arrive with `hidden` on them rather than being left out.
    /// A page that omitted them would be a page the enhancement layer could
    /// only ever narrow further, because a script cannot put back a row the
    /// server never sent; and then filtering-as-you-type would need a second
    /// copy of the list to filter *from*, which is the copy that goes stale.
    ///
    /// `hidden` is not a class. It is the attribute every browser's own
    /// stylesheet already hides, so the scriptless page gets the same answer
    /// from the same markup with nothing of noda's involved.
    pub shown: bool,
}

/// A file the notebook holds that is not a note, as the files page lists it.
pub struct Held {
    pub name: String,
    pub size: u64,
    /// What the server will say it is — the same answer the download itself
    /// carries, so the page cannot promise one thing and the file be another.
    pub kind: String,
    /// How many notes link to it. Zero is what `doctor --links` calls an orphan.
    pub used: usize,
}

/// One tag, and how many notes carry it.
pub struct Tally {
    pub tag: String,
    pub notes: usize,
}

/// One unticked box, and the note that holds it.
///
/// The note is named by title rather than by filename: this is a list of things
/// to do, and which file a task is written in is the answer to a second
/// question. The id is still the address the row links to.
pub struct Task {
    pub id: String,
    pub title: String,
    /// The item's own words, with the `due:` term already lifted out of them.
    pub text: String,
    pub due: Option<String>,
    /// Whether that date has gone past — worked out against the reader's own
    /// day, which is a thing only the server knows.
    pub overdue: bool,
}

/// What a backlinks page is about: a note, or one of the notebook's files.
pub struct Subject {
    /// What to call it — a note's title, a file's name.
    pub what: String,
    /// Where it is, for the way back.
    pub at: String,
    /// Whether the name is the machine's rather than the reader's. A filename is
    /// set in monospace wherever it appears, and a title never is.
    pub mono: bool,
}

/// A note, as its own page shows it.
pub struct Reading {
    pub id: String,
    pub slug: String,
    pub title: String,
    pub tags: Vec<String>,
    pub updated: Option<String>,
    /// The body, **already HTML** — `web::render::body` made it, and it is the
    /// one string on any of these pages that is written out as it stands. The
    /// field is not called `body` for that reason: `escape(&reading.body)` was
    /// the right line while a note was shown as text, and it should not be
    /// possible to leave it there by accident now that it is not.
    pub rendered: String,
}

impl Row {
    pub fn of(file: &NoteFile) -> Row {
        Row {
            id: file.id.clone(),
            title: file.note.title.clone(),
            tags: file.note.tags.clone(),
            updated: file.note.updated.clone(),
            shown: true,
        }
    }
}

/// The five characters that would otherwise be markup.
///
/// Hand-written rather than pulled in: it is five characters, and a note's body
/// is arbitrary text that reaches this on every page, so the one place it
/// happens should be readable in one screen.
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
/// **The day and not the minute, and the reason is that the minute cannot be
/// shown without lying.** noda writes its own stamps in UTC — `...T09:54:23Z` —
/// and the rule is that a rendering uses the offset that was recorded, never
/// this server's. Obeyed literally, a note written at six in the evening is
/// shown as ten in the morning; obeyed with the `Z` cut off to save room, it is
/// shown as ten in the morning *and looks like local time*, which is worse than
/// either. Converting instead would put the server's zone into a fact about the
/// note, and the server is not where the note was written.
///
/// A day has none of that in it. It is also the granularity a listing wants: a
/// row answers "when did I last touch this", not "at which minute".
///
/// The minute is still there for anyone who wants it — the note's own page
/// prints the stamp exactly as the file holds it, `Z` and all, the way `ls -l`
/// does. Anything that is not a date is returned untouched: a stamp noda did not
/// write is still the only copy of what it says.
fn day(value: &str) -> String {
    let bytes = value.as_bytes();
    let dated = bytes.len() >= 10 && bytes[4] == b'-' && bytes[7] == b'-';
    if dated {
        value[..10].to_string()
    } else {
        value.to_string()
    }
}

/// `text`, escaped, with every run matching one of `terms` wrapped in `<mark>`.
///
/// The matching is done before the escaping and the pieces are escaped as they
/// are cut, which is the order that matters: escaping first would have this
/// searching `&amp;` for `&`, and marking first without escaping would put a
/// note's own angle brackets into the markup.
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
/// The stylesheet is inline. One request answers a whole page, which is what a
/// phone on the far end of a tailnet wants; the alternative buys caching and
/// costs a round trip on the first view of every session, plus a question about
/// invalidating it that nothing here is big enough to be worth asking.
/// `app` is the classes the layout hangs off, and there are only ever three of
/// them: `split` for the two screens made of panes, `at-list`/`at-note` for
/// which of the two is being shown, and `indexed` for whether the index pane
/// arrived with rows in it. Every one is a fact about the route, decided by the
/// handler that knows it and never worked out again from the markup.
fn shell(title: &str, app: &str, body: &str) -> String {
    dressed(title, app, None, &[], body)
}

/// The shell, plus the script that makes this page quicker and nothing else.
///
/// Inline, for the same reason the stylesheet is: one request answers a whole
/// page. It goes at the end of the body rather than in the head, because it
/// reads the rows and there is no `defer` on an inline script — and because a
/// script that runs after the page is drawn cannot delay the page being drawn,
/// which is the only guarantee that matters to something optional.
fn scripted(title: &str, app: &str, scripts: &[&str], body: &str) -> String {
    dressed(title, app, None, scripts, body)
}

/// The shell, plus the one thing a page may ask the browser to do on its own.
///
/// `<meta http-equiv="refresh">` is how a page with no JavaScript comes back for
/// news, and the network screen is the only page here that has any. It is a
/// full reload of a page that is a few hundred bytes, which is the cost of not
/// requiring a script to find out whether a push finished — and the same reload
/// the reader would perform by hand, so nothing new can go wrong in it.
fn dressed(title: &str, app: &str, again_in: Option<u32>, scripts: &[&str], body: &str) -> String {
    let refresh = again_in.map_or_else(String::new, |seconds| {
        format!("<meta http-equiv=\"refresh\" content=\"{seconds}\">\n")
    });
    let enhancement = scripts
        .iter()
        .filter(|source| !source.is_empty())
        .map(|source| script::tag(source))
        .collect::<String>();
    let classes = if app.is_empty() {
        String::from("app")
    } else {
        format!("app {app}")
    };
    format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n\
         <meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
         {refresh}<title>{}</title>\n<style>{}{}</style>\n</head>\n<body>\n\
         <div class=\"{classes}\">\n{}</div>\n{enhancement}</body>\n</html>\n",
        escape(title),
        theme::stylesheet(),
        CSS,
        body
    )
}

/// A note being written, as the form holds it.
///
/// Kept as the strings that were typed rather than as anything parsed, because
/// its other job is to be handed back when the write is refused: a reader who
/// has been told a tag is not allowed should find their words where they left
/// them, not an empty form.
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

/// The bar along the bottom, and the reason it exists at all.
///
/// PR 1 shipped without one, because there was nothing to put in it and two
/// greyed-out buttons are not a design. It arrives now with something in every
/// slot it has. Adding it changed nothing above it — a fixed strip at the foot
/// of a page is an extension, not a rearrangement, which is the rule this
/// project applies to a listing's row and applies here to its chrome.
fn action_bar(items: &[(&str, &str, String, bool)]) -> String {
    let mut out = String::from("<nav class=\"actionbar\">");
    for (icon, label, href, here) in items {
        let _ = write!(
            out,
            "<a href=\"{}\"{}><svg viewBox=\"0 0 24 24\" aria-hidden=\"true\">{icon}</svg>\
             <span>{label}</span></a>",
            escape(href),
            // `aria-current` and not a class, because it is what the attribute
            // is for: a screen reader says "current page" and the stylesheet
            // hangs the brighter colour off the same fact. One statement, read
            // two ways.
            if *here { " aria-current=\"page\"" } else { "" }
        );
    }
    out.push_str("</nav>");
    out
}

const NEW: &str = "<path d=\"M12 4.5v15M4.5 12h15\"/>";
/// A page with a folded corner and two lines of writing on it: what the
/// notebook is mostly made of. Not a book — a notebook is a directory of files,
/// and the thing you press this to reach is a list of them.
const NOTES: &str = "<path d=\"M5.5 4.5h9L19 9v10.5h-13.5z\"/><path d=\"M14.5 4.5V9H19\"/>\
<path d=\"M9 13h6\"/><path d=\"M9 16.5h4\"/>";
const EDIT: &str = "<path d=\"M4 20h4L19 9l-4-4L4 16z\"/>";
const TAGS: &str = "<path d=\"M4 4h7l9 9-7 7-9-9z\"/><circle cx=\"8\" cy=\"8\" r=\"1.4\"/>";
const RENAME: &str = "<path d=\"M4 7V5h16v2\"/><path d=\"M12 5v14\"/><path d=\"M9 19h6\"/>";
/// A box with a tick in it, which is what a todo *is* here — the GFM checkbox
/// every other Markdown reader draws.
const TODO: &str = "<rect x=\"4\" y=\"4\" width=\"16\" height=\"16\" rx=\"3.5\"/><path d=\"M8.5 12.2l2.6 2.6 4.6-5.4\"/>";
/// A paperclip: what the notebook holds that is not a note, in the shape
/// everything on a phone uses for exactly that.
const FILES: &str = "<path d=\"M18.5 10.5 11 18a4 4 0 0 1-5.7-5.7l7.8-7.8a2.6 2.6 0 0 1 3.7 3.7\
 l-7.7 7.7a1.2 1.2 0 0 1-1.7-1.7l7.1-7.1\"/>";
/// An arrow arriving at a line, because backlinks are the inbound half. The
/// line is the note being pointed at, and the arrow is everything pointing.
const LINKS: &str = "<path d=\"M19 5v14\"/><path d=\"M4 12h11\"/><path d=\"M11 8l4 4-4 4\"/>";
/// Two arrows passing, one up and one down: what a notebook and its remote do to
/// each other. Not a cloud — a notebook syncs with a repository somebody else's
/// machine is holding, and half the time that machine is their own.
const SYNC: &str = "<path d=\"M7 9l5-5 5 5\"/><path d=\"M12 4v10\"/>\
<path d=\"M17 15l-5 5-5-5\"/><path d=\"M12 20V10\"/>";

/// Where the notebook stands, in the corner of the screen you are already on.
///
/// It is the way to the network screen and it is also the answer that screen
/// exists to give — "is there anything to sync" is worth knowing without
/// pressing anything, and a notebook that is up to date should be able to say so
/// without being asked twice.
/// The pill inside is what is drawn; the link around it is what is pressed.
/// Nothing on a phone may be smaller than 48px, and a 48px pill in a 56px bar
/// would be a bar with a button wedged into it — so the target is the size the
/// rule asks for and the ink is the size the design wants.
///
/// The label repeats the words because a narrow screen may have to shorten them:
/// the text can end in an ellipsis and the label never does.
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

/// The back chevron, and the only icon PR 1 has.
///
/// Inline SVG rather than a character like `‹`: a glyph is whatever the reader's
/// font decides it is — weight, size and where it sits on the line all out of
/// our hands — and this one has to look the same on every phone that reaches the
/// notebook. It is also why it can be given a stroke width at all.
const BACK: &str =
    "<svg viewBox=\"0 0 24 24\" aria-hidden=\"true\"><path d=\"M15 4.5 7.5 12 15 19.5\"/></svg>";

/// Which of the notebook's screens is being drawn, so the bar can say so.
///
/// All four are on the bar. An earlier design left `Notes` off it and made the
/// chevron the only way back to the listing, on the argument that the bar held
/// the places you go *from* the listing. Two things undid that. A rail is read
/// as a list of where you can be, and one that omitted the place you spend most
/// of your time read as an omission rather than as an argument; and on a screen
/// wide enough to hold both panes the listing is no longer somewhere you leave,
/// so "from the listing" had stopped describing anything.
#[derive(Clone, Copy, PartialEq, Eq)]
enum At {
    Notes,
    Tags,
    Todo,
    Files,
    /// The network screen, which is the one notebook screen not on the bar —
    /// it is reached from the chip in the corner, because it is about the
    /// notebook as a whole rather than about something inside it. It is a
    /// variant rather than an absence so that the bar is told where the reader
    /// is on every screen that carries it, and marks nothing only when nothing
    /// on it is where they are.
    Status,
}

/// The bar every notebook-level screen carries, and the one button that is not
/// on it.
///
/// **Four places and one action, told apart by not being in the same row.**
/// Notes, Tags, Todo and Files are somewhere to go; New is something to do, and
/// a row that mixes the two is a row you have to read rather than aim at. The
/// button is lifted off it and set where a thumb already rests.
///
/// The bar is the same four on every screen, because a bar whose contents
/// changed from screen to screen would be worse than no bar. It is also how the
/// files page stopped being reachable only by typing its address, which is what
/// it was until this existed.
fn notebook_bar(book: &str, here: At) -> String {
    let at = escape(book);
    // Wrapped together, and the wrapper is what sticks to the bottom rather
    // than the bar inside it. The button has to travel with the bar: on a
    // screen with little on it the bar has not stuck to anything yet and sits
    // under the last row, and a button pinned to the window would be floating on
    // its own in the space below.
    format!(
        "<div class=\"foot\">{}<a class=\"fab\" href=\"/nb/{at}/new\" aria-label=\"New note\">\
         <svg viewBox=\"0 0 24 24\" aria-hidden=\"true\">{NEW}</svg></a></div>",
        action_bar(&[
            (NOTES, "Notes", format!("/nb/{at}"), here == At::Notes),
            (TAGS, "Tags", format!("/nb/{at}/tags"), here == At::Tags),
            (TODO, "Todo", format!("/nb/{at}/todo"), here == At::Todo),
            (FILES, "Files", format!("/nb/{at}/files"), here == At::Files),
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

/// The front page: which notebooks there are.
pub fn notebooks(books: &[Book]) -> String {
    let rows = if books.is_empty() {
        // An empty screen is an invitation to act, and the act is not on this
        // machine's web server — a notebook is made at a terminal.
        "<div class=\"empty\"><b>No notebooks yet</b>Run <code>noda init</code> in a terminal to make the first one.</div>".to_string()
    } else {
        let mut out = String::new();
        for book in books {
            let _ = write!(
                out,
                "<a class=\"row\" href=\"/nb/{}\"><div class=\"name\">{}</div>\
                 <div class=\"under\"><span class=\"when\">{} · {}</span></div></a>",
                escape(&book.name),
                escape(&book.name),
                plural(book.notes, "note"),
                escape(&book.remote)
            );
        }
        out
    };
    shell(
        "noda",
        "",
        &format!(
            "<section class=\"pane\">\
             <header class=\"topbar\"><span class=\"here lead\">noda</span>\
             <span class=\"count\">{}</span></header>\
             <main class=\"rows cols\">{rows}</main></section>",
            plural(books.len(), "notebook")
        ),
    )
}

/// A notebook's notes, narrowed by whatever was typed.
///
/// `total` is how many the notebook holds, which is only interesting when it
/// differs from how many are shown — a reader who has filtered to nothing needs
/// to be told there is something to go back to.
///
/// `problem` is why what has been typed is not a query yet. It is said and the
/// notes are left alone — the same call `style::INVALID` exists for in the
/// browser: half a query is what every query looks like on the way to being
/// one, and emptying the screen over an unfinished thought is not an answer.
///
/// `drift` is where the notebook stands against its remote, already in words. It
/// rides in the corner of the bar as a link to the network screen — the one
/// screen that is not on the bar along the bottom, because the bar holds places
/// inside the notebook and this is about the notebook as a whole.
pub fn listing(
    book: &str,
    rows: &[Row],
    query: &str,
    terms: &[String],
    problem: Option<&str>,
    drift: &str,
    front: Option<&str>,
) -> String {
    let total = rows.len();
    let shown = rows.iter().filter(|row| row.shown).count();

    let mut body = String::new();
    for row in rows {
        let under = [tag_line(&row.tags), when(row.updated.as_deref())]
            .into_iter()
            .filter(|piece| !piece.is_empty())
            .collect::<Vec<_>>()
            .join("<span class=\"sep\">·</span>");
        let _ = write!(
            body,
            "<a class=\"row\"{} href=\"/nb/{}/n/{}\"><div class=\"title\">{}</div>\
             <div class=\"under\">{under}</div></a>",
            if row.shown { "" } else { " hidden" },
            escape(book),
            escape(&row.id),
            // Marked only on the rows that are being shown for a reason. A
            // hidden row carries its title unmarked, which is the state the
            // script would put it in anyway when it lets the row back through
            // for a different query.
            if row.shown {
                highlight(&row.title, terms)
            } else {
                escape(&row.title)
            }
        );
    }

    // An empty notebook and an empty result are different sentences, and only
    // the second one is ever hidden: the first is the whole state of the
    // notebook, and no amount of typing changes it.
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
            escape(query),
            plural(total, "note"),
            escape(book)
        );
    }

    let counted = if query.is_empty() {
        total.to_string()
    } else {
        format!("{shown} of {total}")
    };

    // `indexed`, because the rows are right here in the markup. It is the same
    // class the script sets on a note route, and it means the same thing in
    // both places: this pane has a listing in it.
    scripted(
        &format!("{book} — noda"),
        "split at-list indexed",
        &[script::LISTING, script::PANES],
        &format!(
            "{}{}{}",
            index_pane(book, query, &counted, problem, drift, &body),
            front_pane(book, front),
            notebook_bar(book, At::Notes)
        ),
    )
}

/// The index pane's frame, and what is under it.
///
/// The frame is the same on both routes that draw it: which notebook, where it
/// stands, and a search field that is a `GET` form on its own — so a reader
/// with no script can still narrow the listing from a note page, by submitting
/// it and landing on the listing.
///
/// `rows` is the part that differs. The listing route puts its notes here; a
/// note route leaves it empty and `script::PANES` fills it in, because a phone
/// that will never show this column should not be sent a copy of it. `counted`
/// is empty for the same reason: a count of rows nobody has is not a fact yet.
fn index_pane(
    book: &str,
    query: &str,
    counted: &str,
    problem: Option<&str>,
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
         enterkeyhint=\"search\" aria-label=\"Search this notebook\">{}{}</form>\
         <main class=\"rows\">{rows}</main></section>",
        back("/", "the notebooks"),
        escape(book),
        drift_chip(book, drift),
        escape(book),
        escape(query),
        // Written by the server and hidden by the server, so that the only
        // thing the script does with it is decide when it applies. A sentence
        // that exists only inside a script is a sentence nothing else can test
        // the wording of.
        hint(),
        problem.map_or_else(String::new, |why| format!(
            "<p class=\"problem\">{}</p>",
            escape(why)
        )),
    )
}

/// The reading pane with no note picked, which only a screen wide enough to
/// show two panes ever sees.
///
/// A notebook that has a `README.md` has already written the page that is about
/// the whole of it — it is what `noda readme` writes and what a git host shows
/// above the file list — so that is what stands here rather than an invitation
/// to press something. Without one, the invitation.
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

/// What the listing says while the script is answering instead of the server.
///
/// Shown only when the two answers can differ — a bare word or `text:` reads
/// the body, and the body is not on the page. It names both halves: what has
/// been narrowed, and the key that finishes the job. Hidden the rest of the
/// time, including on every scriptless page, where it is never true.
fn hint() -> String {
    "<p class=\"hint\" hidden>Filtered by title and tag — press ⏎ to search the text.</p>"
        .to_string()
}

/// Everything the notebook holds that is not a note.
///
/// One row per file, saying the three things that are true of it: how big it
/// is, what it will arrive as, and how many notes point at it. The last is the
/// one worth the walk — a file nothing points at is what `doctor --links` calls
/// an orphan, and this is the same question answered in the same way rather
/// than a second opinion about it.
pub fn files(book: &str, held: &[Held]) -> String {
    let body = if held.is_empty() {
        "<div class=\"empty\"><b>No files yet</b>Run <code>noda file add diagram.png</code> \
         in a terminal to put one here.</div>"
            .to_string()
    } else {
        let mut out = String::new();
        for file in held {
            let under = [size(file.size), escape(&file.kind)].join("<span class=\"sep\">·</span>");
            // Two targets in one row, side by side rather than nested — a link
            // inside a link is not a thing HTML has. The row is the file and
            // goes to the file; the count is a question about it and goes to
            // the answer. It is the only way a notebook's own files can be asked
            // what points at them, since a file has no page of its own.
            //
            // **Zero is not a link.** What `doctor --links` calls an orphan is
            // said in the same words it has always been said in, and it stays
            // text: a link whose destination is a page saying "nothing links
            // here" is a press that cannot tell you anything you had not already
            // read.
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
        out
    };

    shell(
        &format!("Files — {book} — noda"),
        "",
        &format!(
            "<section class=\"pane\">\
             <header class=\"topbar\">{}<span class=\"here\">Files</span>\
             <span class=\"count\">{}</span></header>\
             <main class=\"rows cols wide\">{body}</main></section>{}",
            back(&format!("/nb/{}", escape(book)), book),
            held.len(),
            notebook_bar(book, At::Files)
        ),
    )
}

/// A file's size, in the units a person would say it in.
///
/// Powers of two and one decimal place, which is what every file manager shows;
/// a notebook's attachments are pictures and PDFs, and knowing one is 4.2 MB
/// rather than 4,404,019 bytes is the whole of what this line is for.
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

/// Every tag in the notebook, commonest first.
///
/// The browser's order, for the browser's reason: sorted by name alone, the four
/// tags a notebook actually runs on are buried under every one-off ever typed.
/// Alphabetical within a count, so the list does not reshuffle between visits.
///
/// A row is a link into the listing, narrowed to that tag — which is what makes
/// this a way of getting somewhere rather than a report. `query::scoped` writes
/// the query, because the field it lands in splits the way a shell does and a
/// tag with a space in it has to arrive quoted.
pub fn tags(book: &str, tallies: &[Tally]) -> String {
    let body = if tallies.is_empty() {
        "<div class=\"empty\"><b>No tags yet</b>Tags come from a note's frontmatter. \
         Open a note and press Tags to add one.</div>"
            .to_string()
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
        out
    };

    shell(
        &format!("Tags — {book} — noda"),
        "",
        &format!(
            "<section class=\"pane\">\
             <header class=\"topbar\">{}<span class=\"here\">Tags</span>\
             <span class=\"count\">{}</span></header>\
             <main class=\"rows cols\">{body}</main></section>{}",
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

    shell(
        &format!("Links to {} — noda", subject.what),
        "",
        &format!(
            "<section class=\"pane\">\
             <header class=\"topbar\">{}<span class=\"here\">Backlinks</span>\
             <span class=\"count\">{}</span></header>\
             <main class=\"rows\">\
             <p class=\"said\">What links to <span class=\"{}\">{}</span></p>{body}\
             </main></section>",
            back(&subject.at, &subject.what),
            rows.len(),
            if subject.mono { "mono" } else { "subject" },
            escape(&subject.what)
        ),
    )
}

/// One note, and — on a screen wide enough — the listing it came from.
///
/// `drift` is for the index pane's own bar, which is the notebook's bar rather
/// than the note's. It costs two refs compared, which is what the listing route
/// already pays on every visit.
pub fn note(book: &str, reading: &Reading, drift: &str) -> String {
    let at = format!("/nb/{}/n/{}", escape(book), escape(&reading.id));
    let meta = [tag_line(&reading.tags), updated(reading.updated.as_deref())]
        .into_iter()
        .filter(|piece| !piece.is_empty())
        .collect::<Vec<_>>()
        .join("<span class=\"sep\">·</span>");

    // Past the end of the note, and quiet. Deleting is the one thing here that
    // cannot be undone by doing it again, so it is the one thing not on the bar
    // under a thumb — you scroll the whole note to reach it, and that friction
    // is proportionate rather than invented.
    let perilous =
        format!("<p class=\"perilous\"><a href=\"{at}/delete\">Delete this note</a></p>");
    // Nothing marked as current: a note's bar is four things to do *to* the note
    // you are already on, and there is no "here" among them to be at.
    let bar = action_bar(&[
        (EDIT, "Edit", format!("{at}/edit"), false),
        (TAGS, "Tags", format!("{at}/tags"), false),
        (RENAME, "Rename", format!("{at}/rename"), false),
        (LINKS, "Links", format!("{at}/backlinks"), false),
    ]);
    let home = format!("/nb/{}", escape(book));

    // No `indexed`, and the pane it names is sent empty. The listing is worth
    // about 290 bytes a note — 57KB at two hundred, half a megabyte at two
    // thousand — and below 1024px not one of those bytes is ever drawn. So the
    // frame goes out and `script::PANES` asks for the rest, but only where the
    // column is on screen. With no script the grid is the tablet's two columns:
    // the note, whole, and the chevron back to the listing, which is what a
    // note page has always been.
    scripted(
        &format!("{} — noda", reading.title),
        "split at-note",
        &[script::PANES],
        &format!(
            "{}<section class=\"pane read\">\
             <header class=\"topbar\">{}<span class=\"here\">{}</span></header>\
             <main class=\"note\">\
             <div class=\"note-head\"><h1>{}</h1>\
             <div class=\"filename\"><span class=\"id\">{}</span>\
             <span class=\"slug\">-{}</span><span class=\"ext\">.md</span></div>\
             <div class=\"note-meta\">{meta}</div></div>\
             <div class=\"body\">{}</div>\
             {perilous}</main>{bar}</section>{}",
            index_pane(book, "", "", None, drift, ""),
            back(&home, book),
            // The note, not the notebook. On a phone this bar is the whole
            // chrome and either would do; beside an index pane already headed
            // with the notebook's name, repeating it says nothing and the one
            // thing the bar could have said goes unsaid.
            escape(&reading.title),
            escape(&reading.title),
            escape(&reading.id),
            escape(&reading.slug),
            reading.rendered,
            notebook_bar(book, At::Notes),
        ),
    )
}

/// What every form page is wrapped in: a bar with a way back, an optional line
/// saying what went wrong, and the form itself.
///
/// **`<main>` around both, and it is not decoration.** The wide layout puts one
/// column down the middle of the screen by capping `main`, so anything outside
/// it runs the whole width of a monitor — which is what every form page did
/// until now: a topbar neatly in its column with a textarea stretching past it
/// on both sides. The element was missing, not the rule.
fn form_page(book: &str, title: &str, back_to: &str, said: &str, form: &str) -> String {
    shell(
        &format!("{title} — noda"),
        "",
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

/// A note's body, in a box.
///
/// `was` is the fingerprint the file had when this page was drawn, carried
/// through the form so the write can tell whether anything happened in between.
pub fn editing(book: &str, about: &About, body: &str, was: &str, problem: Option<&str>) -> String {
    form_page(
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

/// The note changed under the reader, and neither version has been lost.
///
/// What is on disk is shown first and cannot be typed into; what they wrote is
/// underneath and still can be. That arrangement is the whole answer: it needs
/// no "keep mine" button, because saving *is* keeping theirs and cancelling is
/// discarding it, and it leaves room for the only thing a person can do that a
/// program cannot — decide what the two versions together should say.
pub fn clashed(book: &str, about: &About, theirs: &str, mine: &str, now: &str) -> String {
    form_page(
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

/// Which tags the note should end up with.
///
/// A ticked box per tag it has, and a field for ones it does not. The form says
/// what should survive; working out the `+`s and `-`s from that is the server's
/// job, because `+work -q3` is a notation for somebody with a keyboard.
///
/// **The field takes as many tags as you can type into it**, because it is cut
/// by `query::split` — the same splitter the search box and the `:` prompt use,
/// so a space separates and a quote holds one together. It says so, and shows a
/// quoted tag in the placeholder rather than describing one: the field read as
/// a one-tag field for as long as its label was singular, which made a thing
/// the server had always done impossible to find.
pub fn tagging(book: &str, about: &About, tags: &[String], problem: Option<&str>) -> String {
    let mut boxes = String::new();
    for (n, tag) in tags.iter().enumerate() {
        let _ = write!(
            boxes,
            "<label class=\"tick\" for=\"t{n}\">\
             <input id=\"t{n}\" type=\"checkbox\" name=\"keep\" value=\"{}\" checked>\
             <span>{}</span></label>",
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

/// The last chance to not.
///
/// It says what git makes true — the note can be brought back — because that is
/// the difference between this and every other notes application, and a warning
/// that overstates the danger teaches people to click through warnings.
pub fn deleting(book: &str, about: &About) -> String {
    form_page(
        book,
        "Delete",
        &about.at(book),
        "",
        &format!(
            "<form class=\"write\" method=\"post\" action=\"{}/delete\">\
             <p class=\"said\"><b>Delete {}?</b> The file goes and the commit that \
             removed it stays, so <code>noda restore</code> brings it back with its id.</p>\
             <div class=\"buttons\"><button class=\"danger\" type=\"submit\">Delete</button>\
             <a class=\"button\" href=\"{}\">Keep it</a></div></form>",
            about.at(book),
            escape(&about.title),
            about.at(book)
        ),
    )
}

/// Where a notebook stands, and what its remote knows about it.
///
/// The same facts `noda status` prints, already in words: which branch, how much
/// is uncommitted, whether there is a remote and how far the two have drifted.
/// Worked out by the caller rather than here, on the rule the rest of this file
/// keeps — the page arranges what it is given and decides nothing.
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
    /// `Synced`, or `Sync failed`, for once it has. Chosen by the caller rather
    /// than worked out here, because it is the same choice the failure colour
    /// below is made from and one fact should be read once.
    pub done: &'a str,
    /// What it printed, or what went wrong. `None` while it is still going.
    pub said: Option<&'a str>,
    pub failed: bool,
    pub seconds: u64,
}

/// The network screen: where the notebook stands, and the three ways to move it.
///
/// **The buttons are `POST`s and the page is a `GET`, and that is the whole
/// design.** A sync is a fetch over somebody's tailnet — it takes as long as it
/// takes, and a request held open for it would leave a phone showing nothing.
/// So the press starts the errand and the answer is a redirect back here, which
/// means a reload asks how it is going rather than doing it again. While one is
/// running the page brings itself back for news; when it stops, the refresh
/// stops with it.
pub fn standing(book: &str, standing: &Standing, errand: Option<&Errand>) -> String {
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
        // The remedy, not just the fact: a notebook with no remote cannot sync,
        // and the command that gives it one is not on any screen here.
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
            // Still going, and how long it has been going. A count of seconds
            // rather than a bar: nothing here knows how long a fetch will take,
            // and a bar that guessed would be the only thing on these pages that
            // is not true.
            None => format!(
                "<p class=\"said working\"><b>{}…</b> {}</p>",
                escape(errand.doing),
                plural(errand.seconds as usize, "second")
            ),
            // What the command printed, whole. Not summarised into a tick: the
            // three lines `sync` prints are the difference between "it worked"
            // and "it worked, and here is what it did".
            Some(said) => format!(
                "<p class=\"said{}\"><b>{}</b><span class=\"outcome\">{}</span></p>",
                if errand.failed { " bad" } else { "" },
                escape(errand.done),
                escape(said)
            ),
        },
    };

    // Disabled while one is running, and the reason is honesty rather than
    // safety: a second press is already refused by the server, so what the
    // greying out prevents is not a second sync but the belief that the first
    // one did not land.
    let busy = errand.is_some_and(|errand| errand.said.is_none());
    let at = escape(book);
    let mut buttons = String::new();
    for (errand, label) in [("sync", "Sync"), ("pull", "Pull"), ("push", "Push")] {
        let _ = write!(
            buttons,
            "<form method=\"post\" action=\"/nb/{at}/status/{errand}\">\
             <button class=\"{}\" type=\"submit\"{}>{label}</button></form>",
            // The accent is on the one that is nearly always right, and the
            // other two are ordinary buttons. Said in colour rather than in
            // layout, so all three stay one row on a phone and three sensible
            // widths on a monitor.
            if errand == "sync" { "go" } else { "" },
            if busy { " disabled" } else { "" }
        );
    }

    dressed(
        &format!("Status — {book} — noda"),
        "",
        busy.then_some(2),
        &[script::STANDING],
        &format!(
            "<section class=\"pane\">\
             <header class=\"topbar\">{}<span class=\"here\">Status</span>\
             <span class=\"count\">{at}</span></header>\
             <main>{said}<div class=\"rows facts\">{rows}</div>\
             <div class=\"abreast\">{buttons}</div></main></section>{}",
            back(&format!("/nb/{at}"), book),
            notebook_bar(book, At::Status),
        ),
    )
}

/// Something went wrong, said in the interface's voice.
///
/// No apology and no blame. What was asked for, why it could not be answered,
/// and the way back — which on a page with no navigation of its own is the only
/// thing that makes it not a dead end.
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
        format!("<span class=\"when\">{}</span>", escape(&day(value)))
    })
}

/// A note's own stamp, exactly as the file holds it.
///
/// The `Z` or the `+08:00` comes with it, which is the point: this is the one
/// place with room for the whole thing, and the whole thing is the only version
/// that cannot be misread.
fn updated(value: Option<&str>) -> String {
    value.map_or_else(String::new, |value| {
        format!("<span class=\"when\">updated {}</span>", escape(value))
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

/// The layout, for the one test that has to read it.
///
/// `script.rs` writes the split breakpoint a second time, and the two numbers
/// have to agree. Exposing the sheet is how that is checked rather than
/// asserted twice in prose.
#[cfg(test)]
pub(crate) fn stylesheet() -> &'static str {
    CSS
}

/// The whole of the layout.
///
/// Mobile first, because that is what this exists for. Two numbers run through
/// it: `--tap`, which no control may be smaller than, and the 16px on the search
/// field — below that, iOS Safari zooms the page when the field takes focus and
/// leaves the reader pinching their way back out.
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
.perilous{padding:8px 16px 24px;margin:0}\
.perilous a{color:var(--alert);display:inline-flex;align-items:center;min-height:var(--tap);\
text-decoration:underline;text-underline-offset:3px;font-size:14px}\
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
/* The row extends rather than stacking: the tags and the day leave the second \
   line and go to the right of the title. Same information, same order — the \
   rule `-l` follows on the CLI's own row. */\
.rows .row{display:flex;align-items:baseline;gap:20px;min-height:0;padding:13px 24px}\
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
.perilous{padding:8px 32px 40px}\
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
.app.split .index .rows .row .title{font-size:15.5px;line-height:1.34}\
.app.split .index .rows .row .under{margin-top:2px;justify-content:flex-start;font-size:12px}\
.app.split .index .searchbar,.app.split .index .topbar{padding-left:20px;padding-right:20px}\
.app.split .index .empty{padding:26px 20px}\
/* Head, body and the delete line share one column so the rule under the title \
   spans exactly what the prose does. The pane keeps the slack. */\
.app.split .read main.note{width:100%;max-width:44em;margin-inline:auto}\
.app.split .read .body{max-width:none}\
/* The note's chevron points at the listing, and here the listing is already \
   on the screen beside it. The index pane keeps its own, which points \
   somewhere you cannot see: the notebooks. Only when the index is actually \
   there — without the script this is the one pane, and the way back with it. */\
.app.split.indexed .read .topbar .back{display:none}}\
/* A monitor wider than a laptop spends the extra on the index, never on the \
   measure: a line of prose has a right length and it is not \"however wide the \
   window is\". */\
@media (min-width:1800px){\
.app.split.indexed{grid-template-columns:var(--rail) clamp(340px,22vw,470px) minmax(0,1fr)}}\
";

#[cfg(test)]
mod tests {
    use super::*;

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

    /// The body arrives already rendered and is written out as it stands — this
    /// page escapes everything else and must not escape this. What keeps a
    /// note's own raw HTML from becoming markup is a floor down, in `render`,
    /// which is where the tests for it are.
    #[test]
    fn the_rendered_body_is_written_out_as_it_stands() {
        let page = note(
            "work",
            &Reading {
                id: "k3f9".into(),
                slug: "notes".into(),
                title: "Notes".into(),
                tags: vec![],
                updated: None,
                rendered: "<p>a <em>rendered</em> note</p>".into(),
            },
            "in sync",
        );
        assert!(page.contains("<p>a <em>rendered</em> note</p>"), "{page}");
    }

    /// A listing shows the day. Showing the minute would mean either printing a
    /// UTC clock that reads as a local one, or converting into this server's
    /// zone — and a note was not written where the server is.
    #[test]
    fn a_listing_shows_the_day_and_never_a_clock() {
        assert_eq!(day("2019-03-14T16:21:00+08:00"), "2019-03-14");
        assert_eq!(day("2026-08-15T08:59:03Z"), "2026-08-15");
        // Not a shape noda wrote. It is still the only copy of what it says.
        assert_eq!(day("last tuesday"), "last tuesday");
        assert_eq!(day(""), "");
    }

    /// The note's own page prints the stamp whole, `Z` and all, the way `ls -l`
    /// does — this is where the minute lives, and where it can be read
    /// correctly because the zone came with it.
    #[test]
    fn the_note_page_prints_the_stamp_as_the_file_holds_it() {
        let page = note(
            "work",
            &Reading {
                id: "k3f9".into(),
                slug: "notes".into(),
                title: "Notes".into(),
                tags: vec![],
                updated: Some("2026-08-15T09:54:23Z".into()),
                rendered: String::new(),
            },
            "in sync",
        );
        assert!(page.contains("updated 2026-08-15T09:54:23Z"), "{page}");
    }

    #[test]
    fn the_matched_run_is_marked_and_the_rest_is_escaped() {
        let out = highlight("Budget <review>", &["budget".to_string()]);
        assert_eq!(out, "<mark>Budget</mark> &lt;review&gt;");
        // No terms is the ordinary case — a listing nobody has filtered.
        assert_eq!(highlight("a & b", &[]), "a &amp; b");
    }

    /// Two terms that overlap mark one run. Nested `<mark>` would draw the
    /// overlap twice as dark, which reads as a third kind of match.
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
            updated: Some("2026-08-12T08:03:00Z".into()),
            shown,
        }
    }

    #[test]
    fn a_filtered_listing_says_what_it_is_hiding() {
        let rows = (0..12)
            .map(|n| row(&format!("k3f{n}"), "Budget review", false))
            .collect::<Vec<_>>();
        let page = listing("work", &rows, "tag:ghost", &[], None, "in sync", None);
        assert!(
            page.contains("No notes match <span class=\"asked\">tag:ghost"),
            "{page}"
        );
        assert!(page.contains("12 notes"), "{page}");
        assert!(page.contains("href=\"/nb/work\""), "{page}");
    }

    /// **The rows the query excluded are still on the page.** A listing the
    /// script could only ever narrow further would make filtering-as-you-type
    /// need a second copy of the notes to filter from, and that copy is the one
    /// that goes stale. `hidden` is the browser's own attribute, so the
    /// scriptless page hides them with nothing of noda's involved.
    #[test]
    fn an_excluded_row_rides_along_hidden_and_unmarked() {
        let rows = [
            row("k3f9", "Budget review", true),
            row("em0x", "Reading list", false),
        ];
        let page = listing(
            "work",
            &rows,
            "budget",
            &["budget".to_string()],
            None,
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
        // The one that is shown says why it is; the one that is not carries its
        // title as it stands, which is the state the script would put it in
        // anyway when a different query lets it back through.
        assert!(page.contains("<mark>Budget</mark>"), "{page}");
        assert!(page.contains(">Reading list</div>"), "{page}");
        // And the count is of what is on the screen, out of what is on the page.
        assert!(page.contains(">1 of 2<"), "{page}");
    }

    #[test]
    fn an_empty_notebook_says_what_to_do_instead_of_nothing() {
        let page = listing("work", &[], "", &[], None, "in sync", None);
        assert!(page.contains("No notes yet"), "{page}");
        assert!(page.contains("noda add"), "{page}");
        // Not the other empty. A notebook with nothing in it is not a query
        // that found nothing, and no amount of typing turns one into the other.
        assert!(!page.contains("No notes match"), "{page}");
    }

    /// The listing carries the sentence and the sentence is switched off. What
    /// the script decides is *when* it applies — never what it says, because a
    /// sentence living inside a script is one nothing else can test the wording
    /// of.
    #[test]
    fn the_hint_is_written_by_the_page_and_hidden_by_it() {
        let page = listing(
            "work",
            &[row("k3f9", "Budget review", true)],
            "",
            &[],
            None,
            "in sync",
            None,
        );
        assert!(page.contains("<p class=\"hint\" hidden>"), "{page}");
        assert!(page.contains("press ⏎ to search the text"), "{page}");
        assert!(page.contains("<script>"), "{page}");
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
                updated: Some("2026-08-15T16:59:00Z".into()),
                rendered: "late".into(),
            },
            "in sync",
        );
        assert!(page.contains(">em0xvn4e</span>"), "{page}");
        assert!(page.contains(">-budget-review</span>"), "{page}");
        assert!(page.contains(">.md</span>"), "{page}");
    }

    /// A row with no tags must not print an empty separator where they would
    /// have been. Tags are the one thing a note may not have, which is why they
    /// go last everywhere else too.
    #[test]
    fn a_row_without_tags_prints_no_separator() {
        let rows = [Row {
            id: "k3f9".into(),
            title: "Reading list".into(),
            tags: vec![],
            updated: Some("2026-08-12T08:03:00Z".into()),
            shown: true,
        }];
        let page = listing("work", &rows, "", &[], None, "in sync", None);
        assert!(!page.contains("·"), "{page}");
        assert!(page.contains("2026-08-12"), "{page}");
        // The clock is not in a listing at all, in either spelling.
        assert!(!page.contains("08:03"), "{page}");
    }

    #[test]
    fn every_page_carries_both_themes_and_the_viewport() {
        let page = listing("work", &[], "", &[], None, "in sync", None);
        assert!(page.contains("width=device-width"), "{page}");
        assert!(page.contains("prefers-color-scheme:dark"), "{page}");
        assert!(page.contains("--tap:48px"), "{page}");
    }
}
