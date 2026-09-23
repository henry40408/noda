//! A note's body, as HTML.
//!
//! `pulldown-cmark` renders; noda rewrites the event stream first, for four
//! decisions:
//!
//! **Where a destination leads.** Relative paths (what reads well on a git host)
//! become `/nb/<book>/n/<id>` and `/nb/<book>/f/<name>`, resolved by
//! `link::target` — the same function `doctor` and `file mv` use, so "is this
//! inside the notebook" has one answer.
//!
//! **What raw HTML turns into.** A code block: shown, not run. Not dropped,
//! because `noda import tiddlywiki` leaves unconverted HTML in the body as the
//! only copy of what it said. A destination with a scheme noda does not allow
//! keeps its text and loses its link; with raw HTML already code, a URL is the
//! only place left to carry a script.
//!
//! **Bare URLs.** `CommonMark` has none, but notes written elsewhere rely on
//! them. GFM's rules, narrowed to `http://` and `https://`: `www.` would mean
//! choosing a scheme for the writer, and a bare email is left to `<me@x>`.
//!
//! **What a link that leaves carries.** A note's address holds its id, which
//! `Referer` would hand over. Such links get `target="_blank"` and
//! `rel="noopener noreferrer"`, and the page also sends
//! `Referrer-Policy: same-origin` as a header (`web::html`) and a `<meta>`
//! (`page::dressed`): a proxy may strip the header, only the meta covers images,
//! and only `noopener` stops a new tab reaching back through `window.opener`.

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
    /// Given a class, so a reader can see before pressing that it stays inside.
    Note(String),
    /// A fragment, a notebook file, `mailto:` or `tel:`: this origin or not a
    /// page, so no new tab.
    To(String),
    /// `http(s)`: opened by hand, since a `Tag::Link` has nowhere for the
    /// attributes.
    Away(String),
    /// The link is dropped; its text stays.
    Nowhere,
}

/// What has to be written when a link ends.
enum Closing {
    Nothing,
    /// The renderer's own `</a>`.
    Rendered,
    /// A literal `</a>`, for an anchor noda wrote itself.
    Written,
}

/// `markdown`, rendered.
pub fn body(markdown: &str, around: &Around) -> String {
    // Task lists because `noda todo` reads them.
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);

    // Two stacks, not a counter: an image's alt text may contain a link.
    let mut links: Vec<Closing> = Vec::new();
    let mut images: Vec<bool> = Vec::new();
    // A code block's contents arrive as `Event::Text`, and an address in one
    // is not a link.
    let mut fenced = false;
    let mut prose = String::new();
    let mut rewritten = Vec::new();

    for event in Parser::new_ext(markdown, options) {
        // Prose is gathered before it is scanned: the parser splits text at
        // every `_` or `*` it considered, which would cut an address at its
        // first underscore. Any non-text event (including entering a link or
        // code block) spills the run.
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
            Event::Start(Tag::HtmlBlock) => {
                fenced = true;
                rewritten.push(Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(
                    "html".into(),
                ))));
            }
            Event::Start(Tag::CodeBlock(kind)) => {
                fenced = true;
                rewritten.push(Event::Start(Tag::CodeBlock(kind)));
            }
            Event::End(TagEnd::HtmlBlock | TagEnd::CodeBlock) => {
                fenced = false;
                rewritten.push(Event::End(TagEnd::CodeBlock));
            }
            // The renderer escapes text and code, so this is the defence.
            Event::Html(raw) => rewritten.push(Event::Text(raw)),
            Event::InlineHtml(raw) => rewritten.push(Event::Code(raw)),

            // `<me@example.com>` has no scheme (the renderer adds `mailto:`),
            // so `route` would take it for a relative filename.
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
                // A `Tag::Link` has nowhere to put the class.
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
                // An image fetch's referrer is covered by the page's `<meta>`.
                Route::To(url) | Route::Away(url) => {
                    images.push(false);
                    rewritten.push(Event::Start(Tag::Image {
                        link_type,
                        dest_url: url.into(),
                        title,
                        id,
                    }));
                }
                // Only the alt text is kept. `![x](a-note.md)` would display a
                // page as a picture, so it is refused too.
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
        // As text, so the renderer escapes it.
        out.push(Event::Text(url.to_string().into()));
        out.push(Event::Html("</a>".into()));
        at = span.end;
    }
    if at < text.len() {
        out.push(Event::Text(text[at..].to_string().into()));
    }
}

/// The anchor for a destination that leaves the notebook, shared by written
/// and bare links so both behave the same.
fn leaving(url: &str) -> String {
    format!(
        "<a href=\"{}\" target=\"_blank\" rel=\"noopener noreferrer\">",
        escape(url)
    )
}

/// The bare `http://` and `https://` addresses in a run of prose.
///
/// GFM's autolink literal rules, narrowed to two schemes:
///
/// - **Start** after nothing, whitespace or one of `*_~(`, so
///   `xhttps://example.com` does not match.
/// - **End** at the first whitespace or `<`, then back over `?!.,:*_~`; a
///   trailing `)` is dropped only if unbalanced, so
///   `https://en.example.org/A_(b)` keeps it. Quotes and `;` stay, as in GFM.
///
/// Running after the parser, an address the parser actually turned into
/// emphasis is not found. That is rare, fails towards prose, and fixing it
/// would mean owning an inline parser.
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

/// `embed` (an image, fetched unasked) narrows what is allowed.
fn route(dest: &str, around: &Around, embed: bool) -> Route {
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

    match link::scheme(dest) {
        Some(scheme) if serveable(scheme, embed) => {
            if scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https") {
                Route::Away(dest.to_string())
            } else {
                Route::To(dest.to_string())
            }
        }
        // No scheme means a path `link::target` refused, e.g. one climbing out.
        Some(_) | None => Route::Nowhere,
    }
}

/// An allow-list, so `data:` and any scheme invented later are refused too.
fn serveable(scheme: &str, embed: bool) -> bool {
    if embed {
        return scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https");
    }
    ["http", "https", "mailto", "tel"]
        .iter()
        .any(|allowed| scheme.eq_ignore_ascii_case(allowed))
}

/// Percent-encodes a path; a space, `#` or `?` would end it. `/` is kept so a
/// nested path means the same as on disk.
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

    #[test]
    fn a_link_to_another_note_becomes_that_notes_address() {
        let out = body("see [the plan](k3f9m2p1-the-plan.md) first", &around());
        assert!(out.contains("href=\"/nb/work/n/k3f9m2p1\""), "{out}");
        assert!(!out.contains(".md"), "{out}");
    }

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
        // Anchors are written by hand, so check they balance.
        assert_eq!(
            out.matches("<a ").count(),
            out.matches("</a>").count(),
            "{out}"
        );
    }

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

    /// `pulldown-cmark` splits text at a `_` it considered for emphasis.
    #[test]
    fn an_address_the_parser_cut_up_is_still_one_address() {
        let out = body("https://en.example.org/wiki/Budget_(finance)", &around());
        assert!(
            out.contains("href=\"https://en.example.org/wiki/Budget_(finance)\""),
            "{out}"
        );
        assert_eq!(out.matches("<a ").count(), 1, "{out}");
    }

    #[test]
    fn what_is_not_an_address_stays_words() {
        for markdown in ["xhttps://a.example", "https:// nothing", "http://"] {
            let out = body(markdown, &around());
            assert!(!out.contains("<a "), "{markdown} gave {out}");
        }
    }

    /// The raw-HTML case checks `fenced` is set on the block this module
    /// writes, not only on real code blocks.
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

    /// Written and bare links are opened by different code paths.
    #[test]
    fn a_link_that_leaves_carries_the_same_two_attributes_either_way() {
        let opening =
            "<a href=\"https://example.com\" target=\"_blank\" rel=\"noopener noreferrer\">";
        let written = body("[the site](https://example.com)", &around());
        assert!(written.contains(opening), "{written}");
        let bare = body("https://example.com", &around());
        assert!(bare.contains(opening), "{bare}");
    }

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

    /// `%20` decodes on the way in and is encoded again on the way out.
    #[test]
    fn a_link_to_a_file_becomes_a_download() {
        let out = body("the [slides](last%20quarter.pdf) say", &around());
        assert!(
            out.contains("href=\"/nb/work/f/last%20quarter.pdf\""),
            "{out}"
        );
    }

    #[test]
    fn an_image_is_embedded_from_the_notebook() {
        let out = body("![the rack](rack.png)", &around());
        assert!(out.contains("<img src=\"/nb/work/f/rack.png\""), "{out}");
        assert!(out.contains("alt=\"the rack\""), "{out}");
    }

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

    #[test]
    fn a_path_that_climbs_out_of_the_notebook_leads_nowhere() {
        let out = body("[keys](../../.ssh/id_rsa)", &around());
        assert!(out.contains("keys"), "{out}");
        assert!(!out.contains("<a "), "{out}");
        assert!(!out.contains("id_rsa"), "{out}");
    }

    #[test]
    fn an_ordinary_web_link_is_left_alone() {
        let out = body(
            "[docs](https://example.com/a) and <me@example.com>",
            &around(),
        );
        assert!(out.contains("href=\"https://example.com/a\""), "{out}");
        assert!(out.contains("mailto:me@example.com"), "{out}");
    }

    #[test]
    fn raw_html_is_shown_as_code_and_never_as_markup() {
        let out = body("<div class=\"tc-tiddler\">imported</div>\n", &around());
        assert!(out.contains("<code class=\"language-html\">"), "{out}");
        assert!(out.contains("&lt;div class=\"tc-tiddler\"&gt;"), "{out}");
        assert!(!out.contains("<div class=\"tc-tiddler\">"), "{out}");

        let inline = body("a <script>alert(1)</script> here", &around());
        assert!(!inline.contains("<script>"), "{inline}");
        assert!(inline.contains("&lt;script&gt;"), "{inline}");
    }

    #[test]
    fn a_task_list_is_boxes() {
        let out = body("- [x] shipped\n- [ ] not yet\n", &around());
        assert!(out.contains("type=\"checkbox\""), "{out}");
        assert!(out.contains("checked"), "{out}");
    }

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
