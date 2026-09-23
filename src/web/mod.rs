//! `noda web` — the notebook over HTTP, for reading and writing it from a phone.
//!
//! Reads through `notebook`, writes through `cmd`.
//!
//! - **A notebook is named in the URL**, never taken from the active pointer,
//!   which belongs to a shell session.
//! - **A note is addressed by id**, and a slug or prefix redirects to it: the
//!   slug follows the title, so a bookmark to it dies at the next rename.
//! - **Every handler opens its own notebook inside `spawn_blocking`**, since
//!   `git2::Repository` is `!Send`: one request, one handle, and a slow walk on
//!   one does not stall the others.

pub mod asset;
pub mod guard;
pub mod log;
pub mod page;
pub mod render;
pub mod script;
pub mod theme;
pub mod watch;
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

struct Server {
    paths: Paths,
    guard: guard::Guard,
    /// Held by every write, so two commits do not race to `index.lock` and fail
    /// with libgit2's unhelpful "file exists". `std::sync::Mutex`, as it is only
    /// taken off the async threads. It does not guard against a terminal writing
    /// at the same time; the fingerprint does.
    writing: Locks,
    /// Outlives a request, because an errand does.
    errands: work::Errands,
    /// Which notes have an editor open, and the thread watching those files.
    watching: watch::Watch,
}

/// One write lock per notebook: a single lock froze Save on every notebook
/// whenever one notebook's remote went quiet.
#[derive(Default)]
struct Locks(std::sync::Mutex<std::collections::BTreeMap<String, Arc<std::sync::Mutex<()>>>>);

impl Locks {
    /// An `Arc` rather than a guard, so the map is free again at once and one
    /// notebook's slow push does not block finding another's lock.
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

/// The optimistic lock: a form carries the note's fingerprint from when the page
/// was drawn, so an edit begun on a phone cannot flatten one made since.
///
/// The blob id, not the `updated` stamp, which `--no-touch` leaves unmoved.
fn fingerprint(path: &std::path::Path) -> Result<String> {
    Ok(git2::Oid::hash_file(git2::ObjectType::Blob, path)?.to_string())
}

enum Merged {
    Clean(String),
    /// Carrying git's conflict markers.
    Conflicted(String),
}

/// Three-way merge of what the reader wrote with what was saved since, against
/// the version the edit began from.
///
/// Bodies only: merging frontmatter would make somebody else's tag change a
/// conflict over a line the reader never saw, and `cmd::rewrite_in` keeps
/// whatever frontmatter is on disk anyway. The labels are for a person, so they
/// name the versions as the page does.
///
/// Cost: +16,608 bytes (+0.22%) on the release binary, for libgit2's xdiff
/// merge; no new dependency.
fn merge(base: &str, mine: &str, theirs: &str) -> Result<Merged> {
    // `merge_file` does not initialise libgit2 and traps inside C if reached
    // first in a process. Callers have always opened a notebook already; this
    // keeps the function independent of that.
    git2::Oid::hash_object(git2::ObjectType::Blob, &[])?;

    let mut ancestor = git2::MergeFileInput::new();
    ancestor.content(base.as_bytes());
    let mut ours = git2::MergeFileInput::new();
    ours.content(mine.as_bytes());
    let mut yours = git2::MergeFileInput::new();
    yours.content(theirs.as_bytes());

    let mut options = git2::MergeFileOptions::new();
    options
        .our_label("what you wrote")
        .their_label("saved since");

    let merged = git2::merge_file(&ancestor, &ours, &yours, Some(&mut options))?;
    let text = String::from_utf8_lossy(merged.content()).into_owned();
    Ok(if merged.is_automergeable() {
        Merged::Clean(text)
    } else {
        Merged::Conflicted(text)
    })
}

type Shared = Arc<Server>;

/// Serves until it is asked to stop.
///
/// Unlike every other command it prints the address rather than returning it,
/// since it does not finish while in use; the `String` is always empty.
///
/// Stopping, in order: a signal closes the listener, in-flight requests are
/// answered (finishing any commit), then `settle` waits for errands. Only then
/// does it exit `0`, so a supervisor can tell a clean stop from a crash.
pub fn serve(paths: &Paths, listen: &str, allow: &[String], format: log::Format) -> Result<String> {
    // Before the bind, so a failure to listen is logged.
    log::start(format);
    let server = Arc::new(Server {
        paths: paths.clone(),
        guard: guard::Guard::new(allow),
        writing: Locks::default(),
        errands: work::Errands::default(),
        watching: watch::Watch::new(),
    });

    // By hand rather than `#[tokio::main]`, every other command being blocking
    // code. I/O only, which also drives signals; nothing here needs timers.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_io()
        .build()?;

    runtime.block_on({
        let server = Arc::clone(&server);
        async move {
            // First, so an early signal has somewhere to go.
            let stop = Stop::listen()?;
            let listener = tokio::net::TcpListener::bind(listen)
                .await
                .map_err(|e| Error::msg(format!("could not listen on {listen}: {e}")))?;
            let at = listener.local_addr()?;
            println!("noda is at http://{at}");
            if !at.ip().is_loopback() {
                println!("reachable from the network — there is no password on it");
            }
            axum::serve(listener, router(Arc::clone(&server)))
                .with_graceful_shutdown(asked_to_stop(stop, server))
                .await?;
            Ok::<(), Error>(())
        }
    })?;

    // Requests are done; only errands can still be running.
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
    // Signalled twice: an errand was cut off, so this is a failure, not `0`.
    Err(Error::msg(format!(
        "left {} unfinished",
        left.iter()
            .map(|(book, errand)| format!("{} in {book}", errand.name()))
            .collect::<Vec<_>>()
            .join(", ")
    )))
}

/// Resolves on the first stop signal and immediately hands the same streams to a
/// task awaiting the second, so no signal falls in a gap.
///
/// The second signal cuts short `settle`, which is unbounded (a push to a dead
/// host is minutes of libgit2 on a socket), not the requests in flight. Printed
/// to stdout beside the startup line, not logged: it is not about a request.
async fn asked_to_stop(mut stop: Stop, server: Shared) {
    println!("{} — finishing what is in flight", stop.next().await);
    // Before the wait: a watch never finishes by itself, so graceful shutdown
    // would wait on it forever.
    server.watching.stop();
    tokio::spawn(async move {
        println!("{} again — not waiting", stop.next().await);
        server.errands.abandon();
    });
}

/// `SIGINT` (Ctrl-C) and `SIGTERM` (every supervisor), listened for once and
/// consumed twice. Each is named when printed, so a container log shows the
/// orchestrator stopped it rather than that it fell over.
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

    /// Streams, not `ctrl_c()`: a `Signal` keeps its registration between the
    /// two awaits, so one landing in between is remembered.
    async fn next(&mut self) -> &'static str {
        tokio::select! {
            _ = self.interrupt.recv() => "SIGINT",
            _ = self.terminate.recv() => "SIGTERM",
        }
    }
}

/// Ctrl-C alone, where there is no `SIGTERM`.
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
        // Inside the guard, so a refused page cannot load its assets either.
        .route("/a/{file}", get(held_asset))
        .route("/nb/{book}", get(listing))
        .route("/nb/{book}/files", get(files))
        .route("/nb/{book}/tags", get(tags))
        .route("/nb/{book}/todo", get(todo))
        // `POST` for the errands, so a reload is a question, not a second push.
        .route("/nb/{book}/status", get(status))
        .route("/nb/{book}/status/{errand}", post(errand))
        // One segment, not a wildcard, so no path is assembled from pieces.
        .route("/nb/{book}/f/{name}", get(held))
        .route("/nb/{book}/f/{name}/backlinks", get(file_backlinks))
        .route("/nb/{book}/new", get(new_form).post(new_note))
        .route("/nb/{book}/n/{key}", get(reading))
        .route("/nb/{book}/n/{key}/backlinks", get(note_backlinks))
        // `GET` shows the form, `POST` does it, at one address: a `GET` that
        // changed something would be a link a prefetcher could press.
        .route("/nb/{book}/n/{key}/edit", get(edit_form).post(edit_note))
        .route("/nb/{book}/n/{key}/watch", get(watching))
        .route(
            "/nb/{book}/n/{key}/rename",
            get(rename_form).post(rename_note),
        )
        .route("/nb/{book}/n/{key}/tags", get(tags_form).post(tag_note))
        // Two routes, not a toggle: a retried POST must be a no-op.
        .route("/nb/{book}/n/{key}/pin", post(pin_note))
        .route("/nb/{book}/n/{key}/unpin", post(unpin_note))
        .route(
            "/nb/{book}/n/{key}/delete",
            get(delete_form).post(delete_note),
        )
        .layer(middleware::from_fn_with_state(
            Arc::clone(&server),
            admitted,
        ))
        // Outside the guard, being declared after its layer. See `health`.
        .route("/health", get(health))
        // Inside the log's layer, so the log times compression too.
        // `DefaultPredicate` already skips bodies under 32 bytes, `image/*` and
        // `text/event-stream` (a deflater would hold a watch's messages back for
        // hours; `tests/web.rs` checks it). PDFs are already deflated, and
        // `octet-stream` is `holding`'s fallback, most often a zip or video.
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

/// The stylesheet, or one of the scripts. `asset::find` matches the name against
/// what this build embedded, so no path is ever joined or read.
///
/// `immutable` for a year: the address carries a hash of the bytes. Pages are
/// `no-cache` to match.
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
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff".to_string()),
        ],
        held.body.clone(),
    )
        .into_response()
}

/// Whether this process can still answer.
///
/// Outside the guard: a probe's `Host` is arbitrary, and a 403 would report a
/// healthy server dead; it discloses only that something is listening. It goes
/// through `spawn_blocking` because every page does, so a stuck pool fails the
/// check. It opens no notebook: a broken repository is not what a restart fixes.
async fn health() -> Response {
    let alive = tokio::task::spawn_blocking(|| ()).await.is_ok();
    if !alive {
        // The pool lost a task, as during a shutdown.
        log::lost();
        return (StatusCode::SERVICE_UNAVAILABLE, plain("unavailable\n")).into_response();
    }
    (StatusCode::OK, plain("ok\n")).into_response()
}

/// `no-store`, so a cache cannot report a stopped server as running.
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

/// A layer, so a route added later is covered without anybody remembering.
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

/// Its own header rather than an `Accept` variant: the type is `text/html`
/// either way, and this only says how much of it.
pub(crate) const PART: &str = "x-noda-fragment";

/// How much of a page a script's fetch will use; it keeps only one region, 48
/// of a note page's 52 KB being chrome.
///
/// * **The part is a substring of the page**, both built from one string in
///   `page.rs`, so there is no second rendering to drift.
/// * **The whole page is always a correct answer**: every script looks up its
///   region in what arrives, so an unknown name just gets the page.
/// * **Only the script asks**, hence `Vary` on every HTML answer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Part {
    /// `.pane.read`, and the page title.
    Read,
    /// The listing's column, rows and count.
    Index,
    /// Both of the listing's panes, for going back.
    Screen,
    /// A backlinks answer's rows.
    Rows,
    /// The network screen's `<main>`, and whether it still refreshes.
    News,
}

impl Part {
    /// Read by `script.rs`.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Part::Read => "read",
            Part::Index => "index",
            Part::Screen => "screen",
            Part::Rows => "rows",
            Part::News => "news",
        }
    }

    /// Asked per part, since each route can send only its own.
    fn wanted(self, headers: &HeaderMap) -> bool {
        headers
            .get(PART)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|said| said == self.name())
    }
}

/// What a handler decided, before it is HTTP.
enum Answer {
    Page(String),
    /// A `303`.
    Elsewhere(String),
    Missing(String, String),
    /// An attachment.
    Held(Held),
}

struct Held {
    bytes: Vec<u8>,
    kind: &'static str,
    /// See `holding`.
    inline: bool,
    name: String,
}

/// Runs the handler on the blocking pool: libgit2 only blocks, and there a
/// `!Send` `Repository` lives and dies without crossing an await.
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
        // A panic, or a shutdown under way.
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
                (header::X_CONTENT_TYPE_OPTIONS, "nosniff".to_string()),
                (header::CONTENT_DISPOSITION, disposition),
                // Belt and braces for any format that turns out able to run a
                // script, as SVG does.
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

/// Percent-encodes everything outside the unreserved set, which is right both
/// for `filename*` (`réunion.pdf`) and a query string (`tag:"24.04 Dark
/// patterns"`).
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

/// A file's content type, and whether it may be shown inline.
///
/// An allow-list: attachments share the pages' origin, so anything inline that
/// can carry a script (SVG) runs as this site. Anything unlisted is an
/// `octet-stream` download.
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
            // Or a cache could hand a fragment to the next reader.
            (header::VARY, header::HeaderValue::from_static(PART)),
            // An address here carries a note id, so a link out must not leak it.
            // Also in the page, as a proxy may strip one. Not `no-referrer`:
            // under it Fetch nulls a form's `Origin`, which `guard` refuses, so
            // every write would be turned away.
            (
                header::REFERRER_POLICY,
                header::HeaderValue::from_static("same-origin"),
            ),
            // A kept page could name assets this build no longer has.
            // `no-cache` means "revalidate", so back still uses the cache.
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
        // Not an error: this page is useful before any notebook is chosen.
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
                // `None`: no remote, and the chip is not a link.
                drift: status.remote.as_ref().map(|_| cmd::drifted(status.drift)),
                last: cmd::format_time(seconds, offset)[..cmd::DATE_WIDTH].to_string(),
            });
        }
        Ok(Answer::Page(page::notebooks(&books)))
    })
    .await
}

/// The notebook's `README.md`, rendered for the pane beside the listing; `None`
/// gets the invitation. Sent on every listing but drawn only above 1024px — a
/// couple of kilobytes against hundreds of rows.
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
    // `--sort` and `-r`. An unknown `sort` is the default rather than an error:
    // links write it, so anything else is a hand-edited address. `r` is a
    // checkbox: present means yes.
    let order = page::Order {
        sort: cmd::Sort::named(&parameter(request.uri().query(), "sort")).unwrap_or_default(),
        reversed: !parameter(request.uri().query(), "r").is_empty(),
    };
    // A search swaps just the column; going back needs the front page too.
    let column = Part::Index.wanted(request.headers());
    let screen = Part::Screen.wanted(request.headers());
    answer(move || {
        let Some(notebook) = open(&server.paths, &book)? else {
            return Ok(missing_notebook(&book));
        };
        let mut notes = notebook.notes()?;
        // `ls`'s own sort and reversal, so the orders cannot drift apart.
        cmd::sort_notes(&mut notes, order.sort);
        if order.reversed {
            notes.reverse();
        }
        // `drift`, not `status`: two refs compared, without `status`'s two walks
        // of the working tree.
        let drift = cmd::standing(
            notebook.remote_url().as_deref(),
            notebook.drift(&notebook.branch()?)?,
        );

        // A query that does not parse says why and leaves every row shown. An
        // empty one is handled first, as `Query::parse` refuses it. Excluded
        // rows are still sent, `hidden`, so the script can widen a query too.
        let mut rows = notes
            .iter()
            .map(|file| page::Row::of(file, order.sort))
            .collect::<Vec<_>>();
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
        // Bound, because `grouping` borrows from the query.
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
        if key != id {
            return Ok(Answer::Elsewhere(format!("/nb/{book}/n/{id}")));
        }

        let text = std::fs::read_to_string(notebook.note_path(&id, &slug))?;
        let note = Note::parse(&text).map_err(|e| Error::msg(format!("{id}-{slug}.md: {e}")))?;
        let around = render::Around::of(&book, &notebook.named_files()?);
        let pinned = note.is_pinned();
        let reading = page::Reading {
            id,
            slug,
            title: note.title,
            tags: note.tags,
            created: note.created,
            updated: note.updated,
            rendered: render::body(&note.body, &around),
            pinned,
        };
        if part {
            return Ok(Answer::Page(page::note_pane(&book, &reading)));
        }
        // For the chip on the index pane, which is sent empty.
        let drift = cmd::standing(
            notebook.remote_url().as_deref(),
            notebook.drift(&notebook.branch()?)?,
        );
        Ok(Answer::Page(page::note(&book, &reading, &drift)))
    })
    .await
}

/// Everything the notebook holds that is not a note, with how many notes link to
/// each — counted by `link::targets`, as `doctor --links` counts orphans.
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
                // Without the `charset`.
                kind: kind.split(';').next().unwrap_or(kind).to_string(),
            });
        }
        Ok(Answer::Page(page::files(&book, &rows)))
    })
    .await
}

/// `notebook::tag_tally`, the TUI's tag list too.
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

/// `noda todo`'s list, with each note's title. Overdue is judged against
/// `cmd::today`, the local date, as the CLI does.
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

/// Touches no network, as `noda status` does not: drift is against the last
/// fetch, and only the three errand buttons go out.
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

/// Starts an errand and redirects to the status screen at once, so a reload is a
/// `GET` and cannot start a second push. Pressing while one runs is not an
/// error: the status screen answers it.
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
            // Its own thread, as no request waits on it. It opens its own
            // `Repository` and takes the write lock against a mid-commit fetch.
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

/// `backlinks_to_note`, as `noda backlinks`: matched on the id, so it survives a
/// retitle.
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

/// Notes linking to a file, which has no id: what a `file mv` without
/// `--update-links` would break. The name is checked by `link::target`, as in
/// `held`.
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
        // As at `/f/`, a note is not a file.
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

/// The only place noda opens a path a reader named, so it goes through
/// `link::target` first: that is what makes `../../.ssh/id_rsa` name nothing.
/// Never serves a note.
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
        // The same rules the renderer used to write this URL.
        let Some(path) = crate::link::target(&name) else {
            return nothing();
        };
        let on_disk = notebook.path.join(&path);
        // Not by suffix: `README.md` is served, and `NOTES.MD` is an attachment.
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
        // Diffed rather than parsed out of `add`'s prose.
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
            // Removed again between the two reads.
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

/// A watch's stream: one message per change, each the note's new fingerprint.
///
/// By hand rather than `tokio-stream`'s `ReceiverStream`, which is this plus a
/// crate. It ends only when the channel closes, which is how
/// `watch::Watch::stop` ends it.
struct Changes(tokio::sync::mpsc::Receiver<String>);

impl futures_core::Stream for Changes {
    type Item = std::result::Result<axum::response::sse::Event, std::convert::Infallible>;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        self.0
            .poll_recv(cx)
            .map(|change| change.map(|hash| Ok(axum::response::sse::Event::default().data(hash))))
    }
}

/// Sends the note's fingerprint each time it changes — never the content, which
/// is somebody's prose; the page compares it with its form's.
///
/// No keep-alives: a proxy timing out an idle stream costs one reconnect, which
/// `EventSource` makes by itself.
async fn watching(
    State(server): State<Shared>,
    Path((book, key)): Path<(String, String)>,
) -> Response {
    // Only the subscription crosses back; the `Notebook` is `!Send`.
    let opened = tokio::task::spawn_blocking({
        let server = Arc::clone(&server);
        move || -> Result<Option<tokio::sync::mpsc::Receiver<String>>> {
            let Aimed::At(notebook, id, slug) = aim(&server.paths, &book, &key, "/watch")? else {
                return Ok(None);
            };
            let path = notebook.note_path(&id, &slug);
            let now = fingerprint(&path)?;
            Ok(Some(server.watching.subscribe(&book, &id, path, &now)))
        }
    })
    .await;

    match opened {
        Ok(Ok(Some(hear))) => axum::response::Sse::new(Changes(hear)).into_response(),
        // Most likely a race with a delete; nobody reads this page.
        Ok(Ok(None)) => StatusCode::NOT_FOUND.into_response(),
        Ok(Err(e)) => {
            log::failed(&e.to_string());
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
        Err(e) => {
            log::failed(&e.to_string());
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
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
        let mut body = parameter(Some(&form), "body");
        let was = parameter(Some(&form), "fingerprint");

        let writing = server.writing.of(&book);
        let _writing = writing.lock();
        let path = notebook.note_path(&id, &slug);
        let now = fingerprint(&path)?;
        if now != was {
            let theirs = Note::parse(&std::fs::read_to_string(&path)?)
                .map_err(|e| Error::msg(format!("{id}-{slug}.md: {e}")))?;
            let about = page::About::of(&id, &slug, &theirs.title);
            // The fingerprint is a blob id, so it also finds the merge base.
            let base = git2::Oid::from_str(&was)
                .ok()
                .and_then(|oid| notebook.blob_text(oid).transpose())
                .transpose()?
                .and_then(|text| Note::parse(&text).ok())
                .map(|note| note.body);
            match base {
                // A clean merge is saved without asking; git keeps both sides.
                Some(base) => match merge(&base, &body, &theirs.body)? {
                    Merged::Clean(text) => body = text,
                    Merged::Conflicted(text) => {
                        return Ok(Answer::Page(page::conflicted(&book, &about, &text, &now)));
                    }
                },
                // No base (the note was never committed): write nothing, and
                // show both versions so neither is lost.
                None => {
                    return Ok(Answer::Page(page::clashed(
                        &book,
                        &about,
                        &theirs.body,
                        &body,
                        &now,
                    )));
                }
            }
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
        // As the TUI's `m` calls it: `--update-links` must be asked for, and
        // this form has no way to.
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

        // Removed = offered (`saw`) minus still ticked (`keep`). Against what
        // the page offered, not the file, so a tag added elsewhere since the
        // page was drawn is not removed as if unticked.
        let kept = parameters(&form, "keep");
        let mut changes: Vec<String> = parameters(&form, "saw")
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

async fn pin_note(
    State(server): State<Shared>,
    Path((book, key)): Path<(String, String)>,
) -> Response {
    pinning(server, book, key, true).await
}

async fn unpin_note(
    State(server): State<Shared>,
    Path((book, key)): Path<(String, String)>,
) -> Response {
    pinning(server, book, key, false).await
}

/// Unlike other writes, no form in between: there is nothing to fill in.
async fn pinning(server: Shared, book: String, key: String, pinned: bool) -> Response {
    answer(move || {
        let tail = if pinned { "/pin" } else { "/unpin" };
        let (notebook, id, _slug) = match aim(&server.paths, &book, &key, tail)? {
            Aimed::At(notebook, id, slug) => (notebook, id, slug),
            Aimed::Missing(answer) => return Ok(answer),
        };
        let writing = server.writing.of(&book);
        let _writing = writing.lock();
        cmd::pin_in(&notebook, &id, pinned, cmd::Touch::Stamp)?;
        Ok(back_to_note(&book, &id))
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
        Ok(Answer::Elsewhere(format!("/nb/{book}")))
    })
    .await
}

/// The notebook open and the note located, or one of the three refusals every
/// note route needs: no notebook, no note, or a redirect to the id's address.
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

/// A `303`, so a reload does not resend the form.
fn back_to_note(book: &str, id: &str) -> Answer {
    Answer::Elsewhere(format!("/nb/{book}/n/{id}"))
}

/// `None` only when the notebook does not exist: one that fails to open is an
/// error, not a 404 that sends somebody looking for a typo.
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

/// By hand rather than a serde derive, for twenty lines.
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

/// Every value of a repeated name, as a form sends one per ticked box.
fn parameters(body: &str, name: &str) -> Vec<String> {
    body.split('&')
        .filter_map(|pair| {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            (decode(key) == name).then(|| decode(value))
        })
        .collect()
}

/// Decodes to bytes and converts once, so a character spread over several
/// `%xx` escapes is not cut apart.
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
                    // A stray `%` stays as typed.
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

    #[test]
    fn a_quoted_tag_survives_the_trip() {
        assert_eq!(
            parameter(Some("q=tag%3A%2224.04+Dark+patterns%22"), "q"),
            "tag:\"24.04 Dark patterns\""
        );
    }

    #[test]
    fn a_multi_byte_character_arrives_whole() {
        assert_eq!(parameter(Some("q=%E7%AD%86%E8%A8%98"), "q"), "筆記");
    }

    #[test]
    fn a_stray_percent_is_left_as_typed() {
        assert_eq!(parameter(Some("q=100%+of+it"), "q"), "100% of it");
        assert_eq!(parameter(Some("q=%zz"), "q"), "%zz");
    }

    const BASE: &str = "one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nnine\nten\n";

    #[test]
    fn changes_in_different_parts_come_back_as_one_note() {
        let mine = BASE.replace("one\n", "ONE\n");
        let theirs = BASE.replace("ten\n", "TEN\n");
        let Merged::Clean(text) = merge(BASE, &mine, &theirs).expect("merge") else {
            panic!("two ends of a note are not a conflict");
        };
        assert!(text.contains("ONE"), "{text}");
        assert!(text.contains("TEN"), "{text}");
        assert!(!text.contains("<<<"), "{text}");
    }

    #[test]
    fn a_change_against_an_untouched_note_is_that_change() {
        let mine = BASE.replace("five\n", "FIVE\n");
        let Merged::Clean(text) = merge(BASE, &mine, BASE).expect("merge") else {
            panic!("only one side changed anything");
        };
        assert_eq!(text, mine);
    }

    #[test]
    fn changes_to_one_line_are_marked_and_named() {
        let mine = BASE.replace("five\n", "mine\n");
        let theirs = BASE.replace("five\n", "theirs\n");
        let Merged::Conflicted(text) = merge(BASE, &mine, &theirs).expect("merge") else {
            panic!("one line written twice is a conflict");
        };
        assert!(text.contains("<<<<<<< what you wrote"), "{text}");
        assert!(text.contains(">>>>>>> saved since"), "{text}");
        assert!(text.contains("mine"), "{text}");
        assert!(text.contains("theirs"), "{text}");
    }
}
