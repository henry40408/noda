//! noda — a git-native notebook for your terminal.
//!
//! `README.md` is the user-facing contract and `docs/` the reasoning behind it.
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

/// What this build is, as `build.rs` described it: a release tag (`0.2.0`), the
/// commits since one (`0.2.0-3-gabc1234`), or a bare commit while there is no
/// tag yet. Not `Cargo.toml`'s `version`, which is a placeholder.
///
/// Here rather than in `main.rs` because both front ends say it — `--version`
/// and the web status screen — and two `env!` calls are two chances for them to
/// come to differ.
pub const VERSION: &str = env!("NODA_VERSION");
