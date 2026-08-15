//! Who is allowed to be talking to this server.
//!
//! noda's web server has no accounts and no login, deliberately: the way it is
//! meant to be reached is over a tailnet or behind something that already does
//! authentication, and both of those do the job better than a notebook could.
//! What that does *not* excuse is the pair of attacks that need no account at
//! all, because they borrow the browser you already have open.
//!
//! **Cross-site requests.** Every write here is a git commit. A page on any
//! other site can make your browser send one — a form that posts to
//! `http://localhost:8080/…` submits with your browser's own reach, not the
//! attacker's. There is no cookie to be missing, because there is no session, so
//! nothing stops it except noticing where the request says it came from. Hence
//! the `Origin` check, and hence it is here in the first pull request rather
//! than in the one that adds the writes: a guard added after the thing it guards
//! is a guard that was missing for a release.
//!
//! **DNS rebinding.** The other half, and the reason an `Origin` check alone is
//! not enough. An attacker points `evil.example` at `127.0.0.1`, gets your
//! browser to load it, and every request it then makes is same-origin by every
//! test the browser applies — `Origin` and `Host` agree, because both say
//! `evil.example`. What gives it away is that the name is a *name*: a rebinding
//! attack needs one, because the whole trick is controlling what it resolves to.
//! So a `Host` that is a bare address is always fine, and a `Host` that is a
//! name has to be one that was asked for.
//!
//! That last rule is what makes `--allow-host` necessary rather than an extra:
//! behind a reverse proxy or on a tailnet, the name in the URL bar *is* a name,
//! and refusing it silently would break the two deployments the documentation
//! recommends. It fails closed and says exactly what to add.

use std::net::IpAddr;

/// Why a request was turned away, in the words the reader needs to fix it.
pub struct Refusal(pub String);

/// The hostnames this server answers to.
pub struct Guard {
    allowed: Vec<String>,
}

impl Guard {
    /// `extra` is what `--allow-host` was given: the names that are not
    /// addresses and are wanted anyway.
    pub fn new(extra: &[String]) -> Self {
        Guard {
            allowed: extra.iter().map(|name| name.to_lowercase()).collect(),
        }
    }

    /// Whether this request may be answered.
    ///
    /// Both headers as the client sent them: `Host` is required by HTTP/1.1 and
    /// its absence is a refusal rather than a pass, and `Origin` is absent on
    /// every ordinary navigation — typing an address, following a link — which
    /// is why its absence cannot be a refusal either.
    pub fn admits(&self, host: Option<&str>, origin: Option<&str>) -> Result<(), Refusal> {
        let Some(host) = host else {
            return Err(Refusal("the request carried no Host header".into()));
        };
        self.admits_host(host)?;

        // `null` is what a sandboxed frame and a `file://` page send. It matches
        // no host, so the comparison below would refuse it anyway; it is named
        // here because "the Origin was null" is a different thing to be told
        // than "the Origin was some other site".
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
        // An address cannot be rebound; that is the whole of the reason this is
        // allowed without being asked for. `localhost` joins it because every
        // resolver on every platform pins it, and refusing the one name people
        // actually type would be a guard nobody gets past.
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

/// The `host:port` out of an origin, which is written as a whole URL.
///
/// Only the scheme is dropped. The port is kept, because `:8080` and `:8081` are
/// different origins to a browser and have to be different here too.
fn authority(origin: &str) -> &str {
    origin
        .split_once("://")
        .map_or(origin, |(_, rest)| rest)
        .trim_end_matches('/')
}

/// The name out of a `host:port`, with an IPv6 literal's brackets taken off.
///
/// `[::1]:8080` is one colon-separated string too many for the obvious split, so
/// the bracketed form is handled before it rather than after.
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

    /// The rebinding case: the name resolves to this machine and the browser
    /// believes every request to it is same-origin. Nothing in the request says
    /// otherwise — which is why the name itself is what has to be checked.
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

    /// A different port is a different origin to a browser, so it has to be one
    /// here: two servers on one machine are two sites.
    #[test]
    fn a_different_port_is_a_different_site() {
        assert!(
            plain()
                .admits(Some("127.0.0.1:8080"), Some("http://127.0.0.1:8081"))
                .is_err()
        );
    }

    /// Typing an address into the bar sends no `Origin` at all. If that were a
    /// refusal the server would answer nothing to anybody.
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
