//! The web server as a browser drives it: the real binary, a real socket, and
//! requests written by hand — because the guard reads `Host` and `Origin`, which
//! an HTTP client will not let a caller lie about, and a rebinding attack is a
//! request whose `Host` is a lie.
//!
//! The port is `0`; the server prints the one it got.
//!
//! `sign = false` is required: libgit2 reads the developer's real git config.

use std::fmt::Write as _;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, ChildStdout, Command, ExitStatus, Stdio};
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

/// Five notes, three files, and one note embedding a file. One note is raw HTML,
/// as `noda import tiddlywiki` leaves such bodies alone.
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

    // A `.png` is shown inline and a `.svg` is not.
    let source = root.0.join("rack.png");
    std::fs::write(&source, b"\x89PNG\r\n\x1a\nnot really").expect("write a png");
    cmd::file_add(&paths, &[source], None).expect("file add");
    let vector = root.0.join("plan.svg");
    std::fs::write(&vector, "<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>").expect("write svg");
    cmd::file_add(&paths, &[vector], None).expect("file add");

    // Markdown but not a note (no id in its stem); the suffix must not decide.
    cmd::readme(&paths, false).expect("readme");

    // So the files page has a non-zero reference count.
    cmd::add(
        &paths,
        Some("The rack"),
        Some("it looks like ![the rack](rack.png)"),
        &[],
    )
    .expect("add");
    (root, paths)
}

/// Follows the page's link, which also checks this build answers that address.
fn linked_stylesheet(server: &Serving, from: &str) -> Answer {
    let page = server.get(from);
    let opening = "<link rel=\"stylesheet\" href=\"";
    let at = page
        .body
        .find(opening)
        .unwrap_or_else(|| panic!("{from} links no stylesheet:\n{}", page.body));
    let href = page.body[at + opening.len()..]
        .split_once('"')
        .map(|(href, _)| href.to_string())
        .expect("an unterminated attribute");
    server.get(&href)
}

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

fn note_file(paths: &Paths, slug: &str) -> PathBuf {
    paths
        .notebooks_dir()
        .join("default")
        .join(format!("{}-{slug}.md", id_of(paths, slug)))
}

/// An open event stream, asked with `contains` against the raw wire, chunked
/// framing and all, so no parser is needed for a body with no length.
struct Watch {
    socket: TcpStream,
    heard: String,
}

impl Watch {
    fn hears(&mut self, what: &str) -> &str {
        let mut buffer = [0u8; 4096];
        while !self.heard.contains(what) {
            match self.socket.read(&mut buffer) {
                Ok(0) => panic!("the stream ended before {what:?}:\n{}", self.heard),
                Ok(read) => self
                    .heard
                    .push_str(&String::from_utf8_lossy(&buffer[..read])),
                Err(e) => panic!("waiting for {what:?}: {e}\nheard so far:\n{}", self.heard),
            }
        }
        &self.heard
    }

    fn ends(&mut self) {
        let mut buffer = [0u8; 4096];
        loop {
            match self.socket.read(&mut buffer) {
                Ok(0) => return,
                Ok(read) => self
                    .heard
                    .push_str(&String::from_utf8_lossy(&buffer[..read])),
                Err(e) => panic!("the stream never ended: {e}\nheard:\n{}", self.heard),
            }
        }
    }
}

struct Answer {
    status: u16,
    location: Option<String>,
    body: String,
    head: String,
}

impl Answer {
    fn says(&self, needle: &str) -> bool {
        self.body.contains(needle)
    }

    /// Whether the row is shown: every listing carries every row, excluded ones
    /// `hidden`, so the script can widen a query. `None` when no row names it.
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

    /// From `main.rows` only: the pane beside renders the README.
    fn titles(&self) -> Vec<String> {
        let rows = self
            .body
            .split_once("<main class=\"rows\">")
            .map_or("", |(_, rest)| rest);
        rows.split("<div class=\"title\">")
            .skip(1)
            .filter_map(|rest| {
                rest.split_once("</div>")
                    .map(|(title, _)| title.to_string())
            })
            .collect()
    }

    fn header(&self, name: &str) -> Option<String> {
        self.head.lines().find_map(|line| {
            let (found, value) = line.split_once(':')?;
            found
                .eq_ignore_ascii_case(name)
                .then(|| value.trim().to_string())
        })
    }

    /// Extracted, because "no clock here" cannot be asked of a page full of colons.
    fn stamps(&self) -> Vec<String> {
        self.body
            .match_indices("class=\"when\"")
            .filter_map(|(at, _)| {
                // A `<time>` on a listing, a `<span>` on a note.
                let (_, inner) = self.body[at..].split_once('>')?;
                let (text, _) = inner.split_once('<')?;
                Some(text.to_string())
            })
            .collect()
    }
}

struct Serving {
    child: Child,
    /// Held, because a closed pipe makes the server's `println!` on the way out
    /// panic, turning a clean shutdown into a crash.
    said: BufReader<ChildStdout>,
    port: u16,
    _root: TempRoot,
}

struct Stopped {
    status: ExitStatus,
    /// Everything it wrote to stdout after the address.
    said: String,
}

impl Serving {
    fn start(root: TempRoot, allow: &[&str]) -> Serving {
        Serving::start_with(root, allow, None)
    }

    fn start_with(root: TempRoot, allow: &[&str], rust_log: Option<&str>) -> Serving {
        let mut command = Command::new(env!("CARGO_BIN_EXE_noda"));
        command.args(["web", "--listen", "127.0.0.1:0"]);
        for name in allow {
            command.args(["--allow-host", name]);
        }
        // Removed, so the developer's shell does not decide what is logged.
        command.env_remove("RUST_LOG");
        if let Some(filter) = rust_log {
            command.env("RUST_LOG", filter);
        }
        // All four: the active-notebook pointer lives in state.
        command
            .env("XDG_CONFIG_HOME", root.0.join("config"))
            .env("XDG_DATA_HOME", root.0.join("data"))
            .env("XDG_STATE_HOME", root.0.join("state"))
            .env("XDG_CACHE_HOME", root.0.join("cache"))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = command.spawn().expect("spawn noda web");
        // The port, from the address line; `println!` is line buffered, so it
        // arrives before the process ends.
        let stdout = child.stdout.take().expect("stdout");
        let mut said = BufReader::new(stdout);
        let mut first = String::new();
        said.read_line(&mut first)
            .expect("the server should say where it is");
        let port = first
            .trim()
            .rsplit_once(':')
            .and_then(|(_, port)| port.parse().ok())
            .unwrap_or_else(|| panic!("could not read a port out of {first:?}"));

        Serving {
            child,
            said,
            port,
            _root: root,
        }
    }

    /// `kill(1)` rather than a crate, to keep the lockfile small.
    #[cfg(unix)]
    fn signalled(&mut self, signal: &str) -> Stopped {
        let pid = self.child.id().to_string();
        let sent = Command::new("kill")
            .arg(format!("-{signal}"))
            .arg(&pid)
            .status()
            .expect("send a signal");
        assert!(sent.success(), "could not send {signal} to {pid}");

        let status = self.waited();
        // After exit, so no read can miss the last line.
        let mut said = String::new();
        self.said
            .read_to_string(&mut said)
            .expect("the rest of stdout");
        Stopped { status, said }
    }

    /// Five seconds, then a failure rather than a hung test.
    #[cfg(unix)]
    fn waited(&mut self) -> ExitStatus {
        for _ in 0..200 {
            if let Some(status) = self.child.try_wait().expect("wait on the server") {
                return status;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        let _ = self.child.kill();
        panic!("the server was asked to stop and did not");
    }

    /// The default filter logs nothing per request.
    fn start_logging(root: TempRoot, allow: &[&str]) -> Serving {
        Serving::start_with(root, allow, Some("noda=debug"))
    }

    /// Consumes the server: stderr reaches EOF only once the process is gone.
    fn logged(mut self) -> String {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let mut stderr = self.child.stderr.take().expect("stderr");
        let mut said = String::new();
        stderr.read_to_string(&mut said).expect("read the log");
        said
    }

    fn get(&self, path: &str) -> Answer {
        self.request(path, &[])
    }

    fn post(&self, path: &str, fields: &[(&str, &str)]) -> Answer {
        let body = fields
            .iter()
            .map(|(name, value)| format!("{}={}", urlencode(name), urlencode(value)))
            .collect::<Vec<_>>()
            .join("&");
        self.send("POST", path, &[], Some(&body))
    }

    /// Polls until a network errand, which outlives its request, has finished.
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

    /// Not `send`, which reads to the end: an event stream has none.
    fn watch(&self, path: &str, encoding: Option<&str>) -> Watch {
        let mut socket =
            TcpStream::connect(("127.0.0.1", self.port)).expect("connect to the server");
        let mut wire = format!(
            "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nAccept: text/event-stream\r\n",
            self.port
        );
        if let Some(encoding) = encoding {
            let _ = write!(wire, "Accept-Encoding: {encoding}\r\n");
        }
        // No `Connection: close`: this one stays.
        wire.push_str("\r\n");
        socket.write_all(wire.as_bytes()).expect("write a request");
        socket
            .set_read_timeout(Some(std::time::Duration::from_secs(15)))
            .expect("a read timeout");
        Watch {
            socket,
            heard: String::new(),
        }
    }

    /// The fingerprint a form carries, to send back (or send back stale).
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

    /// A request with the caller's headers, `Host` included.
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
        // So reading to the end is the whole answer, with no length to parse.
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

/// A bare repository on disk: libgit2's local transport runs the same push and
/// fetch code as HTTPS, without a network.
fn serving_with_a_remote() -> (Serving, Paths, PathBuf) {
    let (root, paths) = a_notebook();
    let branch = noda::notebook::Notebook::open(&paths, "default")
        .expect("open the notebook")
        .branch()
        .expect("its branch");

    let remote = root.0.join("origin.git");
    git2::Repository::init_bare(&remote)
        .expect("init a bare remote")
        // Read off the notebook: `init.defaultBranch` varies by machine.
        .set_head(&format!("refs/heads/{branch}"))
        .expect("point the remote at that branch");
    let url = remote.to_str().expect("utf-8 path").to_string();
    cmd::remote_set(&paths, &url).expect("set the remote");

    (Serving::start(root, &[]), paths, remote)
}

/// The wiring no unit test sees: the layer is on the router, the guard's refusal
/// is logged, and a note is logged by route template, not by address.
#[test]
fn the_server_logs_what_it_did_and_never_which_note_it_was() {
    let (root, paths) = a_notebook();
    let id = noda::notebook::Notebook::open(&paths, "default")
        .expect("open the notebook")
        .notes()
        .expect("its notes")
        .first()
        .expect("the fixture has notes")
        .id
        .clone();

    let server = Serving::start_logging(root, &[]);
    assert_eq!(server.get(&format!("/nb/default/n/{id}")).status, 200);
    // A rebinding attempt.
    assert_eq!(server.request("/", &[("Host", "evil.example")]).status, 403);
    let log = server.logged();

    assert!(log.contains("event=\"http.request\""), "{log}");
    assert!(log.contains("route=\"/nb/{book}/n/{key}\""), "{log}");
    assert!(log.contains("event=\"http.refused\""), "{log}");
    assert!(log.contains("host=\"evil.example\""), "{log}");
    // A log outlives the request and gets shipped elsewhere.
    assert!(!log.contains(&id), "the note's id reached the log:\n{log}");
}

/// Quiet by default, which also keeps the harness's stderr pipe from filling up.
#[test]
fn nothing_is_logged_per_request_until_it_is_asked_for() {
    let (server, _paths) = serving();
    assert_eq!(server.get("/").status, 200);
    assert_eq!(server.get("/nb/default").status, 200);
    let log = server.logged();
    assert!(!log.contains("http.request"), "{log}");
}

#[test]
fn the_front_page_lists_the_notebooks() {
    let (server, _paths) = serving();
    let answer = server.get("/");
    assert_eq!(answer.status, 200);
    assert!(answer.says("href=\"/nb/default\""), "{}", answer.body);
    // The remote's standing, not a sync button.
    assert!(answer.says("no remote"), "{}", answer.body);
    assert!(answer.says("5 notes"), "{}", answer.body);
}

/// A row is `noda status` in a line; the page and the command must agree.
#[test]
fn a_front_page_row_says_what_status_says() {
    let (server, paths) = serving();
    let status = noda::notebook::Notebook::open(&paths, "default")
        .expect("open the notebook")
        .status()
        .expect("its status");
    let answer = server.get("/");

    assert!(
        answer.says(&format!("{} notes", status.notes)),
        "{}",
        answer.body
    );
    assert!(
        answer.says(&format!("{} files", status.files)),
        "{}",
        answer.body
    );
    // Only the day: stamps are UTC, and a bare clock reads as local time.
    let stamp = answer
        .body
        .split_once("<span class=\"stamp\">")
        .and_then(|(_, rest)| rest.split_once("</span>"))
        .map(|(day, _)| day.to_string())
        .expect("a row carries the day it was last written to");
    assert_eq!(stamp.len(), 10, "{stamp} is not just a day");
    assert!(!stamp.contains(':'), "{stamp} has a clock in it");
}

/// A notebook with no remote gets no link to the status screen, only the words.
#[test]
fn the_front_page_leads_to_a_notebook_and_to_where_it_stands() {
    let (server, _paths, _remote) = serving_with_a_remote();
    let answer = server.get("/");
    assert!(answer.says("href=\"/nb/default\""), "{}", answer.body);
    assert!(
        answer.says("href=\"/nb/default/status\""),
        "{}",
        answer.body
    );

    let (plain, _paths) = serving();
    let answer = plain.get("/");
    assert!(answer.says("no remote"), "{}", answer.body);
    assert!(!answer.says("/status"), "{}", answer.body);
}

/// Like `noda notebook ls`'s `*`. Only the front page consults the pointer;
/// every other address names its notebook.
#[test]
fn the_front_page_marks_the_notebook_the_terminal_is_pointed_at() {
    let (server, paths) = serving();
    assert_eq!(
        paths.active_notebook().expect("an active notebook"),
        "default"
    );
    let answer = server.get("/");
    assert_eq!(
        answer.body.matches("class=\"mark\"").count(),
        1,
        "{}",
        answer.body
    );
    assert!(answer.says("<span class=\"sr\">Active"), "{}", answer.body);
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
    // A day, never a clock: a UTC clock without its `Z` reads as local time.
    let stamps = answer.stamps();
    assert_eq!(stamps.len(), 5, "{stamps:?}");
    for stamp in &stamps {
        assert_eq!(stamp.len(), 10, "{stamp} is not just a day");
        assert!(!stamp.contains(':'), "{stamp} has a clock in it");
    }

    // `Query::parse` refuses an empty token list, which once put a complaint on
    // every unfiltered page.
    assert!(
        !answer.says("class=\"problem\""),
        "an unfiltered listing complained:\n{}",
        answer.body
    );
}

/// `?sort=` is `--sort`. One note's stamps are pinned to both ends of the
/// calendar, since the rest share a second and tie-break on a minted id.
#[test]
fn a_listing_comes_back_in_the_order_the_address_asks_for() {
    let (root, paths) = a_notebook();
    let path = note_file(&paths, "reading-list");
    let text = std::fs::read_to_string(&path).expect("the note");
    let pinned = text
        .lines()
        .map(|line| {
            if line.starts_with("created: ") {
                "created: 2019-01-02T00:00:00Z".to_string()
            } else if line.starts_with("updated: ") {
                "updated: 2099-01-02T00:00:00Z".to_string()
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&path, format!("{pinned}\n")).expect("pin the stamps");
    let server = Serving::start(root, &[]);

    let plain = server.get("/nb/default");
    assert_eq!(plain.status, 200);

    assert_eq!(
        server.get("/nb/default?sort=title").titles(),
        [
            "Budget review",
            "Meeting notes",
            "Raw html import",
            "Reading list",
            "The rack"
        ]
    );

    // Newest first, and the row prints the stamp it was ordered by.
    let newest = server.get("/nb/default?sort=updated");
    assert_eq!(
        newest.titles().first().map(String::as_str),
        Some("Reading list"),
        "{}",
        newest.body
    );
    assert!(newest.says("2099-01-02"), "{}", newest.body);

    // The same note is oldest by `created`, so that is its own order.
    let oldest = server.get("/nb/default?sort=created");
    assert_eq!(
        oldest.titles().last().map(String::as_str),
        Some("Reading list"),
        "{}",
        oldest.body
    );
    assert!(oldest.says("2019-01-02"), "{}", oldest.body);

    // `r` reverses whichever order was asked for, the default included.
    let mut backwards = plain.titles();
    backwards.reverse();
    assert_eq!(server.get("/nb/default?r=1").titles(), backwards);
    let mut down_the_alphabet = server.get("/nb/default?sort=title").titles();
    down_the_alphabet.reverse();
    assert_eq!(
        server.get("/nb/default?sort=title&r=1").titles(),
        down_the_alphabet
    );

    // An unknown order is the default, silently: `?sort=` is written by a link,
    // not typed.
    let odd = server.get("/nb/default?sort=newest");
    assert_eq!(odd.titles(), plain.titles());
    assert!(!odd.says("class=\"problem\""), "{}", odd.body);
}

/// The chips carry the query and the form carries the order — a `GET` form
/// sends only its own fields, so the order needs a hidden one.
#[test]
fn an_order_survives_a_search_and_a_search_survives_an_order() {
    let (server, _paths) = serving();
    // `tag:`, so no title is `<mark>`ed and `row` can find it.
    let answer = server.get("/nb/default?q=tag:work&sort=title");
    assert_eq!(answer.status, 200);

    assert_eq!(answer.row("Budget review"), Some(true));
    assert_eq!(answer.row("Meeting notes"), Some(true));
    assert_eq!(answer.row("Reading list"), Some(false));
    assert_eq!(
        answer.titles(),
        [
            "Budget review",
            "Meeting notes",
            "Raw html import",
            "Reading list",
            "The rack"
        ]
    );
    assert!(
        answer.says("<input type=\"hidden\" name=\"sort\" value=\"title\">"),
        "{}",
        answer.body
    );
    // URL-encoded, as a query may hold spaces and `&`.
    assert!(
        answer.says("href=\"/nb/default?q=tag%3Awork&amp;sort=created\""),
        "{}",
        answer.body
    );

    // The default order writes nothing.
    let plain = server.get("/nb/default");
    assert!(!plain.says("name=\"sort\""), "{}", plain.body);
    assert!(!plain.says("name=\"r\""), "{}", plain.body);
}

/// Whether the bytes gunzip is `e2e/`'s question; here it is *which* answers are
/// compressed. No other test sends `Accept-Encoding`, so the rest read plain bodies.
#[test]
fn an_answer_is_compressed_only_when_the_reader_asks_for_it() {
    let (server, _paths) = serving();

    let asked = server.request("/nb/default", &[("Accept-Encoding", "gzip")]);
    assert_eq!(asked.status, 200);
    assert_eq!(asked.header("content-encoding").as_deref(), Some("gzip"));
    // Set by `tower-http`, asserted because it is what makes an asset's
    // year-long `cache-control` safe.
    assert!(
        asked.head.to_lowercase().contains("vary: accept-encoding"),
        "{}",
        asked.head
    );

    let plain = server.get("/nb/default");
    assert_eq!(plain.header("content-encoding"), None, "{}", plain.head);
    assert!(plain.says("Budget review"), "{}", plain.body);
}

/// Both files hold the same highly compressible bytes, so only what `holding`
/// calls them tells them apart.
#[test]
fn what_is_already_compressed_is_not_compressed_again() {
    let (root, paths) = a_notebook();
    let filler = "the quarterly budget is late\n".repeat(40);
    for name in ["notes.txt", "archive.bin"] {
        let path = root.0.join(name);
        std::fs::write(&path, &filler).expect("write the attachment");
        cmd::file_add(&paths, &[path], None).expect("file add");
    }
    let server = Serving::start(root, &[]);

    let text = server.request("/nb/default/f/notes.txt", &[("Accept-Encoding", "gzip")]);
    assert_eq!(text.status, 200);
    assert_eq!(text.header("content-encoding").as_deref(), Some("gzip"));

    // An unknown extension is `application/octet-stream`: possibly compressed.
    let blob = server.request("/nb/default/f/archive.bin", &[("Accept-Encoding", "gzip")]);
    assert_eq!(blob.status, 200);
    assert_eq!(blob.header("content-encoding"), None, "{}", blob.head);

    // `DefaultPredicate` declines images.
    let png = server.request("/nb/default/f/rack.png", &[("Accept-Encoding", "gzip")]);
    assert_eq!(png.status, 200);
    assert_eq!(png.header("content-encoding"), None, "{}", png.head);

    // `/health` is `ok`, shorter than a gzip header.
    let health = server.request("/health", &[("Accept-Encoding", "gzip")]);
    assert_eq!(health.status, 200);
    assert_eq!(health.header("content-encoding"), None, "{}", health.head);
}

#[test]
fn a_query_narrows_the_listing_and_marks_what_matched() {
    let (server, _paths) = serving();
    let answer = server.get("/nb/default?q=budget");
    assert_eq!(answer.status, 200);
    assert!(answer.says("<mark>Budget</mark> review"), "{}", answer.body);
    assert_eq!(answer.row("Reading list"), Some(false), "{}", answer.body);
    // The total, so an empty-looking result is not a mystery.
    assert!(answer.says("of 5"), "{}", answer.body);
}

/// A tag may hold a space; `query::split` handles quoting for the web too.
#[test]
fn a_quoted_tag_survives_a_real_query_string() {
    let (server, _paths) = serving();
    let answer = server.get("/nb/default?q=tag%3A%2224.04+Dark+patterns%22");
    assert_eq!(answer.status, 200);
    assert_eq!(answer.row("Meeting notes"), Some(true), "{}", answer.body);
    assert_eq!(answer.row("Budget review"), Some(false), "{}", answer.body);
}

/// A half-typed query says why and keeps the notes rather than emptying the
/// screen, as the TUI's `/` does.
#[test]
fn an_unfinished_query_says_why_and_keeps_the_notes() {
    let (server, _paths) = serving();
    let answer = server.get("/nb/default?q=OR");
    assert_eq!(answer.status, 200);
    assert!(answer.says("class=\"problem\""), "{}", answer.body);
    assert!(answer.says("Budget review"), "{}", answer.body);
    assert!(answer.says("Reading list"), "{}", answer.body);
    // No grouping is drawn for a query that does not parse.
    assert!(
        answer.says("<div class=\"parse\" hidden></div>"),
        "{}",
        answer.body
    );
}

/// A row is `noda ls -l`'s row, id included; the stylesheet decides whether the
/// id column is shown.
#[test]
fn a_listing_row_carries_the_id_the_notebook_knows_the_note_by() {
    let (server, paths) = serving();
    let id = id_of(&paths, "budget-review");
    let answer = server.get("/nb/default");
    assert_eq!(answer.status, 200);
    assert!(
        answer.says(&format!(
            "<div class=\"ident\"><span class=\"id\">{id}</span>"
        )),
        "{}",
        answer.body
    );
    assert!(
        answer.says(&format!("href=\"/nb/default/n/{id}\"")),
        "{}",
        answer.body
    );
}

/// `a OR b c` is `(a OR b) AND c`, the easily misread rule, so the field draws
/// the grouping — taken from `Query`, so it matches what narrowed the notes.
#[test]
fn the_field_says_how_it_grouped_what_was_typed() {
    let (server, _paths) = serving();
    let answer = server.get("/nb/default?q=tag%3Awork+OR+tag%3Aops+budget");
    assert_eq!(answer.status, 200);
    assert!(
        answer.says(
            "<div class=\"parse\"><span class=\"g\">\
             <b class=\"t\">tag:work</b><i>or</i><b class=\"t\">tag:ops</b></span>\
             <span class=\"and\">and</span><span class=\"g\"><b>budget</b></span></div>"
        ),
        "{}",
        answer.body
    );
    // `budget` is ANDed, not swallowed by the OR.
    assert!(answer.says("<mark>Budget</mark> review"), "{}", answer.body);
    assert!(answer.says("<a class=\"row\" hidden"), "{}", answer.body);
}

/// A bookmark must survive a retitle, so a slug or prefix redirects to the id.
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

#[test]
fn the_note_page_names_the_file_and_stamps_it_whole() {
    let (server, paths) = serving();
    let id = id_of(&paths, "budget-review");
    let answer = server.get(&format!("/nb/default/n/{id}"));

    assert!(answer.says(&format!(">{id}</span>")), "{}", answer.body);
    assert!(answer.says(">-budget-review</span>"), "{}", answer.body);
    assert!(answer.says(">.md</span>"), "{}", answer.body);
    // Both stamps whole, `Z` included: unambiguous without script, and what the
    // script converts to local time.
    assert!(
        answer.says("created <time datetime=\"20"),
        "{}",
        answer.body
    );
    assert!(
        answer.says("updated <time datetime=\"20"),
        "{}",
        answer.body
    );
    assert!(answer.says("Z</time>"), "{}", answer.body);
}

/// A note page carries the index pane's frame and none of its rows (about 290
/// bytes a note, never drawn below 1024px) — a regression invisible on a desktop.
#[test]
fn a_note_page_is_sent_without_the_listing_beside_it() {
    let (server, paths) = serving();
    let id = id_of(&paths, "budget-review");
    let answer = server.get(&format!("/nb/default/n/{id}"));

    assert!(answer.says("class=\"pane index\""), "{}", answer.body);
    assert!(
        answer.says("<form class=\"searchbar\" method=\"get\" action=\"/nb/default\""),
        "{}",
        answer.body
    );
    assert!(
        answer.says("<main class=\"rows\"></main>"),
        "the listing was sent with the note: {}",
        answer.body
    );
    assert!(
        !answer.says("Reading list"),
        "the listing was sent with the note: {}",
        answer.body
    );
    // No `indexed`. The whole attribute, because the inlined stylesheet names the
    // class on every page.
    assert!(
        answer.says("class=\"app split at-note\""),
        "{}",
        answer.body
    );
}

/// Backlinks mean reading every note, so the box goes out empty and
/// `script::BESIDE` fills it where the column is drawn.
#[test]
fn a_note_page_is_sent_without_what_points_at_it() {
    let (server, paths) = serving();
    let id = id_of(&paths, "budget-review");
    cmd::add(
        &paths,
        Some("Pointer"),
        Some(&format!("see [the budget]({id}-budget-review.md)")),
        &[],
    )
    .expect("add");

    let answer = server.get(&format!("/nb/default/n/{id}"));
    assert!(
        answer.says("<aside class=\"beside\" hidden>"),
        "{}",
        answer.body
    );
    assert!(
        answer.says("<div class=\"answer\"></div>"),
        "the margin note arrived with an answer in it: {}",
        answer.body
    );
    // If this fails, the note route started walking the notebook.
    assert!(
        !answer.says("Pointer"),
        "the note page answered a question nobody on it asked: {}",
        answer.body
    );
}

/// The margin note is built at runtime from the backlinks page, so that page's
/// markup is a contract: a renamed class would break it with every other test green.
#[test]
fn the_backlinks_page_writes_the_shape_the_margin_note_reads() {
    let (server, paths) = serving();
    let id = id_of(&paths, "budget-review");
    cmd::add(
        &paths,
        Some("Pointer"),
        Some(&format!("see [the budget]({id}-budget-review.md)")),
        &[],
    )
    .expect("add");
    let pointer = id_of(&paths, "pointer");

    let answer = server.get(&format!("/nb/default/n/{id}/backlinks"));
    assert!(answer.says("<main class=\"rows\">"), "{}", answer.body);
    // The margin note prints the id from the end of the address.
    assert!(
        answer.says(&format!(
            "<a class=\"row\" href=\"/nb/default/n/{pointer}\"><div class=\"title\">Pointer</div>"
        )),
        "{}",
        answer.body
    );
}

/// A request for one pane gets that pane, byte for byte as in the whole page.
#[test]
fn a_note_can_be_asked_for_without_the_page_around_it() {
    let (server, paths) = serving();
    let id = id_of(&paths, "budget-review");
    let at = format!("/nb/default/n/{id}");
    let whole = server.get(&at);
    let part = server.request(&at, &[("X-Noda-Fragment", "read")]);

    assert_eq!(part.status, 200);
    // The `<title>` first: a parser puts it in the head, where the script looks.
    assert!(part.body.starts_with("<title>Budget review — noda</title>"));
    let (title, pane) = part.body.split_once("</title>").expect("a named tab");
    assert!(whole.says(&format!("{title}</title>")), "{}", part.body);
    assert!(
        whole.says(pane),
        "the pane is not the page's own: {}",
        part.body
    );

    // Only the note; the rest is already on screen.
    assert!(part.says("class=\"pane read\""), "{}", part.body);
    assert!(part.says("Budget review"), "{}", part.body);
    for absent in [
        "<!doctype",
        "<link rel=\"stylesheet\"",
        "<script",
        "class=\"pane index\"",
    ] {
        assert!(!part.says(absent), "the fragment carried {absent}");
    }
    // Smaller, though no longer by much: the head, the rail and the index frame.
    assert!(
        part.body.len() < whole.body.len(),
        "{} of {} bytes",
        part.body.len(),
        whole.body.len()
    );
}

/// Only the script sends the header; everything else gets the whole page.
#[test]
fn an_address_asked_for_plainly_is_still_the_whole_page() {
    let (server, paths) = serving();
    let id = id_of(&paths, "budget-review");
    let at = format!("/nb/default/n/{id}");
    assert!(server.get(&at).says("<!doctype html>"), "a page went short");
    // An unknown fragment name gets the whole page.
    let odd = server.request(&at, &[("X-Noda-Fragment", "everything")]);
    assert_eq!(odd.status, 200);
    assert!(odd.says("<!doctype html>"), "{}", odd.body);

    // Two answers at one address told apart by a header: a cache must know.
    assert_eq!(
        server.get(&at).header("vary").as_deref(),
        Some("x-noda-fragment")
    );
}

/// The other three fragments; `news` once re-sent the whole stylesheet every two
/// seconds.
#[test]
fn the_other_three_parts_arrive_without_their_pages() {
    let (server, paths) = serving();
    let id = id_of(&paths, "budget-review");

    let column = server.request("/nb/default", &[("X-Noda-Fragment", "index")]);
    assert!(column.body.starts_with("<section class=\"pane index\">"));
    assert!(column.says("Budget review"), "{}", column.body);
    assert!(
        server.get("/nb/default").says(&column.body),
        "{}",
        column.body
    );

    let links = server.request(
        &format!("/nb/default/n/{id}/backlinks"),
        &[("X-Noda-Fragment", "rows")],
    );
    assert!(links.body.starts_with("<main class=\"rows\">"));
    assert!(
        server
            .get(&format!("/nb/default/n/{id}/backlinks"))
            .says(&links.body),
        "{}",
        links.body
    );

    let news = server.request("/nb/default/status", &[("X-Noda-Fragment", "news")]);
    assert!(news.body.starts_with("<main>"), "{}", news.body);
    assert!(news.says("5 notes, 3 files"), "{}", news.body);
    assert!(
        server.get("/nb/default/status").says(&news.body),
        "{}",
        news.body
    );

    for part in [&column, &links, &news] {
        assert_eq!(part.status, 200);
        assert!(!part.says("<!doctype"), "{}", part.body);
        assert!(!part.says("<style>"), "{}", part.body);
    }
}

/// Going back from a note replaces both panes and the tab's name.
#[test]
fn the_listing_screen_arrives_as_both_of_its_panes() {
    let (server, _paths) = serving();
    let whole = server.get("/nb/default");
    let part = server.request("/nb/default", &[("X-Noda-Fragment", "screen")]);

    assert_eq!(part.status, 200);
    assert!(part.body.starts_with("<title>default — noda</title>"));
    let (title, panes) = part.body.split_once("</title>").expect("a named tab");
    assert!(whole.says(&format!("{title}</title>")), "{}", part.body);
    assert!(
        whole.says(panes),
        "the panes are not the page's own: {}",
        part.body
    );

    assert!(part.says("class=\"pane index\""), "{}", part.body);
    assert!(part.says("class=\"pane read\""), "{}", part.body);
    assert!(part.says("Budget review"), "{}", part.body);
    for absent in ["<!doctype", "<style>", "<script>", "class=\"notebooks\""] {
        assert!(!part.says(absent), "the fragment carried {absent}");
    }

    // `index` off the same route is still the column alone.
    let column = server.request("/nb/default", &[("X-Noda-Fragment", "index")]);
    assert!(column.body.starts_with("<section class=\"pane index\">"));
    assert!(!column.says("class=\"pane read\""), "{}", column.body);
}

/// A searched fragment has the same rows as the scriptless page.
#[test]
fn a_searched_listing_answers_the_same_rows_either_way() {
    let (server, _paths) = serving();
    let part = server.request("/nb/default?q=q3", &[("X-Noda-Fragment", "index")]);
    let whole = server.get("/nb/default?q=q3");

    assert_eq!(part.status, 200);
    assert!(whole.says(&part.body), "the column is not the page's own");
    // `q3` is only in a body, which the script cannot search.
    assert_eq!(part.row("Budget review"), Some(true), "{}", part.body);
    assert_eq!(part.row("Reading list"), Some(false), "{}", part.body);
}

/// The listing's rows are in the markup, so it says `indexed`.
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

/// A notebook's `README.md` fills the reading pane when no note is picked; it is
/// drawn only above 1024px.
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

/// Without a README, an invitation rather than an empty pane.
#[test]
fn a_notebook_with_no_front_page_invites_a_note_instead() {
    let (server, paths) = serving();
    std::fs::remove_file(paths.notebooks_dir().join("default").join("README.md"))
        .expect("could not take the README away");
    let answer = server.get("/nb/default");
    assert!(answer.says("Pick a note"), "{}", answer.body);
    assert!(!answer.says("README.md"), "{}", answer.body);
}

/// `noda import tiddlywiki` leaves raw HTML in bodies; it must arrive escaped as
/// code, or it is an injection.
#[test]
fn a_body_holding_markup_arrives_as_code() {
    let (server, paths) = serving();
    let id = id_of(&paths, "raw-html-import");
    let answer = server.get(&format!("/nb/default/n/{id}"));

    // Inline, being mid-paragraph; `web::render`'s tests cover a whole block.
    assert!(
        answer.says("<code>&lt;div class=\"x\"&gt;"),
        "{}",
        answer.body
    );
    assert!(!answer.says("<div class=\"x\">"), "{}", answer.body);
}

/// Everything that is not a note, with how many notes point at each — the
/// question `doctor --links` answers for orphans.
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
    assert!(answer.says("in 1 note"), "{}", answer.body);
    assert!(answer.says("nothing links to it"), "{}", answer.body);
    // A note is a name carrying an id, not any Markdown.
    assert!(
        answer.says("href=\"/nb/default/f/README.md\""),
        "{}",
        answer.body
    );
    let id = id_of(&paths, "budget-review");
    assert!(
        !answer.says(&format!("{id}-budget-review.md")),
        "{}",
        answer.body
    );
}

/// An image is shown inline; an SVG is not, as it can carry a script that would
/// run on this origin.
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
    assert!(
        svg.header("content-security-policy")
            .is_some_and(|value| value.contains("default-src 'none'")),
        "{:?}",
        svg.header("content-security-policy")
    );
}

/// `link::target` is the gate, the same one `doctor` and `file mv` resolve
/// links with.
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

/// Served as a file, a note would skip every decision the renderer makes.
#[test]
fn a_note_is_not_served_as_a_file() {
    let (server, paths) = serving();
    let id = id_of(&paths, "budget-review");
    let answer = server.get(&format!("/nb/default/f/{id}-budget-review.md"));
    assert_eq!(answer.status, 404, "{}", answer.body);
}

/// The half a suffix test gets wrong: refusing `README.md` made the files page's
/// link to it a dead end.
#[test]
fn a_markdown_file_that_is_not_a_note_is_served() {
    let (server, _paths) = serving();

    let answer = server.get("/nb/default/f/README.md");
    assert_eq!(answer.status, 200, "{}", answer.body);
    assert!(answer.says("default"), "{}", answer.body);

    let links = server.get("/nb/default/f/README.md/backlinks");
    assert_eq!(links.status, 200, "{}", links.body);
}

/// A link to another note's `.md` file points at that note's address.
#[test]
fn a_note_body_is_rendered_and_its_links_point_at_notes() {
    let (server, paths) = serving();
    let budget = id_of(&paths, "budget-review");
    let meeting = id_of(&paths, "meeting-notes");

    // A relative filename, as on a git host.
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
    assert!(server.get("/nb/ghost").says("No such notebook"));
    assert!(server.get("/nb/default/n/zzzzzzzz").says("No such note"));
}

/// There is no session, so without this a form on another site could commit.
#[test]
fn a_page_on_another_site_is_turned_away() {
    let (server, _paths) = serving();
    let answer = server.request("/", &[("Origin", "https://elsewhere.example")]);
    assert_eq!(answer.status, 403);
    assert!(answer.says("elsewhere.example"), "{}", answer.body);
}

/// DNS rebinding: `Origin` and `Host` agree, so the name itself is checked.
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

/// A typed address sends no `Origin`.
#[test]
fn an_ordinary_navigation_is_answered() {
    let (server, _paths) = serving();
    assert_eq!(server.get("/").status, 200);
}

#[test]
fn the_health_check_says_the_server_is_answering() {
    let (server, _paths) = serving();
    let answer = server.get("/health");
    assert_eq!(answer.status, 200);
    assert_eq!(answer.body, "ok\n");
    assert!(
        answer
            .head
            .to_lowercase()
            .contains("cache-control: no-store"),
        "{}",
        answer.head
    );
    assert!(
        answer
            .head
            .to_lowercase()
            .contains("content-type: text/plain"),
        "{}",
        answer.head
    );
}

/// A probe sends whatever `Host` it likes, so the check sits outside the guard.
#[test]
fn the_health_check_answers_a_name_the_guard_would_refuse() {
    let (server, _paths) = serving();
    let name = &[("Host", "kubernetes.default.svc")];
    assert_eq!(server.request("/health", name).status, 200);
    // Only this one route was loosened.
    assert_eq!(server.request("/", name).status, 403);
}

/// Most probes use `HEAD`; axum routes it to the `GET` handler, not a 405.
#[test]
fn the_health_check_answers_a_head_request() {
    let (server, _paths) = serving();
    let answer = server.send("HEAD", "/health", &[], None);
    assert_eq!(answer.status, 200);
    assert!(answer.body.is_empty(), "{:?}", answer.body);
}

/// By route template like any other; a probe is the row seen most under
/// `RUST_LOG=noda=debug`.
#[test]
fn the_health_check_is_logged_like_any_other_route() {
    let (root, _paths) = a_notebook();
    let server = Serving::start_logging(root, &[]);
    assert_eq!(server.get("/health").status, 200);
    let log = server.logged();
    assert!(log.contains("route=\"/health\""), "{log}");
    assert!(log.contains("event=\"http.request\""), "{log}");
}

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

    // The HTML spec has every browser send a `<textarea>`'s line breaks as CRLF.
    let written = std::fs::read_to_string(paths.notebooks_dir().join("default").join(format!(
        "{}.md",
        at.rsplit('/').next().map(|id| format!("{id}-from-the-phone")).unwrap()
    )))
    .expect("the note that was just written");
    assert!(!written.contains('\r'), "{written:?}");
}

/// The optimistic lock: an edit onto a note that moved is never written blind.
/// These two rewrote the same line, so the merge conflicts and the answer is a page.
#[test]
fn an_edit_that_overlaps_one_saved_since_comes_back_to_be_settled() {
    let (server, paths) = serving();
    let id = id_of(&paths, "budget-review");
    let stale = server.fingerprint_on(&format!("/nb/default/n/{id}/edit"));

    // Somebody else saves first.
    let landed = server.post(
        &format!("/nb/default/n/{id}/edit"),
        &[("fingerprint", &stale), ("body", "what the terminal wrote")],
    );
    assert_eq!(landed.status, 303);

    let refused = server.post(
        &format!("/nb/default/n/{id}/edit"),
        &[("fingerprint", &stale), ("body", "what the phone wrote")],
    );
    assert_eq!(refused.status, 200, "a refusal is a page, not a redirect");
    assert!(
        refused.says("Someone else saved while you were writing"),
        "{}",
        refused.body
    );
    // Both versions, inside conflict markers, in one editable box.
    assert!(
        refused.says("&lt;&lt;&lt;&lt;&lt;&lt;&lt; what you wrote"),
        "no conflict markers:\n{}",
        refused.body
    );
    assert!(refused.says("what the terminal wrote"), "{}", refused.body);
    assert!(refused.says("what the phone wrote"), "{}", refused.body);

    let on_disk = server.get(&format!("/nb/default/n/{id}"));
    assert!(on_disk.says("what the terminal wrote"), "{}", on_disk.body);
    assert!(!on_disk.says("what the phone wrote"), "{}", on_disk.body);
}

/// The fingerprint is a blob id, so the base version can be fetched and the two
/// edits merged.
#[test]
fn two_edits_in_different_parts_of_a_note_are_merged_and_both_survive() {
    let (server, paths) = serving();
    cmd::add(
        &paths,
        Some("Release checklist"),
        Some("one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nnine\nten"),
        &[],
    )
    .expect("add");
    let id = id_of(&paths, "release-checklist");
    let base = server.fingerprint_on(&format!("/nb/default/n/{id}/edit"));

    let landed = server.post(
        &format!("/nb/default/n/{id}/edit"),
        &[
            ("fingerprint", &base),
            (
                "body",
                "TERMINAL\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nnine\nten",
            ),
        ],
    );
    assert_eq!(landed.status, 303);

    // The stale form writes at the other end of the note.
    let saved = server.post(
        &format!("/nb/default/n/{id}/edit"),
        &[
            ("fingerprint", &base),
            (
                "body",
                "one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nnine\nPHONE",
            ),
        ],
    );
    assert_eq!(
        saved.status, 303,
        "a merge that holds is a save, not a page:\n{}",
        saved.body
    );

    let on_disk = server.get(&format!("/nb/default/n/{id}"));
    assert!(
        on_disk.says("TERMINAL"),
        "the terminal's line is gone:\n{}",
        on_disk.body
    );
    assert!(
        on_disk.says("PHONE"),
        "the phone's line is gone:\n{}",
        on_disk.body
    );
}

/// A note edited by hand and never committed has no base blob to merge from, so
/// both versions are handed back whole.
#[test]
fn a_clash_with_no_committed_version_to_merge_from_hands_back_both() {
    let (server, paths) = serving();
    let id = id_of(&paths, "reading-list");
    let path = paths
        .notebooks_dir()
        .join("default")
        .join(format!("{id}-reading-list.md"));
    let held = std::fs::read_to_string(&path).expect("the note on disk");
    std::fs::write(&path, held.replace("a book", "a book, uncommitted")).expect("write it back");

    let stale = server.fingerprint_on(&format!("/nb/default/n/{id}/edit"));
    let landed = server.post(
        &format!("/nb/default/n/{id}/edit"),
        &[("fingerprint", &stale), ("body", "what the terminal wrote")],
    );
    assert_eq!(landed.status, 303);

    let refused = server.post(
        &format!("/nb/default/n/{id}/edit"),
        &[("fingerprint", &stale), ("body", "what the phone wrote")],
    );
    assert_eq!(refused.status, 200);
    assert!(
        refused.says("This note changed while you were writing"),
        "the two-pane fallback is what an unfetchable base gets:\n{}",
        refused.body
    );
    assert!(refused.says("what the terminal wrote"), "{}", refused.body);
    assert!(refused.says("what the phone wrote"), "{}", refused.body);
}

/// The reader hears while still typing, not after pressing Save.
#[test]
fn an_open_editor_is_told_the_note_moved_under_it() {
    let (server, paths) = serving();
    let id = id_of(&paths, "budget-review");
    let was = server.fingerprint_on(&format!("/nb/default/n/{id}/edit"));

    let mut watch = server.watch(&format!("/nb/default/n/{id}/watch"), None);
    // Headers are written when the handler returns, so the subscription is in
    // place before the note changes.
    let head = watch.hears("text/event-stream").to_string();
    assert!(head.contains("200 OK"), "{head}");

    let saved = server.post(
        &format!("/nb/default/n/{id}/edit"),
        &[("fingerprint", &was), ("body", "written somewhere else")],
    );
    assert_eq!(saved.status, 303);

    let heard = watch.hears("data: ");
    // The file's current fingerprint, for the form to compare against.
    assert!(
        !heard.contains(&format!("data: {was}")),
        "it said the note is at the fingerprint the form already holds:\n{heard}"
    );
}

/// A deflater would hold a watch's messages back for hours. `DefaultPredicate`
/// already declines event streams; this keeps it that way if the predicate changes.
#[test]
fn a_watch_is_never_compressed() {
    let (server, paths) = serving();
    let id = id_of(&paths, "budget-review");

    let mut watch = server.watch(&format!("/nb/default/n/{id}/watch"), Some("gzip"));
    let head = watch.hears("text/event-stream").to_string();
    assert!(
        !head.to_lowercase().contains("content-encoding"),
        "asked with gzip, an event stream must still arrive as it is written:\n{head}"
    );
}

/// A graceful stop waits for requests in flight, and a watch never finishes on
/// its own.
#[test]
#[cfg(unix)]
fn a_stop_does_not_wait_for_a_watch_that_never_ends() {
    let (root, paths) = a_notebook();
    let mut server = Serving::start(root, &[]);
    let id = id_of(&paths, "budget-review");

    let mut watch = server.watch(&format!("/nb/default/n/{id}/watch"), None);
    watch.hears("text/event-stream");

    let stopped = server.signalled("TERM");
    assert!(
        stopped.status.success(),
        "a watch held it open: {:?}\n{}",
        stopped.status,
        stopped.said
    );
    // And the client's socket is closed.
    watch.ends();
}

/// The editor listens; the one-field forms do not.
#[test]
fn only_the_forms_that_carry_a_fingerprint_listen() {
    let (server, paths) = serving();
    let id = id_of(&paths, "budget-review");
    let hook = noda::web::asset::Asset::Watching.href();

    let editor = server.get(&format!("/nb/default/n/{id}/edit"));
    assert!(
        editor.says(hook),
        "the editor does not listen:\n{}",
        editor.body
    );

    for path in ["rename", "tags", "delete"] {
        let page = server.get(&format!("/nb/default/n/{id}/{path}"));
        assert!(
            !page.says(hook),
            "/{path} listens and need not:\n{}",
            page.body
        );
    }
}

/// A tag added after the page was served is not the page's to remove, so the
/// change is measured against what the form offered.
#[test]
fn saving_tags_from_a_stale_page_does_not_remove_a_tag_added_since() {
    let (server, paths) = serving();
    let id = id_of(&paths, "budget-review");

    let form = server.get(&format!("/nb/default/n/{id}/tags"));
    assert!(
        form.says("name=\"saw\" value=\"work\""),
        "the form does not say what it offered:\n{}",
        form.body
    );

    cmd::tag(&paths, &id, &["+q3".to_string()], cmd::Touch::Stamp).expect("tag");

    let saved = server.post(
        &format!("/nb/default/n/{id}/tags"),
        &[("saw", "work"), ("keep", "work")],
    );
    assert_eq!(saved.status, 303);

    let note = server.get(&format!("/nb/default/n/{id}"));
    assert!(
        note.says("<span class=\"tags\">work, q3</span>"),
        "the tag added since was removed by a page that never showed it:\n{}",
        note.body
    );
}

/// A box the page offered and the reader unticked is still removed.
#[test]
fn unticking_a_tag_on_the_page_that_offered_it_removes_it() {
    let (server, paths) = serving();
    let id = id_of(&paths, "budget-review");

    let saved = server.post(&format!("/nb/default/n/{id}/tags"), &[("saw", "work")]);
    assert_eq!(saved.status, 303);

    let note = server.get(&format!("/nb/default/n/{id}"));
    assert!(
        !note.says("<span class=\"tags\">work</span>"),
        "the unticked tag survived:\n{}",
        note.body
    );
}

/// The form says which tags survive and the server works out the change: one
/// submit, one commit.
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

/// Split by `query::split`: a space separates, quotes hold a tag together. The
/// plural label is what tells the reader so.
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

/// The row rule must out-specify `form.write label`'s `display:block`; a bare
/// `.tick` lost, jamming the box against its tag.
#[test]
fn a_tag_row_centres_its_box_against_its_name() {
    let (server, paths) = serving();
    let id = id_of(&paths, "meeting-notes");

    let sheet = linked_stylesheet(&server, &format!("/nb/default/n/{id}/tags"));
    assert!(
        sheet.says("form.write label.tick{display:flex"),
        "the row rule is out-specified:\n{}",
        sheet.body
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
    // The id never moves.
    assert_eq!(
        saved.location.as_deref(),
        Some(&*format!("/nb/default/n/{id}"))
    );

    let note = server.get(&format!("/nb/default/n/{id}"));
    assert!(note.says("Budget review 2026"), "{}", note.body);
    // The slug has its own span, so the filename is never contiguous.
    assert!(note.says(">-budget-review-2026</span>"), "{}", note.body);
}

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

/// A `GET` never changes anything, so a prefetcher or crawler cannot commit.
#[test]
fn asking_to_delete_only_asks() {
    let (server, paths) = serving();
    let id = id_of(&paths, "reading-list");

    let asked = server.get(&format!("/nb/default/n/{id}/delete"));
    assert_eq!(asked.status, 200);
    assert!(asked.says("Delete"), "{}", asked.body);
    assert_eq!(server.get(&format!("/nb/default/n/{id}")).status, 200);
}

/// The only write on the bar with no page in between, so the action and the
/// button it leaves behind are tested together.
#[test]
fn the_bar_pins_a_note_and_then_offers_to_let_it_down() {
    let (server, paths) = serving();
    let id = id_of(&paths, "budget-review");
    let at = format!("/nb/default/n/{id}");

    let before = server.get(&at);
    assert!(before.says(">Pin</span>"), "{}", before.body);
    assert!(!before.says(">Unpin</span>"), "{}", before.body);

    let pinned = server.post(&format!("{at}/pin"), &[]);
    assert_eq!(pinned.status, 303);

    let note = server.get(&at);
    assert!(note.says(">Unpin</span>"), "{}", note.body);
    let listing = server.get("/nb/default");
    assert!(
        listing.says("<span class=\"pin\">pinned</span>"),
        "{}",
        listing.body
    );

    // Idempotent: the route names the state it wants.
    assert_eq!(server.post(&format!("{at}/pin"), &[]).status, 303);
    assert!(server.get(&at).says(">Unpin</span>"));

    assert_eq!(server.post(&format!("{at}/unpin"), &[]).status, 303);
    let loose = server.get(&at);
    assert!(loose.says(">Pin</span>"), "{}", loose.body);
    assert!(!loose.says(">Unpin</span>"), "{}", loose.body);
}

#[test]
fn another_site_cannot_pin_a_note() {
    let (server, paths) = serving();
    let id = id_of(&paths, "budget-review");
    let refused = server.send(
        "POST",
        &format!("/nb/default/n/{id}/pin"),
        &[("Origin", "https://elsewhere.example")],
        Some(""),
    );
    assert_eq!(refused.status, 403);
    assert!(
        !server
            .get(&format!("/nb/default/n/{id}"))
            .says(">Unpin</span>")
    );
}

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

/// The hashed address keeps for a year; the page naming it is never kept. Either
/// half alone serves a page whose stylesheet is a 404.
#[test]
fn the_stylesheet_is_linked_once_and_kept_for_a_year() {
    let (server, _paths) = serving();
    let sheet = linked_stylesheet(&server, "/nb/default");

    assert_eq!(sheet.status, 200);
    assert_eq!(
        sheet.header("content-type").as_deref(),
        Some("text/css; charset=utf-8")
    );
    assert_eq!(
        sheet.header("cache-control").as_deref(),
        Some("public, max-age=31536000, immutable")
    );
    assert_eq!(
        sheet.header("x-content-type-options").as_deref(),
        Some("nosniff")
    );
    assert!(sheet.says("--tap:48px"), "{}", sheet.body);
    assert!(sheet.says("prefers-color-scheme:dark"), "{}", sheet.body);

    let page = server.get("/nb/default");
    assert_eq!(page.header("cache-control").as_deref(), Some("no-cache"));
    assert!(
        !page.says("--tap:48px"),
        "the sheet is back inside the page"
    );
}

/// Scripts are cached the same way, and a page names only the ones it runs.
#[test]
fn a_page_links_the_scripts_it_uses_and_no_others() {
    let (server, paths) = serving();
    let id = id_of(&paths, "budget-review");

    let listing = server.get("/nb/default");
    for named in ["/a/listing.", "/a/panes.", "/a/beside."] {
        assert!(listing.says(named), "the listing lost {named}");
    }
    assert!(!listing.says("/a/standing."), "{}", listing.body);

    // A note has the panes but no filter.
    let note = server.get(&format!("/nb/default/n/{id}"));
    assert!(note.says("/a/panes."), "{}", note.body);
    assert!(note.says("/a/beside."), "{}", note.body);
    assert!(!note.says("/a/listing."), "{}", note.body);

    // The poll is only on the screen that waits for an errand.
    let status = server.get("/nb/default/status");
    assert!(status.says("/a/standing."), "{}", status.body);
    assert!(!status.says("/a/panes."), "{}", status.body);
}

/// A stale hash must not get the current bytes under a year of `immutable`; and
/// the route stays a lookup, with no path to join.
#[test]
fn an_asset_address_this_build_did_not_write_is_not_answered() {
    let (server, _paths) = serving();
    for missing in [
        "/a/style.000000000000.css",
        "/a/style.css",
        "/a/panes.000000000000.js",
        "/a/..%2f..%2fetc%2fpasswd",
    ] {
        let answer = server.get(missing);
        assert_eq!(
            answer.status, 404,
            "{missing} was answered: {}",
            answer.body
        );
    }
}

/// Only three screens carry a script — each removes a wait the design named in
/// advance — and this keeps the number down.
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

    // The listing filters, the status screen polls, and a note fills its index
    // pane and margin note. The backlinks page above is read, not run.
    for path in &[
        "/nb/default".to_string(),
        "/nb/default/status".to_string(),
        format!("/nb/default/n/{id}"),
    ] {
        assert!(
            server.get(path).says("<script src=\"/a/"),
            "{path} lost its script"
        );
    }

    // No handler attributes: every listener is set from inside a script.
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

/// Every tag, commonest first, each linking into the listing.
#[test]
fn the_tags_screen_counts_them_and_leads_into_the_listing() {
    let (server, _paths) = serving();
    let answer = server.get("/nb/default/tags");

    assert_eq!(answer.status, 200);
    // `work` is on two notes and `ops` on one.
    assert!(answer.says("2 notes"), "{}", answer.body);
    assert!(
        answer.body.find(">work<") < answer.body.find(">ops<"),
        "{}",
        answer.body
    );
    // A tag with a space is quoted, or the search would split it.
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

/// Dates far past and far future, so the test does not depend on the clock.
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
    // A ticked box is not listed.
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
    // The `due:` term is lifted out of the text.
    assert!(!answer.says("due:2000-01-01"), "{}", answer.body);
    assert!(answer.says("Chores"), "{}", answer.body);
}

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
    assert!(!answer.says("Reading list"), "{}", answer.body);
}

/// The match is on the id in the destination, so a retitle — when those links
/// look broken everywhere else — does not hide them.
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

/// An attachment has no page, so its count on the files page links to its backlinks.
#[test]
fn a_files_backlinks_are_reached_from_the_count_beside_it() {
    let (server, _paths) = serving();
    let files = server.get("/nb/default/files");

    assert!(
        files.says("href=\"/nb/default/f/rack.png/backlinks\""),
        "{}",
        files.body
    );
    // Nothing points at the svg, so there is no link to an empty page.
    assert!(files.says("nothing links to it"), "{}", files.body);
    assert!(!files.says("plan.svg/backlinks"), "{}", files.body);

    let answer = server.get("/nb/default/f/rack.png/backlinks");
    assert_eq!(answer.status, 200);
    assert!(answer.says("The rack"), "{}", answer.body);
}

/// A note is not a file here either; its backlinks have their own route.
#[test]
fn a_note_is_not_asked_for_backlinks_as_if_it_were_a_file() {
    let (server, paths) = serving();
    let id = id_of(&paths, "budget-review");
    let answer = server.get(&format!("/nb/default/f/{id}-budget-review.md/backlinks"));
    assert_eq!(answer.status, 404);
    assert_eq!(
        server
            .get("/nb/default/f/..%2F..%2Fetc%2Fpasswd/backlinks")
            .status,
        404
    );
}

/// The same bar on every notebook screen, marking the current one with
/// `aria-current` for the stylesheet and screen readers alike.
#[test]
fn the_notebook_screens_share_one_bar_that_says_where_you_are() {
    let (server, _paths) = serving();
    for (path, here) in [
        ("/nb/default", Some("/nb/default")),
        ("/nb/default/tags", Some("/nb/default/tags")),
        ("/nb/default/todo", Some("/nb/default/todo")),
        ("/nb/default/files", Some("/nb/default/files")),
        // Not on the bar; reached from the chip in the corner.
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
            // With its value: the inlined stylesheet names the bare attribute.
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
        assert!(
            answer.says("class=\"fab\" href=\"/nb/default/new\""),
            "{path} has no way to write: {}",
            answer.body
        );
    }
}

/// The wide layout caps `main`, so a body outside it runs the monitor's full
/// width, as every form page once did.
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

/// `noda status` on a screen, with nothing fetched: a page that waited on the
/// network would hang exactly when the network is the problem.
#[test]
fn the_status_screen_says_where_a_notebook_stands() {
    let (server, _paths) = serving();
    let answer = server.get("/nb/default/status");
    assert_eq!(answer.status, 200);
    assert!(answer.says(">Holds</div>"), "{}", answer.body);
    assert!(answer.says("5 notes, 3 files"), "{}", answer.body);
    assert!(answer.says("clean"), "{}", answer.body);
    // The remedy, since no screen here sets a remote.
    assert!(answer.says("noda remote set"), "{}", answer.body);
    // No errand yet, so no report and no refresh.
    assert!(!answer.says("said working"), "{}", answer.body);
    assert!(
        !answer.says("<meta http-equiv=\"refresh\""),
        "{}",
        answer.body
    );

    // Only a POST starts an errand, so a reload is harmless.
    for _ in 0..3 {
        assert!(
            !server.get("/nb/default/status").says("class=\"said"),
            "a GET started an errand"
        );
    }
}

/// `noda web` has no accounts, so a token in the remote URL (how the container
/// image reaches an HTTPS host) must not reach the page.
#[test]
fn the_status_screen_shows_a_remote_without_its_token() {
    let (root, paths) = a_notebook();
    cmd::remote_set(
        &paths,
        "https://x-access-token:ghp_secret@github.com/me/notes.git",
    )
    .expect("set the remote");
    let server = Serving::start(root, &[]);

    let answer = server.get("/nb/default/status");
    assert_eq!(answer.status, 200);
    assert!(!answer.says("ghp_secret"), "{}", answer.body);
    // Host and path survive.
    assert!(
        answer.says("***@github.com/me/notes.git"),
        "{}",
        answer.body
    );
}

/// The listing's drift chip links to the status screen and carries its answer.
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

    // With a remote, what `noda status` says.
    let (server, _paths, _remote) = serving_with_a_remote();
    assert!(
        server.get("/nb/default").says(">never synced</span>"),
        "the chip did not follow the notebook"
    );
    server.post("/nb/default/status/sync", &[]);
    server.settled("/nb/default/status");
    assert!(
        server.get("/nb/default").says(">in sync</span>"),
        "the chip is stale after a sync"
    );
}

/// On the note's bar, in the danger colour, leading to a page that asks first.
/// The fragment must carry the bar too, being chrome a fragment could drop.
#[test]
fn deleting_is_on_the_bar_and_still_asks_first() {
    let (server, paths) = serving();
    let id = id_of(&paths, "budget-review");
    let at = format!("/nb/default/n/{id}");
    let mark = format!("/n/{id}/delete\" class=\"danger\"");

    let whole = server.get(&at);
    assert!(whole.says(&mark), "{}", whole.body);
    assert!(!whole.says("perilous"), "{}", whole.body);

    let part = server.request(&at, &[("X-Noda-Fragment", "read")]);
    assert!(part.says(&mark), "{}", part.body);

    let asked = server.get(&format!("{at}/delete"));
    assert!(asked.says("Delete Budget review?"), "{}", asked.body);
    // Above the form, not inside: nested, a `.said` is inset twice.
    let said = asked.body.find("class=\"said\"").expect("it says nothing");
    let form = asked.body.find("<form").expect("it has no form");
    assert!(said < form, "{}", asked.body);
}

/// An address here carries a note id, and must not leak via `Referer`.
/// The note is added here, not to the fixture, to keep the fixture's counts.
#[test]
fn nothing_a_note_points_at_is_told_where_it_was_pointed_from() {
    let (server, paths) = serving();
    cmd::add(
        &paths,
        Some("Sources"),
        Some("the figures are at https://example.com/q3, and so is the rest"),
        &[],
    )
    .expect("add");
    let id = id_of(&paths, "sources");
    let answer = server.get(&format!("/nb/default/n/{id}"));

    assert_eq!(
        answer.header("referrer-policy").as_deref(),
        Some("same-origin"),
        "{}",
        answer.head
    );
    // Survives a proxy stripping headers.
    assert!(
        answer.says("<meta name=\"referrer\" content=\"same-origin\">"),
        "{}",
        answer.body
    );
    // A bare URL becomes a link that sends no referrer.
    assert!(
        answer.says(
            "<a href=\"https://example.com/q3\" target=\"_blank\" \
             rel=\"noopener noreferrer\">https://example.com/q3</a>"
        ),
        "{}",
        answer.body
    );
    // The trailing comma is not part of the URL.
    assert!(answer.says("</a>, and so is the rest"), "{}", answer.body);
}

/// Exactly `same-origin`: `no-referrer` looks stricter but makes forms post
/// `Origin: null`, which `web::guard` refuses — and no test here, sending its own
/// `Origin`, would notice.
#[test]
fn every_page_says_an_address_does_not_travel() {
    let (server, _paths) = serving();
    for path in ["/", "/nb/default", "/nb/default/tags", "/nb/default/status"] {
        let answer = server.get(path);
        assert_eq!(
            answer.header("referrer-policy").as_deref(),
            Some("same-origin"),
            "{path}: {}",
            answer.head
        );
    }
}

/// The status screen is not on the bar but carries it.
#[test]
fn the_status_screen_is_not_a_dead_end() {
    let (server, _paths) = serving();
    let answer = server.get("/nb/default/status");
    assert!(answer.says("class=\"actionbar\""), "{}", answer.body);
    // With its value: the inlined stylesheet names the bare attribute.
    assert!(!answer.says("aria-current=\"page\""), "{}", answer.body);
    assert!(answer.says("<main"), "{}", answer.body);
}

/// The press answers at once, the errand runs behind it, and its output lands on
/// the status screen.
#[test]
fn a_sync_answers_before_it_finishes_and_says_how_it_went() {
    let (server, _paths, remote) = serving_with_a_remote();
    let answer = server.get("/nb/default/status");
    assert!(answer.says("never synced"), "{}", answer.body);

    let started = server.post("/nb/default/status/sync", &[]);
    assert_eq!(started.status, 303);
    assert_eq!(started.location.as_deref(), Some("/nb/default/status"));

    let done = server.settled("/nb/default/status");
    assert_eq!(done.status, 200);
    assert!(done.says("push:"), "{}", done.body);
    assert!(!done.says("said bad"), "{}", done.body);
    assert!(!done.says("<meta http-equiv=\"refresh\""), "{}", done.body);
    // The drift is re-read after the errand.
    assert!(done.says("in sync"), "{}", done.body);

    let there = git2::Repository::open_bare(&remote).expect("open the remote");
    assert!(
        there.head().expect("a branch").peel_to_commit().is_ok(),
        "the remote has no commit on it"
    );
}

/// A failure is reported in the command's own words where the button was pressed.
#[test]
fn a_push_with_nowhere_to_send_it_says_so() {
    let (server, _paths) = serving();
    assert_eq!(server.post("/nb/default/status/push", &[]).status, 303);

    let done = server.settled("/nb/default/status");
    assert!(done.says("said bad"), "{}", done.body);
    assert!(done.says("remote"), "{}", done.body);
    assert!(!done.says("<meta http-equiv=\"refresh\""), "{}", done.body);
}

#[test]
fn there_is_nothing_called_fetch_to_do_to_a_notebook() {
    let (server, _paths) = serving();
    assert_eq!(server.post("/nb/default/status/fetch", &[]).status, 404);
    assert_eq!(server.post("/nb/nowhere/status/sync", &[]).status, 404);
    assert_eq!(server.get("/nb/nowhere/status").status, 404);
}

// ---------------------------------------------------------------- stopping it

/// A server that stops when asked exits 0.
#[test]
#[cfg(unix)]
fn a_supervisor_can_stop_it() {
    let (root, _paths) = a_notebook();
    let mut server = Serving::start(root, &[]);
    // Up first, so a failure is about the stop.
    assert_eq!(server.get("/health").status, 200);

    let stopped = server.signalled("TERM");
    assert!(
        stopped.status.success(),
        "asked to stop, it should exit 0: {:?}",
        stopped.status
    );
    assert!(
        stopped.said.contains("SIGTERM"),
        "it should name what stopped it: {:?}",
        stopped.said
    );
}

#[test]
#[cfg(unix)]
fn ctrl_c_stops_it_too() {
    let (root, _paths) = a_notebook();
    let mut server = Serving::start(root, &[]);
    assert_eq!(server.get("/health").status, 200);

    let stopped = server.signalled("INT");
    assert!(
        stopped.status.success(),
        "asked to stop, it should exit 0: {:?}",
        stopped.status
    );
    assert!(
        stopped.said.contains("SIGINT"),
        "it should name what stopped it: {:?}",
        stopped.said
    );
}

#[test]
#[cfg(unix)]
fn nothing_answers_once_it_has_stopped() {
    let (root, _paths) = a_notebook();
    let mut server = Serving::start(root, &[]);
    let port = server.port;
    assert_eq!(server.get("/health").status, 200);

    server.signalled("TERM");
    assert!(
        TcpStream::connect(("127.0.0.1", port)).is_err(),
        "port {port} is still accepting connections"
    );
}

/// A graceful shutdown must not wait on an idle keep-alive connection — a tab
/// left open. `Serving::waited` turns the hang into a failure.
#[test]
#[cfg(unix)]
fn an_idle_connection_does_not_hold_it_open() {
    let (root, _paths) = a_notebook();
    let mut server = Serving::start(root, &[]);

    // No `Connection: close`, so the server keeps it alive.
    let mut socket = TcpStream::connect(("127.0.0.1", server.port)).expect("connect");
    let wire = format!(
        "GET /health HTTP/1.1\r\nHost: 127.0.0.1:{}\r\n\r\n",
        server.port
    );
    socket.write_all(wire.as_bytes()).expect("write a request");
    let mut answered = String::new();
    BufReader::new(&socket)
        .read_line(&mut answered)
        .expect("read the status line");
    assert!(answered.contains("200"), "{answered:?}");

    // And one that never sends anything.
    let _silent = TcpStream::connect(("127.0.0.1", server.port)).expect("connect");

    let stopped = server.signalled("TERM");
    assert!(
        stopped.status.success(),
        "it should have stopped anyway: {:?}",
        stopped.status
    );
}
