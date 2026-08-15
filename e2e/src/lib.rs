//! Browser end-to-end tests for `noda web`.
//!
//! `tests/web.rs` in the root crate already drives the server over a socket and
//! reads what it sent. That answers everything about the *bytes*. What it cannot
//! answer is anything about the page those bytes become — whether a control ends
//! up big enough for a thumb, whether the dark palette is reached at all,
//! whether a phone-width layout stays inside its width. Those need a browser
//! that has laid the page out, and this is that browser.
//!
//! The rule for what belongs here: **if the answer is in the HTML, it is a test
//! in the root crate.** A browser is slow, needs a Chromium installed, and can
//! fail for reasons that have nothing to do with noda; it earns its place only
//! on questions the markup cannot answer.
//!
//! Every scenario runs twice, with the page's own scripts enabled and disabled,
//! and has to pass both ways. PR 1 ships no script at all — which is exactly
//! when that is worth fixing in place, because the contract is easier to keep
//! than to recover once there is an enhancement layer to hide behind.

pub mod browser;
pub mod pages;
pub mod server;
pub mod wait;
pub mod world;

pub use server::Server;
