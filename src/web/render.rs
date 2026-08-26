//! A note's body, as HTML.
//!
//! The rendering is `pulldown-cmark`'s. What is noda's is four decisions it will
//! not make, all taken by rewriting the event stream *before* it is rendered
//! rather than fixing up HTML afterwards:
//!
//! **Where a destination leads.** A note points at its neighbours with ordinary
//! relative paths, because that is what makes a notebook readable on a git host
//! and in an editor. Those become `/nb/<book>/n/<id>` and `/nb/<book>/f/<name>`,
//! resolved by `link::target` — the same function `doctor` and `file mv` use,
//! because a second answer to "is this inside the notebook" is how the
//! network-facing one ends up wrong.
//!
//! **What raw HTML turns into.** A code block: escaped, shown, not run.
//! Dropping it is not harmless — `noda import tiddlywiki` leaves HTML it could
//! not convert in the body, so the raw markup is the only copy of what that note
//! said. A destination carrying a scheme noda does not serve keeps its text and
//! loses its link, which is the whole of the script defence: with raw HTML
//! already code, a URL is the only thing left that can carry one.
//!
//! **What is a link without having been written as one.** `CommonMark` has no
//! bare URLs, and every other Markdown anybody reads does — so notes written
//! elsewhere arrived with their references as unpressable prose.
//!
//! GFM's rules narrowed to `http://` and `https://`. `www.example.com` is not
//! matched, because that means choosing a scheme on the writer's behalf; a bare
//! email is not either, `<me@example.com>` being the writer saying they meant
//! it.
//!
//! **What a link that leaves carries.** A note's address holds somebody's note
//! id, and the `Referer` hands the whole of it to whoever is on the other end.
//! So a destination that leaves is opened here by hand with `target="_blank"`
//! and `rel="noopener noreferrer"`, and the page says
//! `Referrer-Policy: same-origin` twice more — as a header (`web::html`) and in
//! its own `<head>` (`page::dressed`). Not `no-referrer`, for the reason
//! `web::html` gives.
//!
//! Three statements of one rule, because each covers what the others cannot: a
//! proxy may strip the header, only the meta covers an image fetched without the
//! reader choosing anything, and `noopener` is what neither says — a page in a
//! new tab can reach back through `window.opener`.

use std::collections::BTreeMap;
use std::fmt::Write;
use std::ops::Range;

use pulldown_cmark::{CodeBlockKind, Event, LinkType, Options, Parser, Tag, TagEnd, html};

use crate::link;
use crate::web::page::escape;

/// Which notebook the page belongs to, and which filenames are notes.
pub struct Around<'a> {
    pub book: &'a str,
    /// Filename to id: the spelling a body uses, and the address the web has.
    pub notes: BTreeMap<String, String>,
}

impl<'a> Around<'a> {
    /// From `Notebook::named_files`, which opens nothing: rendering one note is
    /// not a reason to parse every other one.
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
    /// Marked as such, because a reader deserves to know before pressing
    /// whether a link stays inside the notebook — `style.rs`'s reason for
    /// colouring an id.
    Note(String),
    /// A fragment, a file the notebook holds, a `mailto:` or a `tel:`. Nothing
    /// is added: the first two are this origin, and the last two are not a page,
    /// so a tab of their own would be a blank one beside a mail client.
    To(String),
    /// The one destination that gives something away by being followed, and the
    /// only one opened by hand — a `Tag::Link` has nowhere for the attributes.
    Away(String),
    /// The link is dropped and its text stays: losing the words would hide that
    /// the note says anything there at all.
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
    // task lists because `noda todo` already reads them.
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);

    // Two stacks and not one counter: an image's alt text may contain a link,
    // so the two nest and their ends arrive tagged differently.
    let mut links: Vec<Closing> = Vec::new();
    let mut images: Vec<bool> = Vec::new();
    // `Event::Code` is its own event, but a fenced block's contents arrive as
    // `Event::Text` — and an address inside one is shown, not offered.
    let mut fenced = false;
    // Why the loop below has a step before its `match`.
    let mut prose = String::new();
    let mut rewritten = Vec::new();

    for event in Parser::new_ext(markdown, options) {
        // **A run of prose is gathered before it is looked at.** The parser
        // cuts text wherever it considered a `_` or `*`, so an address read a
        // piece at a time stops at its first underscore — which is most of
        // Wikipedia. Anything that is not text spills what was gathered.
        //
        // The context cannot change inside a run: entering a link, an image or a
        // code block takes an event, and that event is the one that spills.
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
            // `html` is the language it is, and a later highlighter will want
            // to have been told.
            Event::Start(Tag::HtmlBlock) => {
                fenced = true;
                rewritten.push(Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(
                    "html".into(),
                ))));
            }
            // Marked for the same reason: from here the two are the same
            // events, which is what makes turning one into the other a defence
            // — and why their ends share an arm.
            Event::Start(Tag::CodeBlock(kind)) => {
                fenced = true;
                rewritten.push(Event::Start(Tag::CodeBlock(kind)));
            }
            Event::End(TagEnd::HtmlBlock | TagEnd::CodeBlock) => {
                fenced = false;
                rewritten.push(Event::End(TagEnd::CodeBlock));
            }
            // The renderer escapes text and code, which is what makes the two
            // lines above a defence rather than a presentation choice.
            Event::Html(raw) => rewritten.push(Event::Text(raw)),
            Event::InlineHtml(raw) => rewritten.push(Event::Code(raw)),

            // `<me@example.com>` carries no scheme — the renderer adds
            // `mailto:` — so as a destination it looks like a relative
            // filename.
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
                // A `Tag::Link` has nowhere to put the class. The href was built
                // here out of an id, and is escaped anyway.
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
                // What a reader gets when a link leaves must not depend on
                // whether the note wrote `[text](url)` or just the address.
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
                // What an image gives away is the fetch itself, answered in the
                // page's `<head>` for every subresource at once.
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
/// GFM's autolink literals, narrowed to two schemes. What is kept is the pair of
/// rules deciding where a match begins and ends, both about the sentence around
/// the address rather than the address.
///
/// **Where one may start.** After nothing, a space, or one of `*_~(` — so
/// `(https://example.com)` matches and `xhttps://example.com` does not.
///
/// **Where one ends.** At the first space or `<`, then walking back over
/// `?!.,:*_~`, because `see https://a.example.` ends in the sentence's full
/// stop. A closing bracket needs counting rather than a list:
/// `https://en.example.org/A_(b)` keeps its `)` and `(see https://a.example)`
/// does not.
///
/// Quotes and `;` are left in, which is GFM's answer too — a note should not
/// render one way on a git host and another way here.
///
/// One thing this cannot see, running after the parser rather than inside it: an
/// address merely *considered* for emphasis survives [`spill`], and one actually
/// cut by it does not. It is rare, it fails towards prose rather than a wrong
/// address, and closing it would mean owning an inline parser.
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
        // A scheme and nothing after it is the word "https" with punctuation.
        if end > host {
            found.push(start..end);
            from = end;
        }
    }
    found
}

/// Walks `end` back over the sentence's punctuation. `host` is where the
/// address's own text begins, so the walk cannot eat the scheme.
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

/// `embed` is whether the browser fetches it without being asked, and narrows
/// what is allowed: a reader chooses to follow a link and chooses nothing about
/// an image.
fn route(dest: &str, around: &Around, embed: bool) -> Route {
    // Inside this page: nothing to resolve, and nothing that can leave.
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

    // Either somebody else's, or a path that climbed out and `link::target`
    // refused — which is exactly the one not to hand back to the browser.
    match link::scheme(dest) {
        // Told apart here rather than at the point of writing, so "does this
        // leave" is answered once.
        Some(scheme) if serveable(scheme, embed) => {
            if scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https") {
                Route::Away(dest.to_string())
            } else {
                Route::To(dest.to_string())
            }
        }
        // Or none at all, which is a path `link::target` refused — handing that
        // back would be asking the browser to fetch it.
        Some(_) | None => Route::Nowhere,
    }
}

/// A list of what is allowed rather than of what is not: `javascript:` is the
/// one everybody thinks of and `data:` the one they forget. Anything invented
/// later is refused by not being named.
fn serveable(scheme: &str, embed: bool) -> bool {
    // Fetched without the reader doing anything, so the web or the notebook —
    // `mailto:` and `tel:` are things to press, not to display.
    if embed {
        return scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https");
    }
    ["http", "https", "mailto", "tel"]
        .iter()
        .any(|allowed| scheme.eq_ignore_ascii_case(allowed))
}

/// A filename may hold a space, a `#` or a `?`, each of which ends the path
/// unencoded. `/` is left alone, so a name that acquires a separator later means
/// the same thing here as on disk.
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

    /// A relative path on a git host has to arrive here as the note's address,
    /// and the id is what survives a retitle.
    #[test]
    fn a_link_to_another_note_becomes_that_notes_address() {
        let out = body("see [the plan](k3f9m2p1-the-plan.md) first", &around());
        assert!(out.contains("href=\"/nb/work/n/k3f9m2p1\""), "{out}");
        assert!(!out.contains(".md"), "{out}");
    }

    /// A link that stays inside says so. Only that one: a file and somebody
    /// else's site are both "away from here", and three distinctions where the
    /// reader needs one is how a page gets loud.
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
        // Opened by hand, so one `</a>` per `<a` is worth asserting.
        assert_eq!(
            out.matches("<a ").count(),
            out.matches("</a>").count(),
            "{out}"
        );
    }

    /// The whole anchor and not a piece of it, because the claim is that the
    /// reader gets what they would have had the note spelled the link out.
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

    /// The bracket cannot be a list of characters: the same `)` is punctuation
    /// in one of these and part of the address in the other.
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

    /// The trap the underscore sets, across most of Wikipedia:
    /// `pulldown-cmark` cuts text wherever it weighed a `_`, so scanned an event
    /// at a time the address stops there — leaving a link somewhere else with
    /// the rest of the real one beside it as words.
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

    /// The four places an address is shown rather than offered.
    ///
    /// The raw-HTML one is worth having: the flag saying "this is code now" has
    /// to be set on the block *this module* writes, or an address inside the
    /// markup `noda import tiddlywiki` leaves becomes a link nobody wrote.
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

    /// One assertion made twice: the two are opened by different arms of the
    /// same match, and the day they disagree a reader can tell which they
    /// pressed.
    #[test]
    fn a_link_that_leaves_carries_the_same_two_attributes_either_way() {
        let opening =
            "<a href=\"https://example.com\" target=\"_blank\" rel=\"noopener noreferrer\">";
        let written = body("[the site](https://example.com)", &around());
        assert!(written.contains(opening), "{written}");
        let bare = body("https://example.com", &around());
        assert!(bare.contains(opening), "{bare}");
    }

    /// A fragment and a file are this origin; `mailto:` and `tel:` are not a
    /// page, and a tab of their own would be a blank one.
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

    /// Files are served from one place. `%20` decodes on the way in and encodes
    /// on the way out: the name on disk has a space and the URL may not.
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

    /// Said twice because there are two ways in. The words stay and the link
    /// goes.
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

    /// The markup an import leaves is the only copy of what that note said, so
    /// shown as code it is neither lost nor run. The escaping is the renderer's,
    /// which is why this asserts on what came out.
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
