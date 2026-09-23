//! The stylesheet and scripts, linked and cached rather than inlined, which
//! re-sent tens of KB the browser already had with every full page.
//!
//! **The name is the content**: `/a/style.<hash>.css` cannot go stale, so it is
//! served `immutable`. The ways to get it wrong are serving a hash nobody wrote
//! (so a miss is a 404) and caching the page that links it (`no-cache`).
//!
//! The hash is git's blob id, git's name for "these exact bytes". There is no
//! bundle: a page links only the scripts it uses.

use std::sync::OnceLock;

use crate::web::{page, script};

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
    const ALL: [Asset; 7] = [
        Asset::Style,
        Asset::Listing,
        Asset::Standing,
        Asset::Panes,
        Asset::Beside,
        Asset::Stamps,
        Asset::Watching,
    ];

    /// The readable stem of its address.
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

    /// Sent with `nosniff`, so it is all the browser will treat it as.
    fn kind(self) -> &'static str {
        if self.css() {
            "text/css; charset=utf-8"
        } else {
            "text/javascript; charset=utf-8"
        }
    }

    /// Deterministic, which is what makes hashing it once honest.
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

    pub fn href(self) -> &'static str {
        &self.held().at
    }

    /// `defer` in the head: runs after parsing (the scripts read the rows), in
    /// the listed order, but downloads during it.
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
    /// `/a/<name>.<hash>.<ext>`.
    at: String,
    /// The last segment of `at`, which the route matches.
    file: String,
    pub body: String,
    pub kind: &'static str,
}

/// Hashed at first use, so commands that serve no page never pay for it.
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

/// Twelve hex digits of the git blob id: a cache key over a handful of
/// strings, not an identity.
fn fingerprint(body: &str) -> String {
    git2::Oid::hash_object(git2::ObjectType::Blob, body.as_bytes()).map_or_else(
        |_| "0000".to_string(),
        |oid| oid.to_string()[..12].to_string(),
    )
}

/// Exact match only: answering a stale hash with current bytes would break
/// the promise the address makes.
pub fn find(file: &str) -> Option<&'static Held> {
    held().iter().find(|held| held.file == file)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Nothing else keeps link and route in step, and layout tests would pass
    /// a page without its stylesheet.
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

    #[test]
    fn a_changed_asset_would_be_a_changed_address() {
        let sheet = fingerprint(&Asset::Style.body());
        let changed = fingerprint(&format!("{}{}", Asset::Style.body(), "body{color:red}"));
        assert_ne!(sheet, changed);
        assert_eq!(sheet.len(), 12);
    }

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

    #[test]
    fn an_address_this_build_did_not_write_is_not_answered() {
        assert!(find("style.000000000000.css").is_none());
        assert!(find("style.css").is_none());
        assert!(find("../../etc/passwd").is_none());
    }

    #[test]
    fn a_stylesheet_is_linked_and_a_script_is_deferred() {
        assert!(Asset::Style.tag().starts_with("<link rel=\"stylesheet\""));
        let script = Asset::Panes.tag();
        assert!(script.starts_with("<script src=\"/a/panes."), "{script}");
        assert!(script.ends_with(" defer></script>"), "{script}");
    }
}
