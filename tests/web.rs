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

/// A notebook holding five notes, two files, and one note that embeds one of
/// them. One note carries raw HTML on purpose:
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

    // Two files, because the two answers a file can get are different: a `.png`
    // is shown where it stands and a `.svg` is not, and the difference is the
    // whole of what `holding` decides.
    let source = root.0.join("rack.png");
    std::fs::write(&source, b"\x89PNG\r\n\x1a\nnot really").expect("write a png");
    cmd::file_add(&paths, &[source], None).expect("file add");
    let vector = root.0.join("plan.svg");
    std::fs::write(&vector, "<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>").expect("write svg");
    cmd::file_add(&paths, &[vector], None).expect("file add");

    // And a third file that happens to be Markdown. `README.md` is not a note —
    // its stem carries no id — and the whole point of it here is that the web
    // side must not decide otherwise from the suffix alone.
    cmd::readme(&paths, false).expect("readme");

    // A note that points at one of them, which is what makes the count on the
    // files page a fact about this notebook rather than a hardcoded zero.
    cmd::add(
        &paths,
        Some("The rack"),
        Some("it looks like ![the rack](rack.png)"),
        &[],
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
    /// The header block as it arrived. Kept whole rather than parsed into a map:
    /// what the tests below ask of it is whether a particular line was said, and
    /// a file's answer is carried entirely by its headers.
    head: String,
}

impl Answer {
    fn says(&self, needle: &str) -> bool {
        self.body.contains(needle)
    }

    /// Whether the row naming `title` is one the query let through.
    ///
    /// **"Not on the page" stopped being the question when the enhancement
    /// layer arrived.** Every row of the notebook is on every listing now; the
    /// ones the query excludes arrive with `hidden` on them, so that the script
    /// can widen a query as well as narrow one without a second copy of the
    /// notes to filter from. `None` when no row names it at all, which is a
    /// different failure and should read as one.
    fn row(&self, title: &str) -> Option<bool> {
        self.body
            .split("<a class=\"row\"")
            .skip(1)
            .find(|row| {
                row.split_once("</a>")
                    .is_some_and(|(row, _)| row.contains(title))
            })
            .map(|row| !row.starts_with(" hidden"))
    }

    /// One header, by name.
    fn header(&self, name: &str) -> Option<String> {
        self.head.lines().find_map(|line| {
            let (found, value) = line.split_once(':')?;
            found
                .eq_ignore_ascii_case(name)
                .then(|| value.trim().to_string())
        })
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

    /// A page whose errand has stopped running.
    ///
    /// **"Not finished yet" is not a failure**, which is the rule the browser
    /// tests learned the hard way: one round trip, an answer that can say "not
    /// yet", and a loop that can see it. A network errand is the one thing here
    /// that does not finish inside the request that started it, so this is the
    /// only place a test has to wait at all.
    fn settled(&self, path: &str) -> Answer {
        for _ in 0..200 {
            let answer = self.get(path);
            if !answer.says("said working") {
                return answer;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        panic!("the errand on {path} never finished");
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
            head: head.to_string(),
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

/// The same notebook, wired to a bare repository standing in for a git host.
///
/// libgit2's local transport is the same push and fetch machinery HTTPS and SSH
/// use, so a `sync` here goes through the real code without a network or
/// credentials — which is what `tests/cli.rs` does for the same reason.
fn serving_with_a_remote() -> (Serving, Paths, PathBuf) {
    let (root, paths) = a_notebook();
    let branch = noda::notebook::Notebook::open(&paths, "default")
        .expect("open the notebook")
        .branch()
        .expect("its branch");

    let remote = root.0.join("origin.git");
    git2::Repository::init_bare(&remote)
        .expect("init a bare remote")
        // `main` or `master` depending on the machine's `init.defaultBranch`,
        // so it is read off the notebook rather than assumed.
        .set_head(&format!("refs/heads/{branch}"))
        .expect("point the remote at that branch");
    let url = remote.to_str().expect("utf-8 path").to_string();
    cmd::remote_set(&paths, &url).expect("set the remote");

    (Serving::start(root, &[]), paths, remote)
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
    assert!(answer.says("5 notes"), "{}", answer.body);
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
    assert_eq!(stamps.len(), 5, "{stamps:?}");
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
    assert_eq!(answer.row("Reading list"), Some(false), "{}", answer.body);
    // What was filtered away is still named, so an empty-looking notebook is
    // never a mystery.
    assert!(answer.says("of 5"), "{}", answer.body);
}

/// The reason `query::split` moved out of the browser: this box is the third
/// field standing in for argv, and a tag may hold a space.
#[test]
fn a_quoted_tag_survives_a_real_query_string() {
    let (server, _paths) = serving();
    let answer = server.get("/nb/default?q=tag%3A%2224.04+Dark+patterns%22");
    assert_eq!(answer.status, 200);
    assert_eq!(answer.row("Meeting notes"), Some(true), "{}", answer.body);
    assert_eq!(answer.row("Budget review"), Some(false), "{}", answer.body);
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

/// **A note page carries the index pane's frame and none of its rows.**
///
/// This is the whole of the bargain the two-pane layout makes. The listing is
/// about 290 bytes a note, and below 1024px not one of those bytes is drawn —
/// so the page goes out with the pane's frame, and the script asks for the rest
/// where the column is actually on screen. A regression here is invisible on a
/// desktop, which is exactly why it is asserted rather than looked at.
#[test]
fn a_note_page_is_sent_without_the_listing_beside_it() {
    let (server, paths) = serving();
    let id = id_of(&paths, "budget-review");
    let answer = server.get(&format!("/nb/default/n/{id}"));

    // The frame: the pane, and a search field that is a working form on its own.
    assert!(answer.says("class=\"pane index\""), "{}", answer.body);
    assert!(
        answer.says("<form class=\"searchbar\" method=\"get\" action=\"/nb/default\""),
        "{}",
        answer.body
    );
    // And nothing in it. `main class="rows"` closing immediately is the shape
    // an empty pane has.
    assert!(
        answer.says("<main class=\"rows\"></main>"),
        "the listing was sent with the note: {}",
        answer.body
    );
    // Not another note's row, by any spelling.
    assert!(
        !answer.says("Reading list"),
        "the listing was sent with the note: {}",
        answer.body
    );
    // `indexed` is what says the pane has rows, and this one does not. Asserted
    // as the whole attribute: the stylesheet inlined into every page names the
    // class in a selector, so the bare word is on all of them.
    assert!(
        answer.says("class=\"app split at-note\""),
        "{}",
        answer.body
    );
}

/// The listing route is the other half: its rows are in the markup, so it says
/// `indexed` and needs nobody's help to draw them.
#[test]
fn the_listing_carries_its_own_rows_and_says_so() {
    let (server, _paths) = serving();
    let answer = server.get("/nb/default");
    assert!(
        answer.says("class=\"app split at-list indexed\""),
        "{}",
        answer.body
    );
    assert!(answer.says("Budget review"), "{}", answer.body);
}

/// The pane beside the listing, on a screen wide enough to show one.
///
/// A notebook that has a `README.md` has already written the page that is about
/// the whole of it — the same file `noda readme` writes and a git host shows —
/// so that is what stands there. Rendered, because it is Markdown and the
/// notebook's own; and only ever drawn above 1024px, which is why the phone
/// tests never see it.
#[test]
fn the_notebooks_front_page_stands_where_no_note_is_picked() {
    let (server, paths) = serving();
    let notebook = paths.notebooks_dir().join("default");
    std::fs::write(
        notebook.join("README.md"),
        "# Ledger\n\nWhat this notebook is for.\n",
    )
    .expect("could not write a README");

    let answer = server.get("/nb/default");
    assert!(answer.says("class=\"pane read\""), "{}", answer.body);
    assert!(answer.says("README.md"), "{}", answer.body);
    assert!(
        answer.says("What this notebook is for."),
        "the README was not rendered: {}",
        answer.body
    );
}

/// And without one, an invitation rather than an empty column. An empty screen
/// is a moment for direction.
#[test]
fn a_notebook_with_no_front_page_invites_a_note_instead() {
    let (server, paths) = serving();
    std::fs::remove_file(paths.notebooks_dir().join("default").join("README.md"))
        .expect("could not take the README away");
    let answer = server.get("/nb/default");
    assert!(answer.says("Pick a note"), "{}", answer.body);
    assert!(!answer.says("README.md"), "{}", answer.body);
}

/// `noda import tiddlywiki` leaves raw HTML in a body on purpose. Now that the
/// page renders Markdown, it reaches the reader as a code block — escaped,
/// shown and not run — or it is an injection.
#[test]
fn a_body_holding_markup_arrives_as_code() {
    let (server, paths) = serving();
    let id = id_of(&paths, "raw-html-import");
    let answer = server.get(&format!("/nb/default/n/{id}"));

    // Inline, because that is where this fixture's markup sits — mid-paragraph.
    // A whole block of it becomes a fenced `language-html` block instead, which
    // `web::render`'s own tests cover.
    assert!(
        answer.says("<code>&lt;div class=\"x\"&gt;"),
        "{}",
        answer.body
    );
    assert!(!answer.says("<div class=\"x\">"), "{}", answer.body);
}

/// The files page is the notebook's other half: everything it holds that is
/// not a note, with the count of notes pointing at each one — the same question
/// `doctor --links` answers when it names orphans.
#[test]
fn the_files_page_lists_what_is_not_a_note() {
    let (server, paths) = serving();
    let answer = server.get("/nb/default/files");

    assert_eq!(answer.status, 200);
    assert!(answer.says("rack.png"), "{}", answer.body);
    assert!(answer.says("plan.svg"), "{}", answer.body);
    assert!(
        answer.says("href=\"/nb/default/f/rack.png\""),
        "{}",
        answer.body
    );
    // One note embeds the png and nothing points at the svg.
    assert!(answer.says("in 1 note"), "{}", answer.body);
    assert!(answer.says("nothing links to it"), "{}", answer.body);
    // Markdown is not the test — a name that carries an id is. `README.md` is a
    // file the notebook holds and so belongs here, offered like any other.
    assert!(
        answer.says("href=\"/nb/default/f/README.md\""),
        "{}",
        answer.body
    );
    // Notes have their own pages and are not listed here as files.
    let id = id_of(&paths, "budget-review");
    assert!(
        !answer.says(&format!("{id}-budget-review.md")),
        "{}",
        answer.body
    );
}

/// An image is shown where it stands. Anything that can carry a script is not,
/// and SVG is the one that catches people out — it is a document, and a
/// document served inline from this origin is a script on this page.
#[test]
fn a_file_is_served_and_only_the_safe_ones_are_shown_in_place() {
    let (server, _paths) = serving();

    let png = server.get("/nb/default/f/rack.png");
    assert_eq!(png.status, 200);
    assert!(png.body.contains("PNG"), "{:?}", png.body);
    assert!(png.body.contains("not really"), "{:?}", png.body);
    assert_eq!(png.header("content-type").as_deref(), Some("image/png"));
    assert_eq!(
        png.header("x-content-type-options").as_deref(),
        Some("nosniff")
    );
    assert!(
        png.header("content-disposition")
            .is_some_and(|value| value.starts_with("inline")),
        "{:?}",
        png.header("content-disposition")
    );

    let svg = server.get("/nb/default/f/plan.svg");
    assert_eq!(svg.status, 200);
    assert!(
        svg.header("content-disposition")
            .is_some_and(|value| value.starts_with("attachment")),
        "an svg must arrive as a download: {:?}",
        svg.header("content-disposition")
    );
    // Nothing it could load, and nothing it could run, whatever a browser makes
    // of it later.
    assert!(
        svg.header("content-security-policy")
            .is_some_and(|value| value.contains("default-src 'none'")),
        "{:?}",
        svg.header("content-security-policy")
    );
}

/// The one place noda opens a path somebody else named. `link::target` is the
/// gate, and it is the same gate `doctor` and `file mv` resolve links with.
#[test]
fn a_file_request_cannot_climb_out_of_the_notebook() {
    let (server, _paths) = serving();

    for path in [
        "/nb/default/f/..%2f..%2f..%2fetc%2fpasswd",
        "/nb/default/f/%2Fetc%2Fpasswd",
        "/nb/default/f/nothing-here.png",
    ] {
        let answer = server.get(path);
        assert_eq!(answer.status, 404, "{path} was answered:\n{}", answer.body);
    }
}

/// A note is read at its own address, rendered. Answering for it as a file too
/// would be a second, unrendered way to read one — and the way that skips every
/// decision the renderer makes.
#[test]
fn a_note_is_not_served_as_a_file() {
    let (server, paths) = serving();
    let id = id_of(&paths, "budget-review");
    let answer = server.get(&format!("/nb/default/f/{id}-budget-review.md"));
    assert_eq!(answer.status, 404, "{}", answer.body);
}

/// The other half of that rule, and the half a suffix test gets wrong: Markdown
/// the notebook holds as a file is served like any other file. `README.md` is
/// the one every notebook can have — the files page lists it and links to it,
/// and refusing it here made that link a dead end.
#[test]
fn a_markdown_file_that_is_not_a_note_is_served() {
    let (server, _paths) = serving();

    let answer = server.get("/nb/default/f/README.md");
    assert_eq!(answer.status, 200, "{}", answer.body);
    assert!(answer.says("default"), "{}", answer.body);

    // And the same name resolves on the way to its backlinks, which asks the
    // question of a file rather than opening it.
    let links = server.get("/nb/default/f/README.md/backlinks");
    assert_eq!(links.status, 200, "{}", links.body);
}

/// The body is Markdown and arrives rendered: a heading is a heading, and a
/// link to another note points at that note's address rather than at a `.md`
/// file that means nothing to a browser.
#[test]
fn a_note_body_is_rendered_and_its_links_point_at_notes() {
    let (server, paths) = serving();
    let budget = id_of(&paths, "budget-review");
    let meeting = id_of(&paths, "meeting-notes");

    // Written the way a note on a git host writes it: a relative filename.
    let saved = server.post(
        &format!("/nb/default/n/{meeting}/edit"),
        &[
            (
                "fingerprint",
                &server.fingerprint_on(&format!("/nb/default/n/{meeting}/edit")),
            ),
            (
                "body",
                &format!("# Agenda\n\nsee [the budget]({budget}-budget-review.md)\n"),
            ),
        ],
    );
    assert_eq!(saved.status, 303);

    let answer = server.get(&format!("/nb/default/n/{meeting}"));
    assert!(answer.says("<h1>Agenda</h1>"), "{}", answer.body);
    assert!(
        answer.says(&format!("href=\"/nb/default/n/{budget}\"")),
        "{}",
        answer.body
    );
    assert!(!answer.says("budget-review.md"), "{}", answer.body);
}

/// An embedded image is fetched from the notebook, which is what makes the
/// files route load-bearing rather than a download page.
#[test]
fn an_embedded_image_points_at_the_file_route() {
    let (server, paths) = serving();
    let id = id_of(&paths, "the-rack");
    let answer = server.get(&format!("/nb/default/n/{id}"));
    assert!(
        answer.says("<img src=\"/nb/default/f/rack.png\""),
        "{}",
        answer.body
    );
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

/// One field, as many tags as fit in it — cut by `query::split`, so a space
/// separates and a quote holds a tag together. The server could always do this;
/// what it could not do was say so, which is why the label is plural and the
/// placeholder shows a quoted tag.
#[test]
fn the_add_field_takes_more_than_one_tag() {
    let (server, paths) = serving();
    let id = id_of(&paths, "meeting-notes");

    let saved = server.post(
        &format!("/nb/default/n/{id}/tags"),
        &[("keep", "work"), ("add", "docs infra \"loud neighbours\"")],
    );
    assert_eq!(saved.status, 303);

    let note = server.get(&format!("/nb/default/n/{id}"));
    for tag in ["work", "docs", "infra", "loud neighbours"] {
        assert!(note.says(tag), "{tag} is missing:\n{}", note.body);
    }

    let form = server.get(&format!("/nb/default/n/{id}/tags"));
    assert!(form.says("Add tags"), "{}", form.body);
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

/// Only two pages carry a script, and this is the assertion that keeps the
/// number down.
///
/// It began life as "no page carries a script" and held for six pull requests,
/// which was the point of writing it that early: the scriptless path was
/// finished before anything could quietly start leaning on the other one. What
/// replaces it is the same claim narrowed rather than dropped — a script is
/// allowed exactly where it removes a wait the design named in advance, and
/// nowhere else. A screen that grows one later has to come here and argue for
/// it.
#[test]
fn only_the_screens_that_wait_carry_a_script() {
    let (server, paths) = serving();
    let id = id_of(&paths, "budget-review");
    for path in &[
        "/".to_string(),
        "/nb/default/tags".to_string(),
        "/nb/default/todo".to_string(),
        "/nb/default/files".to_string(),
        format!("/nb/default/n/{id}/backlinks"),
        "/nb/default/f/rack.png/backlinks".to_string(),
    ] {
        let answer = server.get(path);
        assert!(!answer.says("<script"), "{path} carries a script");
    }

    // The listing, which can narrow itself without asking; the network screen,
    // which can ask for news without reloading whole; and a note, whose index
    // pane is the one thing on any of these pages the server does not send —
    // see `script::PANES` for what it costs and why.
    for path in &[
        "/nb/default".to_string(),
        "/nb/default/status".to_string(),
        format!("/nb/default/n/{id}"),
    ] {
        assert!(server.get(path).says("<script>"), "{path} lost its script");
    }

    // Not one handler attribute anywhere, on either kind of page. Every
    // listener the enhancement layer sets is set from inside its own script, so
    // there is no markup on any of these pages whose behaviour depends on
    // JavaScript being there to receive it.
    for path in &[
        "/".to_string(),
        "/nb/default".to_string(),
        "/nb/default/status".to_string(),
        format!("/nb/default/n/{id}"),
    ] {
        assert!(
            !server.get(path).says("onclick"),
            "{path} carries a handler"
        );
        assert!(
            !server.get(path).says("oninput"),
            "{path} carries a handler"
        );
    }
}

/// Every tag, commonest first, each a way into the listing rather than a report.
#[test]
fn the_tags_screen_counts_them_and_leads_into_the_listing() {
    let (server, _paths) = serving();
    let answer = server.get("/nb/default/tags");

    assert_eq!(answer.status, 200);
    // `work` is on two of the fixture's notes and `ops` on one, so `work` comes
    // first — a tag list sorted by name would bury the tags a notebook runs on.
    assert!(answer.says("2 notes"), "{}", answer.body);
    assert!(
        answer.body.find(">work<") < answer.body.find(">ops<"),
        "{}",
        answer.body
    );
    // The row narrows the listing. A tag with a space in it arrives quoted, or
    // the field it lands in would split it into terms and find nothing at all.
    assert!(
        answer.says("q=tag%3Awork"),
        "no plain tag query: {}",
        answer.body
    );
    assert!(
        answer.says("q=tag%3A%2224.04%20Dark%20patterns%22"),
        "no quoted tag query: {}",
        answer.body
    );
}

/// The list `noda todo` prints, in the same order, with the same idea of late.
///
/// The dates are absurd on purpose. `due:2000-01-01` is overdue whenever this
/// test runs and `due:2999-12-31` is not, so what is asserted is the comparison
/// rather than the clock the machine happens to have.
#[test]
fn the_todo_screen_lists_unticked_boxes_soonest_first() {
    let (server, paths) = serving();
    cmd::add(
        &paths,
        Some("Chores"),
        Some("- [ ] much later due:2999-12-31\n- [ ] long overdue due:2000-01-01\n- [x] done\n"),
        &[],
    )
    .expect("add");
    let answer = server.get("/nb/default/todo");

    assert_eq!(answer.status, 200);
    assert!(answer.says("long overdue"), "{}", answer.body);
    // A ticked box is finished, and a list of what is done is not what this is.
    assert!(!answer.says(">done<"), "{}", answer.body);
    assert!(
        answer.body.find("long overdue") < answer.body.find("much later"),
        "{}",
        answer.body
    );
    assert!(
        answer.says("<span class=\"overdue\">2000-01-01</span>"),
        "{}",
        answer.body
    );
    assert!(
        answer.says("<span class=\"when\">2999-12-31</span>"),
        "{}",
        answer.body
    );
    // The `due:` term is lifted out of the words, so the date is not said twice.
    assert!(!answer.says("due:2000-01-01"), "{}", answer.body);
    // Which note it is written in, by title — the row goes there.
    assert!(answer.says("Chores"), "{}", answer.body);
}

/// What points at a note, which is the half nothing else could tell you.
#[test]
fn a_notes_backlinks_are_what_points_at_it() {
    let (server, paths) = serving();
    let id = id_of(&paths, "budget-review");
    cmd::add(
        &paths,
        Some("Pointer"),
        Some(&format!("see [the budget]({id}-budget-review.md)")),
        &[],
    )
    .expect("add");

    let answer = server.get(&format!("/nb/default/n/{id}/backlinks"));
    assert_eq!(answer.status, 200);
    assert!(answer.says("Pointer"), "{}", answer.body);
    assert!(answer.says("What links to"), "{}", answer.body);
    // A note that points at nothing here is not listed.
    assert!(!answer.says("Reading list"), "{}", answer.body);
}

/// **The reason this is worth a screen.** The match is on the id in the
/// destination, so a retitle does not silence it — and just after a retitle is
/// exactly when the links pointing at a note are worth looking at, because every
/// Markdown renderer now shows them broken.
#[test]
fn a_backlink_survives_a_retitle() {
    let (server, paths) = serving();
    let id = id_of(&paths, "budget-review");
    cmd::add(
        &paths,
        Some("Pointer"),
        Some(&format!("see [the budget]({id}-budget-review.md)")),
        &[],
    )
    .expect("add");
    cmd::mv(
        &paths,
        &id,
        "Something else entirely",
        false,
        cmd::Touch::Stamp,
    )
    .expect("mv");

    let answer = server.get(&format!("/nb/default/n/{id}/backlinks"));
    assert_eq!(answer.status, 200);
    assert!(answer.says("Pointer"), "{}", answer.body);
    assert!(answer.says("Something else entirely"), "{}", answer.body);
}

/// A file's backlinks, which is the only way to ask a file that question: an
/// attachment has no page of its own, so the count on the files page is the door.
#[test]
fn a_files_backlinks_are_reached_from_the_count_beside_it() {
    let (server, _paths) = serving();
    let files = server.get("/nb/default/files");

    assert!(
        files.says("href=\"/nb/default/f/rack.png/backlinks\""),
        "{}",
        files.body
    );
    // Nothing points at the svg, so the words stay words. A link to a page that
    // can only say "nothing links here" is a press that tells you nothing.
    assert!(files.says("nothing links to it"), "{}", files.body);
    assert!(!files.says("plan.svg/backlinks"), "{}", files.body);

    let answer = server.get("/nb/default/f/rack.png/backlinks");
    assert_eq!(answer.status, 200);
    assert!(answer.says("The rack"), "{}", answer.body);
}

/// A note is not a file here, exactly as it is not one at `/f/`: it has a page
/// of its own, and its backlinks are on that page's screen.
#[test]
fn a_note_is_not_asked_for_backlinks_as_if_it_were_a_file() {
    let (server, paths) = serving();
    let id = id_of(&paths, "budget-review");
    let answer = server.get(&format!("/nb/default/f/{id}-budget-review.md/backlinks"));
    assert_eq!(answer.status, 404);
    // Nor can the question be used to walk out of the notebook.
    assert_eq!(
        server
            .get("/nb/default/f/..%2F..%2Fetc%2Fpasswd/backlinks")
            .status,
        404
    );
}

/// The bar is the same three places on all four notebook screens, and says which
/// one you are on — with `aria-current`, so the stylesheet and a screen reader
/// are told the same fact once.
#[test]
fn the_notebook_screens_share_one_bar_that_says_where_you_are() {
    let (server, _paths) = serving();
    for (path, here) in [
        ("/nb/default", Some("/nb/default")),
        ("/nb/default/tags", Some("/nb/default/tags")),
        ("/nb/default/todo", Some("/nb/default/todo")),
        ("/nb/default/files", Some("/nb/default/files")),
        // The one notebook screen not on the bar: it is about the notebook
        // rather than about anything inside it, and it is reached from the chip
        // in the corner instead.
        ("/nb/default/status", None),
    ] {
        let answer = server.get(path);
        for place in [
            "/nb/default",
            "/nb/default/tags",
            "/nb/default/todo",
            "/nb/default/files",
        ] {
            assert!(
                answer.says(&format!("href=\"{place}\"")),
                "{path} does not offer {place}: {}",
                answer.body
            );
        }
        match here {
            // The attribute is asked for with its value: the stylesheet inlined
            // into every page names the bare attribute in a selector, so the
            // shorter needle is on all of them.
            None => assert!(
                !answer.says("aria-current=\"page\""),
                "{path}: {}",
                answer.body
            ),
            Some(place) => assert!(
                answer.says(&format!("href=\"{place}\" aria-current=\"page\"")),
                "{path} does not mark itself: {}",
                answer.body
            ),
        }
        // One action, off the row of places.
        assert!(
            answer.says("class=\"fab\" href=\"/nb/default/new\""),
            "{path} has no way to write: {}",
            answer.body
        );
    }
}

/// The wide layout puts one column down the middle by capping `main`, so a page
/// whose body is outside `main` runs the whole width of a monitor. Every form
/// page did until the element was put back.
#[test]
fn every_page_keeps_its_body_inside_the_column() {
    let (server, paths) = serving();
    let id = id_of(&paths, "budget-review");
    for path in &[
        "/nb/default".to_string(),
        "/nb/default/tags".to_string(),
        "/nb/default/todo".to_string(),
        "/nb/default/files".to_string(),
        "/nb/default/new".to_string(),
        format!("/nb/default/n/{id}"),
        format!("/nb/default/n/{id}/edit"),
        format!("/nb/default/n/{id}/tags"),
        format!("/nb/default/n/{id}/rename"),
        format!("/nb/default/n/{id}/delete"),
        format!("/nb/default/n/{id}/backlinks"),
    ] {
        let answer = server.get(path);
        assert_eq!(answer.status, 200, "{path}");
        assert!(answer.says("<main"), "{path} has no main: {}", answer.body);
    }
}

/// The facts `noda status` prints, on a screen instead of a terminal — and none
/// of them fetched. A page that went to the network before drawing itself would
/// hang exactly when the network is why you opened it.
#[test]
fn the_status_screen_says_where_a_notebook_stands() {
    let (server, _paths) = serving();
    let answer = server.get("/nb/default/status");
    assert_eq!(answer.status, 200);
    assert!(answer.says(">Holds</div>"), "{}", answer.body);
    assert!(answer.says("5 notes, 3 files"), "{}", answer.body);
    assert!(answer.says("clean"), "{}", answer.body);
    // The remedy and not only the fact: the command that gives a notebook a
    // remote is on no screen here.
    assert!(answer.says("noda remote set"), "{}", answer.body);
    // Nothing has been asked for, so there is nothing to report and nothing to
    // come back for.
    assert!(!answer.says("said working"), "{}", answer.body);
    assert!(
        !answer.says("<meta http-equiv=\"refresh\""),
        "{}",
        answer.body
    );

    // And asking again does not start anything: only a POST does, which is what
    // makes the reload a slow network invites harmless.
    for _ in 0..3 {
        assert!(
            !server.get("/nb/default/status").says("class=\"said"),
            "a GET started an errand"
        );
    }
}

/// The way in, and the only thing on the listing that says anything about the
/// remote. It is a link because it is a way somewhere, and it carries the answer
/// because that saves going there at all.
#[test]
fn the_listing_says_where_the_notebook_stands_and_leads_to_the_rest() {
    let (server, _paths) = serving();
    let answer = server.get("/nb/default");
    assert!(
        answer.says("class=\"drift\" href=\"/nb/default/status\""),
        "{}",
        answer.body
    );
    assert!(answer.says(">no remote</span>"), "{}", answer.body);

    // And with a remote it says the same thing `noda status` would.
    let (server, _paths, _remote) = serving_with_a_remote();
    assert!(
        server.get("/nb/default").says(">never fetched</span>"),
        "the chip did not follow the notebook"
    );
    server.post("/nb/default/status/sync", &[]);
    server.settled("/nb/default/status");
    assert!(
        server.get("/nb/default").says(">in sync</span>"),
        "the chip is stale after a sync"
    );
}

/// The network screen is not on the bar — the bar holds places inside the
/// notebook — but it carries the bar, so it is not a dead end.
#[test]
fn the_status_screen_is_not_a_dead_end() {
    let (server, _paths) = serving();
    let answer = server.get("/nb/default/status");
    assert!(answer.says("class=\"actionbar\""), "{}", answer.body);
    // `aria-current="page"` and not `aria-current`: the stylesheet embedded in
    // every page names the attribute in a selector, so the shorter needle is
    // always found and the assertion would never fail.
    assert!(!answer.says("aria-current=\"page\""), "{}", answer.body);
    assert!(answer.says("<main"), "{}", answer.body);
}

/// The whole shape of it: the press answers at once, the errand runs behind it,
/// and what it printed is on the screen the reader was sent to.
#[test]
fn a_sync_answers_before_it_finishes_and_says_how_it_went() {
    let (server, _paths, remote) = serving_with_a_remote();
    let answer = server.get("/nb/default/status");
    assert!(answer.says("never fetched"), "{}", answer.body);

    let started = server.post("/nb/default/status/sync", &[]);
    assert_eq!(started.status, 303);
    assert_eq!(started.location.as_deref(), Some("/nb/default/status"));

    let done = server.settled("/nb/default/status");
    assert_eq!(done.status, 200);
    assert!(done.says("push:"), "{}", done.body);
    assert!(!done.says("said bad"), "{}", done.body);
    // Finished means finished: nothing left asking the browser to come back.
    assert!(!done.says("<meta http-equiv=\"refresh\""), "{}", done.body);
    // The drift is re-read rather than remembered, so the screen agrees with
    // the repository it just changed.
    assert!(done.says("in sync"), "{}", done.body);

    // And the notes are actually there, which is the only claim worth making
    // about a push.
    let there = git2::Repository::open_bare(&remote).expect("open the remote");
    assert!(
        there.head().expect("a branch").peel_to_commit().is_ok(),
        "the remote has no commit on it"
    );
}

/// A failure is reported in the words the command used, in the place the button
/// was pressed. The notebook here has no remote at all, which is the commonest
/// way a push cannot happen.
#[test]
fn a_push_with_nowhere_to_send_it_says_so() {
    let (server, _paths) = serving();
    assert_eq!(server.post("/nb/default/status/push", &[]).status, 303);

    let done = server.settled("/nb/default/status");
    assert!(done.says("said bad"), "{}", done.body);
    assert!(done.says("remote"), "{}", done.body);
    assert!(!done.says("<meta http-equiv=\"refresh\""), "{}", done.body);
}

/// Three errands, and the route does not invent a fourth.
#[test]
fn there_is_nothing_called_fetch_to_do_to_a_notebook() {
    let (server, _paths) = serving();
    assert_eq!(server.post("/nb/default/status/fetch", &[]).status, 404);
    assert_eq!(server.post("/nb/nowhere/status/sync", &[]).status, 404);
    assert_eq!(server.get("/nb/nowhere/status").status, 404);
}
