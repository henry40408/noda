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
pub mod theme;

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
        .route("/nb/{book}/n/{key}", get(reading))
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
        Ok(Answer::Page(page::note(
            &book,
            &page::Reading {
                id,
                slug,
                title: note.title,
                tags: note.tags,
                updated: note.updated,
                body: note.body,
            },
        )))
    })
    .await
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
