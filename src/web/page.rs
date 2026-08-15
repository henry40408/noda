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
use crate::web::theme;

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
}

/// A note, as its own page shows it.
pub struct Reading {
    pub id: String,
    pub slug: String,
    pub title: String,
    pub tags: Vec<String>,
    pub updated: Option<String>,
    pub body: String,
}

impl Row {
    pub fn of(file: &NoteFile) -> Row {
        Row {
            id: file.id.clone(),
            title: file.note.title.clone(),
            tags: file.note.tags.clone(),
            updated: file.note.updated.clone(),
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
fn tags(tags: &[String]) -> String {
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
fn shell(title: &str, body: &str) -> String {
    format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n\
         <meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
         <title>{}</title>\n<style>{}{}</style>\n</head>\n<body>\n{}</body>\n</html>\n",
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
fn action_bar(items: &[(&str, &str, String)]) -> String {
    let mut out = String::from("<nav class=\"actionbar\">");
    for (icon, label, href) in items {
        let _ = write!(
            out,
            "<a href=\"{}\"><svg viewBox=\"0 0 24 24\" aria-hidden=\"true\">{icon}</svg>\
             <span>{label}</span></a>",
            escape(href)
        );
    }
    out.push_str("</nav>");
    out
}

const NEW: &str = "<path d=\"M12 4.5v15M4.5 12h15\"/>";
const EDIT: &str = "<path d=\"M4 20h4L19 9l-4-4L4 16z\"/>";
const TAGS: &str = "<path d=\"M4 4h7l9 9-7 7-9-9z\"/><circle cx=\"8\" cy=\"8\" r=\"1.4\"/>";
const RENAME: &str = "<path d=\"M4 7V5h16v2\"/><path d=\"M12 5v14\"/><path d=\"M9 19h6\"/>";

/// The back chevron, and the only icon PR 1 has.
///
/// Inline SVG rather than a character like `‹`: a glyph is whatever the reader's
/// font decides it is — weight, size and where it sits on the line all out of
/// our hands — and this one has to look the same on every phone that reaches the
/// notebook. It is also why it can be given a stroke width at all.
const BACK: &str =
    "<svg viewBox=\"0 0 24 24\" aria-hidden=\"true\"><path d=\"M15 4.5 7.5 12 15 19.5\"/></svg>";

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
        &format!(
            "<header class=\"topbar\"><span class=\"here lead\">noda</span>\
             <span class=\"count\">{}</span></header>\
             <main class=\"rows books\">{rows}</main>",
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
pub fn listing(
    book: &str,
    rows: &[Row],
    query: &str,
    total: usize,
    terms: &[String],
    problem: Option<&str>,
) -> String {
    let body = if rows.is_empty() && !query.is_empty() {
        format!(
            "<div class=\"empty\"><b>No notes match {}</b>\
             This notebook holds {}. <a href=\"/nb/{}\">Clear the search</a> to see them.</div>",
            escape(query),
            plural(total, "note"),
            escape(book)
        )
    } else if rows.is_empty() {
        "<div class=\"empty\"><b>No notes yet</b>Run <code>noda add \"First note\"</code> in a terminal to start one.</div>".to_string()
    } else {
        let mut out = String::new();
        for row in rows {
            let under = [tags(&row.tags), when(row.updated.as_deref())]
                .into_iter()
                .filter(|piece| !piece.is_empty())
                .collect::<Vec<_>>()
                .join("<span class=\"sep\">·</span>");
            let _ = write!(
                out,
                "<a class=\"row\" href=\"/nb/{}/n/{}\"><div class=\"title\">{}</div>\
                 <div class=\"under\">{under}</div></a>",
                escape(book),
                escape(&row.id),
                highlight(&row.title, terms)
            );
        }
        out
    };

    let counted = if query.is_empty() {
        total.to_string()
    } else {
        format!("{} of {}", rows.len(), total)
    };

    shell(
        &format!("{book} — noda"),
        &format!(
            "<header class=\"topbar\">{}<span class=\"here\">{}</span>\
             <span class=\"count\">{counted}</span></header>\
             <form class=\"searchbar\" method=\"get\" action=\"/nb/{}\">\
             <input type=\"search\" name=\"q\" value=\"{}\" \
             placeholder=\"tag:work OR tag:q3 budget\" \
             autocomplete=\"off\" autocapitalize=\"off\" spellcheck=\"false\" \
             enterkeyhint=\"search\" aria-label=\"Search this notebook\">{}</form>\
             <main class=\"rows\">{body}</main>{}",
            back("/", "the notebooks"),
            escape(book),
            escape(book),
            escape(query),
            problem.map_or_else(String::new, |why| format!(
                "<p class=\"problem\">{}</p>",
                escape(why)
            )),
            action_bar(&[(NEW, "New", format!("/nb/{}/new", escape(book)))])
        ),
    )
}

/// One note.
pub fn note(book: &str, reading: &Reading) -> String {
    let at = format!("/nb/{}/n/{}", escape(book), escape(&reading.id));
    let meta = [tags(&reading.tags), updated(reading.updated.as_deref())]
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
    let bar = action_bar(&[
        (EDIT, "Edit", format!("{at}/edit")),
        (TAGS, "Tags", format!("{at}/tags")),
        (RENAME, "Rename", format!("{at}/rename")),
    ]);
    let home = format!("/nb/{}", escape(book));

    shell(
        &format!("{} — noda", reading.title),
        &format!(
            "<header class=\"topbar\">{}<span class=\"here\">{}</span></header>\
             <main class=\"note\">\
             <div class=\"note-head\"><h1>{}</h1>\
             <div class=\"filename\"><span class=\"id\">{}</span>\
             <span class=\"slug\">-{}</span><span class=\"ext\">.md</span></div>\
             <div class=\"note-meta\">{meta}</div></div>\
             <div class=\"body\">{}</div>\
             {perilous}</main>{bar}",
            back(&home, book),
            escape(book),
            escape(&reading.title),
            escape(&reading.id),
            escape(&reading.slug),
            escape(&reading.body),
        ),
    )
}

/// What every form page is wrapped in: a bar with a way back, an optional line
/// saying what went wrong, and the form itself.
fn form_page(book: &str, title: &str, back_to: &str, said: &str, form: &str) -> String {
    shell(
        &format!("{title} — noda"),
        &format!(
            "<header class=\"topbar\">{}<span class=\"here\">{}</span></header>{said}{form}",
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
             <div><label for=\"a\">Add a tag</label>\
             <input id=\"a\" type=\"text\" name=\"add\" placeholder=\"ops\"></div>\
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

/// Something went wrong, said in the interface's voice.
///
/// No apology and no blame. What was asked for, why it could not be answered,
/// and the way back — which on a page with no navigation of its own is the only
/// thing that makes it not a dead end.
pub fn failure(heading: &str, detail: &str) -> String {
    shell(
        &format!("{heading} — noda"),
        &format!(
            "<header class=\"topbar\">{}<span class=\"here\">noda</span></header>\
             <main><div class=\"empty\"><b>{}</b>{}</div></main>",
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

/// The whole of the layout.
///
/// Mobile first, because that is what this exists for. Two numbers run through
/// it: `--tap`, which no control may be smaller than, and the 16px on the search
/// field — below that, iOS Safari zooms the page when the field takes focus and
/// leaves the reader pinching their way back out.
const CSS: &str = "\
*{box-sizing:border-box}\
:root{--tap:48px;\
--mono:ui-monospace,SFMono-Regular,'SF Mono',Menlo,'Cascadia Mono',Consolas,monospace;\
--read:ui-serif,Charter,'Iowan Old Style',Georgia,'Songti TC','Noto Serif CJK TC',serif}\
html{-webkit-text-size-adjust:100%}\
body{margin:0;background:var(--bg);color:var(--text);font-family:var(--mono);font-size:14px;line-height:1.6}\
svg{fill:none;stroke:currentColor;stroke-width:2;stroke-linecap:round;stroke-linejoin:round}\
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
a{color:inherit;text-decoration:none}\
.row{display:block;min-height:64px;padding:12px 16px;border-bottom:1px solid var(--rule);\
-webkit-tap-highlight-color:transparent}\
.row:active{background:var(--press)}\
.row .title{font-family:var(--read);font-size:17px;line-height:1.32}\
.row .name{font-size:15px}\
.row .under{margin-top:4px;font-size:12.5px;display:flex;gap:8px;align-items:baseline;flex-wrap:wrap}\
.row .under:empty{display:none}\
.tags{color:var(--tag)}\
.sep{color:var(--punct)}\
.when{color:var(--muted)}\
mark{background:var(--mark);color:inherit;border-radius:2px;padding:0 1px}\
.note-head{padding:18px 16px 16px;border-bottom:1px solid var(--rule)}\
.note-head h1{font-family:var(--read);font-size:24px;line-height:1.24;margin:0 0 10px;\
font-weight:600;letter-spacing:-0.01em}\
.filename{font-size:13px}\
.filename .id{color:var(--id)}\
.filename .slug{color:var(--id-dim)}\
.filename .ext{color:var(--punct)}\
.note-meta{margin-top:8px;font-size:12.5px;display:flex;gap:8px;align-items:baseline;flex-wrap:wrap}\
.body{padding:16px;font-family:var(--read);font-size:17px;line-height:1.62;white-space:pre-wrap;\
overflow-wrap:break-word}\
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
form.write{padding:16px;display:flex;flex-direction:column;gap:16px}\
form.write label{font-size:12.5px;color:var(--punct);display:block;margin-bottom:6px}\
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
.theirs{margin:0;padding:14px;border:1px solid var(--rule);border-radius:10px;\
background:var(--bg-sunk);font-family:var(--read);font-size:16px;line-height:1.55;\
white-space:pre-wrap;overflow-wrap:break-word}\
.perilous{padding:8px 16px 24px;margin:0}\
.perilous a{color:var(--alert);display:inline-flex;align-items:center;min-height:var(--tap);\
text-decoration:underline;text-underline-offset:3px;font-size:14px}\
@media (min-width:720px){\
/* One column for the whole interface, and the room left over falls on both \
   sides. A `max-width` without a margin is not a narrower page, it is a page \
   pushed against the left edge of a monitor. */\
.topbar,.searchbar,main{max-width:900px;margin-inline:auto}\
/* The row extends rather than stacking: the tags and the day leave the second \
   line and go to the right of the title. Same information, same order — the \
   rule `-l` follows on the CLI's own row. */\
.row{display:flex;align-items:baseline;gap:18px;min-height:0;padding:14px 20px}\
.row .title,.row .name{flex:1 1 auto}\
.row .under{margin:0;flex:0 0 auto}\
.topbar,.searchbar{padding-left:20px;padding-right:20px}\
.topbar .back{margin-left:-12px}\
.note-head{padding:22px 20px 18px}\
/* A measure, and measured in the font it is set in. `ch` and `em` are relative \
   to the element's own type — putting the reading measure on `main`, which is \
   set in the monospace the chrome uses, sized a column of prose by a font the \
   prose is not in. It is the body that is read, so it is the body that is \
   capped. */\
.body{font-size:18px;max-width:36em;padding:20px}}\
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

    /// The body reaches the page unaltered apart from escaping, so this is the
    /// one that matters: a note holding raw HTML is a thing the importer
    /// deliberately produces.
    #[test]
    fn a_body_holding_html_arrives_as_text() {
        let page = note(
            "work",
            &Reading {
                id: "k3f9".into(),
                slug: "notes".into(),
                title: "Notes".into(),
                tags: vec![],
                updated: None,
                body: "a <div class=\"raw\">html</div> here".into(),
            },
        );
        assert!(page.contains("&lt;div class=&quot;raw&quot;&gt;"), "{page}");
        assert!(!page.contains("<div class=\"raw\">"), "{page}");
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
                body: String::new(),
            },
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

    #[test]
    fn a_filtered_listing_says_what_it_is_hiding() {
        let page = listing("work", &[], "tag:ghost", 12, &[], None);
        assert!(page.contains("No notes match tag:ghost"), "{page}");
        assert!(page.contains("12 notes"), "{page}");
        assert!(page.contains("href=\"/nb/work\""), "{page}");
    }

    #[test]
    fn an_empty_notebook_says_what_to_do_instead_of_nothing() {
        let page = listing("work", &[], "", 0, &[], None);
        assert!(page.contains("No notes yet"), "{page}");
        assert!(page.contains("noda add"), "{page}");
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
                body: "late".into(),
            },
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
        }];
        let page = listing("work", &rows, "", 1, &[], None);
        assert!(!page.contains("·"), "{page}");
        assert!(page.contains("2026-08-12"), "{page}");
        // The clock is not in a listing at all, in either spelling.
        assert!(!page.contains("08:03"), "{page}");
    }

    #[test]
    fn every_page_carries_both_themes_and_the_viewport() {
        let page = listing("work", &[], "", 0, &[], None);
        assert!(page.contains("width=device-width"), "{page}");
        assert!(page.contains("prefers-color-scheme:dark"), "{page}");
        assert!(page.contains("--tap:48px"), "{page}");
    }
}
