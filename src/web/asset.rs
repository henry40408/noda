//! The two things every page needs, and no page carries.
//!
//! **A correction to a decision, not an addition to one.** These used to be
//! written into every answer, on the argument that one request draws a whole
//! page and nothing here was big enough to be worth a round trip. Both halves
//! of that changed: the stylesheet is 39,598 bytes and the four scripts 28,458,
//! and once the enhancement layer started asking for fragments, what was left
//! carrying them — a second notebook, a link in from outside, every form page,
//! every screen on a phone — was re-sending 46 KB the browser already had.
//!
//! **Invalidation answers itself when the name is the content.**
//! `/a/style.<hash>.css` cannot go stale, so there is nothing to expire. The two
//! ways to get it wrong are serving a hash nobody wrote, which is a 404, and
//! caching a page that links one, which `no-cache` prevents.
//!
//! The hash is git's `hash_object` — not for any property of SHA-1, but because
//! a notebook is a git repository and this is its name for "these exact bytes".
//!
//! No bundle: a page links only the scripts it uses, as the inline version did.
//! One file would send a note page 8,765 bytes to run none of.

use std::sync::OnceLock;

use crate::web::{page, script};

/// One thing a page links to rather than carries.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Asset {
    /// The whole of the layout, both themes included.
    Style,
    /// The listing's filter.
    Listing,
    /// The network screen's poll.
    Standing,
    /// The two panes.
    Panes,
    /// The margin note.
    Beside,
    /// Every stamp, in the reader's own zone.
    Stamps,
    /// The editor's ear on the note under it.
    Watching,
}

impl Asset {
    /// What the addresses are built from, and what a request is answered out of.
    const ALL: [Asset; 7] = [
        Asset::Style,
        Asset::Listing,
        Asset::Standing,
        Asset::Panes,
        Asset::Beside,
        Asset::Stamps,
        Asset::Watching,
    ];

    /// The stem of its address — the half a person reads.
    fn name(self) -> &'static str {
        match self {
            Asset::Style => "style",
            Asset::Listing => "listing",
            Asset::Standing => "standing",
            Asset::Panes => "panes",
            Asset::Beside => "beside",
            Asset::Stamps => "stamps",
            Asset::Watching => "watching",
        }
    }

    fn css(self) -> bool {
        self == Asset::Style
    }

    /// The only thing the browser will treat it as: `nosniff` goes out beside.
    fn kind(self) -> &'static str {
        if self.css() {
            "text/css; charset=utf-8"
        } else {
            "text/javascript; charset=utf-8"
        }
    }

    /// The same string every time it is asked for, which is what makes hashing
    /// it once honest.
    fn body(self) -> String {
        match self {
            Asset::Style => format!("{}{}", crate::web::theme::stylesheet(), page::stylesheet()),
            Asset::Listing => script::LISTING.to_string(),
            Asset::Standing => script::STANDING.to_string(),
            Asset::Panes => script::PANES.to_string(),
            Asset::Beside => script::BESIDE.to_string(),
            Asset::Stamps => script::STAMPS.to_string(),
            Asset::Watching => script::WATCHING.to_string(),
        }
    }

    /// Where it is served, hash and all.
    pub fn href(self) -> &'static str {
        &self.held().at
    }

    /// `defer` and not the end of the body: the scripts read the rows so they
    /// must run after parsing, and a deferred script in the head downloads while
    /// parsing is still going. They run in the order the page lists them.
    pub fn tag(self) -> String {
        if self.css() {
            format!("<link rel=\"stylesheet\" href=\"{}\">", self.href())
        } else {
            format!("<script src=\"{}\" defer></script>", self.href())
        }
    }

    fn held(self) -> &'static Held {
        let at = Asset::ALL
            .iter()
            .position(|held| *held == self)
            .expect("every asset is in ALL");
        &held()[at]
    }
}

/// An asset with its address worked out.
pub struct Held {
    /// The path it answers at: `/a/<name>.<hash>.<ext>`.
    at: String,
    /// The last segment of that path, which is what the route matches.
    file: String,
    pub body: String,
    pub kind: &'static str,
}

/// All of them, hashed at first use rather than at startup — `noda ls` will
/// never serve a page, and the release profile is tuned for a quick start.
fn held() -> &'static Vec<Held> {
    static HELD: OnceLock<Vec<Held>> = OnceLock::new();
    HELD.get_or_init(|| {
        Asset::ALL
            .iter()
            .map(|asset| {
                let body = asset.body();
                let file = format!(
                    "{}.{}.{}",
                    asset.name(),
                    fingerprint(&body),
                    if asset.css() { "css" } else { "js" }
                );
                Held {
                    at: format!("/a/{file}"),
                    file,
                    body,
                    kind: asset.kind(),
                }
            })
            .collect()
    })
}

/// Twelve hex digits of the git blob id these bytes would have. Short on
/// purpose: a cache key over five strings, not an identity.
fn fingerprint(body: &str) -> String {
    git2::Oid::hash_object(git2::ObjectType::Blob, body.as_bytes()).map_or_else(
        |_| "0000".to_string(),
        |oid| oid.to_string()[..12].to_string(),
    )
}

/// A miss is a miss, never the current version of the same name: a hashed
/// address is a promise about the bytes behind it, and answering one nobody
/// wrote is the single way this scheme can lie.
pub fn find(file: &str) -> Option<&'static Held> {
    held().iter().find(|held| held.file == file)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Nothing else keeps the link and the route in step, and a page pointing at
    /// an unserved hash has no stylesheet — which every layout test would pass.
    #[test]
    fn what_a_page_links_is_what_the_route_answers() {
        for asset in Asset::ALL {
            let href = asset.href();
            let file = href.strip_prefix("/a/").expect("assets live under /a/");
            let found = find(file).expect("the route answers what the page links");
            assert_eq!(found.body, asset.body());
            assert_eq!(found.kind, asset.kind());
        }
    }

    /// Two assets that differ must differ in the address, or one is served under
    /// the other's cache entry for the year `immutable` asks for.
    #[test]
    fn a_changed_asset_would_be_a_changed_address() {
        let sheet = fingerprint(&Asset::Style.body());
        let changed = fingerprint(&format!("{}{}", Asset::Style.body(), "body{color:red}"));
        assert_ne!(sheet, changed);
        assert_eq!(sheet.len(), 12);
    }

    /// The hash alone does not promise this: equal bytes would collide.
    #[test]
    fn no_two_assets_answer_at_one_address() {
        let mut seen = std::collections::BTreeSet::new();
        for asset in Asset::ALL {
            assert!(
                seen.insert(asset.href()),
                "{} is served twice",
                asset.name()
            );
        }
    }

    /// A hash nobody wrote is nothing, not the current bytes under it.
    #[test]
    fn an_address_this_build_did_not_write_is_not_answered() {
        assert!(find("style.000000000000.css").is_none());
        assert!(find("style.css").is_none());
        assert!(find("../../etc/passwd").is_none());
    }

    /// The two kinds are linked the two ways a browser knows.
    #[test]
    fn a_stylesheet_is_linked_and_a_script_is_deferred() {
        assert!(Asset::Style.tag().starts_with("<link rel=\"stylesheet\""));
        let script = Asset::Panes.tag();
        assert!(script.starts_with("<script src=\"/a/panes."), "{script}");
        assert!(script.ends_with(" defer></script>"), "{script}");
    }
}
