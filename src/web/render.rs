//! A note's body, as HTML.
//!
//! The rendering is `pulldown-cmark`'s. What is noda's is the two decisions it
//! will not make for you, and both are made by rewriting the event stream
//! *before* it is rendered rather than by fixing up the HTML afterwards:
//!
//! **Where a destination leads.** A note points at its neighbours with ordinary
//! relative Markdown paths — `[the plan](k3f9m2p1-the-plan.md)` — because that
//! is what makes a notebook readable on a git host, in an editor, and in
//! anything that understands Markdown at all. Here those have to become
//! `/nb/<book>/n/<id>`, and a link to any other file the notebook holds has to
//! become `/nb/<book>/f/<name>`. The path is resolved by `link::target`, the
//! same function `doctor` and `file mv` resolve links with; a second answer to
//! "is this inside the notebook" is how the two drift apart, and the one facing
//! the network is the one that must not.
//!
//! **What raw HTML turns into.** It becomes a code block: escaped, shown, and
//! not run. Dropping it is not the harmless default it looks like — `noda
//! import tiddlywiki` deliberately leaves HTML it could not convert in the body
//! and records `unconverted: html` in the frontmatter, so the raw markup is the
//! only copy of what that note said. Shown as code, nothing is lost and the
//! unconverted original looks like what it is.
//!
//! A destination carrying a scheme noda does not serve — `javascript:` first
//! among them — keeps its text and loses its link. That is the whole of the
//! script defence and it is small on purpose: with raw HTML already turned into
//! code, a URL is the only thing left that can carry one.
//!
//! **What is a link without having been written as one.** `CommonMark` has no
//! bare URLs: `<https://example.com>` is a link because the angle brackets say
//! so, and `https://example.com` on its own is a word to it. Every other
//! Markdown anybody reads makes the second one a link as well, so a note
//! written in the expectation that it would — which is every note written
//! anywhere else — arrived here with its references as prose that could not be
//! pressed. `pulldown-cmark` has no option for it, so the scan is this module's,
//! on the event stream like everything else here.
//!
//! It is GFM's rules, narrowed to `http://` and `https://`: where a URL may
//! start, and which trailing punctuation belongs to the sentence rather than to
//! the address. `www.example.com` is deliberately not matched — making it a link
//! means choosing a scheme on the writer's behalf, and choosing the wrong one
//! sends the reader somewhere else entirely. A bare email is not matched either,
//! for a smaller reason: `<me@example.com>` already is one, and that spelling is
//! the writer saying they meant it to be pressed.
//!
//! **What a link that leaves carries.** A note's address holds somebody's note
//! id — `web/log.rs` refuses to write one into a log for exactly that reason —
//! and the `Referer` on a followed link hands the whole of it to whoever is on
//! the other end. So a destination that leaves this notebook is opened here by
//! hand, carrying `target="_blank"` and `rel="noopener noreferrer"`, and the
//! page it leaves from says `Referrer-Policy: same-origin` twice more: once as
//! a response header (`web::html`) and once in its own `<head>`
//! (`page::dressed`). That value sends nothing to another site, and it is not
//! `no-referrer` for a reason `web::html` sets out — the stricter value also
//! nulls the `Origin` on a form post, which is the header `web::guard` is
//! built on.
//!
//! Three statements of one rule rather than one, because each covers what the
//! others cannot. The header is the cheap one and it is also the one a reverse
//! proxy is free to strip. The meta survives that, and it is the half that
//! covers an image — a picture from somebody else's site is fetched without the
//! reader choosing anything, which makes it the larger leak of the two and the
//! one no attribute on a link would have reached. And `noopener` is the half
//! neither of them says: a page opened in a new tab can reach back through
//! `window.opener` at the page that opened it, and `target="_blank"` is what
//! makes that a question worth answering.

use std::collections::BTreeMap;
use std::fmt::Write;
use std::ops::Range;

use pulldown_cmark::{CodeBlockKind, Event, LinkType, Options, Parser, Tag, TagEnd, html};

use crate::link;
use crate::web::page::escape;

/// What the notebook looks like from the page being rendered: which notebook it
/// belongs to, and which filenames are notes rather than attachments.
pub struct Around<'a> {
    pub book: &'a str,
    /// `<id>-<slug>.md` to id, because that is the spelling a link in a body
    /// uses and the id is the address the web has.
    pub notes: BTreeMap<String, String>,
}

impl<'a> Around<'a> {
    /// Built from `Notebook::named_files`, which reads the directory and opens
    /// nothing: rendering one note is not a reason to parse every other one.
    pub fn of(book: &'a str, named: &[(String, String)]) -> Around<'a> {
        Around {
            book,
            notes: named
                .iter()
                .map(|(id, slug)| (format!("{id}-{slug}.md"), id.clone()))
                .collect(),
        }
    }
}

/// Where a destination is allowed to lead.
enum Route {
    /// Another note in this notebook. Marked as such, because a reader deserves
    /// to know before pressing whether a link stays inside the notebook or
    /// leaves it — the pages colour the two differently for the same reason
    /// `style.rs` colours an id: it says what a thing *is*.
    Note(String),
    /// Somewhere else that is not somebody else's site: a fragment inside this
    /// very page, a file the notebook holds, a `mailto:` or a `tel:`. Nothing is
    /// added to these — the first two are this origin, and the last two are not
    /// a page at all, so opening them in a tab of their own would be asking for
    /// a blank one beside a mail client.
    To(String),
    /// Somebody else's site, over http or https. The one destination that gives
    /// something away by being followed, and the only one this module opens by
    /// hand — a `Tag::Link` has nowhere to put the two attributes it needs.
    Away(String),
    /// Nowhere the browser should be pointed. The link is dropped and its text
    /// stays — losing the words as well would hide from the reader that the
    /// note says anything there at all.
    Nowhere,
}

/// What has to be written when a link ends.
enum Closing {
    /// Nothing: the link was dropped and only its words were kept.
    Nothing,
    /// The renderer's own `</a>`, for a link the renderer opened.
    Rendered,
    /// A literal `</a>`, for the one noda opened itself to carry a class.
    Written,
}

/// `markdown`, rendered.
pub fn body(markdown: &str, around: &Around) -> String {
    // Tables and strikethrough because that is the Markdown people write, and
    // task lists because noda already reads them: `noda todo` collects `- [ ]`
    // across the notebook, and a box that is a checkbox to the CLI would have
    // been a literal `[ ]` here.
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);

    // Whether the enclosing link or image was dropped, one entry per level. Two
    // stacks and not one counter: an image's alt text may itself contain a
    // link, so the two nest and their ends arrive tagged differently.
    let mut links: Vec<Closing> = Vec::new();
    let mut images: Vec<bool> = Vec::new();
    // Whether the text arriving is the inside of a code block. `Event::Code` is
    // its own event and needs no flag, but a fenced block's contents arrive as
    // `Event::Text` like any other prose — and an address inside one is being
    // shown, not offered.
    let mut fenced = false;
    // Text gathered but not yet written out, and the reason the loop below has
    // a step before its `match`.
    let mut prose = String::new();
    let mut rewritten = Vec::new();

    for event in Parser::new_ext(markdown, options) {
        // **A run of prose is gathered before it is looked at.** The parser
        // hands text back in pieces, cut wherever it had to consider a `_` or a
        // `*` — `.../A_(b)` arrives as two events even though neither is
        // emphasis — and an address read one piece at a time is an address cut
        // short at the first underscore in it, which is most of Wikipedia. So
        // consecutive text is joined and scanned whole, and anything that is
        // not text spills what has been gathered first.
        //
        // The context cannot change inside such a run: it takes an event to
        // enter a link, an image or a code block, and that event is the one
        // that spills.
        if let Event::Text(text) = &event
            && links.is_empty()
            && images.is_empty()
            && !fenced
        {
            prose.push_str(text);
            continue;
        }
        spill(&mut prose, &mut rewritten);

        match event {
            // A block of raw HTML becomes a fenced block of it. `html` is the
            // language it is, and a highlighter that arrives later will want to
            // have been told.
            Event::Start(Tag::HtmlBlock) => {
                fenced = true;
                rewritten.push(Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(
                    "html".into(),
                ))));
            }
            // The block a note actually fenced, marked for the same reason: the
            // two arrive here as the same events from this point on, which is
            // what makes turning one into the other a defence at all — and it
            // is why the two ends are one arm. There is nothing left by then
            // that could tell them apart, and nothing that should.
            Event::Start(Tag::CodeBlock(kind)) => {
                fenced = true;
                rewritten.push(Event::Start(Tag::CodeBlock(kind)));
            }
            Event::End(TagEnd::HtmlBlock | TagEnd::CodeBlock) => {
                fenced = false;
                rewritten.push(Event::End(TagEnd::CodeBlock));
            }
            // Text and code are escaped by the renderer, which is what makes
            // the two lines above a defence and not a presentation choice.
            Event::Html(raw) => rewritten.push(Event::Text(raw)),
            Event::InlineHtml(raw) => rewritten.push(Event::Code(raw)),

            // `<me@example.com>` carries its address without a scheme — the
            // renderer is what puts `mailto:` in front of it. Read as a
            // destination it looks exactly like a relative filename, and the
            // notebook does not hold a file called `me@example.com`.
            Event::Start(Tag::Link {
                link_type: LinkType::Email,
                ..
            }) => {
                links.push(Closing::Rendered);
                rewritten.push(event);
            }

            Event::Start(Tag::Link {
                link_type,
                dest_url,
                title,
                id,
            }) => match route(&dest_url, around, false) {
                // Written out rather than handed back as a `Tag::Link`, because
                // a `Tag::Link` has nowhere to put the class. The href is one
                // this module built out of an id, and it is escaped anyway —
                // the day something else can reach this argument, it will
                // already have been.
                Route::Note(url) => {
                    links.push(Closing::Written);
                    rewritten.push(Event::Html(
                        format!("<a class=\"note\" href=\"{}\">", escape(&url)).into(),
                    ));
                }
                Route::To(url) => {
                    links.push(Closing::Rendered);
                    rewritten.push(Event::Start(Tag::Link {
                        link_type,
                        dest_url: url.into(),
                        title,
                        id,
                    }));
                }
                // Written out for the same reason a note's link is, and the
                // same way a bare URL further down is: what a reader gets when
                // a link leaves must not depend on whether the note spelled it
                // `[text](url)` or just said the address.
                Route::Away(url) => {
                    links.push(Closing::Written);
                    rewritten.push(Event::Html(leaving(&url).into()));
                }
                Route::Nowhere => links.push(Closing::Nothing),
            },
            Event::End(TagEnd::Link) => match links.pop() {
                Some(Closing::Rendered) | None => rewritten.push(Event::End(TagEnd::Link)),
                Some(Closing::Written) => rewritten.push(Event::Html("</a>".into())),
                Some(Closing::Nothing) => {}
            },

            Event::Start(Tag::Image {
                link_type,
                dest_url,
                title,
                id,
            }) => match route(&dest_url, around, true) {
                // Both, and no attribute between them. What an image gives away
                // is the fetch itself, and the `<head>` of the page it is on is
                // where that is answered — for every kind of subresource at
                // once, rather than for the one this module happens to build.
                Route::To(url) | Route::Away(url) => {
                    images.push(false);
                    rewritten.push(Event::Start(Tag::Image {
                        link_type,
                        dest_url: url.into(),
                        title,
                        id,
                    }));
                }
                // The alt text is what is left, and it is the right thing to be
                // left with: it is what the note's author wrote to stand in for
                // the picture.
                //
                // A note is in here with the refusals because `![x](a-note.md)`
                // asks the browser to display a page as a picture. That is a
                // mistake in the note rather than an attack, and the answer to
                // it is the same: say what the author said it was.
                Route::Note(_) | Route::Nowhere => images.push(true),
            },
            Event::End(TagEnd::Image) => {
                if !images.pop().unwrap_or(false) {
                    rewritten.push(Event::End(TagEnd::Image));
                }
            }

            other => rewritten.push(other),
        }
    }
    spill(&mut prose, &mut rewritten);

    let mut out = String::with_capacity(markdown.len());
    html::push_html(&mut out, rewritten.into_iter());
    out
}

/// Writes a gathered run of prose out, opening any bare address in it.
///
/// One allocation per run, and the run is a paragraph's worth of a note being
/// rendered for a request — not a path anything measures its startup by, and
/// the alternative is handing the scanner a sentence in pieces and calling the
/// half of an address it can see a link.
fn spill(prose: &mut String, out: &mut Vec<Event<'_>>) {
    if prose.is_empty() {
        return;
    }
    let text = std::mem::take(prose);
    let mut at = 0;
    for span in bare_urls(&text) {
        if span.start > at {
            out.push(Event::Text(text[at..span.start].to_string().into()));
        }
        let url = &text[span.start..span.end];
        out.push(Event::Html(leaving(url).into()));
        // The words are the address, handed back as text so that the renderer
        // escapes them — the same division of labour a note's own links are
        // written with.
        out.push(Event::Text(url.to_string().into()));
        out.push(Event::Html("</a>".into()));
        at = span.end;
    }
    if at < text.len() {
        out.push(Event::Text(text[at..].to_string().into()));
    }
}

/// The anchor this module opens for a destination that leaves the notebook.
///
/// One function, called from both places a link can leave from, so a reader
/// cannot tell by what they get whether the note wrote `[text](url)` or only
/// said the address. `target` is the reason `noopener` is not decoration: a page
/// opened in a tab of its own is handed a reference back to this one unless the
/// link says otherwise.
fn leaving(url: &str) -> String {
    format!(
        "<a href=\"{}\" target=\"_blank\" rel=\"noopener noreferrer\">",
        escape(url)
    )
}

/// The bare `http://` and `https://` addresses in a run of prose.
///
/// GFM's autolink literals, narrowed to the two schemes and with the two rules
/// that only exist to keep `www.` from swallowing prose left out. What is kept
/// is the pair that decide where a match begins and where it ends, and both are
/// about the sentence around the address rather than the address itself.
///
/// **Where one may start.** After nothing, after a space, or after one of
/// `*_~(` — so `(https://example.com)` matches and `xhttps://example.com` does
/// not. Anything else in front of it is a word this is the tail of, and a URL
/// glued to a word is not one somebody meant.
///
/// **Where one ends.** At the first space or `<`, and then walking back over the
/// punctuation a sentence ends with — `?!.,:*_~` — because `see https://a.example.`
/// ends in a full stop that belongs to the sentence. A closing bracket is the
/// case that needs counting rather than a list: `https://en.example.org/A_(b)`
/// keeps its `)` and `(see https://a.example)` does not, and what tells them
/// apart is whether the address holds more of them closing than opening.
///
/// Quotes and `;` are left in, which is GFM's answer too, and it is worth being
/// deliberate about: a note that renders one way on a git host should not render
/// another way here, and *this* is the direction that surprise would run in.
///
/// One thing this cannot see, because it runs after the parser rather than
/// inside it. Text arrives in pieces and [`spill`] joins them, so an address
/// merely *considered* for emphasis survives; one that is actually cut by it
/// does not. `https://a.example/x*y*z` has become a link around an emphasis by
/// the time it reaches here, and only the part before the emphasis is scanned.
/// GFM does not have this problem because its autolinks are found during inline
/// parsing rather than after it. It is rare, it fails towards prose rather than
/// towards a wrong address, and closing it would mean owning an inline parser.
fn bare_urls(text: &str) -> Vec<Range<usize>> {
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(offset) = text[from..].find("http") {
        let start = from + offset;
        from = start + "http".len();

        let before = text[..start].chars().next_back();
        if !before.is_none_or(|c| c.is_whitespace() || matches!(c, '*' | '_' | '~' | '(')) {
            continue;
        }
        let rest = &text[start..];
        let scheme = if rest.starts_with("https://") {
            "https://".len()
        } else if rest.starts_with("http://") {
            "http://".len()
        } else {
            continue;
        };

        let host = start + scheme;
        let tail = &text[host..];
        let stop = tail
            .find(|c: char| c.is_whitespace() || c == '<')
            .unwrap_or(tail.len());
        let end = sentence_off(text, host, host + stop);
        // A scheme and nothing after it is not an address, it is the word
        // "https" with punctuation on it.
        if end > host {
            found.push(start..end);
            from = end;
        }
    }
    found
}

/// Walks `end` back over the punctuation that belongs to the sentence.
///
/// `host` is where the address's own text begins, so the walk can never eat the
/// scheme it was told to keep.
fn sentence_off(text: &str, host: usize, mut end: usize) -> usize {
    while let Some(last) = text[host..end].chars().next_back() {
        match last {
            '?' | '!' | '.' | ',' | ':' | '*' | '_' | '~' => end -= last.len_utf8(),
            ')' => {
                let inside = &text[host..end];
                if inside.matches(')').count() > inside.matches('(').count() {
                    end -= 1;
                } else {
                    break;
                }
            }
            _ => break,
        }
    }
    end
}

/// Where `dest` leads once the notebook is taken into account.
///
/// `embed` is whether the browser will fetch it without being asked — an image
/// does, a link does not — and it narrows what is allowed: a reader chooses to
/// follow a link, and chooses nothing at all about an image.
fn route(dest: &str, around: &Around, embed: bool) -> Route {
    // A bare fragment points inside this very page. Nothing to resolve, and
    // nothing that can leave it.
    if dest.starts_with('#') {
        return Route::To(dest.to_string());
    }

    if let Some(path) = link::target(dest) {
        return match around.notes.get(&path) {
            Some(id) => Route::Note(format!("/nb/{}/n/{}", url_path(around.book), url_path(id))),
            None => Route::To(format!(
                "/nb/{}/f/{}",
                url_path(around.book),
                url_path(&path)
            )),
        };
    }

    // Not a file the notebook holds. Either it names somebody else's, or it
    // climbed out of the notebook and `link::target` refused it — and a path
    // that climbed out is exactly the one not to hand back to the browser.
    match link::scheme(dest) {
        // http and https are somebody else's site; `mailto:` and `tel:` are not
        // a site at all, and the two are told apart here rather than at the
        // point of writing so that "does this leave" is answered once.
        Some(scheme) if serveable(scheme, embed) => {
            if scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https") {
                Route::Away(dest.to_string())
            } else {
                Route::To(dest.to_string())
            }
        }
        // A scheme noda does not serve, or none at all. The second case is a
        // path `link::target` refused — one that climbed out of the notebook —
        // and handing that back would be asking the browser to fetch it.
        Some(_) | None => Route::Nowhere,
    }
}

/// The schemes a browser may be pointed at.
///
/// A list of what is allowed rather than of what is not: `javascript:` is the
/// one everybody thinks of, and `data:` and `vbscript:` are the ones they
/// forget. Anything invented after this was written is refused by not being
/// named, which is the direction the mistake should fall in.
fn serveable(scheme: &str, embed: bool) -> bool {
    // An image is fetched without the reader doing anything, so it may only
    // come from the web or from the notebook — `mailto:` and `tel:` are things
    // to press, not things to display.
    if embed {
        return scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https");
    }
    ["http", "https", "mailto", "tel"]
        .iter()
        .any(|allowed| scheme.eq_ignore_ascii_case(allowed))
}

/// Percent-encodes one path segment's worth of URL.
///
/// A note's filename may hold a space, a `#` or a `?`, and each of those ends
/// the path when it reaches a browser unencoded. `/` is left alone: the
/// notebook is flat today, and a name that acquires a separator later should
/// keep meaning the same thing here as it does on disk.
fn url_path(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        match byte {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                out.push(byte as char);
            }
            other => {
                let _ = write!(out, "%{other:02X}");
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn around() -> Around<'static> {
        Around::of(
            "work",
            &[
                ("k3f9m2p1".to_string(), "the-plan".to_string()),
                ("h9nrrr5n".to_string(), "deploys".to_string()),
            ],
        )
    }

    /// A note points at its neighbour the way it does on a git host — a
    /// relative path to the file — and here that has to arrive as the note's
    /// address. The id is what survives a retitle, so the id is what it becomes.
    #[test]
    fn a_link_to_another_note_becomes_that_notes_address() {
        let out = body("see [the plan](k3f9m2p1-the-plan.md) first", &around());
        assert!(out.contains("href=\"/nb/work/n/k3f9m2p1\""), "{out}");
        assert!(!out.contains(".md"), "{out}");
    }

    /// A link that stays inside the notebook says so, and the pages colour it
    /// the way an id is coloured everywhere else. Only that one: a file and
    /// somebody else's site are both "away from here", and drawing three
    /// distinctions where the reader needs one is how a page gets loud.
    #[test]
    fn only_a_link_to_a_note_is_marked_as_one() {
        let out = body(
            "[a note](k3f9m2p1-the-plan.md), [a file](rack.png), [a site](https://example.com)",
            &around(),
        );
        assert!(
            out.contains("<a class=\"note\" href=\"/nb/work/n/k3f9m2p1\">"),
            "{out}"
        );
        assert!(out.contains("<a href=\"/nb/work/f/rack.png\">"), "{out}");
        assert!(
            out.contains("<a href=\"https://example.com\" target=\"_blank\""),
            "{out}"
        );
        assert_eq!(out.matches("class=\"note\"").count(), 1, "{out}");
        // Opened by hand, so the closing tag is worth an assertion of its own:
        // one `</a>` per `<a`, and nothing left hanging over the rest of the note.
        assert_eq!(
            out.matches("<a ").count(),
            out.matches("</a>").count(),
            "{out}"
        );
    }

    /// The address a note only says, rather than writes as a link.
    ///
    /// The assertion is the whole anchor and not a piece of it, because what is
    /// being claimed is that the reader gets exactly what they would have got
    /// had the note spelled the link out — and that the prose either side of it
    /// is still prose.
    #[test]
    fn a_bare_address_becomes_the_link_it_looks_like() {
        let out = body("see https://example.com/plan for the rest", &around());
        assert!(
            out.contains(
                "<a href=\"https://example.com/plan\" target=\"_blank\" \
                 rel=\"noopener noreferrer\">https://example.com/plan</a>"
            ),
            "{out}"
        );
        assert!(out.contains("see <a"), "{out}");
        assert!(out.contains("</a> for the rest"), "{out}");
    }

    /// Where an address ends, when the sentence holding it ends as well.
    ///
    /// The bracket is the case that cannot be a list of characters: the same
    /// `)` is punctuation in one of these and part of the address in the other,
    /// and only counting tells them apart.
    #[test]
    fn the_sentence_around_an_address_is_not_part_of_it() {
        for (markdown, want) in [
            ("go to https://a.example.", "https://a.example"),
            ("go to https://a.example, then", "https://a.example"),
            ("go to https://a.example/x?y=1!", "https://a.example/x?y=1"),
            ("(https://a.example)", "https://a.example"),
            (
                "https://en.example.org/A_(b)",
                "https://en.example.org/A_(b)",
            ),
        ] {
            let out = body(markdown, &around());
            assert!(
                out.contains(&format!("href=\"{want}\"")),
                "{markdown} gave {out}"
            );
        }
    }

    /// The trap the underscore sets, and it is set across most of Wikipedia.
    ///
    /// `pulldown-cmark` cuts a run of text wherever it had to weigh a `_` or a
    /// `*`, so `.../Budget_(finance)` arrives as more than one event although
    /// none of it is emphasis. Scanned an event at a time, the address stops at
    /// the underscore — and what is left is a link to somewhere else with the
    /// rest of the real address sitting beside it as words.
    #[test]
    fn an_address_the_parser_cut_up_is_still_one_address() {
        let out = body("https://en.example.org/wiki/Budget_(finance)", &around());
        assert!(
            out.contains("href=\"https://en.example.org/wiki/Budget_(finance)\""),
            "{out}"
        );
        assert_eq!(out.matches("<a ").count(), 1, "{out}");
    }

    /// Three things that look like the start of an address and are not one.
    #[test]
    fn what_is_not_an_address_stays_words() {
        for markdown in ["xhttps://a.example", "https:// nothing", "http://"] {
            let out = body(markdown, &around());
            assert!(!out.contains("<a "), "{markdown} gave {out}");
        }
    }

    /// The four places an address is being shown rather than offered.
    ///
    /// The raw-HTML one is the one worth having: this module turns raw HTML
    /// into a fenced block itself, so the flag that says "this is code now" has
    /// to be set on the block *it* writes and not only on the ones a note
    /// wrote. Getting that wrong would turn an address inside unconverted
    /// markup — which is exactly what `noda import tiddlywiki` leaves behind —
    /// into a link nobody wrote.
    #[test]
    fn an_address_in_code_or_in_a_link_is_left_where_it_is() {
        let inline = body("run `curl https://a.example`", &around());
        assert!(!inline.contains("<a "), "{inline}");

        let fenced = body("```\nhttps://a.example\n```", &around());
        assert!(!fenced.contains("<a "), "{fenced}");

        let raw = body("<p>https://a.example</p>", &around());
        assert!(!raw.contains("<a "), "{raw}");

        let inside = body("[https://a.example](k3f9m2p1-the-plan.md)", &around());
        assert_eq!(inside.matches("<a ").count(), 1, "{inside}");
        assert!(inside.contains("href=\"/nb/work/n/k3f9m2p1\""), "{inside}");
    }

    /// However it was written, a link that leaves carries the same two things.
    ///
    /// One assertion made twice, on purpose: the two are opened by different
    /// arms of the same match, and the day they stop agreeing is the day a
    /// reader can tell which of them they pressed.
    #[test]
    fn a_link_that_leaves_carries_the_same_two_attributes_either_way() {
        let opening =
            "<a href=\"https://example.com\" target=\"_blank\" rel=\"noopener noreferrer\">";
        let written = body("[the site](https://example.com)", &around());
        assert!(written.contains(opening), "{written}");
        let bare = body("https://example.com", &around());
        assert!(bare.contains(opening), "{bare}");
    }

    /// And what does not leave carries neither. A fragment and a file are this
    /// origin; `mailto:` and `tel:` are not a page at all, and a tab of their
    /// own would be a blank one left beside a mail client.
    #[test]
    fn what_does_not_leave_the_notebook_is_opened_plainly() {
        let out = body(
            "[a note](k3f9m2p1-the-plan.md), [a file](rack.png), [here](#top), \
             [write](mailto:me@example.com)",
            &around(),
        );
        assert!(!out.contains("target="), "{out}");
        assert!(!out.contains("rel="), "{out}");
        assert_eq!(out.matches("<a ").count(), 4, "{out}");
    }

    /// Anything else the notebook holds is a file, and files are served from one
    /// place. `%20` decodes on the way in and encodes on the way out: the name
    /// on disk has a space in it and the URL may not.
    #[test]
    fn a_link_to_a_file_becomes_a_download() {
        let out = body("the [slides](last%20quarter.pdf) say", &around());
        assert!(
            out.contains("href=\"/nb/work/f/last%20quarter.pdf\""),
            "{out}"
        );
    }

    /// An image is fetched without anybody choosing to, so it comes from the
    /// notebook or from the web and nowhere else.
    #[test]
    fn an_image_is_embedded_from_the_notebook() {
        let out = body("![the rack](rack.png)", &around());
        assert!(out.contains("<img src=\"/nb/work/f/rack.png\""), "{out}");
        assert!(out.contains("alt=\"the rack\""), "{out}");
    }

    /// The whole of the script defence, said twice because there are two ways
    /// in. The words stay — losing them would hide that the note says anything
    /// there at all — and the link goes.
    #[test]
    fn a_script_url_keeps_its_words_and_loses_its_link() {
        let out = body("[press me](javascript:alert(1))", &around());
        assert!(out.contains("press me"), "{out}");
        assert!(!out.contains("javascript:"), "{out}");
        assert!(!out.contains("<a "), "{out}");

        let embedded = body(
            "![x](data:image/svg+xml;base64,PHN2Zz48L3N2Zz4=)",
            &around(),
        );
        assert!(!embedded.contains("<img"), "{embedded}");
        assert!(!embedded.contains("data:"), "{embedded}");
    }

    /// `..` is refused by `link::target`, and what is refused must not be handed
    /// back to the browser as though it were somebody else's URL.
    #[test]
    fn a_path_that_climbs_out_of_the_notebook_leads_nowhere() {
        let out = body("[keys](../../.ssh/id_rsa)", &around());
        assert!(out.contains("keys"), "{out}");
        assert!(!out.contains("<a "), "{out}");
        assert!(!out.contains("id_rsa"), "{out}");
    }

    /// Somebody else's site is still somebody else's site.
    #[test]
    fn an_ordinary_web_link_is_left_alone() {
        let out = body(
            "[docs](https://example.com/a) and <me@example.com>",
            &around(),
        );
        assert!(out.contains("href=\"https://example.com/a\""), "{out}");
        assert!(out.contains("mailto:me@example.com"), "{out}");
    }

    /// `noda import tiddlywiki` leaves HTML it could not convert in the body on
    /// purpose and records `unconverted: html` beside it, so the markup is the
    /// only copy of what that note said. Shown as code it is neither lost nor
    /// run — and the escaping is the renderer's, which is why this asserts on
    /// what came out rather than on what was skipped.
    #[test]
    fn raw_html_is_shown_as_code_and_never_as_markup() {
        let out = body("<div class=\"tc-tiddler\">imported</div>\n", &around());
        assert!(out.contains("<code class=\"language-html\">"), "{out}");
        assert!(out.contains("&lt;div class=\"tc-tiddler\"&gt;"), "{out}");
        assert!(!out.contains("<div class=\"tc-tiddler\">"), "{out}");

        // Inline, in the middle of a paragraph, is the other way it arrives.
        let inline = body("a <script>alert(1)</script> here", &around());
        assert!(!inline.contains("<script>"), "{inline}");
        assert!(inline.contains("&lt;script&gt;"), "{inline}");
    }

    /// `noda todo` reads `- [ ]` across the notebook, so a box has to be a box
    /// here too — a literal `[ ]` would be the same note saying two things.
    #[test]
    fn a_task_list_is_boxes() {
        let out = body("- [x] shipped\n- [ ] not yet\n", &around());
        assert!(out.contains("type=\"checkbox\""), "{out}");
        assert!(out.contains("checked"), "{out}");
    }

    /// The ordinary case, and the one that would be embarrassing to get wrong.
    #[test]
    fn ordinary_markdown_renders() {
        let out = body(
            "# Title\n\nA *word* and a table:\n\n| a | b |\n|---|---|\n| 1 | 2 |\n",
            &around(),
        );
        assert!(out.contains("<h1>Title</h1>"), "{out}");
        assert!(out.contains("<em>word</em>"), "{out}");
        assert!(out.contains("<table>"), "{out}");
    }
}
