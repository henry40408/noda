//! `noda web` — the notebook over HTTP, for reading and writing it from a phone.
//!
//! The same rule the browser follows, one layer out: **read through `notebook`,
//! write through `cmd`**. This is a third renderer over the data the other two
//! already agree about, not a wrapper around either of them — `cmd` renders to
//! text, `tui` renders to a terminal, and this renders to HTML.
//!
//! Three things about the shape here that are not obvious from the code:
//!
//! - **A notebook is named in the URL, never taken from the active pointer.**
//!   `noda use` writes a pointer in `XDG_STATE_HOME` that belongs to a shell
//!   session; a browser has tabs, and a tab that quietly changed which notebook
//!   it was showing because something happened in a terminal would be worse than
//!   a longer URL. So `/nb/<name>/…`, and nothing here reads the pointer.
//!
//! - **A note is addressed by id, and everything else redirects to it.** The
//!   slug follows the title, so a bookmark written against it dies the moment
//!   the note is renamed. The id never moves. An id prefix redirects too, for
//!   the same reason: one page, one address.
//!
//! - **Every handler opens the notebook itself, inside `spawn_blocking`.**
//!   `git2::Repository` is `!Send`: it cannot be shared between threads and it
//!   cannot be held across an await. That reads like a restriction and is
//!   actually the design — one request, one handle, nothing kept between them.
//!   It is also what makes a slow walk on one request not a stall on the others.

pub mod asset;
pub mod guard;
pub mod log;
pub mod page;
pub mod render;
pub mod script;
pub mod theme;
pub mod work;

use std::fmt::Write;
use std::sync::Arc;

use axum::Router;
use axum::extract::{Path, Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};

use crate::note::{self, Note};
use crate::notebook::Notebook;
use crate::query::{self, Query};
use crate::{Error, Paths, Result, cmd};

/// What every handler is given.
struct Server {
    paths: Paths,
    guard: guard::Guard,
    /// Held for the whole of any request that writes, by the notebook it writes
    /// to.
    ///
    /// Reads can happen at once and do; writes to one notebook cannot. Two
    /// commits racing in one repository meet at `index.lock`, and what comes
    /// back is libgit2 saying a file exists — a true statement about a lock file
    /// and no help at all to somebody who pressed Save.
    ///
    /// A `std::sync::Mutex` and not tokio's, because it is only ever taken off
    /// the async threads: the thing it guards is blocking work, and a lock that
    /// could be held across an await would be a lock held while a request is
    /// doing nothing.
    ///
    /// It does **not** lock the notebook against the world. A terminal in
    /// another window is writing to the same repository and always could be;
    /// that is what the fingerprint below is for.
    writing: Locks,
    /// What each notebook's `sync`, `pull` or `push` is doing — the one piece of
    /// state that outlives a request here, because the errand does too.
    errands: work::Errands,
}

/// One write lock per notebook.
///
/// It was a single lock over everything until a network errand held one for as
/// long as a network takes, and the difference showed up at once: a notebook
/// whose remote had gone quiet froze Save on every *other* notebook too, for
/// however long libgit2 takes to give up on a host that is not answering.
///
/// Nothing was ever gained by that. `index.lock` is a file inside one
/// repository, and two notebooks are two repositories with nothing between
/// them. The lock belongs where the collision does.
#[derive(Default)]
struct Locks(std::sync::Mutex<std::collections::BTreeMap<String, Arc<std::sync::Mutex<()>>>>);

impl Locks {
    /// That notebook's lock, made if this is the first anyone has asked.
    ///
    /// An `Arc` out rather than a guard, because a guard would borrow the map —
    /// and the map has to be free the moment this returns, or one notebook's
    /// slow push would be holding the thing every other notebook has to go
    /// through to find its own lock.
    fn of(&self, book: &str) -> Arc<std::sync::Mutex<()>> {
        Arc::clone(
            self.0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .entry(book.to_string())
                .or_default(),
        )
    }
}

/// What a note's file hashes to, as git would hash it.
///
/// The optimistic lock. A form carries the fingerprint the note had when the
/// page was drawn, and the write refuses if it no longer matches — so an edit
/// begun on a phone at breakfast cannot silently flatten one made at a terminal
/// at lunch.
///
/// **The blob id and not the `updated` stamp.** noda has `--no-touch`, which
/// exists precisely so that a note's content can change without its `updated`
/// moving; a session of small corrections to imported notes is the case it was
/// built for. A version marker that goes wrong exactly when somebody is making
/// many small edits is a version marker that fails in the situation it exists
/// for. Content hashes change when content changes, and never otherwise.
fn fingerprint(path: &std::path::Path) -> Result<String> {
    Ok(git2::Oid::hash_file(git2::ObjectType::Blob, path)?.to_string())
}

type Shared = Arc<Server>;

/// Serves until killed.
///
/// The address is printed rather than returned, unlike every other command here:
/// this one does not finish, and what a reader needs is the URL to type into a
/// phone — now, not when the process ends. The `String` it answers with is the
/// shape `main` prints and is always empty, for the same reason.
pub fn serve(paths: &Paths, listen: &str, allow: &[String], format: log::Format) -> Result<String> {
    // Before the bind, so a failure to listen is the first thing the log says
    // rather than the first thing it misses.
    log::start(format);
    let server = Arc::new(Server {
        paths: paths.clone(),
        guard: guard::Guard::new(allow),
        writing: Locks::default(),
        errands: work::Errands::default(),
    });

    // Built by hand rather than with `#[tokio::main]`: `main` is a clap match
    // arm and every other arm is ordinary blocking code. Only I/O is enabled —
    // there are no timers here, and no signal handling beyond what the terminal
    // already does to a foreground process.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_io()
        .build()?;

    runtime.block_on(async move {
        let listener = tokio::net::TcpListener::bind(listen)
            .await
            .map_err(|e| Error::msg(format!("could not listen on {listen}: {e}")))?;
        let at = listener.local_addr()?;
        println!("noda is at http://{at}");
        // Said every time, because it is the difference between a notebook on
        // one machine and a notebook on the network, and it is not something to
        // find out about afterwards.
        if !at.ip().is_loopback() {
            println!("reachable from the network — there is no password on it");
        }
        axum::serve(listener, router(server)).await?;
        Ok(String::new())
    })
}

fn router(server: Shared) -> Router {
    Router::new()
        .route("/", get(front))
        // Inside the guard with everything else. Nothing here is a notebook's
        // and none of it is secret, but a page that the guard refuses should
        // not be able to draw itself either — and a host nobody allowed has no
        // business getting a reply of any kind.
        .route("/a/{file}", get(held_asset))
        .route("/nb/{book}", get(listing))
        .route("/nb/{book}/files", get(files))
        // The three screens that are about the notebook rather than about one
        // note. Each is a walk of every note's body, which is why none of them
        // is a column on the listing — the same line `ls` holds against new
        // columns and `todo` was made a command of its own by.
        .route("/nb/{book}/tags", get(tags))
        .route("/nb/{book}/todo", get(todo))
        // Where the notebook stands, and — under it, one segment down — the
        // three things that change it. The screen is a `GET` and every errand is
        // a `POST` to an address of its own, which is what makes a reload of the
        // screen a question rather than a second push.
        .route("/nb/{book}/status", get(status))
        .route("/nb/{book}/status/{errand}", post(errand))
        // One path segment, not a wildcard: a notebook is a flat directory, and
        // a route that could match `a/b` would be inviting a path to be
        // assembled out of pieces nobody checked.
        .route("/nb/{book}/f/{name}", get(held))
        .route("/nb/{book}/f/{name}/backlinks", get(file_backlinks))
        .route("/nb/{book}/new", get(new_form).post(new_note))
        .route("/nb/{book}/n/{key}", get(reading))
        .route("/nb/{book}/n/{key}/backlinks", get(note_backlinks))
        // One shape for all of them: `GET` shows the form, `POST` does the
        // thing, and both live at the address of the thing they are about. No
        // JavaScript is involved in any of it, so a form is the only way a
        // browser can ask for a change at all — and a `GET` that changed
        // something would be a link a prefetcher could press.
        .route("/nb/{book}/n/{key}/edit", get(edit_form).post(edit_note))
        .route(
            "/nb/{book}/n/{key}/rename",
            get(rename_form).post(rename_note),
        )
        .route("/nb/{book}/n/{key}/tags", get(tags_form).post(tag_note))
        .route(
            "/nb/{book}/n/{key}/delete",
            get(delete_form).post(delete_note),
        )
        .layer(middleware::from_fn_with_state(
            Arc::clone(&server),
            admitted,
        ))
        // Added after the guard's layer and therefore outside it: a layer in
        // axum covers the routes declared before it, and this one is meant not
        // to be covered. See `health` for why.
        .route("/health", get(health))
        // Outside the guard, so a refusal is timed and counted like any other
        // answer. It still runs after routing — axum matches the route before
        // handing the request to the layers — which is what puts the template
        // within reach at all.
        .layer(middleware::from_fn(log::timed))
        .with_state(server)
}

/// The stylesheet, or one of the scripts.
///
/// **The whole of this route is a lookup and two headers.** Nothing is read
/// from disk, nothing is decoded from the path, and the path itself is never
/// joined to anything — `asset::find` compares it against the handful of names
/// this build wrote, so the traversal question every file route has to answer
/// is one this one cannot be asked.
///
/// `immutable`, for a year. The address carries a hash of the bytes behind it,
/// so the only way for this answer to be wrong is for it to be a different
/// answer, and a different answer has a different address. The other half of
/// that bargain is on the pages: they are `no-cache`, so a reader always has
/// the addresses this build wrote.
async fn held_asset(Path(file): Path<String>) -> Response {
    let Some(held) = asset::find(&file) else {
        return (StatusCode::NOT_FOUND, plain("no such asset\n")).into_response();
    };
    (
        [
            (header::CONTENT_TYPE, held.kind.to_string()),
            (
                header::CACHE_CONTROL,
                "public, max-age=31536000, immutable".to_string(),
            ),
            // What noda said it is, is what it is — the same line every other
            // typed answer here carries.
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff".to_string()),
        ],
        held.body.clone(),
    )
        .into_response()
}

/// Whether this process is still able to answer.
///
/// **Outside the guard, and that is the decision worth explaining.** The guard
/// reads `Host` and refuses a name nobody allowed, which is right for a browser
/// and wrong for a probe: a probe is a program, and the `Host` it sends is
/// whatever the thing running it decided on — a pod's own address, a service
/// name, the name on a proxy's certificate. A liveness check that answered 403
/// because `--allow-host` had not been given would report a healthy server as
/// dead, and the restart that followed would not fix it. There is also nothing
/// on the other side of the guard to protect here: no notebook is opened, no
/// header is echoed back, and the only thing the answer discloses is that
/// something is listening — which the caller established by connecting.
///
/// **It goes through `spawn_blocking`, which is the whole of what it tests.**
/// Every page in this server does its work on the blocking pool, because
/// libgit2 offers no other kind; a check that answered from the async side
/// would return 200 with every reader hanging, and a health check that cannot
/// fail is a health check that is not being run. Waiting for one blocking
/// thread costs microseconds when the pool is free and does not come back when
/// it is not, which is exactly the answer a probe's timeout is there to hear.
///
/// **It does not open a notebook, and that is not laziness.** A notebook that
/// will not open is a repository to repair, not a process to restart; a check
/// that failed on one would turn a single broken notebook into a container
/// restarting every thirty seconds and still failing. What this reports is what
/// a restart can mend.
async fn health() -> Response {
    let alive = tokio::task::spawn_blocking(|| ()).await.is_ok();
    if !alive {
        // The pool lost a task the way it does when a shutdown is under way. The
        // page handlers say the same thing here, and this one has a status code
        // for it that a probe already knows how to read.
        log::lost();
        return (StatusCode::SERVICE_UNAVAILABLE, plain("unavailable\n")).into_response();
    }
    (StatusCode::OK, plain("ok\n")).into_response()
}

/// A short answer meant for a program, with the two headers that keeps honest.
///
/// `no-store` because a health check behind a cache is a health check that can
/// report a stopped server as running, and there is nothing here worth keeping
/// anyway. `nosniff` for the reason every other answer here carries it: what
/// noda said this is, is what it is.
fn plain(body: &'static str) -> impl IntoResponse {
    (
        [
            (
                header::CONTENT_TYPE,
                header::HeaderValue::from_static("text/plain; charset=utf-8"),
            ),
            (
                header::CACHE_CONTROL,
                header::HeaderValue::from_static("no-store"),
            ),
            (
                header::X_CONTENT_TYPE_OPTIONS,
                header::HeaderValue::from_static("nosniff"),
            ),
        ],
        body,
    )
}

/// The guard, in front of everything.
///
/// A layer and not a check inside each handler: a route added later must be
/// covered by having been added, not by somebody remembering.
async fn admitted(State(server): State<Shared>, request: Request, next: Next) -> Response {
    let headers = request.headers();
    let host = text(headers, header::HOST);
    let origin = text(headers, header::ORIGIN);
    match server.guard.admits(host.as_deref(), origin.as_deref()) {
        Ok(()) => next.run(request).await,
        Err(refusal) => {
            log::refused(host.as_deref(), origin.as_deref(), &refusal.0);
            (
                StatusCode::FORBIDDEN,
                html(page::failure("Not answered", &refusal.0)),
            )
                .into_response()
        }
    }
}

fn text(headers: &HeaderMap, name: header::HeaderName) -> Option<String> {
    headers
        .get(name)?
        .to_str()
        .ok()
        .map(std::string::ToString::to_string)
}

/// The header a request names a part of a page with.
///
/// Its own name rather than a shape of `Accept`, because this is not content
/// negotiation: what comes back is the same `text/html` either way, and what
/// the header settles is how much of it. A media type saying so would be a type
/// nothing else on the web means anything by.
pub(crate) const PART: &str = "x-noda-fragment";

/// How much of a page a request will use.
///
/// **A page here is one screen's worth of chrome around one changing region**,
/// and the enhancement layer only ever keeps the region: a swap takes
/// `.pane.read` out of a note and drops the rest, which was measured at 48 of
/// the 52 KB a note page weighs. Every one of those fetches asked for a whole
/// page and threw away nine tenths of it, on the round trip a reader is waiting
/// through.
///
/// So a request may say which part it will use, and the server sends that part.
/// Three rules keep it from becoming a second interface:
///
/// * **The part is a substring of the page.** Both come out of one function in
///   `page.rs` — the whole page is built from the same string the fragment is —
///   so there is no shorter rendering to drift from the longer one. `page.rs`
///   tests it by containment.
/// * **The whole page is always a correct answer.** A name nothing here knows
///   is not an error; it is a request with nothing to shorten, and it gets the
///   page. Every one of these fetches parses what arrives and queries it for
///   the region it wants, so a server that ignored the header entirely would
///   still be answering them — later, never differently.
/// * **Nobody but the script asks.** A reader typing an address, a bookmark, a
///   crawler and a browser with no script all send no such header, and the
///   scriptless page is what they get. This is why `Vary` is on every HTML
///   answer: two answers to one address, told apart by a header, is exactly what
///   that header is for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Part {
    /// The note being read: `.pane.read`, and the name of the tab it is in.
    Read,
    /// The listing's own column, rows and count.
    Index,
    /// Both of the listing's panes: the column, and the pane a note was being
    /// read in. What going back to the listing has to put right.
    Screen,
    /// The rows of a backlinks answer, without the page around them.
    Rows,
    /// The network screen's news, and whether it is still moving.
    News,
}

impl Part {
    /// The name a request calls it by. One vocabulary, written here and read by
    /// `script.rs`, which is what the tests in that file pair against.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Part::Read => "read",
            Part::Index => "index",
            Part::Screen => "screen",
            Part::Rows => "rows",
            Part::News => "news",
        }
    }

    /// Whether this request asked for this part of the page it is about.
    ///
    /// Each route knows the one part it can send, so this is asked rather than
    /// parsed: a note route is not made to have an opinion about what the
    /// network screen calls its news.
    fn wanted(self, headers: &HeaderMap) -> bool {
        headers
            .get(PART)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|said| said == self.name())
    }
}

/// What a handler decided, before it is an HTTP anything.
enum Answer {
    Page(String),
    /// One address per page: an id prefix and a slug both land on the id.
    Elsewhere(String),
    Missing(String, String),
    /// A file the notebook holds, as its bytes. The only answer here that is
    /// not a page noda wrote.
    Held(Held),
}

/// A file on its way out, and the two decisions that go with it.
struct Held {
    bytes: Vec<u8>,
    /// What it is, as far as noda is willing to say.
    kind: &'static str,
    /// Whether the browser may show it where it stands. Only the formats that
    /// cannot carry a script may — see `holding`.
    inline: bool,
    name: String,
}

/// Runs the blocking half of a request and turns what it decided into a response.
///
/// Everything a handler does — opening a repository, walking a directory,
/// reading a file — blocks, and libgit2 offers no other kind. `spawn_blocking`
/// is where that belongs, and it is also the only place a `!Send` `Repository`
/// can be created and dropped without ever crossing an await.
async fn answer<F>(work: F) -> Response
where
    F: FnOnce() -> Result<Answer> + Send + 'static,
{
    match tokio::task::spawn_blocking(work).await {
        Ok(Ok(Answer::Page(body))) => html(body).into_response(),
        Ok(Ok(Answer::Elsewhere(to))) => {
            (StatusCode::SEE_OTHER, [(header::LOCATION, to)]).into_response()
        }
        Ok(Ok(Answer::Missing(heading, detail))) => (
            StatusCode::NOT_FOUND,
            html(page::failure(&heading, &detail)),
        )
            .into_response(),
        Ok(Ok(Answer::Held(held))) => held.into_response(),
        Ok(Err(e)) => {
            log::failed(&e.to_string());
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                html(page::failure("Something went wrong", &e.to_string())),
            )
                .into_response()
        }
        // The blocking pool lost the task: a panic, or a shutdown under way.
        // Nothing useful to say to the reader about it, and nothing they can do
        // — which is exactly why it is worth saying somewhere they are not.
        Err(_) => {
            log::lost();
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                html(page::failure(
                    "Something went wrong",
                    "the request did not finish",
                )),
            )
                .into_response()
        }
    }
}

impl IntoResponse for Held {
    fn into_response(self) -> Response {
        // `filename*=UTF-8''…` and not a bare `filename=`: an attachment's name
        // is the whole of its identity here, and the notebook's own files are
        // allowed to be called `réunion.pdf`.
        let disposition = format!(
            "{}; filename*=UTF-8''{}",
            if self.inline { "inline" } else { "attachment" },
            encoded(&self.name)
        );
        (
            [
                (header::CONTENT_TYPE, self.kind.to_string()),
                // Never let the browser decide it knows better what this is.
                // Everything below rests on the type noda declared.
                (header::X_CONTENT_TYPE_OPTIONS, "nosniff".to_string()),
                (header::CONTENT_DISPOSITION, disposition),
                // A file the notebook holds is not a page: it loads nothing,
                // runs nothing and frames nothing. Said out loud so that a
                // format which turns out to be able to do any of those — SVG is
                // the one everybody finds out about — cannot.
                (
                    header::CONTENT_SECURITY_POLICY,
                    "default-src 'none'; sandbox".to_string(),
                ),
            ],
            self.bytes,
        )
            .into_response()
    }
}

/// A value as a URL takes it: percent-encoded, every byte that is not plainly
/// safe spelled out.
///
/// Two callers, and the wider of the two decides the rule. `filename*` needs a
/// notebook's own `réunion.pdf` to survive a header; a query string needs
/// `tag:"24.04 Dark patterns"` to survive a link on the tags page — quotes,
/// colon, spaces and all. Escaping everything but the unreserved set is correct
/// in both places, and one function is one thing to get right.
pub(crate) fn encoded(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for byte in name.bytes() {
        match byte {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            other => {
                let _ = write!(out, "%{other:02X}");
            }
        }
    }
    out
}

/// What noda is willing to say a file is, and whether a browser may show it
/// where it stands.
///
/// **A list of what may be shown, never of what may not.** An attachment is a
/// file somebody put in a notebook, served from the same origin as every page
/// here — so anything shown inline that can carry a script is a script running
/// on this page. Two formats catch people out: SVG is a document and runs
/// script, and HTML is obviously one. Both are served, and both arrive as a
/// download rather than a view. Anything not named is `application/octet-stream`
/// and a download, which is the direction an unknown format should fall in.
fn holding(name: &str) -> (&'static str, bool) {
    let extension = name
        .rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase())
        .unwrap_or_default();
    match extension.as_str() {
        "png" => ("image/png", true),
        "jpg" | "jpeg" => ("image/jpeg", true),
        "gif" => ("image/gif", true),
        "webp" => ("image/webp", true),
        "avif" => ("image/avif", true),
        // Text cannot execute, and a notebook is full of it: a `.txt` beside a
        // note is the one attachment you want to read without saving it first.
        "txt" | "md" | "csv" | "log" => ("text/plain; charset=utf-8", true),
        "pdf" => ("application/pdf", false),
        "svg" => ("image/svg+xml", false),
        _ => ("application/octet-stream", false),
    }
}

fn html(body: String) -> impl IntoResponse {
    (
        [
            (
                header::CONTENT_TYPE,
                header::HeaderValue::from_static("text/html; charset=utf-8"),
            ),
            // On every HTML answer and not only the ones that can be shortened:
            // what this says is that an address is not the whole of what decided
            // this body, and that is true of a route the moment one of its
            // answers depends on the header — a cache that had kept the fragment
            // would hand it to the next reader who typed the address. Saying it
            // once here means a route added later cannot forget to.
            (header::VARY, header::HeaderValue::from_static(PART)),
            // And the other half of what `asset.rs` serves for a year: a page
            // names the addresses this build wrote, so a kept page is a page
            // that could ask for bytes this build does not have. `no-cache` is
            // "ask first", not "do not keep" — going back still comes out of
            // the browser's own memory, which is where a swap left it.
            (
                header::CACHE_CONTROL,
                header::HeaderValue::from_static("no-cache"),
            ),
        ],
        body,
    )
}

async fn front(State(server): State<Shared>) -> Response {
    answer(move || {
        // A missing pointer is not an error here. `noda init` writes one and
        // every command that opens the active notebook complains when it is
        // gone, but this page names all of them and would be worth reading
        // even on a machine that has never chosen one.
        let active = server.paths.active_notebook().ok();
        let mut books = Vec::new();
        for name in Notebook::list(&server.paths)? {
            let notebook = Notebook::open(&server.paths, &name)?;
            let status = notebook.status()?;
            let (seconds, offset) = notebook.last_commit()?;
            books.push(page::Book {
                active: active.as_deref() == Some(name.as_str()),
                name,
                notes: status.notes,
                files: status.files,
                uncommitted: status.uncommitted,
                // `None` is a notebook with nowhere to sync to, and the row
                // draws that case differently: it is the one that is not a
                // link. Said in the type rather than by reading the words back
                // out of the string this used to be.
                drift: status.remote.as_ref().map(|_| cmd::drifted(status.drift)),
                last: cmd::format_time(seconds, offset)[..cmd::DATE_WIDTH].to_string(),
            });
        }
        Ok(Answer::Page(page::notebooks(&books)))
    })
    .await
}

/// The notebook's `README.md`, rendered, for the pane beside the listing.
///
/// The file `noda readme` writes and a git host shows above the file list, so
/// nothing new is being invented here — it is the page the notebook already has
/// about the whole of itself, put where a screen wide enough to hold two panes
/// has room for it. A notebook without one gets the invitation instead.
///
/// It is sent on every listing view and drawn only above 1024px, which is the
/// same bargain the rest of this layout makes and a much smaller one: a README
/// is a couple of kilobytes against a listing of hundreds of rows.
fn front_page(notebook: &Notebook, book: &str) -> Result<Option<String>> {
    let path = notebook.path.join(crate::notebook::README_FILE);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Ok(None);
    };
    let around = render::Around::of(book, &notebook.named_files()?);
    Ok(Some(render::body(&text, &around)))
}

async fn listing(
    State(server): State<Shared>,
    Path(book): Path<String>,
    request: Request,
) -> Response {
    let typed = parameter(request.uri().query(), "q");
    // Two parts off one route, and the difference is what the reader is doing:
    // narrowing a search leaves the note pane alone, and pressing back out of a
    // note has to put it back. Only the second needs the notebook's front page,
    // and it is a file read — so it is read for the answer that shows it.
    let column = Part::Index.wanted(request.headers());
    let screen = Part::Screen.wanted(request.headers());
    answer(move || {
        let Some(notebook) = open(&server.paths, &book)? else {
            return Ok(missing_notebook(&book));
        };
        let mut notes = notebook.notes()?;
        // The same order `ls` and the browser use, from the same function. An
        // order that came out differently depending on where you asked would be
        // two features wearing one name.
        cmd::sort_notes(&mut notes, cmd::Sort::default());
        // For the chip in the corner of the bar. `drift` and not `status`: the
        // full status walks the working tree twice more — once for the notes
        // this handler has already read and once for a whole `git status` — and
        // none of that is on the chip. What is left is two refs compared, which
        // is what a listing can afford to pay on every visit.
        let drift = cmd::standing(
            notebook.remote_url().as_deref(),
            notebook.drift(&notebook.branch()?)?,
        );

        // A query is being typed, and half a query is what every query looks
        // like on the way to being one. So a query that does not parse says why
        // and leaves the notes alone, exactly as the browser's `/` does — it
        // never empties the screen to punish an unfinished thought.
        //
        // Nothing typed is not half a query, though. `Query::parse` refuses an
        // empty token list — correctly, at a command line, where `noda search`
        // with no argument is a mistake worth naming. Here it is the state every
        // listing starts in, and running it through the parser put a red line
        // about "something to look for" on top of a page nobody had asked
        // anything of yet.
        // Every note becomes a row, and the query decides which of them the page
        // shows rather than which of them the page has. The excluded ones ride
        // along `hidden`, which is what lets the enhancement layer widen a query
        // as well as narrow one — see `page::Row::shown`.
        let mut rows = notes.iter().map(page::Row::of).collect::<Vec<_>>();
        // Read for the pane beside the listing, and only when that pane is going
        // out: the column on its own is going into a page whose other half is a
        // note, and the notebook's front page is not on it.
        let front = if column {
            None
        } else {
            front_page(&notebook, &book)?
        };
        let drawn = |rows: &[page::Row], asked: &page::Asked<'_>| {
            if column {
                page::listing_pane(&book, rows, asked, &drift)
            } else if screen {
                page::listing_screen(&book, rows, asked, &drift, front.as_deref())
            } else {
                page::listing(&book, rows, asked, &drift, front.as_deref())
            }
        };
        let tokens = query::split(&typed);
        if tokens.is_empty() {
            return Ok(Answer::Page(drawn(
                &rows,
                &page::Asked {
                    typed: &typed,
                    ..page::Asked::nothing()
                },
            )));
        }
        // The parsed query outlives the borrow of its grouping, so it is bound
        // here rather than being matched into pieces: `grouping` hands back a
        // slice of the query's own words, and the page is drawn while it is
        // still standing.
        let parsed = Query::parse(&tokens);
        let (grouping, terms, problem) = match &parsed {
            Ok(query) => {
                for (row, file) in rows.iter_mut().zip(notes.iter()) {
                    row.shown = query.matches(&file.id, &file.note);
                }
                (query.grouping(), query.excerpt_terms(), None)
            }
            Err(e) => (&[][..], Vec::new(), Some(e.to_string())),
        };

        Ok(Answer::Page(drawn(
            &rows,
            &page::Asked {
                typed: &typed,
                grouping,
                terms: &terms,
                problem: problem.as_deref(),
            },
        )))
    })
    .await
}

async fn reading(
    State(server): State<Shared>,
    Path((book, key)): Path<(String, String)>,
    headers: HeaderMap,
) -> Response {
    let part = Part::Read.wanted(&headers);
    answer(move || {
        let Some(notebook) = open(&server.paths, &book)? else {
            return Ok(missing_notebook(&book));
        };
        let Ok((id, slug)) = notebook.resolve(&key) else {
            return Ok(Answer::Missing(
                "No such note".to_string(),
                format!("Nothing in {book} is called {key}."),
            ));
        };
        // One address per page. A slug or a shortened id is a way of reaching
        // this note, not a place it lives — and a bookmark taken against a slug
        // would die at the next retitle.
        if key != id {
            return Ok(Answer::Elsewhere(format!("/nb/{book}/n/{id}")));
        }

        let text = std::fs::read_to_string(notebook.note_path(&id, &slug))?;
        let note = Note::parse(&text).map_err(|e| Error::msg(format!("{id}-{slug}.md: {e}")))?;
        let around = render::Around::of(&book, &notebook.named_files()?);
        let reading = page::Reading {
            id,
            slug,
            title: note.title,
            tags: note.tags,
            updated: note.updated,
            rendered: render::body(&note.body, &around),
        };
        // A swap replaces the reading pane and leaves the index pane where it
        // is, so a request for that part is not asking about the notebook at
        // all — and the two refs the chip in the index pane's bar costs are two
        // refs nothing on the screen will change.
        if part {
            return Ok(Answer::Page(page::note_pane(&book, &reading)));
        }
        // For the chip in the index pane's bar, which is the notebook's bar
        // rather than the note's. Two refs compared, the same as the listing
        // pays; the notes themselves are not read, which is the whole point of
        // sending this pane empty.
        let drift = cmd::standing(
            notebook.remote_url().as_deref(),
            notebook.drift(&notebook.branch()?)?,
        );
        Ok(Answer::Page(page::note(&book, &reading, &drift)))
    })
    .await
}

/// Everything the notebook holds that is not a note.
///
/// The count of notes pointing at each one is the same judgement `doctor
/// --links` reports as orphans, made with the same function — `link::targets`
/// — so a file this page says nothing points at is exactly a file `doctor`
/// would name. Saying it twice in two ways would be worse than not saying it.
async fn files(State(server): State<Shared>, Path(book): Path<String>) -> Response {
    answer(move || {
        let Some(notebook) = open(&server.paths, &book)? else {
            return Ok(missing_notebook(&book));
        };
        let (notes, held) = notebook.inventory()?;
        let mut used: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
        for file in &notes {
            for target in crate::link::targets(&file.note.body) {
                *used.entry(target).or_default() += 1;
            }
        }

        let mut rows = Vec::new();
        for name in held {
            let size = std::fs::metadata(notebook.path.join(&name))
                .map(|meta| meta.len())
                .unwrap_or_default();
            let (kind, _) = holding(&name);
            rows.push(page::Held {
                used: used.get(&name).copied().unwrap_or_default(),
                name,
                size,
                // Without the parameters: `text/plain; charset=utf-8` is what
                // the download says because a browser needs the encoding, and
                // `text/plain` is what the row says because a reader does not.
                kind: kind.split(';').next().unwrap_or(kind).to_string(),
            });
        }
        Ok(Answer::Page(page::files(&book, &rows)))
    })
    .await
}

/// Every tag in the notebook, and how many notes carry it.
///
/// `notebook::tag_tally` counts and orders it, which is where the browser's tag
/// screen gets the same list. Nothing is decided here — this hands it a walk of
/// the notes and hands the answer to a page.
async fn tags(State(server): State<Shared>, Path(book): Path<String>) -> Response {
    answer(move || {
        let Some(notebook) = open(&server.paths, &book)? else {
            return Ok(missing_notebook(&book));
        };
        let tallies = crate::notebook::tag_tally(&notebook.notes()?)
            .into_iter()
            .map(|(tag, notes)| page::Tally { tag, notes })
            .collect::<Vec<_>>();
        Ok(Answer::Page(page::tags(&book, &tallies)))
    })
    .await
}

/// Everything in the notebook that is not done.
///
/// The same list `noda todo` prints, from the same two functions: `todo::items`
/// finds the boxes and `todo::order` puts them in order. What this adds is the
/// note's title beside each one — a listing on a phone has room for the words a
/// terminal spends on a filename, and "which note is this in" is better answered
/// by the title than by the slug.
///
/// **`cmd::today` decides what is late, and it is the local date.** Nobody
/// writes `due:2026-08-20` meaning UTC. East of UTC an item that went overdue at
/// midnight would otherwise stay unmarked until morning, which is exactly when a
/// todo list is read.
async fn todo(State(server): State<Shared>, Path(book): Path<String>) -> Response {
    answer(move || {
        let Some(notebook) = open(&server.paths, &book)? else {
            return Ok(missing_notebook(&book));
        };
        let today = cmd::today()?;
        let mut found = Vec::new();
        for file in notebook.notes()? {
            for item in crate::todo::items(&file.note.body) {
                found.push((
                    file.id.clone(),
                    file.slug.clone(),
                    file.note.title.clone(),
                    item,
                ));
            }
        }
        found.sort_by(|(_, left_slug, _, left), (_, right_slug, _, right)| {
            crate::todo::order((left_slug, left), (right_slug, right))
        });

        let tasks = found
            .into_iter()
            .map(|(id, _, title, item)| page::Task {
                overdue: item.overdue(&today),
                id,
                title,
                text: item.text,
                due: item.due,
            })
            .collect::<Vec<_>>();
        Ok(Answer::Page(page::todo(&book, &tasks)))
    })
    .await
}

/// Where the notebook stands against its remote.
///
/// **Nothing here touches the network**, which is the same line `noda status`
/// draws: the drift is measured against what the last fetch left behind, so the
/// screen answers instantly on a train and the three buttons under it are the
/// only things that go out. A page that quietly fetched before drawing itself
/// would be a page that hangs, and it would make the Pull button a lie about
/// what pressing it does.
async fn status(
    State(server): State<Shared>,
    Path(book): Path<String>,
    headers: HeaderMap,
) -> Response {
    let part = Part::News.wanted(&headers);
    answer(move || {
        let Some(notebook) = open(&server.paths, &book)? else {
            return Ok(missing_notebook(&book));
        };
        let status = notebook.status()?;
        let report = server.errands.report(&book);
        let errand = report.as_ref().map(|report| {
            let failed = matches!(report.outcome, Some(work::Outcome::Failed(_)));
            page::Errand {
                doing: report.errand.doing(),
                done: if failed {
                    report.errand.stuck()
                } else {
                    report.errand.done()
                },
                said: match &report.outcome {
                    None => None,
                    Some(work::Outcome::Went(said) | work::Outcome::Failed(said)) => {
                        Some(said.as_str())
                    }
                },
                failed,
                seconds: report.took.as_secs(),
            }
        });
        let standing = page::Standing {
            branch: status.branch.clone(),
            notes: status.notes,
            files: status.files,
            uncommitted: status.uncommitted,
            remote: status.remote.clone(),
            drift: cmd::standing(status.remote.as_deref(), status.drift),
            problems: status
                .problems
                .iter()
                .map(|(kind, subjects)| kind.describe(subjects.len()))
                .collect(),
        };
        Ok(Answer::Page(if part {
            page::standing_main(&book, &standing, errand.as_ref())
        } else {
            page::standing(&book, &standing, errand.as_ref())
        }))
    })
    .await
}

/// Starts one of the three, and answers before it finishes.
///
/// The request does not wait, and the reader is sent back to the screen that
/// says what is happening — so what they are holding afterwards is a `GET`, and
/// the reload a slow network invites cannot start a second push.
///
/// Pressing again while one is running is not refused with an error. It is
/// somebody who could not tell whether the first press landed, and the screen
/// they are sent to is the answer to that question.
async fn errand(
    State(server): State<Shared>,
    Path((book, which)): Path<(String, String)>,
) -> Response {
    answer(move || {
        let Some(errand) = work::Errand::of(&which) else {
            return Ok(Answer::Missing(
                "No such errand".to_string(),
                format!("There is nothing called {which} to do to a notebook."),
            ));
        };
        if !Notebook::exists(&server.paths, &book) {
            return Ok(missing_notebook(&book));
        }
        if server.errands.begin(&book, errand) {
            let server = Arc::clone(&server);
            let book = book.clone();
            // A thread of its own, and not the blocking pool: that pool exists
            // for work a request is waiting on, and this is the one piece of
            // work here that no request waits on. It opens the notebook itself
            // because a `Repository` cannot cross a thread, and it takes the
            // write lock because a fetch that lands mid-commit is the collision
            // the lock is for.
            std::thread::spawn(move || {
                let outcome = {
                    let writing = server.writing.of(&book);
                    let _writing = writing.lock();
                    work::work(errand, Notebook::open(&server.paths, &book))
                };
                server.errands.finish(&book, outcome);
            });
        }
        Ok(Answer::Elsewhere(format!("/nb/{}/status", encoded(&book))))
    })
    .await
}

/// What links to a note.
///
/// `backlinks_to_note` answers it, which is what `noda backlinks` asks too — and
/// the match is on the id in the destination rather than on the filename, so the
/// answer survives a retitle. That is the whole reason this is worth a screen:
/// after `mv`, every Markdown renderer sees a broken link where noda still sees
/// an unambiguous one.
async fn note_backlinks(
    State(server): State<Shared>,
    Path((book, key)): Path<(String, String)>,
    headers: HeaderMap,
) -> Response {
    let part = Part::Rows.wanted(&headers);
    answer(move || {
        let (notebook, id, slug) = match aim(&server.paths, &book, &key, "/backlinks")? {
            Aimed::At(notebook, id, slug) => (notebook, id, slug),
            Aimed::Missing(answer) => return Ok(answer),
        };
        let note = Note::parse(&std::fs::read_to_string(notebook.note_path(&id, &slug))?)
            .map_err(|e| Error::msg(format!("{id}-{slug}.md: {e}")))?;
        let rows = notebook
            .backlinks_to_note(&id)?
            .iter()
            .map(page::Row::of)
            .collect::<Vec<_>>();
        let subject = page::Subject {
            what: note.title,
            at: format!("/nb/{book}/n/{id}"),
            mono: false,
        };
        Ok(Answer::Page(if part {
            page::backlinks_rows(&book, &subject, &rows)
        } else {
            page::backlinks(&book, &subject, &rows)
        }))
    })
    .await
}

/// What links to one of the notebook's files.
///
/// The same question as above asked of a different kind of thing, which is why
/// `noda backlinks` takes either. The difference is that a file has no id to
/// fall back on: its name is the whole of its identity, and this is the screen
/// that shows what a `file mv` without `--update-links` would leave pointing at
/// nothing.
///
/// The name goes through `link::target` first, exactly as the download does. It
/// is the same reader-supplied path, and the fact that this one only counts
/// notes rather than opening the file is not a reason to check it less.
async fn file_backlinks(
    State(server): State<Shared>,
    Path((book, name)): Path<(String, String)>,
    headers: HeaderMap,
) -> Response {
    let part = Part::Rows.wanted(&headers);
    answer(move || {
        let Some(notebook) = open(&server.paths, &book)? else {
            return Ok(missing_notebook(&book));
        };
        let nothing = || {
            Ok(Answer::Missing(
                "No such file".to_string(),
                format!("{book} holds no file called {name}."),
            ))
        };
        let Some(path) = crate::link::target(&name) else {
            return nothing();
        };
        // A note is not a file here, the same way it is not one at `/f/`: it has
        // a page of its own, and its backlinks are on that page's own screen.
        if note::names_a_note(&path) || !notebook.path.join(&path).is_file() {
            return nothing();
        }

        let rows = notebook
            .backlinks_to_file(&path)?
            .iter()
            .map(page::Row::of)
            .collect::<Vec<_>>();
        let subject = page::Subject {
            at: format!("/nb/{}/files", encoded(&book)),
            what: path,
            mono: true,
        };
        Ok(Answer::Page(if part {
            page::backlinks_rows(&book, &subject, &rows)
        } else {
            page::backlinks(&book, &subject, &rows)
        }))
    })
    .await
}

/// One of those files, as its bytes.
///
/// **The only place noda opens a path a reader named**, which is why the path
/// goes through `link::target` before anything touches the disk: it is what
/// decides that `../../.ssh/id_rsa` names nothing in the notebook. A note is
/// never served from here — a `.md` file has a page of its own, and answering
/// for it here would be a second, unrendered way to read a note.
async fn held(
    State(server): State<Shared>,
    Path((book, name)): Path<(String, String)>,
) -> Response {
    answer(move || {
        let Some(notebook) = open(&server.paths, &book)? else {
            return Ok(missing_notebook(&book));
        };
        let nothing = || {
            Ok(Answer::Missing(
                "No such file".to_string(),
                format!("{book} holds no file called {name}."),
            ))
        };
        // Resolved as a link destination because that is what it was: the page
        // that sent the reader here wrote this URL from a destination in a
        // note's body, and the same rules have to answer both.
        let Some(path) = crate::link::target(&name) else {
            return nothing();
        };
        let on_disk = notebook.path.join(&path);
        // Exactly as the notebook itself decides what is a note: a stem that
        // splits into an id and a slug, case and all. The suffix on its own is
        // not the test — `README.md` is a file the files page lists and offers,
        // and refusing it here would be a link the reader can see and cannot
        // follow. A case-insensitive test would go wrong the other way, refusing
        // a file called `NOTES.MD` that the notebook holds as an attachment.
        if note::names_a_note(&path) || !on_disk.is_file() {
            return nothing();
        }

        let (kind, inline) = holding(&path);
        Ok(Answer::Held(Held {
            bytes: std::fs::read(&on_disk)?,
            kind,
            inline,
            name: path,
        }))
    })
    .await
}

async fn new_form(State(server): State<Shared>, Path(book): Path<String>) -> Response {
    answer(move || {
        if open(&server.paths, &book)?.is_none() {
            return Ok(missing_notebook(&book));
        }
        Ok(Answer::Page(page::composing(
            &book,
            &page::Draft::default(),
            None,
        )))
    })
    .await
}

async fn new_note(
    State(server): State<Shared>,
    Path(book): Path<String>,
    form: String,
) -> Response {
    answer(move || {
        let Some(notebook) = open(&server.paths, &book)? else {
            return Ok(missing_notebook(&book));
        };
        let draft = page::Draft {
            title: parameter(Some(&form), "title"),
            tags: parameter(Some(&form), "tags"),
            body: parameter(Some(&form), "body"),
        };
        let tags = query::split(&draft.tags);
        let title = draft.title.trim();
        let title = (!title.is_empty()).then_some(title);

        let writing = server.writing.of(&book);
        let _writing = writing.lock();
        // Which ids existed a moment ago, so the new one can be told from them.
        // Not by reading the id out of what `add` printed: that answer is
        // written for a person, and a caller that parses it has quietly made the
        // wording of a message into an interface. The browser answers the same
        // question the same way.
        let before = notebook.taken_ids()?;
        if let Err(e) = cmd::add_in(&notebook, title, &draft.body, &tags) {
            return Ok(Answer::Page(page::composing(
                &book,
                &draft,
                Some(&e.to_string()),
            )));
        }
        let after = notebook.taken_ids()?;
        match after.difference(&before).next() {
            Some(id) => Ok(back_to_note(&book, id)),
            // It was added and something took it away between the two reads.
            // Nothing is wrong with the note; there is just nowhere to send you.
            None => Ok(Answer::Elsewhere(format!("/nb/{book}"))),
        }
    })
    .await
}

async fn edit_form(
    State(server): State<Shared>,
    Path((book, key)): Path<(String, String)>,
) -> Response {
    answer(move || {
        let (notebook, id, slug) = match aim(&server.paths, &book, &key, "/edit")? {
            Aimed::At(notebook, id, slug) => (notebook, id, slug),
            Aimed::Missing(answer) => return Ok(answer),
        };
        let path = notebook.note_path(&id, &slug);
        let note = Note::parse(&std::fs::read_to_string(&path)?)
            .map_err(|e| Error::msg(format!("{id}-{slug}.md: {e}")))?;
        Ok(Answer::Page(page::editing(
            &book,
            &page::About::of(&id, &slug, &note.title),
            &note.body,
            &fingerprint(&path)?,
            None,
        )))
    })
    .await
}

async fn edit_note(
    State(server): State<Shared>,
    Path((book, key)): Path<(String, String)>,
    form: String,
) -> Response {
    answer(move || {
        let (notebook, id, slug) = match aim(&server.paths, &book, &key, "/edit")? {
            Aimed::At(notebook, id, slug) => (notebook, id, slug),
            Aimed::Missing(answer) => return Ok(answer),
        };
        let body = parameter(Some(&form), "body");
        let was = parameter(Some(&form), "fingerprint");

        let writing = server.writing.of(&book);
        let _writing = writing.lock();
        let path = notebook.note_path(&id, &slug);
        let now = fingerprint(&path)?;
        if now != was {
            // Nothing has been written. What the reader typed is handed back to
            // them on top of what is on disk, because the only thing worse than
            // overwriting somebody's work is losing the work of the person
            // standing in front of you to avoid it.
            let theirs = Note::parse(&std::fs::read_to_string(&path)?)
                .map_err(|e| Error::msg(format!("{id}-{slug}.md: {e}")))?;
            return Ok(Answer::Page(page::clashed(
                &book,
                &page::About::of(&id, &slug, &theirs.title),
                &theirs.body,
                &body,
                &now,
            )));
        }
        match cmd::rewrite_in(&notebook, &id, &body, cmd::Touch::Stamp) {
            Ok(_) => Ok(back_to_note(&book, &id)),
            Err(e) => Ok(Answer::Page(page::editing(
                &book,
                &page::About::of(&id, &slug, ""),
                &body,
                &now,
                Some(&e.to_string()),
            ))),
        }
    })
    .await
}

async fn rename_form(
    State(server): State<Shared>,
    Path((book, key)): Path<(String, String)>,
) -> Response {
    answer(move || {
        let (notebook, id, slug) = match aim(&server.paths, &book, &key, "/rename")? {
            Aimed::At(notebook, id, slug) => (notebook, id, slug),
            Aimed::Missing(answer) => return Ok(answer),
        };
        let note = Note::parse(&std::fs::read_to_string(notebook.note_path(&id, &slug))?)
            .map_err(|e| Error::msg(format!("{id}-{slug}.md: {e}")))?;
        Ok(Answer::Page(page::renaming(
            &book,
            &page::About::of(&id, &slug, &note.title),
            &note.title,
            None,
        )))
    })
    .await
}

async fn rename_note(
    State(server): State<Shared>,
    Path((book, key)): Path<(String, String)>,
    form: String,
) -> Response {
    answer(move || {
        let (notebook, id, slug) = match aim(&server.paths, &book, &key, "/rename")? {
            Aimed::At(notebook, id, slug) => (notebook, id, slug),
            Aimed::Missing(answer) => return Ok(answer),
        };
        let title = parameter(Some(&form), "title");
        let writing = server.writing.of(&book);
        let _writing = writing.lock();
        // `update_links: false`, the same call the browser's `m` makes. Rewriting
        // the prose of notes nobody pointed at is a thing to ask for out loud,
        // and there is no way to ask for it here yet.
        match cmd::mv_in(&notebook, &id, &title, false, cmd::Touch::Stamp) {
            Ok(_) => Ok(back_to_note(&book, &id)),
            Err(e) => Ok(Answer::Page(page::renaming(
                &book,
                &page::About::of(&id, &slug, &title),
                &title,
                Some(&e.to_string()),
            ))),
        }
    })
    .await
}

async fn tags_form(
    State(server): State<Shared>,
    Path((book, key)): Path<(String, String)>,
) -> Response {
    answer(move || {
        let (notebook, id, slug) = match aim(&server.paths, &book, &key, "/tags")? {
            Aimed::At(notebook, id, slug) => (notebook, id, slug),
            Aimed::Missing(answer) => return Ok(answer),
        };
        let note = Note::parse(&std::fs::read_to_string(notebook.note_path(&id, &slug))?)
            .map_err(|e| Error::msg(format!("{id}-{slug}.md: {e}")))?;
        Ok(Answer::Page(page::tagging(
            &book,
            &page::About::of(&id, &slug, &note.title),
            &note.tags,
            None,
        )))
    })
    .await
}

async fn tag_note(
    State(server): State<Shared>,
    Path((book, key)): Path<(String, String)>,
    form: String,
) -> Response {
    answer(move || {
        let (notebook, id, slug) = match aim(&server.paths, &book, &key, "/tags")? {
            Aimed::At(notebook, id, slug) => (notebook, id, slug),
            Aimed::Missing(answer) => return Ok(answer),
        };
        let note = Note::parse(&std::fs::read_to_string(notebook.note_path(&id, &slug))?)
            .map_err(|e| Error::msg(format!("{id}-{slug}.md: {e}")))?;

        // A ticked box arrives, an unticked one does not, so what the form says
        // is which tags survived — and the change is the difference between that
        // and what the note holds. Worked out here rather than asked of the
        // reader: `+work -q3` is the command line's way of saying it, and a
        // command line is where somebody has a keyboard.
        let kept = parameters(&form, "keep");
        let mut changes: Vec<String> = note
            .tags
            .iter()
            .filter(|tag| !kept.contains(tag))
            .map(|tag| format!("-{tag}"))
            .collect();
        for added in query::split(&parameter(Some(&form), "add")) {
            changes.push(format!("+{added}"));
        }
        if changes.is_empty() {
            return Ok(back_to_note(&book, &id));
        }

        let writing = server.writing.of(&book);
        let _writing = writing.lock();
        match cmd::tag_in(&notebook, &id, &changes, cmd::Touch::Stamp) {
            Ok(_) => Ok(back_to_note(&book, &id)),
            Err(e) => Ok(Answer::Page(page::tagging(
                &book,
                &page::About::of(&id, &slug, &note.title),
                &note.tags,
                Some(&e.to_string()),
            ))),
        }
    })
    .await
}

async fn delete_form(
    State(server): State<Shared>,
    Path((book, key)): Path<(String, String)>,
) -> Response {
    answer(move || {
        let (notebook, id, slug) = match aim(&server.paths, &book, &key, "/delete")? {
            Aimed::At(notebook, id, slug) => (notebook, id, slug),
            Aimed::Missing(answer) => return Ok(answer),
        };
        let note = Note::parse(&std::fs::read_to_string(notebook.note_path(&id, &slug))?)
            .map_err(|e| Error::msg(format!("{id}-{slug}.md: {e}")))?;
        Ok(Answer::Page(page::deleting(
            &book,
            &page::About::of(&id, &slug, &note.title),
        )))
    })
    .await
}

async fn delete_note(
    State(server): State<Shared>,
    Path((book, key)): Path<(String, String)>,
) -> Response {
    answer(move || {
        let (notebook, id, _slug) = match aim(&server.paths, &book, &key, "/delete")? {
            Aimed::At(notebook, id, slug) => (notebook, id, slug),
            Aimed::Missing(answer) => return Ok(answer),
        };
        let writing = server.writing.of(&book);
        let _writing = writing.lock();
        cmd::rm_in(&notebook, &id)?;
        // Not to the note: it is gone. The listing is where you were before you
        // opened it.
        Ok(Answer::Elsewhere(format!("/nb/{book}")))
    })
    .await
}

/// A note the caller can act on: the notebook open, and the note located.
///
/// Every write handler starts here, and every one of them needs the same three
/// refusals first — no such notebook, no such note, and an address that is not
/// the note's own. Written once so a route added later cannot get two of the
/// three right.
enum Aimed {
    At(Notebook, String, String),
    Missing(Answer),
}

fn aim(paths: &Paths, book: &str, key: &str, tail: &str) -> Result<Aimed> {
    let Some(notebook) = open(paths, book)? else {
        return Ok(Aimed::Missing(missing_notebook(book)));
    };
    let Ok((id, slug)) = notebook.resolve(key) else {
        return Ok(Aimed::Missing(Answer::Missing(
            "No such note".to_string(),
            format!("Nothing in {book} is called {key}."),
        )));
    };
    if key != id {
        return Ok(Aimed::Missing(Answer::Elsewhere(format!(
            "/nb/{book}/n/{id}{tail}"
        ))));
    }
    Ok(Aimed::At(notebook, id, slug))
}

/// Where a change goes when it worked: back to the note it changed.
///
/// `303` and not `200`, so a reload does not offer to send the form again. This
/// is the whole of why a write is a `POST` followed by a redirect rather than a
/// page in its own right.
fn back_to_note(book: &str, id: &str) -> Answer {
    Answer::Elsewhere(format!("/nb/{book}/n/{id}"))
}

/// The notebook by that name, or `None` when there is not one.
///
/// Told apart from a notebook that exists and will not open: the first is a
/// wrong address and the second is something wrong with the notebook, and
/// answering 404 to the second would send somebody looking for a typo.
fn open(paths: &Paths, book: &str) -> Result<Option<Notebook>> {
    if !Notebook::exists(paths, book) {
        return Ok(None);
    }
    Notebook::open(paths, book).map(Some)
}

fn missing_notebook(book: &str) -> Answer {
    Answer::Missing(
        "No such notebook".to_string(),
        format!("There is no notebook called {book} on this machine."),
    )
}

/// One named parameter out of a query string.
///
/// Hand-written, and the reason is the same one the rest of this file gives for
/// its dependencies: pulling in serde's derive to read a single `q=` would be a
/// proc macro and a build-time cost for twenty lines. The decoding builds bytes
/// and converts once at the end, which is what keeps a multi-byte character
/// spread over three `%xx` escapes from being cut in half.
fn parameter(query: Option<&str>, name: &str) -> String {
    let Some(query) = query else {
        return String::new();
    };
    for pair in query.split('&') {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        if decode(key) == name {
            return decode(value);
        }
    }
    String::new()
}

/// Every value sent under `name`, in the order they were sent.
///
/// A form with several boxes ticked sends the name once per box, which is how
/// the tags screen says which tags are still wanted. There is no other way for a
/// form to say "these, and not those" — a checkbox that is not ticked is not
/// sent at all, so what arrives is a list of what survived rather than a list of
/// changes.
fn parameters(body: &str, name: &str) -> Vec<String> {
    body.split('&')
        .filter_map(|pair| {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            (decode(key) == name).then(|| decode(value))
        })
        .collect()
}

fn decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        match bytes[at] {
            // A form sends a space this way, and has since before anyone here
            // was writing HTTP.
            b'+' => {
                out.push(b' ');
                at += 1;
            }
            b'%' if at + 2 < bytes.len() => {
                if let Some(byte) = hex(bytes[at + 1], bytes[at + 2]) {
                    out.push(byte);
                    at += 3;
                } else {
                    // Not an escape after all. A stray `%` is what somebody
                    // typed, so it stays a stray `%`.
                    out.push(b'%');
                    at += 1;
                }
            }
            byte => {
                out.push(byte);
                at += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex(high: u8, low: u8) -> Option<u8> {
    let digit = |c: u8| match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    };
    Some(digit(high)? << 4 | digit(low)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_query_string_gives_up_its_parameter() {
        assert_eq!(parameter(Some("q=budget"), "q"), "budget");
        assert_eq!(parameter(Some("a=1&q=budget&b=2"), "q"), "budget");
        assert_eq!(parameter(Some("a=1"), "q"), "");
        assert_eq!(parameter(None, "q"), "");
        // A form sends an empty field rather than leaving it out.
        assert_eq!(parameter(Some("q="), "q"), "");
    }

    /// What a phone actually sends when the query has a tag with a space in it:
    /// the quotes and the colon are escaped, and the space is a `+`.
    #[test]
    fn a_quoted_tag_survives_the_trip() {
        assert_eq!(
            parameter(Some("q=tag%3A%2224.04+Dark+patterns%22"), "q"),
            "tag:\"24.04 Dark patterns\""
        );
    }

    /// The reason the decoder collects bytes and converts once: a CJK character
    /// arrives as three escapes, and converting each on its own would produce
    /// three replacement characters.
    #[test]
    fn a_multi_byte_character_arrives_whole() {
        assert_eq!(parameter(Some("q=%E7%AD%86%E8%A8%98"), "q"), "筆記");
    }

    #[test]
    fn a_stray_percent_is_left_as_typed() {
        assert_eq!(parameter(Some("q=100%+of+it"), "q"), "100% of it");
        assert_eq!(parameter(Some("q=%zz"), "q"), "%zz");
    }
}
