//! Who is allowed to be talking to this server.
//!
//! There are no accounts: the server is meant for a tailnet or behind an
//! authenticating proxy. These checks are the whole defence against the two
//! attacks that borrow a browser you already have open:
//!
//! **Cross-site requests.** A form on any site can post to `localhost:8080`,
//! and with no session cookie to be missing, only `Origin` stops it.
//!
//! **DNS rebinding**, where `evil.example` resolves to `127.0.0.1` and both
//! headers agree. The attack needs a *name*, so a bare address always passes
//! and a name must be one given with `--allow-host` (needed behind a proxy or on
//! a tailnet). It fails closed and says what to add.

use std::net::IpAddr;

/// Why a request was turned away, in the words the reader needs to fix it.
pub struct Refusal(pub String);

/// The hostnames this server answers to.
pub struct Guard {
    allowed: Vec<String>,
}

impl Guard {
    /// `extra` is `--allow-host`.
    pub fn new(extra: &[String]) -> Self {
        Guard {
            allowed: extra.iter().map(|name| name.to_lowercase()).collect(),
        }
    }

    /// A missing `Host` is refused (HTTP/1.1 requires it); a missing `Origin`
    /// is not, as ordinary navigation omits it.
    pub fn admits(&self, host: Option<&str>, origin: Option<&str>) -> Result<(), Refusal> {
        let Some(host) = host else {
            return Err(Refusal("the request carried no Host header".into()));
        };
        self.admits_host(host)?;

        // A sandboxed frame or `file://` page sends `null`; it would fail the
        // comparison anyway, but deserves its own message.
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
        // An address cannot be rebound, and resolvers pin `localhost`.
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

/// The `host:port` out of an origin; the port is kept, since it is part of
/// the origin.
fn authority(origin: &str) -> &str {
    origin
        .split_once("://")
        .map_or(origin, |(_, rest)| rest)
        .trim_end_matches('/')
}

/// The name out of a `host:port`, including a bracketed `[::1]:8080`.
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

    /// DNS rebinding: only the name gives it away.
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

    #[test]
    fn a_different_port_is_a_different_site() {
        assert!(
            plain()
                .admits(Some("127.0.0.1:8080"), Some("http://127.0.0.1:8081"))
                .is_err()
        );
    }

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
