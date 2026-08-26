//! `noda web` — the notebook over HTTP, for reading and writing it from a phone.
//!
//! **Read through `notebook`, write through `cmd`**: a third renderer over the
//! data the other two agree about, not a wrapper around either.
//!
//! Three things not obvious from the code:
//!
//! - **A notebook is named in the URL, never taken from the active pointer.**
//!   That pointer belongs to a shell session, and a tab that changed which
//!   notebook it showed because of something in a terminal is worse than a
//!   longer URL.
//!
//! - **A note is addressed by id, and everything else redirects to it.** The
//!   slug follows the title, so a bookmark written against it dies at the next
//!   rename. One page, one address.
//!
//! - **Every handler opens the notebook itself, inside `spawn_blocking`.**
//!   `git2::Repository` is `!Send`, which reads like a restriction and is the
//!   design: one request, one handle, and a slow walk on one is not a stall on
//!   the others.

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
use tower_http::compression::CompressionLayer;
use tower_http::compression::predicate::{DefaultPredicate, NotForContentType, Predicate};

use crate::note::{self, Note};
use crate::notebook::Notebook;
use crate::query::{self, Query};
use crate::{Error, Paths, Result, cmd};

/// What every handler is given.
struct Server {
    paths: Paths,
    guard: guard::Guard,
    /// Held for any request that writes, by the notebook it writes to.
    ///
    /// Reads happen at once; writes to one notebook cannot. Two commits racing
    /// meet at `index.lock`, and what comes back is libgit2 saying a file
    /// exists — no help at all to somebody who pressed Save.
    ///
    /// `std::sync::Mutex` and not tokio's, because it is only taken off the
    /// async threads: a lock held across an await is a lock held while a request
    /// does nothing.
    ///
    /// It does **not** lock the notebook against the world — a terminal in
    /// another window always could be writing, which the fingerprint is for.
    writing: Locks,
    /// The one piece of state outliving a request, because the errand does.
    errands: work::Errands,
}

/// One write lock per notebook.
///
/// A single lock over everything froze Save on every *other* notebook whenever
/// one notebook's remote went quiet. `index.lock` is a file inside one
/// repository, so the lock belongs where the collision is.
#[derive(Default)]
struct Locks(std::sync::Mutex<std::collections::BTreeMap<String, Arc<std::sync::Mutex<()>>>>);

impl Locks {
    /// An `Arc` out rather than a guard: a guard borrows the map, and the map
    /// has to be free the moment this returns, or one notebook's slow push holds
    /// what every other notebook goes through to find its own lock.
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

/// The optimistic lock: a form carries the fingerprint the note had when the
/// page was drawn, so an edit begun on a phone cannot flatten one made at a
/// terminal since.
///
/// **The blob id and not the `updated` stamp**, because `--no-touch` exists so
/// content can change without `updated` moving — a marker that fails during a
/// session of small corrections fails in the situation it exists for.
fn fingerprint(path: &std::path::Path) -> Result<String> {
    Ok(git2::Oid::hash_file(git2::ObjectType::Blob, path)?.to_string())
}

type Shared = Arc<Server>;

/// Serves until it is asked to stop.
///
/// The address is printed rather than returned, unlike every other command: this
/// one does not finish while anybody is using it, and the URL is needed now. The
/// `String` it answers with is always empty.
///
/// **Stopping is three steps and the order is the whole of it.** A signal closes
/// the listener; the requests in flight are answered, which finishes a commit a
/// browser is waiting on; then `settle` waits for the work that outlives a
/// request. Only then does the process end, with a `0` — a supervisor is
/// entitled to tell a clean stop from a crash.
pub fn serve(paths: &Paths, listen: &str, allow: &[String], format: log::Format) -> Result<String> {
    // Before the bind, or a failure to listen is what the log misses first.
    log::start(format);
    let server = Arc::new(Server {
        paths: paths.clone(),
        guard: guard::Guard::new(allow),
        writing: Locks::default(),
        errands: work::Errands::default(),
    });

    // By hand rather than `#[tokio::main]`, because every other clap arm is
    // ordinary blocking code. I/O only — which is also what makes the signals
    // arrive, tokio's signal driver being part of its I/O driver. No timers:
    // nothing here waits for a length of time, the shutdown included.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_io()
        .build()?;

    runtime.block_on({
        let server = Arc::clone(&server);
        async move {
            // So a signal in the first millisecond has somewhere to go.
            let stop = Stop::listen()?;
            let listener = tokio::net::TcpListener::bind(listen)
                .await
                .map_err(|e| Error::msg(format!("could not listen on {listen}: {e}")))?;
            let at = listener.local_addr()?;
            println!("noda is at http://{at}");
            // The difference between a notebook on one machine and one on the
            // network is not something to find out about afterwards.
            if !at.ip().is_loopback() {
                println!("reachable from the network — there is no password on it");
            }
            axum::serve(listener, router(Arc::clone(&server)))
                .with_graceful_shutdown(asked_to_stop(stop, server))
                .await?;
            Ok::<(), Error>(())
        }
    })?;

    // Nothing is listening and every in-flight request is answered. What can
    // still run is an errand, which never was a request — see
    // `work::Errands::settle`.
    for (book, errand) in server.errands.running() {
        println!(
            "waiting for {} in {book} — signal again to leave it unfinished",
            errand.name()
        );
    }
    let left = server.errands.settle();
    if left.is_empty() {
        return Ok(String::new());
    }
    // Signalled twice: the process ends with an errand halfway through, which
    // is what the wait exists to avoid, so this is a failure rather than a `0`.
    Err(Error::msg(format!(
        "left {} unfinished",
        left.iter()
            .map(|(book, errand)| format!("{} in {book}", errand.name()))
            .collect::<Vec<_>>()
            .join(", ")
    )))
}

/// Resolves on the first signal that means stop, and arms the second.
///
/// What it does *after* resolving is easy to miss: it hands the same streams to
/// a task waiting for the next one. Arming that any later leaves a window where
/// a signal reaches nobody — exactly when somebody presses again because nothing
/// seems to be happening.
///
/// **A second signal cuts short the wait, not the shutdown.** It ends `settle`,
/// which is unbounded because a push to a host that stopped answering is minutes
/// of libgit2 on a socket. The requests in flight are bounded by what a request
/// does.
///
/// Its line goes to stdout beside the startup's: this is the command talking
/// about itself rather than an event about a request.
async fn asked_to_stop(mut stop: Stop, server: Shared) {
    println!("{} — finishing what is in flight", stop.next().await);
    tokio::spawn(async move {
        println!("{} again — not waiting", stop.next().await);
        server.errands.abandon();
    });
}

/// The signals that mean stop, listened for once and consumed twice.
///
/// **Both, and the pair is the point.** `SIGINT` is Ctrl-C; `SIGTERM` is every
/// supervisor, and the one that arrives when nobody is watching — so handling
/// only the first is being careful exactly when a person could see it.
///
/// Each is named in what is printed, because `SIGTERM` in a container's log is
/// the difference between "the orchestrator stopped it" and "it fell over".
#[cfg(unix)]
struct Stop {
    interrupt: tokio::signal::unix::Signal,
    terminate: tokio::signal::unix::Signal,
}

#[cfg(unix)]
impl Stop {
    fn listen() -> std::io::Result<Stop> {
        use tokio::signal::unix::{SignalKind, signal};

        Ok(Stop {
            interrupt: signal(SignalKind::interrupt())?,
            terminate: signal(SignalKind::terminate())?,
        })
    }

    /// Streams and not `tokio::signal::ctrl_c()`, because this is awaited twice
    /// and a `Signal` holds its registration across both — so one landing
    /// between them is remembered rather than delivered to nobody.
    async fn next(&mut self) -> &'static str {
        tokio::select! {
            _ = self.interrupt.recv() => "SIGINT",
            _ = self.terminate.recv() => "SIGTERM",
        }
    }
}

/// Ctrl-C alone, where there is no `SIGTERM` to have an opinion about.
#[cfg(not(unix))]
struct Stop;

#[cfg(not(unix))]
impl Stop {
    fn listen() -> std::io::Result<Stop> {
        Ok(Stop)
    }

    async fn next(&mut self) -> &'static str {
        let _ = tokio::signal::ctrl_c().await;
        "Ctrl-C"
    }
}

fn router(server: Shared) -> Router {
    Router::new()
        .route("/", get(front))
        // Inside the guard: a page the guard refuses should not be able to draw
        // itself either.
        .route("/a/{file}", get(held_asset))
        .route("/nb/{book}", get(listing))
        .route("/nb/{book}/files", get(files))
        // The three screens about the notebook rather than one note. Each walks
        // every body, which is why none is a column on the listing.
        .route("/nb/{book}/tags", get(tags))
        .route("/nb/{book}/todo", get(todo))
        // Where the notebook stands, and one segment down the three things that
        // change it: a `GET` for the screen, a `POST` each for the errands, so a
        // reload is a question rather than a second push.
        .route("/nb/{book}/status", get(status))
        .route("/nb/{book}/status/{errand}", post(errand))
        // One segment, not a wildcard: a route matching `a/b` invites a path
        // assembled out of pieces nobody checked.
        .route("/nb/{book}/f/{name}", get(held))
        .route("/nb/{book}/f/{name}/backlinks", get(file_backlinks))
        .route("/nb/{book}/new", get(new_form).post(new_note))
        .route("/nb/{book}/n/{key}", get(reading))
        .route("/nb/{book}/n/{key}/backlinks", get(note_backlinks))
        // One shape: `GET` shows the form, `POST` does the thing, both at the
        // address of the thing. A `GET` that changed something would be a link a
        // prefetcher could press.
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
        // Outside the guard: an axum layer covers the routes declared before
        // it, and this one is meant not to be covered. See `health`.
        .route("/health", get(health))
        // **Compression, and the two content types it is kept away from.**
        //
        // Inside the log's layer, so what the log times is what the reader waits
        // for: an answer is not finished until it is compressed.
        //
        // `DefaultPredicate` already declines a body under 32 bytes and anything
        // `image/`. The two added here are the rest of what a notebook holds: a
        // PDF is a container of already-deflated streams, and `octet-stream` is
        // `holding`'s fallback, which here is most often a zip or a video.
        //
        // The risk runs the cheap way round: a `.json` attachment also lands on
        // `octet-stream` and costs one bigger download, where guessing the other
        // way costs a phone re-deflating a video that was already deflated.
        .layer(
            CompressionLayer::new().compress_when(
                DefaultPredicate::new()
                    .and(NotForContentType::const_new("application/pdf"))
                    .and(NotForContentType::const_new("application/octet-stream")),
            ),
        )
        .layer(middleware::from_fn(log::timed))
        .with_state(server)
}

/// The stylesheet, or one of the scripts.
///
/// **A lookup and two headers.** Nothing is read from disk and the path is never
/// joined to anything — `asset::find` compares it against the names this build
/// wrote, so the traversal question cannot be asked here.
///
/// `immutable`, for a year: the address carries a hash of the bytes, so a
/// different answer has a different address. The other half is on the pages,
/// which are `no-cache`.
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
            // What noda said it is, is what it is.
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff".to_string()),
        ],
        held.body.clone(),
    )
        .into_response()
}

/// Whether this process is still able to answer.
///
/// **Outside the guard**, because a probe's `Host` is whatever the thing running
/// it decided on, and a 403 for want of `--allow-host` would report a healthy
/// server as dead. Nothing here needs protecting: the only thing disclosed is
/// that something is listening, which the caller established by connecting.
///
/// **Through `spawn_blocking`, which is the whole of what it tests.** Every page
/// works on the blocking pool, so a check answering from the async side would
/// return 200 with every reader hanging — a health check that cannot fail is not
/// being run.
///
/// **It does not open a notebook**: one that will not open is a repository to
/// repair, not a process to restart. What this reports is what a restart can
/// mend.
async fn health() -> Response {
    let alive = tokio::task::spawn_blocking(|| ()).await.is_ok();
    if !alive {
        // The pool lost a task, as during a shutdown — and a probe already
        // knows how to read this status code.
        log::lost();
        return (StatusCode::SERVICE_UNAVAILABLE, plain("unavailable\n")).into_response();
    }
    (StatusCode::OK, plain("ok\n")).into_response()
}

/// `no-store`, because a health check behind a cache can report a stopped
/// server as running. `nosniff`, for every other answer's reason.
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

/// A layer and not a check inside each handler, so a route added later is
/// covered by having been added rather than by somebody remembering.
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

/// Its own name rather than a shape of `Accept`: this is not content
/// negotiation — the type is `text/html` either way and the header settles how
/// much of it.
pub(crate) const PART: &str = "x-noda-fragment";

/// How much of a page a request will use.
///
/// **A page is one screen's worth of chrome around one changing region**, and
/// the enhancement layer only keeps the region — measured at 48 of the 52 KB a
/// note page weighs, thrown away on the round trip a reader waits through.
///
/// Three rules keep this from becoming a second interface:
///
/// * **The part is a substring of the page**, both built from one string in
///   `page.rs`, so there is no shorter rendering to drift. Tested by
///   containment.
/// * **The whole page is always a correct answer.** An unknown name is a request
///   with nothing to shorten; every such fetch queries what arrives for the
///   region it wants, so ignoring the header answers later, never differently.
/// * **Nobody but the script asks.** A reader, a bookmark and a crawler all send
///   no such header. Hence `Vary` on every HTML answer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Part {
    /// The note being read: `.pane.read`, and the name of the tab it is in.
    Read,
    /// The listing's own column, rows and count.
    Index,
    /// Both of the listing's panes — what going back has to put right.
    Screen,
    /// The rows of a backlinks answer, without the page around them.
    Rows,
    /// The network screen's news, and whether it is still moving.
    News,
}

impl Part {
    /// One vocabulary, written here and read by `script.rs`.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Part::Read => "read",
            Part::Index => "index",
            Part::Screen => "screen",
            Part::Rows => "rows",
            Part::News => "news",
        }
    }

    /// Each route knows the one part it can send, so this is asked rather than
    /// parsed — a note route needs no opinion about the network screen's news.
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
    /// The only answer here that is not a page noda wrote.
    Held(Held),
}

/// A file on its way out, and the two decisions that go with it.
struct Held {
    bytes: Vec<u8>,
    /// What it is, as far as noda is willing to say.
    kind: &'static str,
    /// Only the formats that cannot carry a script may — see `holding`.
    inline: bool,
    name: String,
}

/// Everything a handler does blocks, libgit2 offering no other kind — and
/// `spawn_blocking` is the only place a `!Send` `Repository` can be created and
/// dropped without crossing an await.
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
        // A panic, or a shutdown under way. Nothing the reader can do, which is
        // why it is worth saying somewhere they are not.
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
        // Not a bare `filename=`: a notebook's file may be `réunion.pdf`.
        let disposition = format!(
            "{}; filename*=UTF-8''{}",
            if self.inline { "inline" } else { "attachment" },
            encoded(&self.name)
        );
        (
            [
                (header::CONTENT_TYPE, self.kind.to_string()),
                // Everything below rests on the type noda declared.
                (header::X_CONTENT_TYPE_OPTIONS, "nosniff".to_string()),
                (header::CONTENT_DISPOSITION, disposition),
                // An attachment loads nothing, runs nothing, frames nothing.
                // Said out loud so a format that turns out able to — SVG is the
                // one everybody finds out about — cannot.
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

/// Percent-encoded, every byte not plainly safe spelled out.
///
/// The wider of two callers decides the rule: `filename*` needs `réunion.pdf` to
/// survive a header, and a query string needs `tag:"24.04 Dark patterns"` to
/// survive a link — quotes, colon and spaces. The unreserved set is correct in
/// both places.
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

/// What noda is willing to say a file is, and whether it may be shown in place.
///
/// **A list of what may be shown, never of what may not.** An attachment is
/// served from the same origin as every page, so anything inline that can carry
/// a script is a script running on this page — SVG is the one that catches
/// people out. Anything unnamed is `octet-stream` and a download, which is the
/// direction an unknown format should fall in.
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
        // Text cannot execute, and is the one attachment worth reading in
        // place.
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
            // On every HTML answer: a cache holding a fragment would hand it to
            // the next reader who typed the address, and saying it once here
            // means a route added later cannot forget.
            (header::VARY, header::HeaderValue::from_static(PART)),
            // **An address here is somebody's note id** — the same fact
            // `web::log` acts on, said to the network: without this, following a
            // link out of a note hands `/nb/<book>/n/<id>` to whoever is on the
            // other end. On the answer as well as in the page, because a reverse
            // proxy may strip one and not the other.
            //
            // **`same-origin` and not `no-referrer`.** Both send nothing to
            // another site, but Fetch nulls a form's `Origin` under
            // `no-referrer` exactly as it nulls the referrer — and `guard`
            // refuses an opaque origin, so every write would be turned away by
            // noda's own defence, logged as the attack the check is for.
            (
                header::REFERRER_POLICY,
                header::HeaderValue::from_static("same-origin"),
            ),
            // The other half of `asset.rs`'s year: a kept page could ask for
            // bytes this build does not have. `no-cache` is "ask first", not
            // "do not keep" — going back still comes out of the browser.
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
        // Not an error here: this page names every notebook and is worth
        // reading on a machine that has never chosen one.
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
                // `None` has nowhere to sync to, and is the row that is not a
                // link. Said in the type rather than read back out of a string.
                drift: status.remote.as_ref().map(|_| cmd::drifted(status.drift)),
                last: cmd::format_time(seconds, offset)[..cmd::DATE_WIDTH].to_string(),
            });
        }
        Ok(Answer::Page(page::notebooks(&books)))
    })
    .await
}

/// The notebook's `README.md`, rendered, for the pane beside the listing — the
/// page it already has about itself, where a wide screen has room. A notebook
/// without one gets the invitation.
///
/// Sent on every listing view and drawn only above 1024px, which is the layout's
/// standing bargain and a small one here: a couple of kilobytes against hundreds
/// of rows.
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
    // `--sort` and `-r` over HTTP. An unknown order is the default rather than
    // a complaint: `q` is typed, so half of one is a thought in progress, while
    // `?sort=` is written by a link and anything else is a hand-edited address.
    // `r` is a checkbox's bargain: sent means yes.
    let order = page::Order {
        sort: cmd::Sort::named(&parameter(request.uri().query(), "sort")).unwrap_or_default(),
        reversed: !parameter(request.uri().query(), "r").is_empty(),
    };
    // Two parts off one route: narrowing a search leaves the note pane alone,
    // and backing out of a note has to put it back. Only the second needs the
    // front page, which is a file read.
    let column = Part::Index.wanted(request.headers());
    let screen = Part::Screen.wanted(request.headers());
    answer(move || {
        let Some(notebook) = open(&server.paths, &book)? else {
            return Ok(missing_notebook(&book));
        };
        let mut notes = notebook.notes()?;
        // `ls`'s function: an order differing by where you asked would be two
        // features wearing one name.
        cmd::sort_notes(&mut notes, order.sort);
        // After it, as `ls` applies `-r`: every order gets one for free.
        if order.reversed {
            notes.reverse();
        }
        // `drift` and not `status`, whose two extra walks of the working tree
        // put nothing on the chip. Two refs compared is what a listing can
        // afford on every visit.
        let drift = cmd::standing(
            notebook.remote_url().as_deref(),
            notebook.drift(&notebook.branch()?)?,
        );

        // Half a query is what every query looks like on the way to being one,
        // so one that does not parse says why and leaves the notes alone rather
        // than emptying the screen to punish an unfinished thought.
        //
        // Nothing typed is not half a query, though: `Query::parse` rightly
        // refuses an empty token list at a command line, and here that is the
        // state every listing starts in.
        //
        // The query decides which rows the page *shows*, not which it *has* —
        // the excluded ones ride along `hidden`, which is what lets the
        // enhancement layer widen a query as well as narrow one.
        let mut rows = notes
            .iter()
            .map(|file| page::Row::of(file, order.sort))
            .collect::<Vec<_>>();
        // Only when that pane is going out: the column alone lands in a page
        // whose other half is a note.
        let front = if column {
            None
        } else {
            front_page(&notebook, &book)?
        };
        let drawn = |rows: &[page::Row], asked: &page::Asked<'_>| {
            if column {
                page::listing_pane(&book, rows, asked, order, &drift)
            } else if screen {
                page::listing_screen(&book, rows, asked, order, &drift, front.as_deref())
            } else {
                page::listing(&book, rows, asked, order, &drift, front.as_deref())
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
        // Bound rather than matched into pieces: `grouping` hands back a slice
        // of the query's own words, and the page is drawn while it stands.
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
        // One address per page: a slug is a way of reaching this note, not a
        // place it lives, and a bookmark against one dies at the next retitle.
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
            created: note.created,
            updated: note.updated,
            rendered: render::body(&note.body, &around),
        };
        // A swap leaves the index pane where it is, so this request is not
        // asking about the notebook and nothing on screen will change.
        if part {
            return Ok(Answer::Page(page::note_pane(&book, &reading)));
        }
        // For the chip in the index pane's bar, which is the notebook's. Two
        // refs, and no notes read — the point of sending this pane empty.
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
/// The count of notes pointing at each is `doctor --links`' orphan judgement,
/// made with the same `link::targets` — saying it twice in two ways would be
/// worse than not saying it.
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
                // The browser needs the encoding; a reader reading the row
                // does not.
                kind: kind.split(';').next().unwrap_or(kind).to_string(),
            });
        }
        Ok(Answer::Page(page::files(&book, &rows)))
    })
    .await
}

/// `notebook::tag_tally` counts and orders it, which is where the browser's tag
/// screen gets the same list. Nothing is decided here.
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

/// `noda todo`'s list, from the same two functions. What this adds is the note's
/// title beside each item: a phone has room for the words a terminal spends on a
/// filename.
///
/// **`cmd::today` decides what is late, and it is the local date.** East of UTC
/// an item that went overdue at midnight would otherwise stay unmarked until
/// morning, which is exactly when a todo list is read.
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

/// **Nothing here touches the network**, as in `noda status`: the drift is
/// measured against the last fetch, so the screen answers instantly and the
/// three buttons are the only things that go out. A page that fetched before
/// drawing itself would hang, and would make Pull a lie about what it does.
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

/// Starts one of the three and answers before it finishes, sending the reader to
/// the screen that says what is happening — so what they hold afterwards is a
/// `GET`, and the reload a slow network invites cannot start a second push.
///
/// Pressing again is not an error: it is somebody who could not tell whether the
/// first press landed, and that screen is the answer.
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
            // Its own thread, not the blocking pool: that pool is for work a
            // request waits on, and nothing waits on this. It opens the notebook
            // itself because a `Repository` cannot cross a thread, and takes the
            // write lock because a fetch landing mid-commit is the collision.
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

/// `backlinks_to_note`, as `noda backlinks` asks it: matched on the id, so the
/// answer survives a retitle. Which is why it is worth a screen — after `mv`,
/// every Markdown renderer sees a broken link where noda sees an unambiguous
/// one.
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
            // Not a listing, so no order to choose: `Sort::default()`.
            .map(|file| page::Row::of(file, cmd::Sort::default()))
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

/// The question above asked of a different kind of thing. A file has no id to
/// fall back on — its name is its whole identity — so this shows what a
/// `file mv` without `--update-links` would leave pointing at nothing.
///
/// The name goes through `link::target` as the download does: the same
/// reader-supplied path, and counting notes rather than opening a file is not a
/// reason to check it less.
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
        // A note is not a file here, as at `/f/`: it has a page of its own.
        if note::names_a_note(&path) || !notebook.path.join(&path).is_file() {
            return nothing();
        }

        let rows = notebook
            .backlinks_to_file(&path)?
            .iter()
            .map(|file| page::Row::of(file, cmd::Sort::default()))
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

/// **The only place noda opens a path a reader named**, which is why it goes
/// through `link::target` before anything touches the disk: that is what decides
/// `../../.ssh/id_rsa` names nothing here. A note is never served from here.
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
        // The page that sent the reader here wrote this URL from a destination
        // in a note's body, so the same rules have to answer both.
        let Some(path) = crate::link::target(&name) else {
            return nothing();
        };
        let on_disk = notebook.path.join(&path);
        // As the notebook decides it: a stem splitting into an id and a slug,
        // case and all. The suffix alone is not the test — `README.md` is listed
        // and offered, and `NOTES.MD` is an attachment.
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
        // Not by reading the id out of what `add` printed: that answer is
        // written for a person, and parsing it makes a message into an
        // interface.
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
            // Added, then taken away between the two reads: nowhere to send
            // you.
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
            // Nothing written. What the reader typed is handed back on top of
            // what is on disk: worse than overwriting somebody's work is losing
            // the work of the person standing in front of you to avoid it.
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
        // As the browser's `m` calls it. Rewriting the prose of notes nobody
        // pointed at has to be asked for out loud, and there is no way to here.
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

        // An unticked box is not sent, so the form says which tags survived and
        // the change is the difference. Worked out here rather than asked for:
        // `+work -q3` is for somebody with a keyboard.
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
        // Not to the note: it is gone.
        Ok(Answer::Elsewhere(format!("/nb/{book}")))
    })
    .await
}

/// The notebook open and the note located. Every write handler needs the same
/// three refusals first — no such notebook, no such note, an address that is not
/// the note's own — so a route added later cannot get two of the three right.
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

/// `303` and not `200`, so a reload does not offer to send the form again —
/// which is why a write is a `POST` and a redirect rather than a page.
fn back_to_note(book: &str, id: &str) -> Answer {
    Answer::Elsewhere(format!("/nb/{book}/n/{id}"))
}

/// Told apart from a notebook that exists and will not open: 404 for the second
/// would send somebody looking for a typo.
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

/// Hand-written: serde's derive for a single `q=` is a proc macro for twenty
/// lines. The decoding builds bytes and converts once at the end, which keeps a
/// character spread over three `%xx` escapes from being cut in half.
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

/// A form sends the name once per ticked box, which is how the tags screen says
/// which tags are still wanted: an unticked box is not sent at all, so what
/// arrives is a list of survivors rather than of changes.
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
            // A form sends a space this way.
            b'+' => {
                out.push(b' ');
                at += 1;
            }
            b'%' if at + 2 < bytes.len() => {
                if let Some(byte) = hex(bytes[at + 1], bytes[at + 2]) {
                    out.push(byte);
                    at += 3;
                } else {
                    // A stray `%` is what somebody typed.
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

    /// A phone sends the quotes and colon escaped and the space as `+`.
    #[test]
    fn a_quoted_tag_survives_the_trip() {
        assert_eq!(
            parameter(Some("q=tag%3A%2224.04+Dark+patterns%22"), "q"),
            "tag:\"24.04 Dark patterns\""
        );
    }

    /// A CJK character arrives as three escapes, and converting each alone
    /// would produce three replacement characters.
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
