//! `style.rs`'s palette translated to ratatui cells; no colour is chosen here.

use anstyle::{AnsiColor, Effects};
use ratatui::style::{Color, Modifier, Style};

/// Only foreground, bold and dim cross over; a background or underline added in
/// `style.rs` would silently do nothing here.
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

/// Palette slots, not RGB, so the terminal's theme still decides the colours.
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
        // The id's yellow, dimmed, so id and slug read as one filename.
        let slug = from(style::SLUG);
        assert_eq!(slug.fg, Some(Color::Yellow));
        assert!(slug.add_modifier.contains(Modifier::DIM));
    }

    /// A terminal that ignores `dim` would draw the whole column in one cyan.
    #[test]
    fn tag_punctuation_steps_back_without_leaning_on_dim() {
        let punct = from(style::TAGS_PUNCT);
        assert_eq!(punct.fg, Some(Color::DarkGray));
        assert!(punct.add_modifier.is_empty());
        assert_ne!(punct.fg, from(style::TAGS).fg);
    }
}
