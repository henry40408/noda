//! The two things every page needs, and no page carries.
//!
//! **This file is a correction to a decision, not an addition to one.** Until
//! now the stylesheet and the scripts were written into every answer, and
//! `page::shell` said why: one request draws a whole page, which is what a
//! phone on the far end of a tailnet wants, and the alternative bought caching
//! at the price of a round trip and a question about invalidation "nothing here
//! is big enough to be worth asking".
//!
//! Both halves of that have changed.
//!
//! **The price is now paid on every page rather than saved on one.** The
//! stylesheet is 37,273 bytes and the scripts up to 17,027, against a listing's
//! own few thousand — and since the enhancement layer started asking for parts
//! of pages, most of what a reader fetches after their first page is a fragment
//! that carries none of this. What is left carrying it is the case the
//! fragments could not cover: opening a second notebook, following a link into
//! a note from outside, every form page, and every screen on a phone, where
//! the panes never split and every press is still a whole page. Each of those
//! was re-sending 46 KB of identical bytes the browser had already been given.
//!
//! **And the invalidation question answers itself when the name is the
//! content.** `/a/style.<hash>.css` cannot go stale: change the bytes and the
//! hash changes, the page points somewhere else, and the old address is one
//! nobody asks for. There is nothing to expire and nothing to purge — the only
//! two ways to get this wrong are to serve a hash you did not write, which is a
//! 404, or to let a page linking one be cached, which is what `no-cache` on
//! every page is for.
//!
//! The hash is git's, from the same `hash_object` a blob id comes from. Not for
//! any property of SHA-1 that matters here — twelve hex digits of anything
//! would do — but because a notebook is a git repository and this is the naming
//! it already uses for "these exact bytes".
//!
//! What is *not* here is a bundle. Each script is its own address and a page
//! links the ones it uses, which is the rule the inline version already
//! followed: a note page has never been sent the listing's filter, and putting
//! all of them in one file to save a request would send it 8,765 bytes to run
//! none of. The first view of a notebook costs at most four requests that are
//! never made again.

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
}

impl Asset {
    /// Every one of them, which is what the addresses are built from at startup
    /// and what a request is answered out of.
    const ALL: [Asset; 6] = [
        Asset::Style,
        Asset::Listing,
        Asset::Standing,
        Asset::Panes,
        Asset::Beside,
        Asset::Stamps,
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
        }
    }

    fn css(self) -> bool {
        self == Asset::Style
    }

    /// What noda says it is, and it is the only thing the browser will treat it
    /// as — `nosniff` goes out beside it.
    fn kind(self) -> &'static str {
        if self.css() {
            "text/css; charset=utf-8"
        } else {
            "text/javascript; charset=utf-8"
        }
    }

    /// The bytes, built once. `theme::stylesheet` formats the palette into
    /// custom properties and the rest is `const`, so this is the same string
    /// every time it is asked for — which is what makes hashing it once honest.
    fn body(self) -> String {
        match self {
            Asset::Style => format!("{}{}", crate::web::theme::stylesheet(), page::stylesheet()),
            Asset::Listing => script::LISTING.to_string(),
            Asset::Standing => script::STANDING.to_string(),
            Asset::Panes => script::PANES.to_string(),
            Asset::Beside => script::BESIDE.to_string(),
            Asset::Stamps => script::STAMPS.to_string(),
        }
    }

    /// Where it is served, hash and all.
    pub fn href(self) -> &'static str {
        &self.held().at
    }

    /// The element that links it, which is the only difference between the two
    /// kinds worth having in the markup.
    ///
    /// `defer` and not the end of the body: the scripts read the rows, so they
    /// have to run after the document is parsed, and a deferred script in the
    /// head starts downloading while it still is. They run in the order they
    /// are written, which is the order the page lists them in.
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

/// All of them, hashed once.
///
/// At first use rather than at startup: `noda ls` is a process that will never
/// serve a page, and the release profile is tuned for a binary that starts
/// quickly. Hashing five strings costs microseconds, and the ones it costs are
/// spent by `noda web` on its way to the first request.
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

/// Twelve hex digits of the git blob id these bytes would have.
///
/// Short on purpose: this is a cache key, not an identity, and the whole set of
/// them is five strings written by this repository. The failure it has to rule
/// out is a stale page reaching a changed asset, which any change to the hash
/// rules out.
fn fingerprint(body: &str) -> String {
    git2::Oid::hash_object(git2::ObjectType::Blob, body.as_bytes()).map_or_else(
        |_| "0000".to_string(),
        |oid| oid.to_string()[..12].to_string(),
    )
}

/// The asset a request named, if this build wrote it.
///
/// A miss is a miss and not the current version of the same name: an address
/// with a hash in it is a promise about the bytes behind it, and answering a
/// hash nobody wrote with different bytes would be the one way this scheme can
/// lie. It is also unreachable in practice — every page carries the addresses
/// this build wrote, and a page is never cached.
pub fn find(file: &str) -> Option<&'static Held> {
    held().iter().find(|held| held.file == file)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The link on the page and the address the route answers are the same
    /// string, and nothing else keeps them that way — a page pointing at a hash
    /// this build does not serve is a page with no stylesheet, which every
    /// layout test would go on passing through.
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

    /// The name is the content. Two assets that differ have to differ in the
    /// address, or one of them is served under the other's cache entry — for a
    /// year, which is what `immutable` asks for.
    #[test]
    fn a_changed_asset_would_be_a_changed_address() {
        let sheet = fingerprint(&Asset::Style.body());
        let changed = fingerprint(&format!("{}{}", Asset::Style.body(), "body{color:red}"));
        assert_ne!(sheet, changed);
        assert_eq!(sheet.len(), 12);
    }

    /// Every address is distinct, which the hash alone does not promise: two
    /// scripts that happened to hold the same bytes would collide on it.
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

    /// A hash nobody wrote is nothing, rather than the current bytes under an
    /// address that promised different ones.
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
