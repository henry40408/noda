//! What the server says about itself while it runs.
//!
//! **Every other command answers and exits, so its output is its answer.**
//! `noda web` outlives the question, so the only way to find out afterwards what
//! it refused or failed at is for it to have said so at the time.
//!
//! **The log goes to stderr**, because stdout carries the address to type into a
//! phone — the command's answer, and the line the test harnesses read the port
//! out of. `tracing_subscriber::fmt` defaults to stdout, which would put a
//! timestamp in front of that answer.
//!
//! **What is on a log line is `RUST_LOG`'s business.** The default
//! `error,noda=info` is quiet: a healthy server logs nothing per request, and
//! `RUST_LOG=noda=debug` turns the stream on.
//!
//! The startup lines are deliberately **not** events: they are the answer,
//! already on stdout, and announcing an address twice is not greppable.

use std::time::{Duration, Instant};

use axum::extract::{MatchedPath, Request};
use axum::http::{Extensions, Method};
use axum::middleware::Next;
use axum::response::Response;
use tracing_subscriber::Layer as _;
use tracing_subscriber::filter::Targets;
use tracing_subscriber::layer::SubscriberExt as _;
use tracing_subscriber::util::SubscriberInitExt as _;

/// Two and not four: `full`, `compact` and `pretty` are the subscriber's
/// vocabulary, and the question a reader has is whether a person or a program is
/// reading.
#[derive(Clone, Copy, Debug, Default, clap::ValueEnum)]
pub enum Format {
    /// For a person, at a terminal.
    #[default]
    Text,
    /// One JSON object per line, for something that collects them.
    Json,
}

/// Other crates at `error`, noda's at `info`. A server logging every request by
/// default has a log to be turned *down* before it is useful.
const DEFAULT_FILTER: &str = "error,noda=info";

/// A request this slow is a WARN rather than a DEBUG.
///
/// Nothing noda does per request should come near a second — the most expensive
/// page is tens of milliseconds at two thousand notes — so this is not a
/// threshold that trims a busy log but the number above which something is
/// wrong.
pub const SLOW_REQUEST: Duration = Duration::from_secs(1);

/// The `route` of a request that matched nothing. Deliberately not the path it
/// asked for — see `log_finished`.
pub const UNMATCHED: &str = "<unmatched>";

/// Installs the subscriber. Called once, by `serve`.
///
/// **`Targets` and not `EnvFilter`**, whose regex engine measured 355 KB — 69%
/// of everything logging cost — in a binary whose cold start is a feature and
/// whose tree holds no other regex. `Targets` reads the same directives; what it
/// will not do is filter on spans and fields, and nothing here writes either.
///
/// **`RUST_LOG` is layered onto the default rather than replacing it**, which is
/// `Targets`' one sharp edge: it reads a bare word as a *target name* at
/// `TRACE`, so any typo parses happily into a filter naming a target nothing
/// writes to and silences everything, warnings included. Starting from the
/// default means the worst a typo does is fail to have its effect.
///
/// A level on its own is the exception and replaces the default outright:
/// `RUST_LOG=off` means all of it.
pub fn start(format: Format) {
    let filter = wanted(std::env::var("RUST_LOG").ok().as_deref());
    // `anstream`'s two questions, asked by hand because a subscriber cannot read
    // its mind: is colour wanted, and can anything show it.
    let colour = std::env::var_os("NO_COLOR").is_none()
        && std::io::IsTerminal::is_terminal(&std::io::stderr());
    // `fmt()`'s builder takes an `EnvFilter`; this one is on the layer.
    let layer = tracing_subscriber::fmt::layer()
        .with_writer(std::io::stderr)
        .with_ansi(colour);
    let layer = match format {
        Format::Text => layer.with_filter(filter).boxed(),
        Format::Json => layer.json().with_filter(filter).boxed(),
    };
    tracing_subscriber::registry().with(layer).init();
}

/// Split out of `start` because the interesting half is what it does with a line
/// somebody typed wrong.
fn wanted(rust_log: Option<&str>) -> Targets {
    let asked = rust_log.and_then(|directives| directives.parse::<Targets>().ok());
    let base = match asked.as_ref().and_then(Targets::default_level) {
        // A bare level was given, and it means everything: noda's own floor
        // goes with the rest.
        Some(level) => Targets::new().with_default(level),
        None => DEFAULT_FILTER.parse().expect("the default filter parses"),
    };
    match asked {
        Some(asked) => base.with_targets(
            asked
                .iter()
                .map(|(target, level)| (target.to_owned(), level)),
        ),
        None => base,
    }
}

/// Layered outermost, so it also sees the requests the guard refuses without
/// ever reaching a handler.
pub async fn timed(request: Request, next: Next) -> Response {
    let method = request.method().clone();
    // Before `next` consumes the request, and neither is expensive.
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
/// **The route is the template the router matched, never the path asked for**,
/// and here that is a stronger rule than in most servers: an id is the name of
/// somebody's note, a filename is often the whole of what it is about, and a
/// query string is the reader's own words. None of it belongs in a file that
/// outlives the request — nor is it what anybody aggregates on, `/nb/{book}/n/
/// {key}` being one series against two thousand. An unmatched path is whatever a
/// scanner put in the URL, so it is labelled rather than repeated.
///
/// Split out so both branches can be tested against an exact duration. The
/// duration is attached twice: `elapsed` reads well at a terminal, `elapsed_ms`
/// is the bare number to total under `--log-format json`.
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

/// **WARN, and the one event here worth an alert.** Everything the guard refuses
/// is either a misconfiguration hit once or the rebinding attack the guard exists
/// for, and neither is visible anywhere else.
///
/// `Host` and `Origin` are logged because they are the whole of what was decided
/// on. Being attacker-controlled, they arrive as fields rather than inside the
/// message, where a renderer quotes and escapes them.
pub fn refused(host: Option<&str>, origin: Option<&str>, why: &str) {
    tracing::warn!(
        event = "http.refused",
        host = host.unwrap_or("<none>"),
        origin = origin.unwrap_or("<none>"),
        why,
        "the guard turned a request away"
    );
}

/// The one thing a 500 page shows and nothing records. noda's own text and not
/// the request's, which is why it can go in the message.
pub fn failed(error: &str) {
    tracing::error!(
        event = "http.failed",
        error,
        "the request could not be answered"
    );
}

/// A panic in a handler, or a shutdown under way. The reader is told the request
/// did not finish; this says so where somebody can act on it.
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

    /// This thread only: the guard has to outlive the events, and `start`
    /// installs a global one — right for a server, wrong for a test binary.
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

    /// A note's id is the name of somebody's note, so the row carries the
    /// template it matched.
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

    /// The one thing worth waking somebody for, so it says what it was given as
    /// well as what it decided.
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

    /// The sharp edge, pinned: `Targets` reads a bare word as a target name at
    /// `TRACE`, so a mistyped `RUST_LOG` parses into a filter that says nothing —
    /// a log gone silent from a typo in a unit file.
    #[test]
    fn a_mistyped_rust_log_cannot_silence_the_server() {
        let typo = wanted(Some("nonsense"));
        assert!(
            typo.would_enable("noda::web::log", &tracing::Level::WARN),
            "a refusal must still be said: {typo:?}"
        );
        // And what was actually asked for is still applied on top of it.
        assert!(
            typo.would_enable("nonsense", &tracing::Level::TRACE),
            "{typo:?}"
        );
    }

    #[test]
    fn what_rust_log_asks_for_wins_over_the_default() {
        let asked = wanted(Some("noda=debug"));
        assert!(asked.would_enable("noda::web::log", &tracing::Level::DEBUG));
        // Other crates keep the default's floor rather than going silent.
        assert!(
            asked.would_enable("git2", &tracing::Level::ERROR),
            "{asked:?}"
        );
    }

    /// A level on its own means all of it, including noda's own floor.
    #[test]
    fn a_bare_level_replaces_the_default_outright() {
        let off = wanted(Some("off"));
        assert!(
            !off.would_enable("noda::web::log", &tracing::Level::ERROR),
            "{off:?}"
        );
        let all = wanted(Some("debug"));
        assert!(all.would_enable("git2", &tracing::Level::DEBUG), "{all:?}");
    }

    #[test]
    fn nothing_set_is_the_default_filter() {
        let quiet = wanted(None);
        assert!(quiet.would_enable("noda::web::log", &tracing::Level::INFO));
        assert!(!quiet.would_enable("noda::web::log", &tracing::Level::DEBUG));
        assert!(!quiet.would_enable("git2", &tracing::Level::WARN));
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
