//! The web server, driven the way a browser drives it: the real binary, a real
//! socket, and requests written by hand.
//!
//! **By hand, and that is not stubbornness.** Half of what is under test here is
//! the guard, and the guard reads `Host` and `Origin` — headers a decent HTTP
//! client exists to fill in correctly and will not let a caller lie about. A
//! rebinding attack is exactly a request whose `Host` is a lie, so a test that
//! cannot write one cannot test the thing.
//!
//! The port is `0`, so the operating system picks one and the tests can run
//! together without agreeing on numbers in advance. The server says which one it
//! got on its first line of output, which is also the line a reader needs, so
//! nothing exists here purely for the tests.
//!
//! The harness is `tests/cli.rs`'s and `tests/pty.rs`'s, restated rather than
//! shared: an integration test is its own crate. `sign = false` is not optional
//! — libgit2 reads the developer's real git config, so a machine that signs its
//! commits would send every commit here to gpg.

use std::fmt::Write as _;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

use noda::cmd;
use noda::paths::Paths;

struct TempRoot(PathBuf);

impl TempRoot {
    fn new() -> Self {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("noda-web-test-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&path).expect("create temp root");
        TempRoot(path)
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A notebook holding four notes. The last one carries raw HTML on purpose:
/// `noda import tiddlywiki` deliberately leaves such bodies alone, so a note
/// that is markup is an ordinary note and not a hypothetical.
fn a_notebook() -> (TempRoot, Paths) {
    let root = TempRoot::new();
    let paths = Paths::rooted(&root.0);
    std::fs::create_dir_all(paths.config_dir()).expect("config dir");
    std::fs::write(paths.config_dir().join("config.toml"), "sign = false\n").expect("config");
    cmd::init(&paths).expect("init");

    cmd::add(
        &paths,
        Some("Budget review"),
        Some("the q3 budget is late"),
        &["work".to_string()],
    )
    .expect("add");
    cmd::add(
        &paths,
        Some("Meeting notes"),
        Some("# Agenda\n\nthe budget, again"),
        &["work".to_string(), "24.04 Dark patterns".to_string()],
    )
    .expect("add");
    cmd::add(&paths, Some("Reading list"), Some("a book"), &[]).expect("add");
    cmd::add(
        &paths,
        Some("Raw html import"),
        Some("a <div class=\"x\">html</div> here"),
        &["ops".to_string()],
    )
    .expect("add");
    (root, paths)
}

/// Percent-encoding, for the handful of characters a test actually sends.
///
/// Not a general encoder: a test that needed one would be a test whose fixture
/// had got away from it.
fn urlencode(value: &str) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            b' ' => out.push('+'),
            _ => {
                let _ = write!(out, "%{byte:02X}");
            }
        }
    }
    out
}

/// The id of the note with this slug, read off the filename.
///
/// An id is minted, so it cannot be spelled out in a test — the standing rule
/// about fixtures built from minted ids, one layer up.
fn id_of(paths: &Paths, slug: &str) -> String {
    let ending = format!("-{slug}.md");
    let notebooks = std::fs::read_dir(paths.notebooks_dir()).expect("notebooks");
    for notebook in notebooks.flatten() {
        let Ok(entries) = std::fs::read_dir(notebook.path()) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if let Some(id) = name.strip_suffix(&ending) {
                return id.to_string();
            }
        }
    }
    panic!("no note called {slug}");
}

/// What came back.
struct Answer {
    status: u16,
    location: Option<String>,
    body: String,
}

impl Answer {
    fn says(&self, needle: &str) -> bool {
        self.body.contains(needle)
    }

    /// What every timestamp on the page actually reads as.
    ///
    /// Pulled out rather than searched for, because the question is what the
    /// stamps *are* and not whether some string appears somewhere: a page is
    /// full of colons — in its stylesheet, in every URL — so "there is no clock
    /// on this page" cannot be asked of the page as a whole.
    fn stamps(&self) -> Vec<String> {
        self.body
            .match_indices("<span class=\"when\">")
            .filter_map(|(at, opening)| {
                let rest = &self.body[at + opening.len()..];
                rest.split_once("</span>").map(|(text, _)| text.to_string())
            })
            .collect()
    }
}

struct Serving {
    // Declared before the root so it is killed before the directory it is
    // reading goes away.
    child: Child,
    port: u16,
    _root: TempRoot,
}

impl Serving {
    fn start(root: TempRoot, allow: &[&str]) -> Serving {
        let mut command = Command::new(env!("CARGO_BIN_EXE_noda"));
        command.args(["web", "--listen", "127.0.0.1:0"]);
        for name in allow {
            command.args(["--allow-host", name]);
        }
        // All four, `XDG_STATE_HOME` included: the active-notebook pointer lives
        // in state, and a run that missed it would reach past this notebook.
        command
            .env("XDG_CONFIG_HOME", root.0.join("config"))
            .env("XDG_DATA_HOME", root.0.join("data"))
            .env("XDG_STATE_HOME", root.0.join("state"))
            .env("XDG_CACHE_HOME", root.0.join("cache"))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = command.spawn().expect("spawn noda web");
        // The address it actually got, from the line it prints for the reader.
        // `println!` is line buffered, so this arrives as soon as it is written
        // rather than when the process ends — which is just as well, because it
        // does not end.
        let stdout = child.stdout.take().expect("stdout");
        let mut first = String::new();
        BufReader::new(stdout)
            .read_line(&mut first)
            .expect("the server should say where it is");
        let port = first
            .trim()
            .rsplit_once(':')
            .and_then(|(_, port)| port.parse().ok())
            .unwrap_or_else(|| panic!("could not read a port out of {first:?}"));

        Serving {
            child,
            port,
            _root: root,
        }
    }

    fn get(&self, path: &str) -> Answer {
        self.request(path, &[])
    }

    /// A form, submitted the way a browser submits one.
    ///
    /// `application/x-www-form-urlencoded` and nothing else: there is no script
    /// on any of these pages, so a form is the only thing that can ask for a
    /// change, and this is the only shape a form without a file in it sends.
    fn post(&self, path: &str, fields: &[(&str, &str)]) -> Answer {
        let body = fields
            .iter()
            .map(|(name, value)| format!("{}={}", urlencode(name), urlencode(value)))
            .collect::<Vec<_>>()
            .join("&");
        self.send("POST", path, &[], Some(&body))
    }

    /// The fingerprint a form was handed, so a test can send it back — or send
    /// back a stale one on purpose.
    fn fingerprint_on(&self, path: &str) -> String {
        let body = self.get(path).body;
        let at = body
            .find("name=\"fingerprint\" value=\"")
            .unwrap_or_else(|| panic!("no fingerprint on {path}:\n{body}"));
        let rest = &body[at + "name=\"fingerprint\" value=\"".len()..];
        rest.split_once('"')
            .map(|(value, _)| value.to_string())
            .expect("an unterminated attribute")
    }

    /// A request with headers of the caller's choosing, `Host` included — which
    /// is the whole point of writing these by hand.
    fn request(&self, path: &str, headers: &[(&str, &str)]) -> Answer {
        self.send("GET", path, headers, None)
    }

    fn send(
        &self,
        method: &str,
        path: &str,
        headers: &[(&str, &str)],
        body: Option<&str>,
    ) -> Answer {
        let mut socket =
            TcpStream::connect(("127.0.0.1", self.port)).expect("connect to the server");
        let host = headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("host"))
            .map_or_else(
                || format!("127.0.0.1:{}", self.port),
                |(_, v)| (*v).to_string(),
            );

        let mut wire = format!("{method} {path} HTTP/1.1\r\nHost: {host}\r\n");
        if let Some(body) = body {
            let _ = write!(
                wire,
                "Content-Type: application/x-www-form-urlencoded\r\n\
                 Content-Length: {}\r\n",
                body.len()
            );
        }
        for (name, value) in headers {
            if !name.eq_ignore_ascii_case("host") {
                let _ = write!(wire, "{name}: {value}\r\n");
            }
        }
        // Closed by the server when it is done, which is what makes "read to the
        // end" a complete answer without parsing a length.
        wire.push_str("Connection: close\r\n\r\n");
        if let Some(body) = body {
            wire.push_str(body);
        }
        socket.write_all(wire.as_bytes()).expect("write a request");

        let mut raw = Vec::new();
        socket.read_to_end(&mut raw).expect("read the answer");
        let text = String::from_utf8_lossy(&raw).into_owned();
        let (head, body) = text.split_once("\r\n\r\n").unwrap_or((text.as_str(), ""));

        let status = head
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .and_then(|code| code.parse().ok())
            .unwrap_or_else(|| panic!("no status line in {head:?}"));
        let location = head.lines().find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("location")
                .then(|| value.trim().to_string())
        });

        Answer {
            status,
            location,
            body: body.to_string(),
        }
    }
}

impl Drop for Serving {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn serving() -> (Serving, Paths) {
    let (root, paths) = a_notebook();
    (Serving::start(root, &[]), paths)
}

#[test]
fn the_front_page_lists_the_notebooks() {
    let (server, _paths) = serving();
    let answer = server.get("/");
    assert_eq!(answer.status, 200);
    assert!(answer.says("href=\"/nb/default\""), "{}", answer.body);
    // The remote's standing in git's own words, not a verb asking whether you
    // would like to sync.
    assert!(answer.says("no remote"), "{}", answer.body);
    assert!(answer.says("4 notes"), "{}", answer.body);
}

#[test]
fn the_listing_names_every_note() {
    let (server, _paths) = serving();
    let answer = server.get("/nb/default");
    assert_eq!(answer.status, 200);
    for title in [
        "Budget review",
        "Meeting notes",
        "Reading list",
        "Raw html import",
    ] {
        assert!(answer.says(title), "{title} is missing:\n{}", answer.body);
    }
    assert!(answer.says(">work</span>"), "{}", answer.body);
    // A day, and never a clock: noda's stamps are UTC, and a UTC clock with its
    // `Z` cut off to fit a row reads as a local one — wrong by whatever the
    // reader's offset is, and wrong in a way nothing on the page admits to.
    let stamps = answer.stamps();
    assert_eq!(stamps.len(), 4, "{stamps:?}");
    for stamp in &stamps {
        assert_eq!(stamp.len(), 10, "{stamp} is not just a day");
        assert!(!stamp.contains(':'), "{stamp} has a clock in it");
    }

    // Nothing was asked, so nothing is wrong. `Query::parse` refuses an empty
    // token list, and running the ordinary listing through it put a complaint
    // on top of every unfiltered page.
    assert!(
        !answer.says("class=\"problem\""),
        "an unfiltered listing complained:\n{}",
        answer.body
    );
}

#[test]
fn a_query_narrows_the_listing_and_marks_what_matched() {
    let (server, _paths) = serving();
    let answer = server.get("/nb/default?q=budget");
    assert_eq!(answer.status, 200);
    assert!(answer.says("<mark>Budget</mark> review"), "{}", answer.body);
    assert!(!answer.says("Reading list"), "{}", answer.body);
    // What was filtered away is still named, so an empty-looking notebook is
    // never a mystery.
    assert!(answer.says("of 4"), "{}", answer.body);
}

/// The reason `query::split` moved out of the browser: this box is the third
/// field standing in for argv, and a tag may hold a space.
#[test]
fn a_quoted_tag_survives_a_real_query_string() {
    let (server, _paths) = serving();
    let answer = server.get("/nb/default?q=tag%3A%2224.04+Dark+patterns%22");
    assert_eq!(answer.status, 200);
    assert!(answer.says("Meeting notes"), "{}", answer.body);
    assert!(!answer.says("Budget review"), "{}", answer.body);
}

/// Half a query is what every query looks like on the way to being one, so the
/// screen says why and holds still — it does not empty itself over an
/// unfinished thought. The same call the browser's `/` makes.
#[test]
fn an_unfinished_query_says_why_and_keeps_the_notes() {
    let (server, _paths) = serving();
    let answer = server.get("/nb/default?q=OR");
    assert_eq!(answer.status, 200);
    assert!(answer.says("class=\"problem\""), "{}", answer.body);
    assert!(answer.says("Budget review"), "{}", answer.body);
    assert!(answer.says("Reading list"), "{}", answer.body);
}

/// One address per page. A slug follows the title and an id prefix is a
/// convenience; a bookmark has to survive a retitle, so both land on the id.
#[test]
fn a_slug_and_a_prefix_both_lead_to_the_id() {
    let (server, paths) = serving();
    let id = id_of(&paths, "budget-review");

    let by_slug = server.get("/nb/default/n/budget-review");
    assert_eq!(by_slug.status, 303);
    assert_eq!(
        by_slug.location.as_deref(),
        Some(&*format!("/nb/default/n/{id}"))
    );

    let by_prefix = server.get(&format!("/nb/default/n/{}", &id[..4]));
    assert_eq!(by_prefix.status, 303);
    assert_eq!(
        by_prefix.location.as_deref(),
        Some(&*format!("/nb/default/n/{id}"))
    );

    let by_id = server.get(&format!("/nb/default/n/{id}"));
    assert_eq!(by_id.status, 200);
}

/// The signature, end to end: the id and the slug drawn as the one filename
/// they have always been.
#[test]
fn the_note_page_names_the_file_and_stamps_it_whole() {
    let (server, paths) = serving();
    let id = id_of(&paths, "budget-review");
    let answer = server.get(&format!("/nb/default/n/{id}"));

    assert!(answer.says(&format!(">{id}</span>")), "{}", answer.body);
    assert!(answer.says(">-budget-review</span>"), "{}", answer.body);
    assert!(answer.says(">.md</span>"), "{}", answer.body);
    // The stamp whole, `Z` and all — this is the page with room for it, and the
    // whole thing is the only version that cannot be misread.
    assert!(answer.says("updated 20"), "{}", answer.body);
    assert!(answer.says("Z</span>"), "{}", answer.body);
}

/// `noda import tiddlywiki` leaves raw HTML in a body on purpose. It reaches
/// this page as text or it is an injection.
#[test]
fn a_body_holding_markup_arrives_as_text() {
    let (server, paths) = serving();
    let id = id_of(&paths, "raw-html-import");
    let answer = server.get(&format!("/nb/default/n/{id}"));

    assert!(
        answer.says("&lt;div class=&quot;x&quot;&gt;"),
        "{}",
        answer.body
    );
    assert!(!answer.says("<div class=\"x\">"), "{}", answer.body);
}

#[test]
fn a_wrong_address_is_told_apart_from_a_broken_notebook() {
    let (server, _paths) = serving();
    assert_eq!(server.get("/nb/ghost").status, 404);
    assert_eq!(server.get("/nb/default/n/zzzzzzzz").status, 404);
    // And says which, rather than showing a bare code.
    assert!(server.get("/nb/ghost").says("No such notebook"));
    assert!(server.get("/nb/default/n/zzzzzzzz").says("No such note"));
}

/// Every write here is a git commit and there is no session to be missing, so
/// a form on another site would otherwise reach straight in.
#[test]
fn a_page_on_another_site_is_turned_away() {
    let (server, _paths) = serving();
    let answer = server.request("/", &[("Origin", "https://elsewhere.example")]);
    assert_eq!(answer.status, 403);
    assert!(answer.says("elsewhere.example"), "{}", answer.body);
}

/// The rebinding case. `Origin` and `Host` agree — they always do in this
/// attack — so the name itself is what has to be checked.
#[test]
fn a_hostname_nobody_asked_for_is_turned_away() {
    let (server, _paths) = serving();
    let answer = server.request(
        "/",
        &[("Host", "evil.example"), ("Origin", "http://evil.example")],
    );
    assert_eq!(answer.status, 403);
    assert!(answer.says("--allow-host evil.example"), "{}", answer.body);
}

/// The other half of that rule: it has to be possible to say yes, or the two
/// deployments the documentation recommends are both refused.
#[test]
fn a_hostname_that_was_asked_for_is_admitted() {
    let (root, _paths) = a_notebook();
    let server = Serving::start(root, &["noda.tail1234.ts.net"]);
    let answer = server.request(
        "/",
        &[
            ("Host", "noda.tail1234.ts.net"),
            ("Origin", "https://noda.tail1234.ts.net"),
        ],
    );
    assert_eq!(answer.status, 200);
}

/// Typing an address into the bar sends no `Origin` at all, which is the
/// ordinary case and must not be the refused one.
#[test]
fn an_ordinary_navigation_is_answered() {
    let (server, _paths) = serving();
    assert_eq!(server.get("/").status, 200);
}

/// A note written from a phone, end to end: the form, the commit, the redirect
/// to the note that now exists.
#[test]
fn a_note_can_be_written_from_the_browser() {
    let (server, paths) = serving();
    let made = server.post(
        "/nb/default/new",
        &[
            ("title", "From the phone"),
            ("tags", "web ops"),
            ("body", "first line\r\nsecond line"),
        ],
    );
    assert_eq!(made.status, 303);
    let at = made.location.expect("somewhere to go");
    assert!(at.starts_with("/nb/default/n/"), "{at}");

    let note = server.get(&at);
    assert_eq!(note.status, 200);
    assert!(note.says("From the phone"), "{}", note.body);
    assert!(note.says("second line"), "{}", note.body);

    // What a `<textarea>` sends, gone by the time it reaches the file. The
    // HTML specification says a form normalises line breaks to CRLF, so this is
    // every browser rather than a quirk of one.
    let written = std::fs::read_to_string(paths.notebooks_dir().join("default").join(format!(
        "{}.md",
        at.rsplit('/').next().map(|id| format!("{id}-from-the-phone")).unwrap()
    )))
    .expect("the note that was just written");
    assert!(!written.contains('\r'), "{written:?}");
}

/// The optimistic lock, and the whole reason it is a content hash: a stale form
/// is refused, and nothing on disk moves.
#[test]
fn an_edit_against_a_stale_note_is_refused_and_loses_nothing() {
    let (server, paths) = serving();
    let id = id_of(&paths, "budget-review");
    let stale = server.fingerprint_on(&format!("/nb/default/n/{id}/edit"));

    // Somebody else saves first — a terminal in another window, which is the
    // case this exists for.
    let landed = server.post(
        &format!("/nb/default/n/{id}/edit"),
        &[("fingerprint", &stale), ("body", "what the terminal wrote")],
    );
    assert_eq!(landed.status, 303);

    // Now the phone submits the form it was given before any of that.
    let refused = server.post(
        &format!("/nb/default/n/{id}/edit"),
        &[("fingerprint", &stale), ("body", "what the phone wrote")],
    );
    assert_eq!(refused.status, 200, "a refusal is a page, not a redirect");
    assert!(
        refused.says("changed while you were writing"),
        "{}",
        refused.body
    );
    // Both versions are on that page: what is saved, and what they typed — the
    // second still in a box they can edit.
    assert!(refused.says("what the terminal wrote"), "{}", refused.body);
    assert!(refused.says("what the phone wrote"), "{}", refused.body);

    let on_disk = server.get(&format!("/nb/default/n/{id}"));
    assert!(on_disk.says("what the terminal wrote"), "{}", on_disk.body);
    assert!(!on_disk.says("what the phone wrote"), "{}", on_disk.body);
}

/// The tags form says which tags survived; the `+`s and `-`s are the server's
/// problem. One submit is one commit, which is why the boxes are a form rather
/// than a row of links.
#[test]
fn ticking_the_boxes_is_what_says_which_tags_stay() {
    let (server, paths) = serving();
    let id = id_of(&paths, "meeting-notes");

    let saved = server.post(
        &format!("/nb/default/n/{id}/tags"),
        // `work` was ticked off; `24.04 Dark patterns` stays; `ops` is new.
        &[("keep", "24.04 Dark patterns"), ("add", "ops")],
    );
    assert_eq!(saved.status, 303);

    let note = server.get(&format!("/nb/default/n/{id}"));
    assert!(note.says("24.04 Dark patterns"), "{}", note.body);
    assert!(note.says("ops"), "{}", note.body);
    assert!(
        !note.says(">work<"),
        "work should have gone:\n{}",
        note.body
    );
}

/// A tag row is a box beside a word, centred against each other. The rule that
/// makes it one has to out-specify `form.write label` above it, which is the
/// field-label rule and says `display:block`; a bare `.tick` loses that contest,
/// and the row falls back to a baseline with the box and its tag jammed
/// together. The assertion is on the selector because that is where it broke.
#[test]
fn a_tag_row_centres_its_box_against_its_name() {
    let (server, paths) = serving();
    let id = id_of(&paths, "meeting-notes");

    let form = server.get(&format!("/nb/default/n/{id}/tags"));
    assert!(
        form.says("form.write label.tick{display:flex"),
        "the row rule is out-specified:\n{}",
        form.body
    );
}

#[test]
fn a_note_can_be_renamed_and_keeps_its_address() {
    let (server, paths) = serving();
    let id = id_of(&paths, "budget-review");

    let saved = server.post(
        &format!("/nb/default/n/{id}/rename"),
        &[("title", "Budget review 2026")],
    );
    assert_eq!(saved.status, 303);
    // The id never moves, so the address the browser was on is still the note's.
    assert_eq!(
        saved.location.as_deref(),
        Some(&*format!("/nb/default/n/{id}"))
    );

    let note = server.get(&format!("/nb/default/n/{id}"));
    assert!(note.says("Budget review 2026"), "{}", note.body);
    // The slug half of the filename, in its own span — the id and the slug are
    // coloured differently on purpose, so the string is never contiguous.
    assert!(note.says(">-budget-review-2026</span>"), "{}", note.body);
}

/// A refusal comes back to the form with the reason on it, not as a 500 and not
/// as an empty form.
#[test]
fn a_refused_change_is_handed_back_with_the_reason() {
    let (server, paths) = serving();
    let id = id_of(&paths, "budget-review");

    let refused = server.post(&format!("/nb/default/n/{id}/rename"), &[("title", "  ")]);
    assert_eq!(refused.status, 200);
    assert!(refused.says("a note needs a title"), "{}", refused.body);
    assert!(refused.says("<form"), "the form should still be there");
}

#[test]
fn a_note_can_be_deleted_and_the_commit_that_removed_it_stays() {
    let (server, paths) = serving();
    let id = id_of(&paths, "reading-list");

    let gone = server.post(&format!("/nb/default/n/{id}/delete"), &[]);
    assert_eq!(gone.status, 303);
    assert_eq!(gone.location.as_deref(), Some("/nb/default"));
    assert_eq!(server.get(&format!("/nb/default/n/{id}")).status, 404);

    let listing = server.get("/nb/default");
    assert!(!listing.says("Reading list"), "{}", listing.body);
}

/// A `GET` never changes anything. Every form is a `POST`, so a link a
/// prefetcher or a crawler follows cannot commit to the notebook.
#[test]
fn asking_to_delete_only_asks() {
    let (server, paths) = serving();
    let id = id_of(&paths, "reading-list");

    let asked = server.get(&format!("/nb/default/n/{id}/delete"));
    assert_eq!(asked.status, 200);
    assert!(asked.says("Delete"), "{}", asked.body);
    // Still there.
    assert_eq!(server.get(&format!("/nb/default/n/{id}")).status, 200);
}

/// The guard is in front of the writes too, and it always was — which is why it
/// shipped in the pull request before them.
#[test]
fn another_site_cannot_write_either() {
    let (server, paths) = serving();
    let id = id_of(&paths, "budget-review");
    let refused = server.send(
        "POST",
        &format!("/nb/default/n/{id}/delete"),
        &[("Origin", "https://elsewhere.example")],
        Some(""),
    );
    assert_eq!(refused.status, 403);
    assert_eq!(server.get(&format!("/nb/default/n/{id}")).status, 200);
}

/// Nothing on any of these pages needs a script, and this is the assertion that
/// keeps it that way: the enhancement layer arrives later and must stay an
/// enhancement.
#[test]
fn no_page_carries_a_script() {
    let (server, paths) = serving();
    let id = id_of(&paths, "budget-review");
    for path in &[
        "/".to_string(),
        "/nb/default".to_string(),
        format!("/nb/default/n/{id}"),
    ] {
        let answer = server.get(path);
        assert!(!answer.says("<script"), "{path} carries a script");
        assert!(!answer.says("onclick"), "{path} carries a handler");
    }
}
