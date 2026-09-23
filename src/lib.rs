//! noda — a git-native notebook for your terminal.
//!
//! Everything is a library so the CLI stays a thin shell and commands are tested
//! without spawning a process.

pub mod cmd;
pub mod config;
pub mod error;
pub mod import;
pub mod link;
pub mod note;
pub mod notebook;
pub mod paths;
pub mod query;
pub mod remote;
pub mod sign;
pub mod style;
pub mod todo;
pub mod tui;
pub mod web;

pub use error::{Error, Result};
pub use paths::Paths;

/// The build as `build.rs` described it: a release tag (`0.2.0`), commits since
/// one (`0.2.0-3-gabc1234`), or a bare commit — not `Cargo.toml`'s placeholder
/// `version`. Defined once because `--version` and the web status screen both
/// show it.
pub const VERSION: &str = env!("NODA_VERSION");
