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
/// The part of a search result that matched.
pub const MATCH: Style = AnsiColor::Yellow.on_default().bold();
/// A due date that has passed, in `todo`.
///
/// The one exception to the rule above, and it is worth naming as one: this
/// colours a row for what it *means*, not for what it is. It earns the
/// exception by being the only thing anybody scans a todo list for — a list
/// that does not distinguish late from not-yet is a list you have to read
/// rather than glance at. Nothing else may follow it without the same argument.
pub const OVERDUE: Style = AnsiColor::Red.on_default();

/// Wraps `text` in `style`. The `:#` form writes the reset sequence.
pub fn paint(style: Style, text: &str) -> String {
    format!("{style}{text}{style:#}")
}
