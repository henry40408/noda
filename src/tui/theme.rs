//! noda's palette, said in ratatui's terms.
//!
//! No colour is chosen here. `style.rs` decides what an id looks like and this
//! translates that decision for a different writer: `anstyle` describes a colour
//! for a stream of escape sequences, ratatui describes one for a cell in a
//! buffer. Keeping the choice in one place is what makes an id the same yellow
//! in `noda ls` and in `noda tui` — it is the same thing being named, so it had
//! better not depend on which command is doing the naming.

use anstyle::{AnsiColor, Effects};
use ratatui::style::{Color, Modifier, Style};

/// An `anstyle` style as ratatui would write it.
///
/// Only the parts noda's palette actually uses are carried across: a foreground
/// colour, bold, and dim. A background or an underline would silently do
/// nothing, which is why `style.rs` is the place to look before adding one.
pub fn from(style: anstyle::Style) -> Style {
    let mut out = Style::default();
    if let Some(anstyle::Color::Ansi(colour)) = style.get_fg_color() {
        out = out.fg(ansi(colour));
    }
    let effects = style.get_effects();
    if effects.contains(Effects::BOLD) {
        out = out.add_modifier(Modifier::BOLD);
    }
    if effects.contains(Effects::DIMMED) {
        out = out.add_modifier(Modifier::DIM);
    }
    out
}

/// The sixteen terminal colours, one enum to the other. Both name the same
/// slots in the same palette, so the terminal's own theme keeps deciding what
/// each one looks like.
fn ansi(colour: AnsiColor) -> Color {
    match colour {
        AnsiColor::Black => Color::Black,
        AnsiColor::Red => Color::Red,
        AnsiColor::Green => Color::Green,
        AnsiColor::Yellow => Color::Yellow,
        AnsiColor::Blue => Color::Blue,
        AnsiColor::Magenta => Color::Magenta,
        AnsiColor::Cyan => Color::Cyan,
        AnsiColor::White => Color::Gray,
        AnsiColor::BrightBlack => Color::DarkGray,
        AnsiColor::BrightRed => Color::LightRed,
        AnsiColor::BrightGreen => Color::LightGreen,
        AnsiColor::BrightYellow => Color::LightYellow,
        AnsiColor::BrightBlue => Color::LightBlue,
        AnsiColor::BrightMagenta => Color::LightMagenta,
        AnsiColor::BrightCyan => Color::LightCyan,
        AnsiColor::BrightWhite => Color::White,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style;

    #[test]
    fn an_id_is_the_same_yellow_in_both_writers() {
        assert_eq!(from(style::ID).fg, Some(Color::Yellow));
        assert_eq!(from(style::TAGS).fg, Some(Color::Cyan));
    }

    #[test]
    fn effects_survive_the_crossing() {
        assert!(from(style::MUTED).add_modifier.contains(Modifier::DIM));
        assert!(from(style::MATCH).add_modifier.contains(Modifier::BOLD));
        // The slug is the id's colour a step down, and both halves of that have
        // to arrive or the two columns stop reading as one filename.
        let slug = from(style::SLUG);
        assert_eq!(slug.fg, Some(Color::Yellow));
        assert!(slug.add_modifier.contains(Modifier::DIM));
    }

    /// The brackets around a tag list step back by being a different colour, not
    /// by being the tags' colour weakened — a terminal that ignores `dim` would
    /// have drawn the whole column in one cyan.
    #[test]
    fn tag_punctuation_steps_back_without_leaning_on_dim() {
        let punct = from(style::TAGS_PUNCT);
        assert_eq!(punct.fg, Some(Color::DarkGray));
        assert!(punct.add_modifier.is_empty());
        assert_ne!(punct.fg, from(style::TAGS).fg);
    }
}
