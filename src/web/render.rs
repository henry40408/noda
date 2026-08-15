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

use std::collections::BTreeMap;
use std::fmt::Write;

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
    /// Somewhere else: a file the notebook holds, or somebody else's site. This
    /// URL, which may be the one that was written.
    To(String),
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
    let mut rewritten = Vec::new();

    for event in Parser::new_ext(markdown, options) {
        match event {
            // A block of raw HTML becomes a fenced block of it. `html` is the
            // language it is, and a highlighter that arrives later will want to
            // have been told.
            Event::Start(Tag::HtmlBlock) => rewritten.push(Event::Start(Tag::CodeBlock(
                CodeBlockKind::Fenced("html".into()),
            ))),
            Event::End(TagEnd::HtmlBlock) => rewritten.push(Event::End(TagEnd::CodeBlock)),
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
                Route::To(url) => {
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

    let mut out = String::with_capacity(markdown.len());
    html::push_html(&mut out, rewritten.into_iter());
    out
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
        Some(scheme) if serveable(scheme, embed) => Route::To(dest.to_string()),
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
        assert!(out.contains("<a href=\"https://example.com\">"), "{out}");
        assert_eq!(out.matches("class=\"note\"").count(), 1, "{out}");
        // Opened by hand, so the closing tag is worth an assertion of its own:
        // one `</a>` per `<a`, and nothing left hanging over the rest of the note.
        assert_eq!(
            out.matches("<a ").count(),
            out.matches("</a>").count(),
            "{out}"
        );
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
