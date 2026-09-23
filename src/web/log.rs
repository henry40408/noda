//! What the server says about itself while it runs.
//!
//! **The log goes to stderr**, because stdout carries the address to open — the
//! command's answer, and where the test harnesses read the port from.
//! `tracing_subscriber::fmt` defaults to stdout.
//!
//! **The default `error,noda=info` is quiet**: a healthy server logs nothing per
//! request; `RUST_LOG=noda=debug` turns that on. The startup lines are not
//! events, since they are already the answer on stdout.

use std::time::{Duration, Instant};

use axum::extract::{MatchedPath, Request};
use axum::http::{Extensions, Method};
use axum::middleware::Next;
use axum::response::Response;
use tracing_subscriber::Layer as _;
use tracing_subscriber::filter::Targets;
use tracing_subscriber::layer::SubscriberExt as _;
use tracing_subscriber::util::SubscriberInitExt as _;

/// A person or a program; the subscriber's `full`/`compact`/`pretty` are not
/// offered.
#[derive(Clone, Copy, Debug, Default, clap::ValueEnum)]
pub enum Format {
    /// For a person, at a terminal.
    #[default]
    Text,
    /// One JSON object per line, for something that collects them.
    Json,
}

const DEFAULT_FILTER: &str = "error,noda=info";

/// A request this slow is a WARN rather than a DEBUG. The most expensive page
/// is tens of milliseconds at two thousand notes, so a second means something
/// is wrong.
pub const SLOW_REQUEST: Duration = Duration::from_secs(1);

/// The `route` of a request that matched nothing; never its path (see
/// `log_finished`).
pub const UNMATCHED: &str = "<unmatched>";

/// Installs the subscriber. Called once, by `serve`.
///
/// **`Targets`, not `EnvFilter`**, whose regex engine measured 355 KB (69% of
/// logging's cost). `Targets` reads the same directives but cannot filter on
/// spans or fields, which nothing here uses.
///
/// **`RUST_LOG` is layered onto the default**, because `Targets` reads a bare
/// word as a target at `TRACE`: a typo would otherwise silence everything,
/// warnings included. A bare level is the exception and replaces the default
/// (`RUST_LOG=off` means all of it).
pub fn start(format: Format) {
    let filter = wanted(std::env::var("RUST_LOG").ok().as_deref());
    // `anstream`'s decision, made by hand for the subscriber.
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

/// The filter for a `RUST_LOG` value; see `start`.
fn wanted(rust_log: Option<&str>) -> Targets {
    let asked = rust_log.and_then(|directives| directives.parse::<Targets>().ok());
    let base = match asked.as_ref().and_then(Targets::default_level) {
        // A bare level replaces noda's floor too.
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

/// Layered outermost, so it also sees requests the guard refuses.
pub async fn timed(request: Request, next: Next) -> Response {
    let method = request.method().clone();
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
/// **The route is the matched template, never the path**: ids, filenames and
/// query strings are somebody's notes and words, and a template is also what
/// one aggregates on. An unmatched path is labelled, not repeated.
///
/// `elapsed` is for a terminal, `elapsed_ms` for totalling under
/// `--log-format json`.
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

/// **WARN, the one event worth an alert**: a refusal is a misconfiguration or
/// the rebinding attack the guard exists for.
///
/// `Host` and `Origin` are what was decided on. Being attacker-controlled, they
/// go in fields, which the formatter quotes and escapes, not the message.
pub fn refused(host: Option<&str>, origin: Option<&str>, why: &str) {
    tracing::warn!(
        event = "http.refused",
        host = host.unwrap_or("<none>"),
        origin = origin.unwrap_or("<none>"),
        why,
        "the guard turned a request away"
    );
}

/// What a 500 page showed. noda's own text, so it may go in the message.
pub fn failed(error: &str) {
    tracing::error!(
        event = "http.failed",
        error,
        "the request could not be answered"
    );
}

/// A panic in a handler, or a shutdown under way.
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

    /// Thread-local, unlike `start`'s global subscriber; hold the guard.
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
        assert!(log.contains("elapsed_ms=12"), "{log}");
    }

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

    #[test]
    fn a_request_at_the_threshold_is_a_warning_instead() {
        let (log, _guard) = capture();
        log_finished(&Method::GET, "/", 200, SLOW_REQUEST);
        let log = log.text();
        assert!(log.contains("event=\"http.slow_request\""), "{log}");
        assert!(log.contains("WARN"), "{log}");
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

    /// `Targets` reads a bare word as a target name at `TRACE`.
    #[test]
    fn a_mistyped_rust_log_cannot_silence_the_server() {
        let typo = wanted(Some("nonsense"));
        assert!(
            typo.would_enable("noda::web::log", &tracing::Level::WARN),
            "a refusal must still be said: {typo:?}"
        );
        assert!(
            typo.would_enable("nonsense", &tracing::Level::TRACE),
            "{typo:?}"
        );
    }

    #[test]
    fn what_rust_log_asks_for_wins_over_the_default() {
        let asked = wanted(Some("noda=debug"));
        assert!(asked.would_enable("noda::web::log", &tracing::Level::DEBUG));
        assert!(
            asked.would_enable("git2", &tracing::Level::ERROR),
            "{asked:?}"
        );
    }

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
