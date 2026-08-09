//! One line being typed into, and what readline means by the keys around it.
//!
//! There are four places in the browser where a person types — the query, the
//! prompt, the command line and the filter over the list of commands — and until
//! this existed all four were a `String` with characters pushed onto the end of
//! it. That is fine right up until the typo is three words back: the only way to
//! reach it was to delete everything after it, because there was no cursor to
//! move and every chord anyone reached for out of habit was swallowed on the way
//! in.
//!
//! Swallowed was the right first answer — `Ctrl-D` arrives as `Char('d')`, and a
//! field that took it at face value would have put a `d` in the middle of
//! somebody's title. But a field where `Ctrl-A` does nothing is one where the
//! keys a shell taught you are keys you have to unlearn, and the rest of the
//! browser is built out of what the reader already knows. So the chords are
//! answered here rather than ignored, and answered the way readline answers
//! them, because readline is where the habit comes from: the same bindings
//! `bash`, `zsh`, `psql` and everything else linked against it have.
//!
//! What is deliberately not here: the kill ring is one entry deep rather than a
//! ring, and consecutive kills replace each other rather than accumulating.
//! `Ctrl-Y` is what makes `Ctrl-W` safe to press — it is the undo for a field
//! that has no undo — and one entry is enough for that. A ring would be a second
//! thing to learn for a line that is rarely longer than a title.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// What a key did to the field, for the caller that has to react to it.
///
/// Told apart because a query is run against the notebook again every time its
/// text changes, and moving a cursor through a query is not a reason to walk
/// every note in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edit {
    /// The text is not what it was.
    Typed,
    /// Only the cursor moved.
    Moved,
}

/// What counts as a word, which readline answers twice.
///
/// Its word keys — `Alt-B`, `Alt-F`, `Alt-D`, `Alt-Backspace` — step over runs
/// of letters and digits, so they stop at the punctuation in `tag:work`.
/// `Ctrl-W` is older than they are and stops only at whitespace, which is what
/// makes it the key that takes a whole `tag:"12.34 foo"` off the end of a query.
/// Both are kept, on the keys readline keeps them on: a habit is about the key,
/// not about the definition behind it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Word {
    /// Letters and digits: what the word keys step over.
    Alnum,
    /// Anything that is not a space: what `Ctrl-W` takes.
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

/// A line being typed into, and where in it the next character goes.
#[derive(Debug, Default, Clone)]
pub struct Field {
    text: String,
    /// Where the cursor is, as a byte offset into `text`.
    ///
    /// Bytes and not characters, because every use of it is a slice of the text
    /// — what to draw to the left of the cursor, where to insert, what a kill
    /// takes out. It is only ever moved by walking characters, so it is always
    /// on a character boundary, which is what makes those slices safe. A
    /// notebook whose titles are Chinese is the ordinary case here and not the
    /// exotic one.
    at: usize,
    /// What the last kill took out, for `Ctrl-Y` to put back.
    ///
    /// Kept across `set` and `clear`: a field refilled for a retitle is not the
    /// reader having changed their mind about what they cut.
    cut: String,
}

impl Field {
    /// What has been typed.
    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// What is to the left of the cursor, which is what says how far along the
    /// line the terminal's own cursor belongs.
    pub fn before(&self) -> &str {
        &self.text[..self.at]
    }

    /// Fills the field, with the cursor after what was put in it. Somebody
    /// handed a title to edit is being handed a starting point, and a starting
    /// point you have to walk to the end of before you can add to it is a worse
    /// one.
    pub fn set(&mut self, text: String) {
        self.at = text.len();
        self.text = text;
    }

    pub fn clear(&mut self) {
        self.text.clear();
        self.at = 0;
    }

    /// Takes the line and leaves the field empty, for the caller that is about
    /// to run what was typed.
    pub fn take(&mut self) -> String {
        self.at = 0;
        std::mem::take(&mut self.text)
    }

    /// Answers a key, or says it was not one of ours.
    ///
    /// `None` means the caller still has to deal with it: `Enter`, `Esc` and the
    /// keys a particular field gives its own meaning to are not editing keys,
    /// and neither is a chord this does not bind — an unbound chord does nothing
    /// rather than typing its own letter, which is the trap this module exists
    /// for.
    pub fn key(&mut self, key: KeyEvent) -> Option<Edit> {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        match key.code {
            // Shift is not one of these: `Q` arrives as `Char('Q')` with the
            // shift bit set, and a field with no capital letters in it would be
            // a strange thing to ship.
            KeyCode::Char(c) if ctrl => self.chord(c),
            KeyCode::Char(c) if alt => self.meta(c),
            KeyCode::Char(c) => Some(self.insert(c)),
            // `Alt-Backspace` is the word-sized one; `Ctrl-H` is the terminal's
            // own name for the plain one and is answered with the chords.
            KeyCode::Backspace if alt => self.kill(self.word_back(Word::Alnum), self.at),
            KeyCode::Backspace => self.kill(self.back(), self.at),
            KeyCode::Delete => self.kill(self.at, self.forward()),
            // The arrows carry the same modifiers the word keys do, because a
            // terminal that sends `Ctrl-Left` is one whose user meant the word
            // and not the character.
            KeyCode::Left if ctrl || alt => Some(self.to(self.word_back(Word::Alnum))),
            KeyCode::Right if ctrl || alt => Some(self.to(self.word_forward(Word::Alnum))),
            KeyCode::Left => Some(self.to(self.back())),
            KeyCode::Right => Some(self.to(self.forward())),
            KeyCode::Home => Some(self.to(0)),
            KeyCode::End => Some(self.to(self.text.len())),
            _ => None,
        }
    }

    /// The subset for a field whose cursor is not drawn.
    ///
    /// The filter over the list of commands is shown in that card's title and
    /// nowhere else, so there is no cursor on the screen to move. Everything
    /// here leaves the cursor at the end of the line, which is where the reader
    /// can see it — a `Ctrl-A` that silently moved the insertion point behind
    /// text with no cursor drawn in it would be worse than one that does
    /// nothing.
    pub fn erasing(&mut self, key: KeyEvent) -> Option<Edit> {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        match key.code {
            KeyCode::Char('w') if ctrl => self.kill(self.word_back(Word::Blank), self.at),
            // The two that clear the line are the same key here: with the cursor
            // at the end, everything is behind it.
            KeyCode::Char('u' | 'k') if ctrl => self.kill(0, self.at),
            KeyCode::Char('h') if ctrl => self.kill(self.back(), self.at),
            KeyCode::Char(_) if ctrl || alt => None,
            KeyCode::Char(c) => Some(self.insert(c)),
            KeyCode::Backspace if alt => self.kill(self.word_back(Word::Alnum), self.at),
            KeyCode::Backspace => self.kill(self.back(), self.at),
            _ => None,
        }
    }

    /// The `Ctrl` bindings, in readline's words for them.
    fn chord(&mut self, c: char) -> Option<Edit> {
        match c {
            'a' => Some(self.to(0)),
            'e' => Some(self.to(self.text.len())),
            'b' => Some(self.to(self.back())),
            'f' => Some(self.to(self.forward())),
            'h' => self.kill(self.back(), self.at),
            // Not readline's end-of-file, deliberately. That is what `Ctrl-D`
            // means to a shell on an empty line, and outside a field it is what
            // deletes the note under the cursor — a key that did either
            // depending on how much had been typed is one nobody should have to
            // think about while typing.
            'd' => self.kill(self.at, self.forward()),
            'w' => self.kill(self.word_back(Word::Blank), self.at),
            'u' => self.kill(0, self.at),
            'k' => self.kill(self.at, self.text.len()),
            'y' => self.yank(),
            _ => None,
        }
    }

    /// The `Alt` bindings.
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

    /// Takes out `from..to`, keeps it for `Ctrl-Y`, and leaves the cursor where
    /// what was removed used to start.
    ///
    /// Nothing removed is nothing done, rather than an empty kill: `Ctrl-K` at
    /// the end of a line has taken nothing out, and letting it clear what
    /// `Ctrl-W` was holding would turn the key that puts text back into one that
    /// sometimes silently does not.
    fn kill(&mut self, from: usize, to: usize) -> Option<Edit> {
        if from >= to {
            return None;
        }
        self.cut = self.text[from..to].to_string();
        self.text.replace_range(from..to, "");
        self.at = from;
        Some(Edit::Typed)
    }

    /// Puts back what the last kill took, and keeps holding it: a yank is a
    /// paste and not a hand-over, so the same text can go in twice.
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

    /// One character back, and the start of the line when there is none.
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

    /// Where the word before the cursor starts: back over whatever separates the
    /// words first, then over the word itself. That order is what makes the key
    /// work both from just after a word and from the run of spaces after one,
    /// which is where a cursor usually is when somebody reaches for it.
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

    /// The same walk the other way.
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
        // The reason `at` is in bytes: everything that reads it slices the text,
        // and a notebook whose titles are Chinese is the ordinary case.
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
        // The older definition, and what makes `Ctrl-W` the key for taking a
        // term off the end of a query: it stops only at whitespace.
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
        // What makes `Ctrl-W` safe to press: it is the only way back in a field
        // that has no undo.
        let mut field = typed("tag:work budget");
        field.key(ctrl('w'));
        assert_eq!(field.text(), "tag:work ");
        assert_eq!(field.key(ctrl('y')), Some(Edit::Typed));
        assert_eq!(field.text(), "tag:work budget");
        assert_eq!(field.before(), "tag:work budget");
        // And twice over, because a yank keeps what it put back.
        field.key(ctrl('y'));
        assert_eq!(field.text(), "tag:work budgetbudget");
    }

    #[test]
    fn a_kill_that_took_nothing_keeps_what_the_last_one_took() {
        let mut field = typed("budget");
        field.key(ctrl('u'));
        // At the start of an empty line there is nothing to either side of it.
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
        // At the end of the line it is nothing at all, rather than the shell's
        // end-of-file or the browser's delete.
        field.key(ctrl('e'));
        assert_eq!(field.key(ctrl('d')), None);
        assert_eq!(field.text(), "dget");
    }

    #[test]
    fn a_chord_this_does_not_bind_does_nothing_at_all() {
        // The trap the module is here for: `Ctrl-P` arrives as `Char('p')`, and
        // a field that took it at face value would quietly put a `p` in the
        // middle of a title.
        let mut field = typed("Trip");
        assert_eq!(field.key(ctrl('p')), None);
        assert_eq!(field.key(alt('x')), None);
        assert_eq!(field.text(), "Trip");
        // A capital is still a capital: shift is not a chord.
        let shifted = KeyEvent::new(KeyCode::Char('Q'), KeyModifiers::SHIFT);
        assert_eq!(field.key(shifted), Some(Edit::Typed));
        assert_eq!(field.text(), "TripQ");
    }

    #[test]
    fn a_field_with_no_cursor_drawn_only_erases_from_the_end() {
        let mut field = typed("push");
        // The keys that leave the cursor where the reader can see it.
        assert_eq!(field.erasing(ctrl('h')), Some(Edit::Typed));
        assert_eq!(field.text(), "pus");
        assert_eq!(field.erasing(ctrl('u')), Some(Edit::Typed));
        assert_eq!(field.text(), "");
        // And the ones that would move it somewhere invisible do nothing.
        field.set("push".to_string());
        assert_eq!(field.erasing(ctrl('a')), None);
        assert_eq!(field.erasing(alt('b')), None);
        assert_eq!(field.erasing(key(KeyCode::Left)), None);
        field.erasing(key(KeyCode::Char('x')));
        assert_eq!(field.text(), "pushx", "and typing still goes on the end");
    }

    #[test]
    fn what_is_put_in_the_field_can_be_added_to_straight_away() {
        // The retitle case: a title handed over to be edited, with the cursor
        // after it rather than in front of it.
        let mut field = typed("Budget review");
        field.key(key(KeyCode::Char('!')));
        assert_eq!(field.text(), "Budget review!");
    }
}
