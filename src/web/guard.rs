//! Who is allowed to be talking to this server.
//!
//! There are no accounts, deliberately — this is meant to sit on a tailnet or
//! behind something that already authenticates. That does not cover the two
//! attacks needing no account, because they borrow a browser you already have
//! open, and these checks are the whole defence against them.
//!
//! **Cross-site requests.** Every write is a git commit, and a form on any other
//! site posts to `localhost:8080` with your browser's reach. No session means no
//! cookie to be missing, so only `Origin` stops it.
//!
//! **DNS rebinding**, which `Origin` alone does not stop: point `evil.example`
//! at `127.0.0.1` and both headers agree. What gives it away is that the trick
//! needs a *name* to control the resolution of — so a bare address always
//! passes, and a name must be one that was asked for.
//!
//! Hence `--allow-host`: behind a proxy or on a tailnet the name in the URL bar
//! is a name, and refusing it silently would break both recommended
//! deployments. It fails closed and says what to add.

use std::net::IpAddr;

/// Why a request was turned away, in the words the reader needs to fix it.
pub struct Refusal(pub String);

/// The hostnames this server answers to.
pub struct Guard {
    allowed: Vec<String>,
}

impl Guard {
    /// `extra` is `--allow-host`: names that are not addresses and are wanted.
    pub fn new(extra: &[String]) -> Self {
        Guard {
            allowed: extra.iter().map(|name| name.to_lowercase()).collect(),
        }
    }

    /// Both headers as the client sent them. A missing `Host` is a refusal —
    /// HTTP/1.1 requires it — but a missing `Origin` cannot be, because every
    /// ordinary navigation omits it.
    pub fn admits(&self, host: Option<&str>, origin: Option<&str>) -> Result<(), Refusal> {
        let Some(host) = host else {
            return Err(Refusal("the request carried no Host header".into()));
        };
        self.admits_host(host)?;

        // A sandboxed frame and a `file://` page send `null`, which the
        // comparison below would refuse anyway. Named separately because it is a
        // different thing to be told than "some other site".
        match origin {
            None => Ok(()),
            Some("null") => Err(Refusal(
                "the request came from an opaque origin, which cannot be checked".into(),
            )),
            Some(origin) => {
                let from = authority(origin);
                if from.eq_ignore_ascii_case(host) {
                    Ok(())
                } else {
                    Err(Refusal(format!(
                        "the request says it came from {origin}, which is not {host} — \
                         a page on another site cannot make changes here"
                    )))
                }
            }
        }
    }

    fn admits_host(&self, host: &str) -> Result<(), Refusal> {
        let name = hostname(host);
        // An address cannot be rebound. `localhost` joins it because every
        // resolver pins it, and refusing the one name people actually type would
        // be a guard nobody gets past.
        if name.parse::<IpAddr>().is_ok() || name.eq_ignore_ascii_case("localhost") {
            return Ok(());
        }
        if self
            .allowed
            .iter()
            .any(|allowed| allowed.eq_ignore_ascii_case(name))
        {
            return Ok(());
        }
        Err(Refusal(format!(
            "this server was not told to answer to the name {name} — \
             start it with `--allow-host {name}` if that is where you meant to reach it"
        )))
    }
}

/// The `host:port` out of an origin. Only the scheme is dropped: `:8080` and
/// `:8081` are different origins to a browser, so they must differ here too.
fn authority(origin: &str) -> &str {
    origin
        .split_once("://")
        .map_or(origin, |(_, rest)| rest)
        .trim_end_matches('/')
}

/// The name out of a `host:port`. `[::1]:8080` is one colon too many for the
/// obvious split, so the bracketed form is handled first.
fn hostname(host: &str) -> &str {
    if let Some(rest) = host.strip_prefix('[') {
        return rest.split_once(']').map_or(rest, |(inside, _)| inside);
    }
    host.split_once(':').map_or(host, |(name, _)| name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain() -> Guard {
        Guard::new(&[])
    }

    #[test]
    fn an_address_is_admitted_without_being_asked_for() {
        for host in [
            "127.0.0.1:8080",
            "localhost:8080",
            "localhost",
            "192.168.1.4:8080",
            "[::1]:8080",
        ] {
            assert!(
                plain().admits(Some(host), None).is_ok(),
                "{host} should have been admitted"
            );
        }
    }

    /// The rebinding case: nothing in the request says otherwise, which is why
    /// the name itself has to be checked.
    #[test]
    fn a_name_nobody_asked_for_is_refused_even_when_the_origin_agrees() {
        let refusal = plain()
            .admits(Some("evil.example"), Some("http://evil.example"))
            .expect_err("a name that was not asked for");
        assert!(
            refusal.0.contains("--allow-host evil.example"),
            "{}",
            refusal.0
        );
    }

    #[test]
    fn a_name_that_was_asked_for_is_admitted() {
        let guard = Guard::new(&["noda.tail1234.ts.net".to_string()]);
        assert!(
            guard
                .admits(
                    Some("noda.tail1234.ts.net"),
                    Some("https://noda.tail1234.ts.net")
                )
                .is_ok()
        );
    }

    #[test]
    fn another_site_cannot_reach_in() {
        let refusal = plain()
            .admits(Some("127.0.0.1:8080"), Some("https://elsewhere.example"))
            .expect_err("a cross-site request");
        assert!(refusal.0.contains("elsewhere.example"), "{}", refusal.0);
    }

    /// Two servers on one machine are two sites.
    #[test]
    fn a_different_port_is_a_different_site() {
        assert!(
            plain()
                .admits(Some("127.0.0.1:8080"), Some("http://127.0.0.1:8081"))
                .is_err()
        );
    }

    /// If a missing `Origin` were a refusal, the server would answer nobody.
    #[test]
    fn an_ordinary_navigation_carries_no_origin_and_is_fine() {
        assert!(plain().admits(Some("127.0.0.1:8080"), None).is_ok());
    }

    #[test]
    fn an_opaque_origin_is_named_rather_than_lumped_in() {
        let refusal = plain()
            .admits(Some("127.0.0.1:8080"), Some("null"))
            .expect_err("an opaque origin");
        assert!(refusal.0.contains("opaque"), "{}", refusal.0);
    }

    #[test]
    fn a_request_with_no_host_is_refused() {
        assert!(plain().admits(None, None).is_err());
    }
}
