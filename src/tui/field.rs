//! A one-line text field with readline's keys.
//!
//! A chord arrives as its letter (`Ctrl-D` is `Char('d')`), so an unbound chord
//! must do nothing rather than type the letter; bound ones follow readline,
//! where the habit comes from.
//!
//! The kill ring is one entry deep and consecutive kills replace: enough for
//! `Ctrl-Y` to undo a `Ctrl-W`, in a line rarely longer than a title.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Told apart because a query reruns when its text changes, not when the
/// cursor moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edit {
    Typed,
    Moved,
}

/// Readline's two words: the `Alt` keys stop at punctuation (`tag:work`), while
/// `Ctrl-W` stops only at whitespace and takes a whole `tag:"12.34"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Word {
    Alnum,
    /// Blank-delimited: anything but whitespace.
    Blank,
}

impl Word {
    fn holds(self, c: char) -> bool {
        match self {
            Word::Alnum => c.is_alphanumeric(),
            Word::Blank => !c.is_whitespace(),
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct Field {
    text: String,
    /// A byte offset, since every use slices the text; only ever moved by
    /// walking characters, so always on a char boundary.
    at: usize,
    /// Kept across `set` and `clear`.
    cut: String,
}

impl Field {
    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// The text left of the cursor, which places the terminal's cursor.
    pub fn before(&self) -> &str {
        &self.text[..self.at]
    }

    /// Leaves the cursor at the end.
    pub fn set(&mut self, text: String) {
        self.at = text.len();
        self.text = text;
    }

    pub fn clear(&mut self) {
        self.text.clear();
        self.at = 0;
    }

    pub fn take(&mut self) -> String {
        self.at = 0;
        std::mem::take(&mut self.text)
    }

    /// `None` leaves the key to the caller, including an unbound chord, which
    /// must not type its letter.
    pub fn key(&mut self, key: KeyEvent) -> Option<Edit> {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        match key.code {
            // Not Shift, or the field would take no capitals.
            KeyCode::Char(c) if ctrl => self.chord(c),
            KeyCode::Char(c) if alt => self.meta(c),
            KeyCode::Char(c) => Some(self.insert(c)),
            KeyCode::Backspace if alt => self.kill(self.word_back(Word::Alnum), self.at),
            KeyCode::Backspace => self.kill(self.back(), self.at),
            KeyCode::Delete => self.kill(self.at, self.forward()),
            KeyCode::Left if ctrl || alt => Some(self.to(self.word_back(Word::Alnum))),
            KeyCode::Right if ctrl || alt => Some(self.to(self.word_forward(Word::Alnum))),
            KeyCode::Left => Some(self.to(self.back())),
            KeyCode::Right => Some(self.to(self.forward())),
            KeyCode::Home => Some(self.to(0)),
            KeyCode::End => Some(self.to(self.text.len())),
            _ => None,
        }
    }

    /// For a field whose cursor is not drawn: only keys that keep the cursor at
    /// the end, so nothing moves the insertion point out of sight.
    pub fn erasing(&mut self, key: KeyEvent) -> Option<Edit> {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        match key.code {
            KeyCode::Char('w') if ctrl => self.kill(self.word_back(Word::Blank), self.at),
            // The cursor is at the end, so both kill everything.
            KeyCode::Char('u' | 'k') if ctrl => self.kill(0, self.at),
            KeyCode::Char('h') if ctrl => self.kill(self.back(), self.at),
            KeyCode::Char(_) if ctrl || alt => None,
            KeyCode::Char(c) => Some(self.insert(c)),
            KeyCode::Backspace if alt => self.kill(self.word_back(Word::Alnum), self.at),
            KeyCode::Backspace => self.kill(self.back(), self.at),
            _ => None,
        }
    }

    fn chord(&mut self, c: char) -> Option<Edit> {
        match c {
            'a' => Some(self.to(0)),
            'e' => Some(self.to(self.text.len())),
            'b' => Some(self.to(self.back())),
            'f' => Some(self.to(self.forward())),
            'h' => self.kill(self.back(), self.at),
            // Never readline's end-of-file: outside a field `Ctrl-D` deletes a
            // note, so it must not depend on what has been typed.
            'd' => self.kill(self.at, self.forward()),
            'w' => self.kill(self.word_back(Word::Blank), self.at),
            'u' => self.kill(0, self.at),
            'k' => self.kill(self.at, self.text.len()),
            'y' => self.yank(),
            _ => None,
        }
    }

    fn meta(&mut self, c: char) -> Option<Edit> {
        match c {
            'b' => Some(self.to(self.word_back(Word::Alnum))),
            'f' => Some(self.to(self.word_forward(Word::Alnum))),
            'd' => self.kill(self.at, self.word_forward(Word::Alnum)),
            _ => None,
        }
    }

    fn insert(&mut self, c: char) -> Edit {
        self.text.insert(self.at, c);
        self.at += c.len_utf8();
        Edit::Typed
    }

    /// Removing nothing keeps the previous cut, so an idle `Ctrl-K` at the end
    /// of a line does not lose what `Ctrl-W` took.
    fn kill(&mut self, from: usize, to: usize) -> Option<Edit> {
        if from >= to {
            return None;
        }
        self.cut = self.text[from..to].to_string();
        self.text.replace_range(from..to, "");
        self.at = from;
        Some(Edit::Typed)
    }

    /// Keeps the cut, so it can be yanked again.
    fn yank(&mut self) -> Option<Edit> {
        if self.cut.is_empty() {
            return None;
        }
        let cut = std::mem::take(&mut self.cut);
        self.text.insert_str(self.at, &cut);
        self.at += cut.len();
        self.cut = cut;
        Some(Edit::Typed)
    }

    fn to(&mut self, at: usize) -> Edit {
        self.at = at;
        Edit::Moved
    }

    fn back(&self) -> usize {
        self.text[..self.at]
            .chars()
            .next_back()
            .map_or(self.at, |c| self.at - c.len_utf8())
    }

    fn forward(&self) -> usize {
        self.text[self.at..]
            .chars()
            .next()
            .map_or(self.at, |c| self.at + c.len_utf8())
    }

    /// Separators first, then the word, so it works from inside trailing spaces.
    fn word_back(&self, word: Word) -> usize {
        let mut at = self.at;
        let mut into = false;
        while let Some(c) = self.text[..at].chars().next_back() {
            if word.holds(c) {
                into = true;
            } else if into {
                break;
            }
            at -= c.len_utf8();
        }
        at
    }

    fn word_forward(&self, word: Word) -> usize {
        let mut at = self.at;
        let mut into = false;
        while let Some(c) = self.text[at..].chars().next() {
            if word.holds(c) {
                into = true;
            } else if into {
                break;
            }
            at += c.len_utf8();
        }
        at
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed(text: &str) -> Field {
        let mut field = Field::default();
        field.set(text.to_string());
        field
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn ctrl(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    fn alt(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::ALT)
    }

    #[test]
    fn typing_goes_in_at_the_cursor_and_not_at_the_end() {
        let mut field = typed("budget");
        field.key(ctrl('a'));
        field.key(key(KeyCode::Char('a')));
        assert_eq!(field.text(), "abudget");
        assert_eq!(field.before(), "a");
    }

    #[test]
    fn the_cursor_walks_by_character_and_stops_at_both_ends() {
        let mut field = typed("ab");
        assert_eq!(field.key(ctrl('e')), Some(Edit::Moved));
        for _ in 0..4 {
            field.key(ctrl('b'));
        }
        assert_eq!(field.before(), "");
        for _ in 0..4 {
            field.key(ctrl('f'));
        }
        assert_eq!(field.before(), "ab");
    }

    #[test]
    fn a_cursor_never_lands_inside_a_character() {
        let mut field = typed("預算");
        field.key(key(KeyCode::Left));
        assert_eq!(field.before(), "預");
        field.key(key(KeyCode::Backspace));
        assert_eq!(field.text(), "算");
        field.key(key(KeyCode::Char('的')));
        assert_eq!(field.text(), "的算");
        assert_eq!(field.before(), "的");
    }

    #[test]
    fn the_word_keys_step_over_letters_and_digits() {
        let mut field = typed("tag:work budget");
        assert_eq!(field.key(alt('b')), Some(Edit::Moved));
        assert_eq!(field.before(), "tag:work ");
        field.key(alt('b'));
        assert_eq!(field.before(), "tag:");
        field.key(alt('f'));
        assert_eq!(field.before(), "tag:work");
    }

    #[test]
    fn ctrl_w_takes_a_whole_term_however_it_is_punctuated() {
        let mut field = typed("tag:work tag:\"12.34\"");
        assert_eq!(field.key(ctrl('w')), Some(Edit::Typed));
        assert_eq!(field.text(), "tag:work ");
        field.key(ctrl('w'));
        assert_eq!(field.text(), "");
    }

    #[test]
    fn the_kills_take_each_end_of_the_line() {
        let mut field = typed("budget review");
        field.key(alt('b'));
        assert_eq!(field.key(ctrl('k')), Some(Edit::Typed));
        assert_eq!(field.text(), "budget ");
        assert_eq!(field.key(ctrl('u')), Some(Edit::Typed));
        assert_eq!(field.text(), "");
    }

    #[test]
    fn alt_d_takes_the_word_in_front_of_the_cursor() {
        let mut field = typed("budget review");
        field.key(ctrl('a'));
        assert_eq!(field.key(alt('d')), Some(Edit::Typed));
        assert_eq!(field.text(), " review");
    }

    #[test]
    fn what_a_kill_took_can_be_put_back() {
        let mut field = typed("tag:work budget");
        field.key(ctrl('w'));
        assert_eq!(field.text(), "tag:work ");
        assert_eq!(field.key(ctrl('y')), Some(Edit::Typed));
        assert_eq!(field.text(), "tag:work budget");
        assert_eq!(field.before(), "tag:work budget");
        field.key(ctrl('y'));
        assert_eq!(field.text(), "tag:work budgetbudget");
    }

    #[test]
    fn a_kill_that_took_nothing_keeps_what_the_last_one_took() {
        let mut field = typed("budget");
        field.key(ctrl('u'));
        assert_eq!(field.key(ctrl('k')), None);
        assert_eq!(field.key(ctrl('u')), None);
        assert_eq!(field.key(ctrl('h')), None);
        field.key(ctrl('y'));
        assert_eq!(field.text(), "budget");
    }

    #[test]
    fn ctrl_d_deletes_forward_and_nothing_else() {
        let mut field = typed("budget");
        field.key(ctrl('a'));
        assert_eq!(field.key(ctrl('d')), Some(Edit::Typed));
        assert_eq!(field.text(), "udget");
        assert_eq!(field.key(key(KeyCode::Delete)), Some(Edit::Typed));
        assert_eq!(field.text(), "dget");
        // At the end: neither end-of-file nor the browser's delete.
        field.key(ctrl('e'));
        assert_eq!(field.key(ctrl('d')), None);
        assert_eq!(field.text(), "dget");
    }

    #[test]
    fn a_chord_this_does_not_bind_does_nothing_at_all() {
        // `Ctrl-P` arrives as `Char('p')`.
        let mut field = typed("Trip");
        assert_eq!(field.key(ctrl('p')), None);
        assert_eq!(field.key(alt('x')), None);
        assert_eq!(field.text(), "Trip");
        let shifted = KeyEvent::new(KeyCode::Char('Q'), KeyModifiers::SHIFT);
        assert_eq!(field.key(shifted), Some(Edit::Typed));
        assert_eq!(field.text(), "TripQ");
    }

    #[test]
    fn a_field_with_no_cursor_drawn_only_erases_from_the_end() {
        let mut field = typed("push");
        assert_eq!(field.erasing(ctrl('h')), Some(Edit::Typed));
        assert_eq!(field.text(), "pus");
        assert_eq!(field.erasing(ctrl('u')), Some(Edit::Typed));
        assert_eq!(field.text(), "");
        field.set("push".to_string());
        assert_eq!(field.erasing(ctrl('a')), None);
        assert_eq!(field.erasing(alt('b')), None);
        assert_eq!(field.erasing(key(KeyCode::Left)), None);
        field.erasing(key(KeyCode::Char('x')));
        assert_eq!(field.text(), "pushx", "and typing still goes on the end");
    }

    #[test]
    fn what_is_put_in_the_field_can_be_added_to_straight_away() {
        // The retitle case: the cursor lands after the handed-over title.
        let mut field = typed("Budget review");
        field.key(key(KeyCode::Char('!')));
        assert_eq!(field.text(), "Budget review!");
    }
}
