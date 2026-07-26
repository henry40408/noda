//! noda — a git-native notebook for your terminal.
//!
//! The user-facing contract lives in `README.md` and `docs/PRFAQ.md`. This crate
//! implements it; everything is exposed as a library so the CLI stays a thin shell
//! and the commands can be tested without spawning a process.

pub mod cmd;
pub mod error;
pub mod note;
pub mod notebook;
pub mod paths;
pub mod remote;
pub mod style;

pub use error::{Error, Result};
pub use paths::Paths;
