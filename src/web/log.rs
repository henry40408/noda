//! What the server says about itself while it runs.
//!
//! **Every other noda command answers and exits, so its output is its answer.**
//! `noda web` outlives the question: the only way to find out afterwards what it
//! refused, what it failed at and what was slow is for it to have said so at the
//! time. Until this existed it said two lines at startup and then nothing, so a
//! 500 was a sentence on somebody's phone and a rebinding attempt was silence.
//!
//! **The log goes to stderr, and that is not a detail.** `noda web`'s stdout
//! carries the address to type into a phone — the command's answer, the same
//! contract every other command here keeps, and the line the test harnesses read
//! the port out of. `tracing_subscriber::fmt` writes to stdout by default;
//! leaving it there would have put a timestamp and a level in front of that
//! answer and mixed two streams that are read by different things. Both
//! reference servers write to stdout because neither has anything else to say
//! there.
//!
//! **What is on the row of a log line is `RUST_LOG`'s business.** The default is
//! `error,noda=info`, which is quiet: a healthy server logs nothing per request.
//! `RUST_LOG=noda=debug` turns the request stream on, and
//! `RUST_LOG=noda::web::log=debug` turns on only this module's half of it.
//!
//! The startup lines are deliberately **not** events. They are the command's
//! answer, they are already on stdout, and a server that announced its address
//! twice in two formats would be a server nobody could grep.

use std::time::{Duration, Instant};

use axum::extract::{MatchedPath, Request};
use axum::http::{Extensions, Method};
use axum::middleware::Next;
use axum::response::Response;
use tracing_subscriber::EnvFilter;

/// How the log is rendered.
///
/// Two and not four. `tracing-subscriber` offers `full`, `compact` and `pretty`
/// as well, and one of the two servers this was modelled on exposes all of them
/// — but those are the subscriber's vocabulary rather than noda's, and the
/// question a reader actually has is whether a person or a program is reading.
#[derive(Clone, Copy, Debug, Default, clap::ValueEnum)]
pub enum Format {
    /// For a person, at a terminal.
    #[default]
    Text,
    /// One JSON object per line, for something that collects them.
    Json,
}

/// What is logged when nothing says otherwise.
///
/// Other crates at `error`, noda's own at `info`. A server that logged every
/// request by default would be one whose log has to be turned *down* before it
/// is useful, and the request stream is a `RUST_LOG` away.
const DEFAULT_FILTER: &str = "error,noda=info";

/// A request taking at least this long is a `http.slow_request` at WARN rather
/// than a `http.request` at DEBUG.
///
/// One second. Nothing noda does per request should come near it: the most
/// expensive page walks every note in the notebook, which is tens of
/// milliseconds for a notebook of two thousand. So this is not a threshold that
/// trims a busy log — it is the number above which something is wrong, and it
/// is worth a line in a log that is otherwise silent about requests.
pub const SLOW_REQUEST: Duration = Duration::from_secs(1);

/// The `route` of a request that matched nothing. Deliberately not the path it
/// asked for — see `log_finished`.
pub const UNMATCHED: &str = "<unmatched>";

/// Installs the subscriber. Called once, by `serve`.
pub fn start(format: Format) {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(DEFAULT_FILTER));
    // The same two questions `anstream` asks for the rest of noda's output, and
    // asked here by hand because a subscriber cannot read its mind: is colour
    // wanted, and is anything at the other end that could show it.
    let colour = std::env::var_os("NO_COLOR").is_none()
        && std::io::IsTerminal::is_terminal(&std::io::stderr());
    let builder = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_ansi(colour);
    match format {
        Format::Text => builder.init(),
        Format::Json => builder.json().init(),
    }
}

/// Times a request and says one thing about it once the response is ready.
///
/// Layered outermost, so it also sees the requests the guard refuses without
/// ever reaching a handler.
pub async fn timed(request: Request, next: Next) -> Response {
    let method = request.method().clone();
    // Both are taken before `next` consumes the request. Neither is expensive:
    // a `Method` is an inline enum for the ordinary verbs, and a route template
    // is a short static string.
    let route = route_of(request.extensions()).to_owned();

    let started = Instant::now();
    let response = next.run(request).await;

    log_finished(
        &method,
        &route,
        response.status().as_u16(),
        started.elapsed(),
    );
    response
}

/// The matched route template, or [`UNMATCHED`].
fn route_of(extensions: &Extensions) -> &str {
    extensions
        .get::<MatchedPath>()
        .map_or(UNMATCHED, MatchedPath::as_str)
}

/// Says that a request finished.
///
/// **The route is the template the router matched, never the path that was
/// asked for, and in noda that is a stronger rule than it is in most servers.**
/// A note's address is `/nb/work/n/k3f9m2p1`, and a file's is
/// `/nb/work/f/scan-of-the-lease.pdf` — an id is the name of somebody's note
/// and a filename is often the whole of what it is about. A search is a query
/// string, which is the reader's own words. None of that belongs in a file that
/// outlives the request and is routinely shipped somewhere else, and none of it
/// is what anybody wants to aggregate on either: `/nb/{book}/n/{key}` is one
/// series, and a per-note path is two thousand. A request that matched no route
/// has no template and its path is whatever a scanner put in the URL, so it is
/// labelled rather than repeated.
///
/// Split out so both branches can be tested against an exact duration. Reaching
/// the WARN branch through the middleware would need a test that genuinely
/// blocks for a second.
///
/// The duration is attached twice on purpose: `elapsed` is the human-readable
/// debug (`1.96ms`) that reads well at a terminal, `elapsed_ms` the bare number
/// to filter and total under `--log-format json`.
fn log_finished(method: &Method, route: &str, status: u16, elapsed: Duration) {
    let elapsed_ms = elapsed.as_secs_f64() * 1000.0;
    if elapsed >= SLOW_REQUEST {
        tracing::warn!(
            event = "http.slow_request",
            method = %method,
            route,
            status,
            ?elapsed,
            elapsed_ms,
            threshold_ms = SLOW_REQUEST.as_secs_f64() * 1000.0,
            "request took longer than the slow-request threshold"
        );
    } else {
        tracing::debug!(
            event = "http.request",
            method = %method,
            route,
            status,
            ?elapsed,
            elapsed_ms,
            "request finished"
        );
    }
}

/// Says that the guard turned a request away.
///
/// **WARN, and the one event here worth an alert.** Everything the guard refuses
/// is either a misconfiguration a person will hit once — a reverse proxy whose
/// name has not been allowed — or somebody attempting the DNS rebinding attack
/// the guard exists for. Both are worth knowing about, and neither is visible
/// anywhere else: the reader gets a page saying no, and the process used to say
/// nothing at all.
///
/// The `Host` and `Origin` are logged because they are the whole of what was
/// decided on, and because a refusal nobody can read the reason for is a refusal
/// nobody can fix. They are attacker-controlled, so they arrive as fields rather
/// than inside the message, where a renderer quotes and escapes them.
pub fn refused(host: Option<&str>, origin: Option<&str>, why: &str) {
    tracing::warn!(
        event = "http.refused",
        host = host.unwrap_or("<none>"),
        origin = origin.unwrap_or("<none>"),
        why,
        "the guard turned a request away"
    );
}

/// Says that a handler failed, which the reader only ever saw as a page.
///
/// The error is the one thing a 500 page shows and nothing records. It is
/// noda's own text — a git error, a path that would not read — and not the
/// request's, which is why it can go in the message.
pub fn failed(error: &str) {
    tracing::error!(
        event = "http.failed",
        error,
        "the request could not be answered"
    );
}

/// Says that the blocking pool lost a task: a panic in a handler, or a shutdown
/// under way. The reader is told the request did not finish; this is the half
/// that says so where somebody can act on it.
pub fn lost() {
    tracing::error!(
        event = "http.lost",
        "a request did not finish — the blocking task was lost"
    );
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::sync::{Arc, Mutex};

    use super::*;

    /// Everything the subscriber wrote while a test held the guard.
    #[derive(Clone, Default)]
    struct Written(Arc<Mutex<Vec<u8>>>);

    impl Written {
        fn text(&self) -> String {
            String::from_utf8(self.0.lock().expect("no test panics holding it").clone())
                .expect("the formatter writes UTF-8")
        }
    }

    impl io::Write for Written {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0
                .lock()
                .expect("no test panics holding it")
                .extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Written {
        type Writer = Self;

        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }

    /// A DEBUG subscriber for this thread only. The guard has to outlive the
    /// events, and `start` cannot be used because it installs a global one —
    /// which is right for a process that serves and wrong for a test binary
    /// that runs hundreds of them.
    fn capture() -> (Written, tracing::subscriber::DefaultGuard) {
        let written = Written::default();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(written.clone())
            .with_max_level(tracing::Level::DEBUG)
            .with_ansi(false)
            .finish();
        let guard = tracing::subscriber::set_default(subscriber);
        (written, guard)
    }

    #[test]
    fn a_finished_request_says_what_it_was_and_how_long_it_took() {
        let (log, _guard) = capture();
        log_finished(&Method::GET, "/nb/{book}", 200, Duration::from_millis(12));
        let log = log.text();
        assert!(log.contains("event=\"http.request\""), "{log}");
        assert!(log.contains("DEBUG"), "{log}");
        assert!(log.contains("method=GET"), "{log}");
        assert!(log.contains("status=200"), "{log}");
        // The number is what a JSON reader totals; the pretty one is for a person.
        assert!(log.contains("elapsed_ms=12"), "{log}");
    }

    /// The rule the whole module is built around: a note's id is the name of
    /// somebody's note, so the row carries the template it matched.
    #[test]
    fn a_note_is_logged_as_its_route_and_never_as_its_address() {
        let (log, _guard) = capture();
        log_finished(&Method::GET, "/nb/{book}/n/{key}", 200, Duration::ZERO);
        let log = log.text();
        assert!(log.contains("route=\"/nb/{book}/n/{key}\""), "{log}");
        assert!(!log.contains("k3f9m2p1"), "{log}");
    }

    #[test]
    fn a_request_that_matched_nothing_is_labelled_rather_than_repeated() {
        assert_eq!(route_of(&Extensions::new()), UNMATCHED);
    }

    /// At the threshold, not past it.
    #[test]
    fn a_request_at_the_threshold_is_a_warning_instead() {
        let (log, _guard) = capture();
        log_finished(&Method::GET, "/", 200, SLOW_REQUEST);
        let log = log.text();
        assert!(log.contains("event=\"http.slow_request\""), "{log}");
        assert!(log.contains("WARN"), "{log}");
        // The threshold travels with the event, so an alert can say what it was.
        assert!(log.contains("threshold_ms=1000"), "{log}");
    }

    #[test]
    fn a_request_just_under_it_stays_at_debug() {
        assert_eq!(SLOW_REQUEST, Duration::from_secs(1));
        let (log, _guard) = capture();
        log_finished(&Method::GET, "/", 200, Duration::from_millis(999));
        let log = log.text();
        assert!(log.contains("event=\"http.request\""), "{log}");
        assert!(!log.contains("slow_request"), "{log}");
    }

    /// A refusal is the one thing here worth waking somebody for, so it says
    /// what it was given as well as what it decided.
    #[test]
    fn a_refusal_says_what_it_was_given() {
        let (log, _guard) = capture();
        refused(Some("evil.example"), None, "the Host is not allowed");
        let log = log.text();
        assert!(log.contains("event=\"http.refused\""), "{log}");
        assert!(log.contains("WARN"), "{log}");
        assert!(log.contains("host=\"evil.example\""), "{log}");
        assert!(log.contains("origin=\"<none>\""), "{log}");
    }

    #[test]
    fn a_failure_records_what_the_page_only_showed() {
        let (log, _guard) = capture();
        failed("could not open the notebook");
        let log = log.text();
        assert!(log.contains("event=\"http.failed\""), "{log}");
        assert!(log.contains("ERROR"), "{log}");
        assert!(log.contains("could not open the notebook"), "{log}");
    }
}
