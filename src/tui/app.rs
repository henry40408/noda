//! What a browsing session holds, and how one keystroke changes it.
//!
//! Nothing here opens a file, a repository or a terminal. A key goes in, the
//! state moves, and everything that needs the world outside — leaving, reading
//! the notebook again, and every change to a note — comes back out as an
//! [`Action`] for the runtime to carry out. That is what lets the whole of the
//! interaction be tested with no terminal attached, which matters more here than
//! anywhere else in noda: every other command is a function returning a string,
//! and this one is a loop.
//!
//! The keys that change a note ask a command to do it. `e` is `noda edit`, `#`
//! is `noda tag`, and the answer that comes back is the line that command would
//! have printed — so what a change means is written down once, in `cmd`, and
//! this is a second way of asking for it rather than a second version of it.
//!
//! The notes are held in memory for the length of the session. `noda search`
//! already reads every body on every invocation, so this is that cost paid once
//! instead of once per query — which is the whole point of typing into a filter
//! rather than into a shell.

use std::collections::BTreeSet;
use std::path::PathBuf;

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::widgets::TableState;

use crate::Result;
use crate::cmd::{self, Change, Step, Touch};
use crate::note;
use crate::notebook::{NoteFile, Status};
use crate::query::Query;

/// Something the runtime has to do that the state cannot do for itself.
///
/// The five that change a notebook name a command and its arguments, never an
/// edit: what a change *means* — the title validated, the tags cleaned,
/// `updated` stamped, the result committed — lives in `cmd`, and a second
/// account of it is the one thing that must not exist. So nothing here describes
/// a file to write; each is a call to make, and the answer comes back as the
/// line that command would have printed.
///
/// A note is named by its id rather than by the row it was on. The command opens
/// the notebook again for itself, and by then the listing is only a picture of
/// what was there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Quit,
    /// Read the notebook again: another process may have written a note, and
    /// this is the browser's answer to that rather than a file watcher.
    Reload,
    /// `noda edit` — the one action that needs the terminal handed back for the
    /// length of it, because `$EDITOR` is a full-screen program too.
    Edit {
        key: String,
        touch: Touch,
    },
    /// `noda add`, with the title as typed. `None` is a title left to the body,
    /// which is what `add` does when it is given none.
    ///
    /// No `touch`: `add` has none either. A note that has never been changed has
    /// been changed as recently as it was made.
    Add(Option<String>),
    /// `noda mv`: a new title, and the slug follows it.
    Retitle {
        key: String,
        title: String,
        touch: Touch,
    },
    /// `noda tag`, with the `+tag` / `-tag` changes as typed.
    Tag {
        key: String,
        changes: Vec<String>,
        touch: Touch,
    },
    /// `noda rm`, once the confirmation has been given. Nothing is left to stamp.
    Remove(String),
    /// The queue, sent: every change in it, over the notes each was aimed at, in
    /// one commit. `cmd::bulk` is what carries it out — the same code writing the
    /// same files as the keys above, with the commit boundary moved out one
    /// level so that a queue arrives in the history as the one thing it was.
    Send(Vec<Step>),
}

/// What a command said when it was asked to change something.
///
/// Its own words. A browser that summarised them would be deciding what a
/// command meant, which is the same mistake as writing the change twice.
pub struct Message {
    pub text: String,
    /// Failures get a card and successes get the footer: an acknowledgement is
    /// read in passing, a reason has to be read.
    pub failed: bool,
}

impl Message {
    /// As much of it as one line of footer can hold.
    pub fn line(&self) -> &str {
        self.text.lines().next().unwrap_or_default()
    }
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
    /// Typing the one thing a change needs said in words.
    Ask(Ask),
    /// Waiting for a `y` before something that cannot be taken back. The
    /// confirmation `noda rm` does not ask for at the prompt is asked for here,
    /// and asked for on the screen: the terminal is in raw mode, so a command
    /// that read a line from stdin would be reading keystrokes out from under
    /// the browser.
    Confirm(What),
    /// Reading the queue: what is waiting to be sent, and what to drop from it.
    Queue,
    /// A command said why it would not do something, or said more than a line.
    /// Dismissed like the help card, by anything at all.
    Alert,
}

/// What a `y` would agree to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum What {
    /// Delete the note under the cursor, now.
    Delete,
    /// Send the queue. Asked once, at the point where nothing can be taken back
    /// — which is the send, not the queueing: a delete sitting in the queue has
    /// not happened and can still be dropped from it.
    Send,
}

/// The one thing a change needs said in words before it can be asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ask {
    /// A title for a new note. `$EDITOR` opens on the body once it is given.
    Title,
    /// A new title for the note under the cursor.
    Retitle,
    /// `+tag` / `-tag` changes, in the words `noda tag` takes them in.
    Tags,
}

impl Ask {
    /// What the footer calls the field.
    pub fn prompt(self) -> &'static str {
        match self {
            Ask::Title => "new note",
            Ask::Retitle => "retitle",
            Ask::Tags => "tags",
        }
    }

    /// The part of the answer that cannot be guessed from the prompt, shown
    /// where the count would be. Nothing, where the prompt says it all.
    pub fn hint(self) -> &'static str {
        match self {
            Ask::Title => "Enter alone takes the title from the body",
            Ask::Retitle => "",
            // The quotes are in the example because a tag with a space in it is
            // the one case the syntax does not survive being guessed at.
            Ask::Tags => "+work -q3 -\"two words\"",
        }
    }
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
    /// What is being typed into the prompt, when there is one. A title, a new
    /// title or a run of tag changes — one field, because only one of them can
    /// be open at a time and [`Mode::Ask`] already says which.
    pub input: String,
    /// What the last command that changed something had to say.
    pub message: Option<Message>,
    /// The notes picked out to be changed together, by id.
    ///
    /// Kept apart from the query, deliberately. `/` narrows what can be seen and
    /// marking says what is meant to change, and neither undoes the other: a
    /// note the query is currently hiding is still marked and still gets
    /// changed. Otherwise "mark these, then search for the next lot" would
    /// quietly drop the first lot, and the two would be one feature wearing two
    /// keys.
    ///
    /// Ordered, so the queue reads the same way twice and a commit message does
    /// not depend on the order a hash happened to produce.
    pub marks: BTreeSet<String>,
    /// The changes waiting to be sent, in the order they were added.
    ///
    /// Each is a `cmd::Step` — a change and the notes it is aimed at — because
    /// that is what will be handed to `cmd::bulk` unaltered. A queue that held
    /// something else would have to be translated at the end, and a translation
    /// is where a second account of what a change means gets in.
    pub queue: Vec<Step>,
    /// Where the cursor is in the queue view.
    queue_at: usize,
    /// Whether the changes made from here move a note's `updated`.
    ///
    /// A session-long setting rather than something said per keystroke. At a
    /// prompt `--no-touch` is written on the one command it applies to; here
    /// there is no room to qualify a single key, and the reason for wanting it —
    /// a sitting of small corrections to notes whose dates came from somewhere
    /// else — lasts longer than one keystroke anyway. It says so in the header
    /// for as long as it is on, because a setting you cannot see is one you
    /// forget you left on.
    pub touch: Touch,
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
            input: String::new(),
            message: None,
            marks: BTreeSet::new(),
            queue: Vec::new(),
            queue_at: 0,
            touch: Touch::Stamp,
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
    ///
    /// When it is not there, the row it was on is what is kept instead. That is
    /// the case `d` makes ordinary: deleting the fortieth note of two hundred
    /// and being returned to the first would be a reason not to press that key
    /// either.
    pub fn replace(&mut self, status: Status, notes: Vec<NoteFile>) {
        let was = self.selected().map(|file| file.id.clone());
        let row = self.table.selected();
        self.status = status;
        self.notes = notes;
        self.refilter();
        match was.and_then(|id| self.visible.iter().position(|&i| self.notes[i].id == id)) {
            Some(at) => self.select(at),
            None => self.select(row.unwrap_or(0)),
        }
        // Dropped rather than kept: the file on disk is what changed, and this
        // copy of it is what the reload was pressed to get rid of.
        self.preview = None;
        // A mark on a note that is no longer there is a change aimed at nothing.
        // The queue keeps its own copy of the ids on purpose — an entry is a
        // record of what was asked for, and `bulk` is what says so if one of
        // them has since gone.
        let ids: BTreeSet<&str> = self.notes.iter().map(|file| file.id.as_str()).collect();
        self.marks.retain(|id| ids.contains(id.as_str()));
    }

    /// Whether the note is one of the ones picked out.
    pub fn marked(&self, id: &str) -> bool {
        self.marks.contains(id)
    }

    /// The marked notes, in listing order, for a change about to be aimed at
    /// them. Ids, because that is what a command takes and what survives a
    /// rename.
    fn marked_keys(&self) -> Vec<String> {
        self.marks.iter().cloned().collect()
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
            // Any key at all dismisses either card. They are cards, not menus.
            Mode::Help | Mode::Alert => {
                self.mode = Mode::Browse;
                self.message = None;
                None
            }
            Mode::Search => self.searching(key),
            Mode::Ask(what) => self.asking(what, key),
            Mode::Confirm(what) => self.confirming(what, key),
            Mode::Queue => self.queueing(key),
            Mode::Browse => self.browsing(key),
        }
    }

    fn browsing(&mut self, key: KeyEvent) -> Option<Action> {
        // Whatever the last command said has now been read, or has not been and
        // was only an acknowledgement. Either way the next key is the reader
        // moving on, and the footer goes back to saying what there is to press.
        self.message = None;
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
            // Not a change of its own — a change to what the next one records.
            KeyCode::Char('T') => {
                self.touch = match self.touch {
                    Touch::Stamp => Touch::Keep,
                    Touch::Keep => Touch::Stamp,
                };
            }
            // The keys that change a note. Each asks a command for it, and the
            // three that need something said first open the prompt instead.
            KeyCode::Char('e') => {
                return Some(Action::Edit {
                    key: self.selected()?.id.clone(),
                    touch: self.touch,
                });
            }
            KeyCode::Char('a') => self.ask(Ask::Title, String::new()),
            KeyCode::Char('m') => {
                let title = self.selected()?.note.title.clone();
                self.ask(Ask::Retitle, title);
            }
            // With notes marked these two aim at the marked set and go into the
            // queue; with none marked they are what they have always been, and
            // act on the note under the cursor at once. One key, one meaning —
            // "do this to what is picked out" — and what is picked out is the
            // cursor until you say otherwise. The header says which it will be.
            KeyCode::Char('#') => {
                // Nothing to tag is nothing to ask about — the same reason `m`
                // and `d` do nothing over an empty list.
                if self.marks.is_empty() {
                    self.selected()?;
                }
                self.ask(Ask::Tags, String::new());
            }
            KeyCode::Char('d') => {
                if self.marks.is_empty() {
                    self.selected()?;
                    self.mode = Mode::Confirm(What::Delete);
                } else {
                    // Not asked about here: queueing a delete deletes nothing,
                    // and the question belongs at the send, where it is the last
                    // moment it can still be answered no.
                    let keys = self.marked_keys();
                    self.enqueue(Step {
                        keys,
                        change: Change::Remove,
                    });
                }
            }
            // Marking. `Space` is the one under the cursor; `*` is everything
            // the query is showing, which is what makes a search and a selection
            // compose: narrow to what you mean, take the lot, search again.
            KeyCode::Char(' ') => {
                let id = self.selected()?.id.clone();
                if !self.marks.remove(&id) {
                    self.marks.insert(id);
                }
            }
            KeyCode::Char('*') => self.mark_shown(),
            KeyCode::Char('Q') => {
                self.mode = Mode::Queue;
                self.queue_at = self.queue_at.min(self.queue.len().saturating_sub(1));
            }
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
            // The way back out of whatever narrowing is in force, one layer at a
            // time: the query first, and once there is no query, the marks. Two
            // things that both make the notebook smaller than it is, undone by
            // the key that already meant "never mind" — and in that order,
            // because a query is cheap to retype and a selection is not.
            //
            // The queue is not in this chain. Dropping work by pressing Escape
            // once too often is the sort of thing a queue exists to prevent.
            KeyCode::Esc => {
                if self.search.is_empty() {
                    self.marks.clear();
                } else {
                    self.search.clear();
                    self.refilter();
                }
                self.focus = Focus::List;
            }
            _ => {}
        }
        None
    }

    /// Marks everything the query is showing, or takes the marks off it when it
    /// is all marked already — one key for both, because the answer to "what
    /// does this do now" is on the screen in front of you.
    ///
    /// What it does not touch: a marked note the query is hiding. `*` says
    /// something about what is shown, and nothing about what is not.
    fn mark_shown(&mut self) {
        let shown: Vec<String> = self.rows().map(|file| file.id.clone()).collect();
        if shown.iter().all(|id| self.marks.contains(id)) {
            for id in &shown {
                self.marks.remove(id);
            }
        } else {
            self.marks.extend(shown);
        }
    }

    /// Puts a change in the queue, or says why it cannot be one.
    ///
    /// Checked here rather than at the send, so a tag that cannot be written
    /// down is refused where it was typed — at the end of a sitting is too late
    /// to remember what was meant by it.
    fn enqueue(&mut self, step: Step) {
        if let Err(e) = cmd::check(&step.change) {
            self.report(Err(e));
            return;
        }
        self.queue.push(step);
    }

    fn queueing(&mut self, key: KeyEvent) -> Option<Action> {
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                self.queue_at = (self.queue_at + 1).min(self.queue.len().saturating_sub(1));
            }
            KeyCode::Char('k') | KeyCode::Up => self.queue_at = self.queue_at.saturating_sub(1),
            // Dropping an entry is the queue's whole point: what is waiting can
            // be reconsidered, which is what makes queueing safe to do quickly.
            KeyCode::Char('d' | 'x') | KeyCode::Backspace => {
                if self.queue_at < self.queue.len() {
                    self.queue.remove(self.queue_at);
                    self.queue_at = self.queue_at.min(self.queue.len().saturating_sub(1));
                }
            }
            KeyCode::Enter => return self.send(),
            KeyCode::Esc | KeyCode::Char('q' | 'Q') => self.mode = Mode::Browse,
            _ => {}
        }
        None
    }

    /// Sends the queue, once anything irreversible in it has been agreed to.
    fn send(&mut self) -> Option<Action> {
        if self.queue.is_empty() {
            self.mode = Mode::Browse;
            return None;
        }
        // A tag can be put back; a note that has been removed comes back only
        // through git. So the question is asked when there is a deletion in the
        // queue, and not otherwise — a confirmation that appears every time is
        // one nobody reads.
        if self.queue.iter().any(|step| step.change == Change::Remove) {
            self.mode = Mode::Confirm(What::Send);
            return None;
        }
        self.mode = Mode::Browse;
        Some(Action::Send(self.queue.clone()))
    }

    /// What the queue is aimed at altogether, for the confirmation to say.
    pub fn queued_notes(&self) -> usize {
        let notes: BTreeSet<&str> = self
            .queue
            .iter()
            .flat_map(|step| step.keys.iter().map(String::as_str))
            .collect();
        notes.len()
    }

    /// How many of the queued changes would delete something.
    pub fn queued_deletions(&self) -> usize {
        self.queue
            .iter()
            .filter(|step| step.change == Change::Remove)
            .map(|step| step.keys.len())
            .sum()
    }

    /// Which entry the queue view has its cursor on.
    pub fn queue_at(&self) -> usize {
        self.queue_at
    }

    /// Everything the send changed is now somebody else's business: the queue
    /// has been carried out and the notes it was aimed at are no longer picked
    /// out for anything.
    pub fn sent(&mut self) {
        self.queue.clear();
        self.queue_at = 0;
        self.marks.clear();
    }

    fn searching(&mut self, key: KeyEvent) -> Option<Action> {
        // A chord is not a character. `KeyCode::Char('d')` is what arrives for
        // Ctrl-D as much as for `d`, so without this every control key anyone
        // reaches for out of habit would type its own letter into the query —
        // silently, and into a query that is being read as it is typed.
        //
        // Shift is not one of these: `G` arrives as `Char('G')` with the shift
        // bit set, and a query with no capital letters in it would be a strange
        // thing to ship.
        if key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
        {
            return None;
        }
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

    /// Opens the prompt, with whatever it should start out holding — the current
    /// title, for a retitle, so the common edit is a few keystrokes rather than
    /// typing it all again.
    fn ask(&mut self, what: Ask, start: String) {
        self.mode = Mode::Ask(what);
        self.input = start;
    }

    fn asking(&mut self, what: Ask, key: KeyEvent) -> Option<Action> {
        // The same rule the query field lives by, and for the same reason: Ctrl-D
        // arrives as `Char('d')`, and a prompt that took it at face value would
        // quietly put a `d` in somebody's title. See `searching`.
        if key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
        {
            return None;
        }
        match key.code {
            KeyCode::Enter => return self.answered(what),
            KeyCode::Esc => {
                self.mode = Mode::Browse;
                self.input.clear();
            }
            KeyCode::Backspace => {
                self.input.pop();
            }
            KeyCode::Char(c) => self.input.push(c),
            _ => {}
        }
        None
    }

    /// Turns a filled-in prompt into the command it stands for.
    ///
    /// An empty answer is a way out rather than an error: somebody who has
    /// cleared the field has changed their mind, and reporting "a note needs a
    /// title" at them would be answering a question they stopped asking. The one
    /// exception is a new note, where nothing typed is what `noda add` already
    /// means by no title — the body will say what the note is called.
    fn answered(&mut self, what: Ask) -> Option<Action> {
        let answer = self.input.trim().to_string();
        self.mode = Mode::Browse;
        self.input.clear();
        match what {
            Ask::Title => Some(Action::Add((!answer.is_empty()).then_some(answer))),
            _ if answer.is_empty() => None,
            Ask::Retitle => Some(Action::Retitle {
                key: self.selected()?.id.clone(),
                title: answer,
                touch: self.touch,
            }),
            // The one prompt that answers to the marks: with a set picked out,
            // the tags it was given are queued against that set rather than run
            // against the note the cursor happens to be on.
            Ask::Tags if !self.marks.is_empty() => {
                let keys = self.marked_keys();
                self.enqueue(Step {
                    keys,
                    change: Change::Tag {
                        changes: split_quoted(&answer),
                        touch: self.touch,
                    },
                });
                None
            }
            Ask::Tags => Some(Action::Tag {
                key: self.selected()?.id.clone(),
                touch: self.touch,
                changes: split_quoted(&answer),
            }),
        }
    }

    /// `y` agrees; anything else is a way out. Not `n` alone — the key that
    /// cancels a destructive question should be every key but one.
    fn confirming(&mut self, what: What, key: KeyEvent) -> Option<Action> {
        self.mode = Mode::Browse;
        if !matches!(key.code, KeyCode::Char('y' | 'Y')) {
            return None;
        }
        match what {
            What::Delete => Some(Action::Remove(self.selected()?.id.clone())),
            What::Send => Some(Action::Send(self.queue.clone())),
        }
    }

    /// Takes down what a command said, in its own words.
    ///
    /// Public because the runtime is what ran the command: the state asked for
    /// it and has no way of finding out how it went.
    pub fn report(&mut self, outcome: Result<String>) {
        self.message = Some(match outcome {
            Ok(text) => {
                let text = text.trim_end().to_string();
                // A line is an acknowledgement and lives in the footer. More
                // than a line has to be read, so it gets the card: `bulk` puts
                // what it could not do underneath what it did, and that second
                // part is the whole reason it is worth printing.
                if text.lines().count() > 1 {
                    self.mode = Mode::Alert;
                }
                Message {
                    text,
                    failed: false,
                }
            }
            Err(e) => {
                // A card, and the whole of it: an editor that saved a broken
                // frontmatter block is told where the file was left, and losing
                // that to the width of a footer would be losing the part that
                // says what to do next.
                self.mode = Mode::Alert;
                Message {
                    text: e.to_string(),
                    failed: true,
                }
            }
        });
    }

    /// The ids the notebook held when it was last read, for the runtime to tell
    /// a note that has just been made from the ones that were already there.
    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.notes.iter().map(|file| file.id.as_str())
    }

    /// Puts the cursor on a note by id, if the query has left it on screen.
    ///
    /// What `a` needs: a note made and then left somewhere in a list of two
    /// hundred is a note you have to go and find.
    pub fn select_id(&mut self, id: &str) {
        if let Some(at) = self.visible.iter().position(|&i| self.notes[i].id == id) {
            self.select(at);
        }
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

/// A prompt's answer split the way a shell would split it: on whitespace, but
/// not inside quotes.
///
/// The space bar does the shell's job here, and this is the part of that job the
/// space bar cannot do. A tag is allowed to contain a space — `24.04 Dark
/// patterns` is the sort of thing a `TiddlyWiki` import leaves behind — and at a
/// prompt it is the shell's quoting that keeps it in one piece. Without this,
/// `-24.04 Dark patterns` arrives as three changes and the command rejects the
/// second, so a tag you can see in the listing is one you cannot remove from the
/// screen it is on.
///
/// Either quote character, because both are what the hands reach for. An
/// unclosed quote runs to the end rather than being an error: the line is being
/// typed, and the character that would close it is usually the next one.
fn split_quoted(text: &str) -> Vec<String> {
    let mut pieces = Vec::new();
    let mut piece = String::new();
    let mut quote: Option<char> = None;
    for c in text.chars() {
        match quote {
            Some(open) if c == open => quote = None,
            None if c == '"' || c == '\'' => quote = Some(c),
            // Only outside the quotes does a space end a piece. Inside them it
            // is part of the tag, which is the whole point.
            None if c.is_whitespace() => {
                if !piece.is_empty() {
                    pieces.push(std::mem::take(&mut piece));
                }
            }
            _ => piece.push(c),
        }
    }
    if !piece.is_empty() {
        pieces.push(piece);
    }
    pieces
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
    fn a_chord_does_not_type_its_own_letter_into_a_query() {
        let mut app = an_app();
        app.on_key(key(KeyCode::Char('/')));
        typing(&mut app, "budget");

        // Ctrl-D arrives as `Char('d')` with a modifier, and a query field that
        // took it at face value would quietly become `budgetd`.
        app.on_key(ctrl('d'));
        app.on_key(ctrl('w'));
        app.on_key(ctrl('a'));
        assert_eq!(app.search, "budget");

        // A capital is still a capital, though: shift is not a chord.
        app.on_key(KeyEvent::new(KeyCode::Char('Q'), KeyModifiers::SHIFT));
        assert_eq!(app.search, "budgetQ");
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

    #[test]
    fn e_asks_for_the_note_under_the_cursor_by_id() {
        let mut app = an_app();
        app.on_key(key(KeyCode::Char('j')));
        assert_eq!(
            app.on_key(key(KeyCode::Char('e'))),
            Some(Action::Edit {
                key: "bbbb2222".to_string(),
                touch: Touch::Stamp,
            })
        );
        // And the browser has not moved: the command is the runtime's to run.
        assert_eq!(app.mode, Mode::Browse);
    }

    #[test]
    fn a_new_note_is_titled_at_the_prompt() {
        let mut app = an_app();
        assert_eq!(app.on_key(key(KeyCode::Char('a'))), None);
        assert_eq!(app.mode, Mode::Ask(Ask::Title));

        typing(&mut app, "Trip plan");
        assert_eq!(app.input, "Trip plan");
        assert_eq!(
            app.on_key(key(KeyCode::Enter)),
            Some(Action::Add(Some("Trip plan".to_string())))
        );
        assert_eq!(app.mode, Mode::Browse);
        assert!(app.input.is_empty());
    }

    #[test]
    fn a_new_note_with_no_title_leaves_it_to_the_body() {
        let mut app = an_app();
        app.on_key(key(KeyCode::Char('a')));
        // Which is what `noda add` with no title of its own already means.
        assert_eq!(app.on_key(key(KeyCode::Enter)), Some(Action::Add(None)));
    }

    #[test]
    fn a_retitle_starts_from_the_title_it_has() {
        let mut app = an_app();
        app.on_key(key(KeyCode::Char('m')));
        assert_eq!(app.mode, Mode::Ask(Ask::Retitle));
        assert_eq!(app.input, "Budget review");

        for _ in 0.."review".len() {
            app.on_key(key(KeyCode::Backspace));
        }
        typing(&mut app, "revision");
        assert_eq!(
            app.on_key(key(KeyCode::Enter)),
            Some(Action::Retitle {
                key: "aaaa1111".to_string(),
                title: "Budget revision".to_string(),
                touch: Touch::Stamp,
            })
        );
    }

    #[test]
    fn tag_changes_are_split_the_way_a_shell_would_split_them() {
        let mut app = an_app();
        app.on_key(key(KeyCode::Char('#')));
        assert_eq!(app.mode, Mode::Ask(Ask::Tags));

        typing(&mut app, "+urgent -work");
        assert_eq!(
            app.on_key(key(KeyCode::Enter)),
            Some(Action::Tag {
                key: "aaaa1111".to_string(),
                changes: vec!["+urgent".to_string(), "-work".to_string()],
                touch: Touch::Stamp,
            })
        );
    }

    #[test]
    fn a_tag_with_a_space_in_it_survives_the_prompt() {
        let mut app = an_app();
        app.on_key(key(KeyCode::Char('#')));
        // What the listing shows for a note imported from TiddlyWiki, and what
        // you would type at a prompt to be rid of it.
        typing(&mut app, "-\"24.04 Dark patterns\" +work");
        assert_eq!(
            app.on_key(key(KeyCode::Enter)),
            Some(Action::Tag {
                key: "aaaa1111".to_string(),
                changes: vec!["-24.04 Dark patterns".to_string(), "+work".to_string()],
                touch: Touch::Stamp,
            })
        );
    }

    #[test]
    fn the_prompt_splits_the_way_a_shell_does() {
        assert_eq!(split_quoted("+work -q3"), vec!["+work", "-q3"]);
        assert_eq!(split_quoted("  +work   "), vec!["+work"]);
        assert!(split_quoted("   ").is_empty());
        // Either quote, and the quotes around the name rather than around the
        // whole piece — the `-` in front of them is what says remove.
        assert_eq!(split_quoted("-'a b' +c"), vec!["-a b", "+c"]);
        assert_eq!(split_quoted("-\"a b\""), vec!["-a b"]);
        // Quoted whole, which is what a hand used to a shell may well type.
        assert_eq!(split_quoted("\"-a b\""), vec!["-a b"]);
        // Half-typed: the line is still being written, so the quote that has not
        // been closed yet takes the rest rather than failing.
        assert_eq!(split_quoted("-\"a b"), vec!["-a b"]);
    }

    #[test]
    fn t_leaves_updated_alone_for_as_long_as_it_is_on() {
        let mut app = an_app();
        assert_eq!(app.touch, Touch::Stamp, "stamping is what a change means");

        assert_eq!(app.on_key(key(KeyCode::Char('T'))), None);
        assert_eq!(app.touch, Touch::Keep);

        // Every change made while it is on carries it, and it is the command's
        // own `--no-touch` that is being asked for rather than anything the
        // browser does to the file itself.
        assert_eq!(
            app.on_key(key(KeyCode::Char('e'))),
            Some(Action::Edit {
                key: "aaaa1111".to_string(),
                touch: Touch::Keep,
            })
        );
        app.on_key(key(KeyCode::Char('#')));
        typing(&mut app, "+urgent");
        assert_eq!(
            app.on_key(key(KeyCode::Enter)),
            Some(Action::Tag {
                key: "aaaa1111".to_string(),
                changes: vec!["+urgent".to_string()],
                touch: Touch::Keep,
            })
        );

        // And it is a toggle: the same key puts the stamping back.
        app.on_key(key(KeyCode::Char('T')));
        assert_eq!(app.touch, Touch::Stamp);
        assert_eq!(
            app.on_key(key(KeyCode::Char('e'))),
            Some(Action::Edit {
                key: "aaaa1111".to_string(),
                touch: Touch::Stamp,
            })
        );
    }

    #[test]
    fn t_is_a_letter_while_a_query_or_a_prompt_is_being_typed() {
        let mut app = an_app();
        app.on_key(key(KeyCode::Char('/')));
        typing(&mut app, "T");
        assert_eq!(app.search, "T");
        assert_eq!(app.touch, Touch::Stamp);

        app.on_key(key(KeyCode::Esc));
        app.on_key(key(KeyCode::Char('m')));
        typing(&mut app, "T");
        assert!(app.input.ends_with('T'));
        assert_eq!(app.touch, Touch::Stamp);
    }

    #[test]
    fn an_emptied_prompt_is_a_way_out_rather_than_an_error() {
        let mut app = an_app();
        app.on_key(key(KeyCode::Char('m')));
        for _ in 0.."Budget review".len() {
            app.on_key(key(KeyCode::Backspace));
        }
        assert_eq!(app.on_key(key(KeyCode::Enter)), None);
        assert_eq!(app.mode, Mode::Browse);

        // Esc is the other way out, and leaves nothing behind either.
        app.on_key(key(KeyCode::Char('#')));
        typing(&mut app, "+urgent");
        assert_eq!(app.on_key(key(KeyCode::Esc)), None);
        assert_eq!(app.mode, Mode::Browse);
        assert!(app.input.is_empty());
    }

    #[test]
    fn a_chord_does_not_type_its_own_letter_into_a_prompt() {
        let mut app = an_app();
        app.on_key(key(KeyCode::Char('a')));
        typing(&mut app, "Trip");
        // The same trap the query field has: Ctrl-D arrives as `Char('d')`.
        app.on_key(ctrl('d'));
        app.on_key(ctrl('w'));
        assert_eq!(app.input, "Trip");
    }

    #[test]
    fn a_delete_is_asked_about_first() {
        let mut app = an_app();
        assert_eq!(app.on_key(key(KeyCode::Char('d'))), None);
        assert_eq!(app.mode, Mode::Confirm(What::Delete));

        // Anything but `y` keeps the note — including the key that would have
        // deleted the next one.
        assert_eq!(app.on_key(key(KeyCode::Char('d'))), None);
        assert_eq!(app.mode, Mode::Browse);

        app.on_key(key(KeyCode::Char('d')));
        assert_eq!(
            app.on_key(key(KeyCode::Char('y'))),
            Some(Action::Remove("aaaa1111".to_string()))
        );
        assert_eq!(app.mode, Mode::Browse);
    }

    #[test]
    fn nothing_under_the_cursor_is_nothing_to_change() {
        let mut app = App::new(
            "personal".to_string(),
            PathBuf::from("/notebook"),
            a_status(),
            Vec::new(),
        );
        for pressed in ['e', 'm', '#', 'd'] {
            assert_eq!(app.on_key(key(KeyCode::Char(pressed))), None);
            assert_eq!(app.mode, Mode::Browse, "`{pressed}` opened something");
        }
        // A note can still be made in an empty notebook — that is what one is
        // for.
        app.on_key(key(KeyCode::Char('a')));
        assert_eq!(app.mode, Mode::Ask(Ask::Title));
    }

    #[test]
    fn what_a_command_said_is_shown_and_then_left_behind() {
        let mut app = an_app();
        app.report(Ok("aaaa1111  budget-review  [work, urgent]".to_string()));
        let said = app.message.as_ref().expect("the command said something");
        assert!(said.text.contains("urgent"));
        assert!(!said.failed);
        assert_eq!(app.mode, Mode::Browse, "an acknowledgement is not a card");

        app.on_key(key(KeyCode::Char('j')));
        assert!(app.message.is_none(), "the next key moves on from it");
    }

    #[test]
    fn a_command_that_refused_puts_its_reason_on_a_card() {
        let mut app = an_app();
        app.report(Err(crate::Error::msg(
            "/notebook/aaaa1111-budget-review.md: no frontmatter\nthe file was left as you saved it",
        )));
        assert_eq!(app.mode, Mode::Alert);
        let said = app.message.as_ref().expect("a reason");
        assert!(said.failed);
        // The whole of it, both lines: the second is the one that says what to
        // do about the first.
        assert!(
            said.text.contains("was left as you saved it"),
            "{}",
            said.text
        );

        // And it goes away the way the help card does.
        app.on_key(key(KeyCode::Char('x')));
        assert_eq!(app.mode, Mode::Browse);
        assert!(app.message.is_none());
    }

    #[test]
    fn a_deleted_note_leaves_the_cursor_where_it_was() {
        let mut app = an_app();
        app.on_key(key(KeyCode::Char('j')));
        assert_eq!(app.selected().map(|f| f.id.as_str()), Some("bbbb2222"));

        // What the runtime does after `d`: the notebook is read again, and the
        // note that was under the cursor is not in it.
        app.replace(
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
                    "cccc3333",
                    "reading-list",
                    "Reading list",
                    &[],
                    "a book about budgets",
                ),
            ],
        );
        assert_eq!(
            app.selected().map(|f| f.id.as_str()),
            Some("cccc3333"),
            "the row is kept, so the next note is the one under the cursor"
        );
    }

    /// Marks the notes the ids name, the way `Space` would with the cursor on
    /// each of them.
    fn mark(app: &mut App, ids: &[&str]) {
        for id in ids {
            app.select_id(id);
            app.on_key(key(KeyCode::Char(' ')));
        }
    }

    #[test]
    fn a_mark_and_a_query_do_not_touch_each_other() {
        let mut app = an_app();
        mark(&mut app, &["aaaa1111"]);
        assert!(app.marked("aaaa1111"));

        // Searching does not unmark, and the mark does not narrow the search.
        app.on_key(key(KeyCode::Char('/')));
        typing(&mut app, "tag:q3");
        app.on_key(key(KeyCode::Enter));
        assert_eq!(app.shown(), 1, "the query answers to itself alone");
        assert!(
            app.marked("aaaa1111"),
            "a note the query is hiding is still marked"
        );

        // And marking while a query is on adds to what was marked before it.
        app.on_key(key(KeyCode::Char(' ')));
        assert_eq!(app.marks.len(), 2);
    }

    #[test]
    fn the_star_takes_what_the_query_shows_and_leaves_the_rest() {
        let mut app = an_app();
        mark(&mut app, &["cccc3333"]);

        app.on_key(key(KeyCode::Char('/')));
        typing(&mut app, "tag:work");
        app.on_key(key(KeyCode::Enter));
        assert_eq!(app.shown(), 2);

        app.on_key(key(KeyCode::Char('*')));
        assert_eq!(app.marks.len(), 3, "the two shown, and the one from before");

        // Again, and the two it marked come off — the one it never touched stays.
        app.on_key(key(KeyCode::Char('*')));
        assert_eq!(app.marks.len(), 1);
        assert!(app.marked("cccc3333"));
    }

    #[test]
    fn escape_drops_the_query_first_and_the_marks_after() {
        let mut app = an_app();
        mark(&mut app, &["aaaa1111"]);
        app.on_key(key(KeyCode::Char('/')));
        typing(&mut app, "tag:q3");
        app.on_key(key(KeyCode::Enter));

        app.on_key(key(KeyCode::Esc));
        assert!(app.search.is_empty(), "the query goes first");
        assert_eq!(app.marks.len(), 1, "and the marks are still there");

        app.on_key(key(KeyCode::Esc));
        assert!(app.marks.is_empty());
    }

    #[test]
    fn with_notes_marked_a_tag_is_queued_over_all_of_them() {
        let mut app = an_app();
        mark(&mut app, &["aaaa1111", "bbbb2222"]);

        app.on_key(key(KeyCode::Char('#')));
        typing(&mut app, "-work +archive");
        // Nothing to run: the change is now waiting rather than done.
        assert_eq!(app.on_key(key(KeyCode::Enter)), None);
        assert_eq!(
            app.queue,
            vec![Step {
                keys: vec!["aaaa1111".to_string(), "bbbb2222".to_string()],
                change: Change::Tag {
                    changes: vec!["-work".to_string(), "+archive".to_string()],
                    touch: Touch::Stamp,
                },
            }]
        );
        // The marks are not spent by queueing — the next change can be aimed at
        // the same set.
        assert_eq!(app.marks.len(), 2);
    }

    #[test]
    fn with_nothing_marked_the_same_key_still_acts_at_once() {
        let mut app = an_app();
        app.on_key(key(KeyCode::Char('#')));
        typing(&mut app, "+archive");
        assert!(matches!(
            app.on_key(key(KeyCode::Enter)),
            Some(Action::Tag { .. })
        ));
        assert!(app.queue.is_empty());
    }

    #[test]
    fn a_queued_delete_asks_nothing_until_it_is_sent() {
        let mut app = an_app();
        mark(&mut app, &["aaaa1111", "cccc3333"]);

        assert_eq!(app.on_key(key(KeyCode::Char('d'))), None);
        assert_eq!(app.mode, Mode::Browse, "queueing a delete deletes nothing");
        assert_eq!(app.queued_deletions(), 2);

        // The question comes at the send, which is the last moment it can still
        // be answered no.
        app.on_key(key(KeyCode::Char('Q')));
        assert_eq!(app.on_key(key(KeyCode::Enter)), None);
        assert_eq!(app.mode, Mode::Confirm(What::Send));

        let sent = app.on_key(key(KeyCode::Char('y')));
        assert!(matches!(sent, Some(Action::Send(steps)) if steps.len() == 1));
    }

    #[test]
    fn a_queue_of_tags_goes_without_being_asked_about() {
        let mut app = an_app();
        mark(&mut app, &["aaaa1111"]);
        app.on_key(key(KeyCode::Char('#')));
        typing(&mut app, "+archive");
        app.on_key(key(KeyCode::Enter));

        app.on_key(key(KeyCode::Char('Q')));
        // A tag can be put back; a confirmation nobody needs is one nobody reads.
        assert!(matches!(
            app.on_key(key(KeyCode::Enter)),
            Some(Action::Send(_))
        ));
    }

    #[test]
    fn an_entry_can_be_dropped_from_the_queue() {
        let mut app = an_app();
        mark(&mut app, &["aaaa1111"]);
        for tag in ["+one", "+two", "+three"] {
            app.on_key(key(KeyCode::Char('#')));
            typing(&mut app, tag);
            app.on_key(key(KeyCode::Enter));
        }
        assert_eq!(app.queue.len(), 3);

        app.on_key(key(KeyCode::Char('Q')));
        app.on_key(key(KeyCode::Char('j')));
        app.on_key(key(KeyCode::Char('d')));
        assert_eq!(app.queue.len(), 2);
        assert!(
            app.queue
                .iter()
                .all(|step| step.describe() != "tag: +two (1 note)")
        );
        assert_eq!(app.mode, Mode::Queue, "dropping one is not leaving");

        app.on_key(key(KeyCode::Esc));
        assert_eq!(app.mode, Mode::Browse);
    }

    #[test]
    fn a_tag_that_cannot_be_written_down_never_reaches_the_queue() {
        let mut app = an_app();
        mark(&mut app, &["aaaa1111"]);
        app.on_key(key(KeyCode::Char('#')));
        // Refused where it was typed rather than at the end of a sitting.
        typing(&mut app, "+q3,urgent");
        assert_eq!(app.on_key(key(KeyCode::Enter)), None);
        assert!(app.queue.is_empty());
        assert_eq!(app.mode, Mode::Alert);
        assert!(
            app.message
                .as_ref()
                .is_some_and(|said| said.text.contains("cannot contain"))
        );
    }

    #[test]
    fn sending_spends_the_queue_and_the_marks() {
        let mut app = an_app();
        mark(&mut app, &["aaaa1111"]);
        app.on_key(key(KeyCode::Char('#')));
        typing(&mut app, "+archive");
        app.on_key(key(KeyCode::Enter));

        app.sent();
        assert!(app.queue.is_empty());
        assert!(app.marks.is_empty());
    }

    #[test]
    fn a_mark_on_a_note_that_has_gone_goes_with_it() {
        let mut app = an_app();
        mark(&mut app, &["aaaa1111", "bbbb2222"]);
        app.replace(
            a_status(),
            vec![a_note(
                "bbbb2222",
                "meeting-notes",
                "Meeting notes",
                &["work"],
                "agenda",
            )],
        );
        assert_eq!(app.marks.len(), 1);
        assert!(app.marked("bbbb2222"));
    }

    #[test]
    fn a_note_just_made_can_be_found_by_id() {
        let mut app = an_app();
        assert!(!app.ids().any(|id| id == "dddd4444"));

        // Made, and by slug it lands nowhere near the cursor.
        let notes = vec![
            a_note(
                "aaaa1111",
                "budget-review",
                "Budget review",
                &["work"],
                "the q3 budget",
            ),
            a_note("bbbb2222", "meeting-notes", "Meeting notes", &["work"], "x"),
            a_note("dddd4444", "trip-plan", "Trip plan", &[], "flights"),
        ];
        app.replace(a_status(), notes);
        assert_eq!(app.selected().map(|f| f.id.as_str()), Some("aaaa1111"));
        app.select_id("dddd4444");
        assert_eq!(app.selected().map(|f| f.id.as_str()), Some("dddd4444"));
    }
}
