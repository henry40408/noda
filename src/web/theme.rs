//! noda's palette, said in CSS terms. No colour is chosen here, exactly as none
//! is chosen in `tui/theme.rs`.
//!
//! **The difficulty: a terminal brings its own theme and a browser does not.**
//! `AnsiColor::Yellow` is a slot the terminal fills, which is why `tui/theme.rs`
//! can hand ratatui the slot and stop. CSS has no slots, so this file carries
//! what a terminal would have — two themes, because ANSI yellow on white is not
//! readable. Which colour an id *is* remains `style.rs`'s answer.

use std::fmt::Write;

use anstyle::{AnsiColor, Effects};

use crate::style;

/// Only the slots noda's palette reaches for. A colour added to `style.rs` and
/// forgotten here falls through `fill` to the default foreground — visible, and
/// not a crash on a page somebody is reading.
struct Terminal {
    /// The default foreground: what an unstyled character is.
    text: &'static str,
    /// A colour and not an opacity, for `TAGS_PUNCT`'s reason: an effect is a
    /// request, a colour is an answer.
    dim: &'static str,
    yellow: &'static str,
    /// `style::SLUG`. Must stay recognisably the same hue — id and slug side by
    /// side are the note's filename.
    yellow_dim: &'static str,
    cyan: &'static str,
    /// `BrightBlack`, which is grey everywhere anyone has ever set up.
    grey: &'static str,
    red: &'static str,
    /// Not a slot — a terminal never asks the program about its background.
    background: &'static str,
    /// A sunk panel. Half a step from `background`, never a second hue.
    sunk: &'static str,
    /// The line between two rows. The quietest thing that can still be seen.
    rule: &'static str,
    /// What a row looks like on the way down under a thumb.
    press: &'static str,
    /// `style::MATCH`. The one background colour, because that is what marking
    /// a run of text inside a line means.
    mark: &'static str,
}

const LIGHT: Terminal = Terminal {
    text: "#17191d",
    dim: "#6b7280",
    // Unreadable on white, so round it to amber as light terminal themes do.
    yellow: "#8a6100",
    yellow_dim: "#a98741",
    cyan: "#0e6f7a",
    grey: "#8b929c",
    red: "#b3261e",
    background: "#ffffff",
    sunk: "#f4f5f7",
    rule: "#e3e5e9",
    press: "#ebedf0",
    mark: "#f3d98a",
};

const DARK: Terminal = Terminal {
    text: "#d8dce2",
    dim: "#868e9a",
    yellow: "#e0ac4d",
    yellow_dim: "#9c7c3c",
    cyan: "#56b6c2",
    grey: "#6b7280",
    red: "#e06c75",
    // Not black: under white text, pure black smears as the eye moves.
    background: "#14161a",
    sunk: "#1b1e24",
    rule: "#272b32",
    press: "#242830",
    mark: "#5a4a1e",
};

/// An `anstyle` style as this theme draws it. Only foreground and `dim` cross
/// over; `bold` is left to the markup, because saying it twice is how the two
/// answers start to disagree.
fn fill(style: anstyle::Style, terminal: &Terminal) -> &'static str {
    let dimmed = style.get_effects().contains(Effects::DIMMED);
    match style.get_fg_color() {
        Some(anstyle::Color::Ansi(AnsiColor::Yellow)) if dimmed => terminal.yellow_dim,
        Some(anstyle::Color::Ansi(AnsiColor::Yellow)) => terminal.yellow,
        Some(anstyle::Color::Ansi(AnsiColor::Cyan)) => terminal.cyan,
        Some(anstyle::Color::Ansi(AnsiColor::BrightBlack)) => terminal.grey,
        Some(anstyle::Color::Ansi(AnsiColor::Red)) => terminal.red,
        // `style::MUTED`: how a timestamp steps back without becoming a hue.
        None if dimmed => terminal.dim,
        _ => terminal.text,
    }
}

/// Named after `style.rs`'s constants, not after what they look like: `--tag`
/// and not `--cyan`, because a variable called `--cyan` invites being used for
/// anything that merely wants to be cyan.
fn properties(terminal: &Terminal) -> String {
    let mut css = String::new();
    for (name, value) in [
        ("--bg", terminal.background),
        ("--bg-sunk", terminal.sunk),
        ("--press", terminal.press),
        ("--rule", terminal.rule),
        ("--mark", terminal.mark),
        ("--text", fill(anstyle::Style::new(), terminal)),
        ("--muted", fill(style::MUTED, terminal)),
        ("--id", fill(style::ID, terminal)),
        ("--id-dim", fill(style::SLUG, terminal)),
        ("--tag", fill(style::TAGS, terminal)),
        ("--punct", fill(style::TAGS_PUNCT, terminal)),
        ("--alert", fill(style::INVALID, terminal)),
        // The same red as `--alert` today, and its own property anyway:
        // `OVERDUE` is the one colour marking what a thing *means*, and spelling
        // it `--alert` would quietly drop that argument.
        ("--overdue", fill(style::OVERDUE, terminal)),
    ] {
        let _ = write!(css, "{name}:{value};");
    }
    css
}

/// Light on the bare selector, dark inside the query — not as a fallback (no
/// engine reports `no-preference` any more) but because it is shorter: the query
/// then holds only what differs.
///
/// No toggle and no stored preference: the reader already told their phone.
pub fn stylesheet() -> String {
    format!(
        ":root{{{}}}@media (prefers-color-scheme:dark){{:root{{{}}}}}",
        properties(&LIGHT),
        properties(&DARK)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_id_is_the_same_thing_in_both_themes_and_a_commit_is_an_id() {
        assert_eq!(fill(style::ID, &LIGHT), LIGHT.yellow);
        assert_eq!(fill(style::ID, &DARK), DARK.yellow);
        // Deliberately the same colour — see `style.rs`.
        assert_eq!(fill(style::COMMIT, &DARK), fill(style::ID, &DARK));
    }

    /// The web is the first interface drawing id and slug as one string; lose
    /// the step down and it reads as two.
    #[test]
    fn the_slug_stays_the_ids_hue_a_step_down() {
        for terminal in [&LIGHT, &DARK] {
            let id = fill(style::ID, terminal);
            let slug = fill(style::SLUG, terminal);
            assert_ne!(id, slug);
            assert_eq!(slug, terminal.yellow_dim);
        }
    }

    /// `style.rs`'s argument, for the browser: a different colour, never the
    /// tags' colour weakened.
    #[test]
    fn tag_punctuation_steps_back_by_hue() {
        assert_eq!(fill(style::TAGS_PUNCT, &DARK), DARK.grey);
        assert_ne!(fill(style::TAGS_PUNCT, &DARK), fill(style::TAGS, &DARK));
    }

    #[test]
    fn dim_with_no_colour_is_what_a_timestamp_gets() {
        assert_eq!(fill(style::MUTED, &LIGHT), LIGHT.dim);
        assert_ne!(fill(style::MUTED, &LIGHT), LIGHT.text);
    }

    #[test]
    fn both_themes_reach_the_stylesheet_and_neither_is_the_other() {
        let css = stylesheet();
        assert!(css.contains("prefers-color-scheme:dark"), "{css}");
        assert!(css.contains(LIGHT.yellow), "{css}");
        assert!(css.contains(DARK.yellow), "{css}");
        // Light is what a browser saying nothing gets.
        assert!(
            css.find(LIGHT.background) < css.find(DARK.background),
            "{css}"
        );
    }
}
