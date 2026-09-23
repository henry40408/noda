//! noda's palette in CSS. No colour is chosen here, as none is in
//! `tui/theme.rs`: which colour an id is remains `style.rs`'s answer.
//!
//! A terminal fills `AnsiColor::Yellow` from its own theme; CSS has no such
//! slots, so this file supplies what a terminal would — in two themes, because
//! ANSI yellow on white is unreadable.

use std::fmt::Write;

use anstyle::{AnsiColor, Effects};

use crate::style;

/// Only the slots noda's palette uses. One added to `style.rs` and forgotten
/// here falls through `fill` to the default foreground.
struct Terminal {
    /// The default foreground.
    text: &'static str,
    /// A colour, not an opacity, for `TAGS_PUNCT`'s reason.
    dim: &'static str,
    yellow: &'static str,
    /// `style::SLUG`: must stay recognisably the id's hue.
    yellow_dim: &'static str,
    cyan: &'static str,
    /// `BrightBlack`.
    grey: &'static str,
    red: &'static str,
    /// `style::PIN`: the one hue not otherwise used.
    magenta: &'static str,
    /// Not a slot: a terminal never tells the program its background.
    background: &'static str,
    /// A sunk panel: half a step from `background`, not a second hue.
    sunk: &'static str,
    /// The line between two rows.
    rule: &'static str,
    /// A row being pressed.
    press: &'static str,
    /// `style::MATCH`, as a background, which is how a run inside a line is
    /// marked.
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
    magenta: "#a626a4",
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
    magenta: "#c678dd",
    // Not black: under white text, pure black smears as the eye moves.
    background: "#14161a",
    sunk: "#1b1e24",
    rule: "#272b32",
    press: "#242830",
    mark: "#5a4a1e",
};

/// An `anstyle` style as this theme draws it. Only foreground and `dim` cross
/// over; `bold` is left to the markup, so it is said in one place.
fn fill(style: anstyle::Style, terminal: &Terminal) -> &'static str {
    let dimmed = style.get_effects().contains(Effects::DIMMED);
    match style.get_fg_color() {
        Some(anstyle::Color::Ansi(AnsiColor::Yellow)) if dimmed => terminal.yellow_dim,
        Some(anstyle::Color::Ansi(AnsiColor::Yellow)) => terminal.yellow,
        Some(anstyle::Color::Ansi(AnsiColor::Cyan)) => terminal.cyan,
        Some(anstyle::Color::Ansi(AnsiColor::BrightBlack)) => terminal.grey,
        Some(anstyle::Color::Ansi(AnsiColor::Red)) => terminal.red,
        Some(anstyle::Color::Ansi(AnsiColor::Magenta)) => terminal.magenta,
        // `style::MUTED`.
        None if dimmed => terminal.dim,
        _ => terminal.text,
    }
}

/// Named after `style.rs`'s constants (`--tag`, not `--cyan`), so a property
/// is not reused for anything that merely wants its colour.
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
        // Its own property though it is `--alert`'s red: it marks meaning.
        ("--overdue", fill(style::OVERDUE, terminal)),
        ("--pin", fill(style::PIN, terminal)),
    ] {
        let _ = write!(css, "{name}:{value};");
    }
    css
}

/// Light on `:root`, dark inside the media query. No toggle and no stored
/// preference: the reader already told their device.
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
        assert_eq!(fill(style::COMMIT, &DARK), fill(style::ID, &DARK));
    }

    #[test]
    fn the_slug_stays_the_ids_hue_a_step_down() {
        for terminal in [&LIGHT, &DARK] {
            let id = fill(style::ID, terminal);
            let slug = fill(style::SLUG, terminal);
            assert_ne!(id, slug);
            assert_eq!(slug, terminal.yellow_dim);
        }
    }

    #[test]
    fn tag_punctuation_steps_back_by_hue() {
        assert_eq!(fill(style::TAGS_PUNCT, &DARK), DARK.grey);
        assert_ne!(fill(style::TAGS_PUNCT, &DARK), fill(style::TAGS, &DARK));
    }

    /// A colour missing here would fall through to the text colour.
    #[test]
    fn the_pin_has_a_hue_of_its_own_in_both_themes() {
        for terminal in [&LIGHT, &DARK] {
            assert_eq!(fill(style::PIN, terminal), terminal.magenta);
            assert_ne!(fill(style::PIN, terminal), terminal.text);
            assert_ne!(fill(style::PIN, terminal), fill(style::TAGS, terminal));
        }
        assert!(stylesheet().contains("--pin:"));
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
        // Light is what a browser with no preference gets.
        assert!(
            css.find(LIGHT.background) < css.find(DARK.background),
            "{css}"
        );
    }
}
