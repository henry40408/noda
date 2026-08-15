//! noda's palette, said in CSS terms.
//!
//! No colour is chosen here, exactly as none is chosen in `tui/theme.rs`.
//! `style.rs` decides what an id looks like; this translates that decision for a
//! third writer. Keeping the choice in one place is what makes an id the same
//! yellow in `noda ls`, in `noda tui` and in a browser — it is the same thing
//! being named, so it had better not depend on which one is doing the naming.
//!
//! **What is different here, and it is the whole of the difficulty: a terminal
//! brings its own theme and a browser does not.** `AnsiColor::Yellow` is a slot,
//! and the terminal decides what fills it — which is why `tui/theme.rs` can
//! hand ratatui the slot and stop. CSS has no slots. So this file carries what a
//! terminal would have carried: a theme. Two of them, because a browser has a
//! light mode and a dark mode, and ANSI yellow on white is not readable.
//!
//! That is the only thing being added. Which colour an id *is* remains
//! `style.rs`'s answer; what yellow looks like on white is a rendering question
//! that had to be answered somewhere, and this is somewhere.

use std::fmt::Write;

use anstyle::{AnsiColor, Effects};

use crate::style;

/// What one terminal theme fills the slots with.
///
/// Only the slots noda's palette actually reaches for. Adding a colour to
/// `style.rs` and forgetting this file is caught by `fill`, which sends anything
/// unnamed to the default foreground rather than guessing — visible, and not a
/// crash on a page somebody is reading.
struct Terminal {
    /// The default foreground: what an unstyled character is.
    text: &'static str,
    /// What `dim` does to that default. A colour and not an opacity, for the
    /// same reason `style.rs` gives for `TAGS_PUNCT`: an effect is a request,
    /// and a colour is an answer.
    dim: &'static str,
    yellow: &'static str,
    /// Yellow with `dim` on it — `style::SLUG`. It has to stay recognisably the
    /// same hue: the id and the slug side by side are the note's filename, and
    /// reading them as one thing is the point.
    yellow_dim: &'static str,
    cyan: &'static str,
    /// `BrightBlack`, which is grey everywhere anyone has ever set up.
    grey: &'static str,
    red: &'static str,
    /// Not a slot: the page has to stand on something, and a terminal's
    /// background is the one colour it never asks the program about.
    background: &'static str,
    /// A sunk panel — the search field, the action bar. Half a step from
    /// `background`, never a second hue.
    sunk: &'static str,
    /// The line between two rows. The quietest thing that can still be seen.
    rule: &'static str,
    /// What a row looks like on the way down under a thumb.
    press: &'static str,
    /// `style::MATCH`'s highlight. The one place a background colour is used,
    /// because that is what marking a run of text inside a line means.
    mark: &'static str,
}

const LIGHT: Terminal = Terminal {
    text: "#17191d",
    dim: "#6b7280",
    // Not the terminal's yellow. On white it is unreadable, so a light theme
    // does what every light terminal theme does and takes it round to amber.
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
    // Not black. A note is read for minutes at a time, and pure black under
    // white text is what makes the text smear as the eye moves.
    background: "#14161a",
    sunk: "#1b1e24",
    rule: "#272b32",
    press: "#242830",
    mark: "#5a4a1e",
};

/// An `anstyle` style as this theme draws it.
///
/// Only a foreground colour and `dim` are carried across, which is every part
/// `style.rs` uses that means anything here. `bold` is left to the markup: a
/// heading is a heading because of the element it is, and saying it twice is how
/// the two answers start to disagree.
fn fill(style: anstyle::Style, terminal: &Terminal) -> &'static str {
    let dimmed = style.get_effects().contains(Effects::DIMMED);
    match style.get_fg_color() {
        Some(anstyle::Color::Ansi(AnsiColor::Yellow)) if dimmed => terminal.yellow_dim,
        Some(anstyle::Color::Ansi(AnsiColor::Yellow)) => terminal.yellow,
        Some(anstyle::Color::Ansi(AnsiColor::Cyan)) => terminal.cyan,
        Some(anstyle::Color::Ansi(AnsiColor::BrightBlack)) => terminal.grey,
        Some(anstyle::Color::Ansi(AnsiColor::Red)) => terminal.red,
        // No colour of its own. `style::MUTED` is exactly this: dim and nothing
        // else, which is how a timestamp steps back without becoming a hue.
        None if dimmed => terminal.dim,
        _ => terminal.text,
    }
}

/// The custom properties one theme sets, in the order a reader would want them.
///
/// Named after `style.rs`'s constants rather than after what they look like.
/// `--tag` and not `--cyan`: the next person to change the colour should have to
/// go to the file that decides what a tag is, and a variable called `--cyan`
/// invites being used for something that merely wants to be cyan.
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
    ] {
        let _ = write!(css, "{name}:{value};");
    }
    css
}

/// Both themes, as a browser picks between them.
///
/// Light on the bare selector and dark inside the query, so a browser that says
/// nothing gets the light one. There is no toggle and no stored preference: the
/// reader has already told their phone which they want, and a notebook is not
/// the place to ask again.
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
        // `style.rs` says these are deliberately the same colour, because both
        // are the short string you copy out of a listing.
        assert_eq!(fill(style::COMMIT, &DARK), fill(style::ID, &DARK));
    }

    /// The id and the slug side by side are the note's filename, and the web is
    /// the first interface that draws them as one string. If the slug stopped
    /// being the id's colour a step down, that string would read as two.
    #[test]
    fn the_slug_stays_the_ids_hue_a_step_down() {
        for terminal in [&LIGHT, &DARK] {
            let id = fill(style::ID, terminal);
            let slug = fill(style::SLUG, terminal);
            assert_ne!(id, slug);
            assert_eq!(slug, terminal.yellow_dim);
        }
    }

    /// The same argument `style.rs` makes for the terminal: the brackets around
    /// a tag list step back by being a different colour, never by being the
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
        // Light is what a browser saying nothing gets, so it comes first.
        assert!(
            css.find(LIGHT.background) < css.find(DARK.background),
            "{css}"
        );
    }
}
