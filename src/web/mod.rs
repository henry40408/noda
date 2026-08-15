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

pub mod guard;
pub mod page;
pub mod render;
pub mod theme;

use std::fmt::Write;
use std::sync::Arc;

use axum::Router;
use axum::extract::{Path, Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::get;

use crate::note::Note;
use crate::notebook::Notebook;
use crate::query::{self, Query};
use crate::{Error, Paths, Result, cmd};

/// What every handler is given.
struct Server {
    paths: Paths,
    guard: guard::Guard,
    /// Held for the whole of any request that writes.
    ///
    /// Reads can happen at once and do; writes cannot. Two commits racing in one
    /// repository meet at `index.lock`, and what comes back is libgit2 saying a
    /// file exists — a true statement about a lock file and no help at all to
    /// somebody who pressed Save.
    ///
    /// A `std::sync::Mutex` and not tokio's, because it is only ever taken
    /// inside `spawn_blocking`: the thing it guards is blocking work, and a lock
    /// that could be held across an await would be a lock held while a request
    /// is doing nothing.
    ///
    /// It does **not** lock the notebook against the world. A terminal in
    /// another window is writing to the same repository and always could be;
    /// that is what the fingerprint below is for.
    writing: std::sync::Mutex<()>,
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
pub fn serve(paths: &Paths, listen: &str, allow: &[String]) -> Result<String> {
    let server = Arc::new(Server {
        paths: paths.clone(),
        guard: guard::Guard::new(allow),
        writing: std::sync::Mutex::new(()),
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
        .route("/nb/{book}", get(listing))
        .route("/nb/{book}/files", get(files))
        // One path segment, not a wildcard: a notebook is a flat directory, and
        // a route that could match `a/b` would be inviting a path to be
        // assembled out of pieces nobody checked.
        .route("/nb/{book}/f/{name}", get(held))
        .route("/nb/{book}/new", get(new_form).post(new_note))
        .route("/nb/{book}/n/{key}", get(reading))
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
        .with_state(server)
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
        Err(refusal) => (
            StatusCode::FORBIDDEN,
            html(page::failure("Not answered", &refusal.0)),
        )
            .into_response(),
    }
}

fn text(headers: &HeaderMap, name: header::HeaderName) -> Option<String> {
    headers
        .get(name)?
        .to_str()
        .ok()
        .map(std::string::ToString::to_string)
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
        Ok(Err(e)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            html(page::failure("Something went wrong", &e.to_string())),
        )
            .into_response(),
        // The blocking pool lost the task: a panic, or a shutdown under way.
        // Nothing useful to say about it, and nothing the reader can do.
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            html(page::failure(
                "Something went wrong",
                "the request did not finish",
            )),
        )
            .into_response(),
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
            encoded_name(&self.name)
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

/// A filename as `filename*` takes it: percent-encoded, every byte that is not
/// plainly safe spelled out.
fn encoded_name(name: &str) -> String {
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
        [(
            header::CONTENT_TYPE,
            header::HeaderValue::from_static("text/html; charset=utf-8"),
        )],
        body,
    )
}

async fn front(State(server): State<Shared>) -> Response {
    answer(move || {
        let mut books = Vec::new();
        for name in Notebook::list(&server.paths)? {
            let notebook = Notebook::open(&server.paths, &name)?;
            let status = notebook.status()?;
            books.push(page::Book {
                name,
                notes: status.notes,
                remote: standing(&status),
            });
        }
        Ok(Answer::Page(page::notebooks(&books)))
    })
    .await
}

/// Where a notebook stands against its remote, in git's own words.
///
/// `ahead` and `behind` rather than a verb: "sync" asks whether you want to,
/// and this says whether there is anything to. The counts are already in
/// `Status` — `noda status` prints them — so this is the same judgement said
/// shorter, not a second one.
fn standing(status: &crate::notebook::Status) -> String {
    match (status.remote.as_ref(), status.drift) {
        (None, _) => "no remote".to_string(),
        (Some(_), None) => "never fetched".to_string(),
        (Some(_), Some((0, 0))) => "in sync".to_string(),
        (Some(_), Some((ahead, 0))) => format!("{ahead} to push"),
        (Some(_), Some((0, behind))) => format!("{behind} to pull"),
        (Some(_), Some((ahead, behind))) => format!("{ahead} to push, {behind} to pull"),
    }
}

async fn listing(
    State(server): State<Shared>,
    Path(book): Path<String>,
    request: Request,
) -> Response {
    let typed = parameter(request.uri().query(), "q");
    answer(move || {
        let Some(notebook) = open(&server.paths, &book)? else {
            return Ok(missing_notebook(&book));
        };
        let mut notes = notebook.notes()?;
        // The same order `ls` and the browser use, from the same function. An
        // order that came out differently depending on where you asked would be
        // two features wearing one name.
        cmd::sort_notes(&mut notes, cmd::Sort::default());
        let total = notes.len();

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
        let tokens = query::split(&typed);
        if tokens.is_empty() {
            let rows = notes.iter().map(page::Row::of).collect::<Vec<_>>();
            return Ok(Answer::Page(page::listing(
                &book,
                &rows,
                &typed,
                total,
                &[],
                None,
            )));
        }
        let (rows, terms, problem) = match Query::parse(&tokens) {
            Ok(query) => {
                let terms = query.excerpt_terms();
                let rows = notes
                    .iter()
                    .filter(|file| query.matches(&file.id, &file.note))
                    .map(page::Row::of)
                    .collect::<Vec<_>>();
                (rows, terms, None)
            }
            Err(e) => (
                notes.iter().map(page::Row::of).collect(),
                Vec::new(),
                Some(e.to_string()),
            ),
        };

        Ok(Answer::Page(page::listing(
            &book,
            &rows,
            &typed,
            total,
            &terms,
            problem.as_deref(),
        )))
    })
    .await
}

async fn reading(
    State(server): State<Shared>,
    Path((book, key)): Path<(String, String)>,
) -> Response {
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
        Ok(Answer::Page(page::note(
            &book,
            &page::Reading {
                id,
                slug,
                title: note.title,
                tags: note.tags,
                updated: note.updated,
                rendered: render::body(&note.body, &around),
            },
        )))
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
        // Exactly as the notebook itself decides what is a note — `strip_suffix(".md")`,
        // case and all. A case-insensitive test here would refuse to serve a
        // file called `NOTES.MD`, which the notebook holds as an attachment and
        // will happily list on the files page.
        #[expect(
            clippy::case_sensitive_file_extension_comparisons,
            reason = "matches Notebook::inventory, which is what decides this everywhere else"
        )]
        let is_note = path.ends_with(".md");
        if is_note || !on_disk.is_file() {
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

        let _writing = server.writing.lock();
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

        let _writing = server.writing.lock();
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
        let _writing = server.writing.lock();
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

        let _writing = server.writing.lock();
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
        let _writing = server.writing.lock();
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

    /// `noda status` already answers this; the words are the only new part.
    #[test]
    fn a_notebook_says_where_it_stands_in_gits_own_words() {
        let standing_of = |remote: Option<&str>, drift| {
            standing(&crate::notebook::Status {
                branch: "main".into(),
                notes: 0,
                files: 0,
                uncommitted: 0,
                remote: remote.map(std::string::ToString::to_string),
                drift,
                problems: Vec::new(),
            })
        };
        assert_eq!(standing_of(None, None), "no remote");
        assert_eq!(standing_of(Some("git@x:y.git"), None), "never fetched");
        assert_eq!(standing_of(Some("git@x:y.git"), Some((0, 0))), "in sync");
        assert_eq!(standing_of(Some("git@x:y.git"), Some((2, 0))), "2 to push");
        assert_eq!(standing_of(Some("git@x:y.git"), Some((0, 1))), "1 to pull");
        assert_eq!(
            standing_of(Some("git@x:y.git"), Some((2, 1))),
            "2 to push, 1 to pull"
        );
    }
}
