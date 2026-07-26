//! The palette. Deliberately small: colour marks what a line *is*, never what
//! it means, and noda never colours a note's own text — that is the user's file.
//!
//! Nothing here decides whether colour is wanted. `anstream` strips the escapes
//! when the output is not a terminal, so commands can style unconditionally and
//! a piped `noda show` still emits exactly the bytes on disk.

use anstyle::{AnsiColor, Style};

/// Commit ids, in `log`.
pub const COMMIT: Style = AnsiColor::Yellow.on_default();
/// Timestamps and other supporting detail.
pub const MUTED: Style = Style::new().dimmed();
/// The `+` side of a diff.
pub const ADDED: Style = AnsiColor::Green.on_default();
/// The `-` side of a diff.
pub const REMOVED: Style = AnsiColor::Red.on_default();
/// `@@` hunk headers.
pub const HUNK: Style = AnsiColor::Cyan.on_default();
/// File headers in a diff.
pub const HEADING: Style = Style::new().bold();

/// Wraps `text` in `style`. The `:#` form writes the reset sequence.
pub fn paint(style: Style, text: &str) -> String {
    format!("{style}{text}{style:#}")
}
