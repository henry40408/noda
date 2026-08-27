//! noda — a git-native notebook for your terminal.
//!
//! `README.md` is the user-facing contract and `docs/PRFAQ.md` the reasoning
//! behind it. Everything is a library so the CLI stays a thin shell and commands
//! are tested without spawning a process.

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
