//! What a browsing session holds, and how one keystroke changes it.
//!
//! Nothing here opens a file, a repository or a terminal. A key goes in, the
//! state moves, and the two things that need the world outside — leaving, and
//! reading the notebook again — come back out as an [`Action`] for the runtime
//! to carry out. That is what lets the whole of the interaction be tested with
//! no terminal attached, which matters more here than anywhere else in noda:
//! every other command is a function returning a string, and this one is a loop.
//!
//! The notes are held in memory for the length of the session. `noda search`
//! already reads every body on every invocation, so this is that cost paid once
//! instead of once per query — which is the whole point of typing into a filter
//! rather than into a shell.

use std::path::PathBuf;

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::widgets::TableState;

use crate::note;
use crate::notebook::{NoteFile, Status};
use crate::query::Query;

/// Something the runtime has to do that the state cannot do for itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Quit,
    /// Read the notebook again: another process may have written a note, and
    /// this is the browser's answer to that rather than a file watcher.
    Reload,
}

/// Which half of the screen the arrow keys are steering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    List,
    Preview,
}

/// What the keyboard currently means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Browse,
    /// Typing a query. The list narrows on every keystroke, so there is no
    /// "run the search" step — `Enter` only puts the keyboard back on the list.
    Search,
    Help,
}

/// A note's text as the preview last read it from disk.
///
/// Read from the file rather than rendered back from the parsed note, for the
/// reason `noda show` reads the file: the preview should show what is on disk,
/// including a frontmatter field noda does not interpret and would not know to
/// write back.
pub struct Preview {
    pub id: String,
    pub text: String,
}

pub struct App {
    /// The active notebook's name, for the header.
    pub notebook: String,
    /// Its directory, which is what turns a note under the cursor into a file
    /// for the runtime to read.
    pub root: PathBuf,
    /// Where the notebook stands, as of the last load. Nothing here touches the
    /// network — the drift is what the last sync left behind, exactly as
    /// `noda status` reports it.
    pub status: Status,
    /// Every note the notebook holds, in the order the walk produced: by slug,
    /// which is what `noda ls` shows without `--sort`.
    notes: Vec<NoteFile>,
    /// Indices into `notes` that the current query admits, in the same order.
    visible: Vec<usize>,
    /// The cursor, and the list's scroll offset, which ratatui keeps for us.
    pub table: TableState,
    pub mode: Mode,
    pub focus: Focus,
    /// The query as typed. Split on whitespace it is what `noda search` takes,
    /// one token per argument — the shell's job, done here by the space bar.
    pub search: String,
    /// The text terms of the active query, for picking the match out of a title
    /// and a body. A `tag:` or an `id:` matched something the prose does not
    /// contain, so there is nothing in the preview to point at.
    pub terms: Vec<String>,
    /// Why the query as typed is not a query yet.
    pub error: Option<String>,
    pub preview: Option<Preview>,
    pub scroll: u16,
    /// How many rows the list last had room for, written back by the drawing
    /// code. `Ctrl-d` means "half of what I can see", and only the drawing knows
    /// how much that is.
    page: u16,
}

impl App {
    pub fn new(notebook: String, root: PathBuf, status: Status, notes: Vec<NoteFile>) -> App {
        let visible = (0..notes.len()).collect();
        let mut app = App {
            notebook,
            root,
            status,
            notes,
            visible,
            table: TableState::new(),
            mode: Mode::Browse,
            focus: Focus::List,
            search: String::new(),
            terms: Vec::new(),
            error: None,
            preview: None,
            scroll: 0,
            page: 10,
        };
        app.select(0);
        app
    }

    /// Swaps in a freshly read notebook, keeping the query and — when the note
    /// is still there — the cursor. A reload that jumped back to the top would
    /// be a reason not to press the key.
    pub fn replace(&mut self, status: Status, notes: Vec<NoteFile>) {
        let was = self.selected().map(|file| file.id.clone());
        self.status = status;
        self.notes = notes;
        self.refilter();
        if let Some(id) = was
            && let Some(at) = self.visible.iter().position(|&i| self.notes[i].id == id)
        {
            self.select(at);
        }
        // Dropped rather than kept: the file on disk is what changed, and this
        // copy of it is what the reload was pressed to get rid of.
        self.preview = None;
    }

    /// The note under the cursor.
    pub fn selected(&self) -> Option<&NoteFile> {
        let at = self.table.selected()?;
        self.notes.get(*self.visible.get(at)?)
    }

    /// The notes the query admits, in listing order.
    pub fn rows(&self) -> impl Iterator<Item = &NoteFile> {
        self.visible.iter().filter_map(|&i| self.notes.get(i))
    }

    pub fn shown(&self) -> usize {
        self.visible.len()
    }

    pub fn total(&self) -> usize {
        self.notes.len()
    }

    /// The note whose file the runtime should read, when the preview on screen
    /// is not the note under the cursor. Owned, because the caller needs the
    /// state back to put the answer in it.
    pub fn preview_wanted(&self) -> Option<(String, PathBuf)> {
        let file = self.selected()?;
        match &self.preview {
            Some(preview) if preview.id == file.id => None,
            _ => Some((
                file.id.clone(),
                self.root.join(note::file_name(&file.id, &file.slug)),
            )),
        }
    }

    pub fn set_preview(&mut self, id: String, text: String) {
        self.preview = Some(Preview { id, text });
        // A new note starts at its top. Carrying the old offset over would open
        // a short note somewhere past its end.
        self.scroll = 0;
    }

    /// Told by the drawing code, which is the only part that knows.
    pub fn set_page(&mut self, rows: u16) {
        self.page = rows.max(1);
    }

    /// Applies a keystroke. `None` means the state moved and nothing else.
    pub fn on_key(&mut self, key: KeyEvent) -> Option<Action> {
        // Windows sends a press and a release for every key; acting on both
        // would move the cursor twice per stroke.
        if key.kind != KeyEventKind::Press {
            return None;
        }
        // Ctrl-C leaves from wherever you are, including mid-query. It is the
        // one key that means the same thing in every mode, because it is what
        // the terminal itself would have meant by it.
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            return Some(Action::Quit);
        }
        match self.mode {
            // Any key at all dismisses the help. It is a card, not a menu.
            Mode::Help => {
                self.mode = Mode::Browse;
                None
            }
            Mode::Search => self.searching(key),
            Mode::Browse => self.browsing(key),
        }
    }

    fn browsing(&mut self, key: KeyEvent) -> Option<Action> {
        let half = i32::from(self.page.max(2) / 2);
        let page = i32::from(self.page);
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('d') => self.step(half),
                KeyCode::Char('u') => self.step(-half),
                _ => {}
            }
            return None;
        }
        match key.code {
            KeyCode::Char('q') => return Some(Action::Quit),
            KeyCode::Char('r') => return Some(Action::Reload),
            KeyCode::Char('?') => self.mode = Mode::Help,
            KeyCode::Char('/') => self.mode = Mode::Search,
            KeyCode::Char('j') | KeyCode::Down => self.step(1),
            KeyCode::Char('k') | KeyCode::Up => self.step(-1),
            KeyCode::PageDown => self.step(page),
            KeyCode::PageUp => self.step(-page),
            KeyCode::Char('g') | KeyCode::Home => self.jump(Edge::First),
            KeyCode::Char('G') | KeyCode::End => self.jump(Edge::Last),
            KeyCode::Tab => {
                self.focus = match self.focus {
                    Focus::List => Focus::Preview,
                    Focus::Preview => Focus::List,
                };
            }
            KeyCode::Char('h') | KeyCode::Left => self.focus = Focus::List,
            KeyCode::Char('l') | KeyCode::Right | KeyCode::Enter => self.focus = Focus::Preview,
            // Whatever the query narrowed the notebook to, this is the way back
            // out — the same key that would have abandoned the typing.
            KeyCode::Esc => {
                self.search.clear();
                self.refilter();
                self.focus = Focus::List;
            }
            _ => {}
        }
        None
    }

    fn searching(&mut self, key: KeyEvent) -> Option<Action> {
        match key.code {
            // The list is already narrowed, so there is nothing left to run:
            // this only hands the keyboard back to the list.
            KeyCode::Enter => {
                self.mode = Mode::Browse;
                self.focus = Focus::List;
            }
            // Leaving the query behind means leaving what it selected behind
            // too, which is what makes it an escape rather than a commit.
            KeyCode::Esc => {
                self.search.clear();
                self.refilter();
                self.mode = Mode::Browse;
            }
            KeyCode::Backspace => {
                self.search.pop();
                self.refilter();
            }
            // The cursor still moves while the query is being typed, so a hit
            // can be read in the preview without stopping to leave the field.
            KeyCode::Down => self.step(1),
            KeyCode::Up => self.step(-1),
            KeyCode::Char(c) => {
                self.search.push(c);
                self.refilter();
            }
            _ => {}
        }
        None
    }

    /// Moves the cursor, or the preview, by `delta` rows.
    fn step(&mut self, delta: i32) {
        if self.focus == Focus::Preview && self.mode != Mode::Search {
            self.scroll = scrolled(self.scroll, delta, self.preview_height());
            return;
        }
        let Some(at) = self.table.selected() else {
            return;
        };
        let last = self.visible.len().saturating_sub(1);
        let moved = match usize::try_from(delta) {
            Ok(down) => at.saturating_add(down).min(last),
            Err(_) => at.saturating_sub(delta.unsigned_abs() as usize),
        };
        self.select(moved);
    }

    fn jump(&mut self, edge: Edge) {
        if self.focus == Focus::Preview && self.mode != Mode::Search {
            self.scroll = match edge {
                Edge::First => 0,
                Edge::Last => self.preview_height(),
            };
            return;
        }
        match edge {
            Edge::First => self.select(0),
            Edge::Last => self.select(self.visible.len().saturating_sub(1)),
        }
    }

    /// How far the preview may be scrolled: its last line, so the note's end
    /// can be brought to the top of the pane and no further.
    ///
    /// Counted before wrapping, so a note of long lines can be scrolled less far
    /// than it is tall. Under-shooting leaves text reachable; over-shooting
    /// would scroll into blank space, which reads like a bug.
    fn preview_height(&self) -> u16 {
        self.preview.as_ref().map_or(0, |preview| {
            preview.text.lines().count().saturating_sub(1) as u16
        })
    }

    fn select(&mut self, at: usize) {
        if self.visible.is_empty() {
            self.table.select(None);
        } else {
            self.table.select(Some(at.min(self.visible.len() - 1)));
        }
    }

    /// Reruns the query over every note.
    ///
    /// Whole rather than incremental: a keystroke can widen a query as easily as
    /// narrow it (a backspace, or an `OR` completed), so there is no subset to
    /// refine. At `noda ls` speeds the notebook is walked in memory in well
    /// under a frame.
    fn refilter(&mut self) {
        let tokens: Vec<String> = self
            .search
            .split_whitespace()
            .map(std::string::ToString::to_string)
            .collect();

        if tokens.is_empty() {
            self.error = None;
            self.terms.clear();
            self.visible = (0..self.notes.len()).collect();
            self.select(0);
            return;
        }

        match Query::parse(&tokens) {
            Ok(query) => {
                self.error = None;
                self.terms = query.excerpt_terms();
                self.visible = self
                    .notes
                    .iter()
                    .enumerate()
                    .filter(|(_, file)| query.matches(&file.id, &file.note))
                    .map(|(at, _)| at)
                    .collect();
                self.select(0);
            }
            // Half a query is the ordinary state of one being typed: `tag:`
            // before its value, `budget OR` before its alternative. Say so, and
            // leave the last good result under the cursor — emptying the list at
            // every other keystroke would make the query harder to type, which
            // is the opposite of what filtering as you go is for.
            Err(e) => self.error = Some(e.to_string()),
        }
    }
}

enum Edge {
    First,
    Last,
}

/// A scroll offset moved by `delta` and kept inside `[0, max]`.
fn scrolled(from: u16, delta: i32, max: u16) -> u16 {
    let moved = match u16::try_from(delta) {
        Ok(down) => from.saturating_add(down),
        Err(_) => from.saturating_sub(u16::try_from(delta.unsigned_abs()).unwrap_or(u16::MAX)),
    };
    moved.min(max)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::note::Note;
    use crate::notebook::Status;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn ctrl(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    fn typing(app: &mut App, text: &str) {
        for c in text.chars() {
            app.on_key(key(KeyCode::Char(c)));
        }
    }

    fn a_status() -> Status {
        Status {
            branch: "main".to_string(),
            notes: 3,
            files: 0,
            uncommitted: 0,
            remote: None,
            drift: None,
            problems: Vec::new(),
        }
    }

    fn a_note(id: &str, slug: &str, title: &str, tags: &[&str], body: &str) -> NoteFile {
        NoteFile {
            id: id.to_string(),
            slug: slug.to_string(),
            note: Note {
                title: title.to_string(),
                tags: tags.iter().map(|t| (*t).to_string()).collect(),
                created: None,
                updated: None,
                extra: Vec::new(),
                body: body.to_string(),
            },
        }
    }

    // Fixed ids, never minted: a test that draws its ids from the notebook is a
    // test that changes what it asserts every time it runs.
    fn an_app() -> App {
        App::new(
            "personal".to_string(),
            PathBuf::from("/notebook"),
            a_status(),
            vec![
                a_note(
                    "aaaa1111",
                    "budget-review",
                    "Budget review",
                    &["work"],
                    "the q3 budget",
                ),
                a_note(
                    "bbbb2222",
                    "meeting-notes",
                    "Meeting notes",
                    &["work", "q3"],
                    "agenda",
                ),
                a_note(
                    "cccc3333",
                    "reading-list",
                    "Reading list",
                    &[],
                    "a book about budgets",
                ),
            ],
        )
    }

    #[test]
    fn it_opens_on_the_first_note() {
        let app = an_app();
        assert_eq!(app.selected().map(|f| f.id.as_str()), Some("aaaa1111"));
        assert_eq!(app.shown(), 3);
    }

    #[test]
    fn the_cursor_stops_at_both_ends() {
        let mut app = an_app();
        for _ in 0..10 {
            app.on_key(key(KeyCode::Char('j')));
        }
        assert_eq!(app.selected().map(|f| f.id.as_str()), Some("cccc3333"));
        for _ in 0..10 {
            app.on_key(key(KeyCode::Char('k')));
        }
        assert_eq!(app.selected().map(|f| f.id.as_str()), Some("aaaa1111"));
    }

    #[test]
    fn g_and_shift_g_reach_the_ends_in_one_key() {
        let mut app = an_app();
        app.on_key(key(KeyCode::Char('G')));
        assert_eq!(app.selected().map(|f| f.id.as_str()), Some("cccc3333"));
        app.on_key(key(KeyCode::Char('g')));
        assert_eq!(app.selected().map(|f| f.id.as_str()), Some("aaaa1111"));
    }

    #[test]
    fn the_query_narrows_as_it_is_typed() {
        let mut app = an_app();
        app.on_key(key(KeyCode::Char('/')));
        assert_eq!(app.mode, Mode::Search);

        typing(&mut app, "budget");
        // The title of one, the body of another: `text:` is what a bare word
        // searches, and that is the same rule `noda search` follows.
        assert_eq!(app.shown(), 2);

        typing(&mut app, " tag:work");
        assert_eq!(app.shown(), 1);
        assert_eq!(app.selected().map(|f| f.id.as_str()), Some("aaaa1111"));
    }

    #[test]
    fn a_half_typed_query_keeps_the_last_good_result() {
        let mut app = an_app();
        app.on_key(key(KeyCode::Char('/')));
        typing(&mut app, "tag:work");
        assert_eq!(app.shown(), 2);

        // Every keystroke up to this one still spells a query — a partial word
        // is a `text:` term and matches whatever it matches. The count only has
        // to hold still across the keystroke that breaks the grammar.
        typing(&mut app, " O");
        let last_good = app.shown();
        assert!(app.error.is_none());

        // `OR` with nothing after it yet: the state every alternative passes
        // through on its way to being one.
        typing(&mut app, "R");
        assert!(app.error.is_some());
        assert_eq!(
            app.shown(),
            last_good,
            "an unfinished query leaves the list where it was"
        );

        typing(&mut app, " tag:q3");
        assert!(app.error.is_none());
        assert_eq!(app.shown(), 2);
    }

    #[test]
    fn escape_gives_the_whole_notebook_back() {
        let mut app = an_app();
        app.on_key(key(KeyCode::Char('/')));
        typing(&mut app, "tag:q3");
        assert_eq!(app.shown(), 1);

        app.on_key(key(KeyCode::Esc));
        assert_eq!(app.mode, Mode::Browse);
        assert_eq!(app.shown(), 3);
        assert!(app.search.is_empty());
    }

    #[test]
    fn enter_keeps_the_query_and_hands_back_the_keyboard() {
        let mut app = an_app();
        app.on_key(key(KeyCode::Char('/')));
        typing(&mut app, "tag:q3");
        app.on_key(key(KeyCode::Enter));

        assert_eq!(app.mode, Mode::Browse);
        assert_eq!(app.focus, Focus::List);
        assert_eq!(app.shown(), 1);
    }

    #[test]
    fn a_query_that_matches_nothing_leaves_no_cursor() {
        let mut app = an_app();
        app.on_key(key(KeyCode::Char('/')));
        typing(&mut app, "tag:nothing");
        assert_eq!(app.shown(), 0);
        assert!(app.selected().is_none());
        assert!(app.preview_wanted().is_none());
    }

    #[test]
    fn the_preview_follows_the_cursor() {
        let mut app = an_app();
        assert_eq!(
            app.preview_wanted(),
            Some((
                "aaaa1111".to_string(),
                PathBuf::from("/notebook/aaaa1111-budget-review.md")
            ))
        );
        app.set_preview("aaaa1111".to_string(), "one\ntwo\nthree\n".to_string());
        assert!(app.preview_wanted().is_none());

        app.on_key(key(KeyCode::Char('j')));
        assert_eq!(
            app.preview_wanted(),
            Some((
                "bbbb2222".to_string(),
                PathBuf::from("/notebook/bbbb2222-meeting-notes.md")
            ))
        );
    }

    #[test]
    fn the_preview_scrolls_only_when_it_has_the_focus() {
        let mut app = an_app();
        app.set_preview(
            "aaaa1111".to_string(),
            "one\ntwo\nthree\nfour\n".to_string(),
        );

        app.on_key(key(KeyCode::Char('j')));
        assert_eq!(app.scroll, 0, "the list had the focus");

        app.on_key(key(KeyCode::Tab));
        assert_eq!(app.focus, Focus::Preview);
        app.on_key(key(KeyCode::Char('j')));
        assert_eq!(app.scroll, 1);
        // And it stops at the end rather than running off into blank space.
        for _ in 0..20 {
            app.on_key(key(KeyCode::Char('j')));
        }
        assert_eq!(app.scroll, 3);
    }

    #[test]
    fn a_new_note_starts_at_its_top() {
        let mut app = an_app();
        app.set_preview("aaaa1111".to_string(), "one\ntwo\nthree\n".to_string());
        app.on_key(key(KeyCode::Tab));
        app.on_key(key(KeyCode::Char('j')));
        assert_eq!(app.scroll, 1);

        app.set_preview("bbbb2222".to_string(), "agenda\n".to_string());
        assert_eq!(app.scroll, 0);
    }

    #[test]
    fn ctrl_d_moves_by_half_of_what_is_on_screen() {
        let mut app = an_app();
        app.set_page(4);
        app.on_key(ctrl('d'));
        assert_eq!(app.selected().map(|f| f.id.as_str()), Some("cccc3333"));
        app.on_key(ctrl('u'));
        assert_eq!(app.selected().map(|f| f.id.as_str()), Some("aaaa1111"));
    }

    #[test]
    fn quitting_and_reloading_are_the_runtimes_business() {
        let mut app = an_app();
        assert_eq!(app.on_key(key(KeyCode::Char('q'))), Some(Action::Quit));
        assert_eq!(app.on_key(key(KeyCode::Char('r'))), Some(Action::Reload));
        assert_eq!(app.on_key(ctrl('c')), Some(Action::Quit));
    }

    #[test]
    fn ctrl_c_leaves_from_inside_a_query_too() {
        let mut app = an_app();
        app.on_key(key(KeyCode::Char('/')));
        typing(&mut app, "budget");
        assert_eq!(app.on_key(ctrl('c')), Some(Action::Quit));
    }

    #[test]
    fn q_is_a_letter_while_a_query_is_being_typed() {
        let mut app = an_app();
        app.on_key(key(KeyCode::Char('/')));
        assert_eq!(app.on_key(key(KeyCode::Char('q'))), None);
        assert_eq!(app.search, "q");
    }

    #[test]
    fn help_is_dismissed_by_anything() {
        let mut app = an_app();
        app.on_key(key(KeyCode::Char('?')));
        assert_eq!(app.mode, Mode::Help);
        app.on_key(key(KeyCode::Char('x')));
        assert_eq!(app.mode, Mode::Browse);
    }

    #[test]
    fn a_reload_keeps_the_query_and_the_note_under_the_cursor() {
        let mut app = an_app();
        app.on_key(key(KeyCode::Char('/')));
        typing(&mut app, "tag:work");
        app.on_key(key(KeyCode::Enter));
        app.on_key(key(KeyCode::Char('j')));
        assert_eq!(app.selected().map(|f| f.id.as_str()), Some("bbbb2222"));

        let mut notes = vec![
            a_note(
                "aaaa1111",
                "budget-review",
                "Budget review",
                &["work"],
                "the q3 budget",
            ),
            a_note(
                "bbbb2222",
                "meeting-notes",
                "Meeting notes",
                &["work", "q3"],
                "agenda, revised",
            ),
            a_note(
                "cccc3333",
                "reading-list",
                "Reading list",
                &[],
                "a book about budgets",
            ),
        ];
        notes.push(a_note(
            "dddd4444",
            "trip-plan",
            "Trip plan",
            &["work"],
            "flights",
        ));
        app.replace(a_status(), notes);

        assert_eq!(app.search, "tag:work");
        assert_eq!(app.shown(), 3);
        assert_eq!(app.selected().map(|f| f.id.as_str()), Some("bbbb2222"));
        // The copy on screen was the reason to press the key, so it goes.
        assert!(app.preview_wanted().is_some());
    }

    #[test]
    fn a_reload_that_removes_the_selected_note_lands_somewhere_real() {
        let mut app = an_app();
        app.on_key(key(KeyCode::Char('G')));
        assert_eq!(app.selected().map(|f| f.id.as_str()), Some("cccc3333"));

        app.replace(
            a_status(),
            vec![a_note(
                "aaaa1111",
                "budget-review",
                "Budget review",
                &["work"],
                "q3",
            )],
        );
        assert_eq!(app.selected().map(|f| f.id.as_str()), Some("aaaa1111"));
    }

    #[test]
    fn an_empty_notebook_has_nowhere_to_put_the_cursor() {
        let mut app = App::new(
            "personal".to_string(),
            PathBuf::from("/notebook"),
            a_status(),
            Vec::new(),
        );
        assert!(app.selected().is_none());
        app.on_key(key(KeyCode::Char('j')));
        app.on_key(key(KeyCode::Char('G')));
        assert!(app.selected().is_none());
    }
}
