//! Browser end-to-end tests for `noda web`.
//!
//! Only for what the markup cannot answer — touch-target size, whether the dark
//! palette is reached, whether a phone-width layout fits. **If the answer is in
//! the HTML, it is a test in the root crate's `tests/web.rs`:** a browser is
//! slow, needs Chrome, and fails for reasons unrelated to noda.
//!
//! Every scenario runs with the page's scripts enabled and disabled; see
//! `tests/e2e/main.rs`.

pub mod browser;
pub mod pages;
pub mod server;
pub mod wait;
pub mod world;

pub use server::Server;
