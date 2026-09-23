//! What a browsing session holds, and how one keystroke changes it.
//!
//! Nothing here opens a file, a repository or a terminal: a key goes in, the
//! state moves, and anything needing the outside world comes back as an
//! [`Action`], so the whole interaction is testable with no terminal.
//!
//! Keys that change a note ask a `cmd` command to do it and show the line it
//! would have printed; what a change means is written once, in `cmd`.
//!
//! A session is a **stack of screens**, the notes at the bottom and never
//! popped. Each keeps its own cursor, query and scroll, so going back lands
//! where you left.
//!
//! The notes are held in memory for the session, paying once the body reads
//! `noda search` pays per query.

use std::collections::BTreeSet;
use std::path::PathBuf;

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::widgets::TableState;

use super::command;
use super::field::{Edit, Field};
use crate::Result;
use crate::cmd::{self, Change, Sort, Step, Touch};
use crate::note;
use crate::notebook::{self, BlameLine, Deleted, Entry, NoteFile, Status};
use crate::query::{self, Query};
use crate::todo;

/// Something the runtime has to do that the state cannot do for itself.
///
/// Changes name a command and its arguments, never an edit, since what a change
/// means lives in `cmd`. A note is named by id rather than by row: the command
/// reopens the notebook, and by then the listing may be stale.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Quit,
    /// Re-read the notebook: the answer to another process writing, rather
    /// than a file watcher.
    Reload,
    /// The one action needing the terminal handed back, to `$EDITOR`.
    Edit {
        key: String,
        touch: Touch,
    },
    /// `None` leaves the title to the body, as `add` does. No `touch`: `add`
    /// has none.
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
    /// `noda pin` / `noda unpin`, one variant because the key is a toggle.
    Pin {
        key: String,
        pinned: bool,
        touch: Touch,
    },
    /// `noda rm`, once confirmed.
    Remove(String),
    /// Every change in the queue, in one commit via `cmd::bulk` — the same code
    /// as the keys above with the commit boundary one level out.
    Send(Vec<Step>),
    /// `noda restore`: a new commit, so nothing is rewritten and nothing asked.
    Restore {
        key: String,
        rev: String,
        touch: Touch,
    },
    /// Open a note the prompt named, on a screen of its own. The runtime
    /// resolves the key through `Notebook::resolve`, so there is one answer to
    /// what a prefix or slug names.
    Open(String),
    /// A screen *about* a note named by key, resolved by the runtime likewise.
    Show {
        key: String,
        look: Look,
    },
    /// Not a `Run`: the runtime builds a new session for the other notebook.
    Use(String),
    /// A command that reads or changes the notebook and answers with a line.
    Run(Run),
}

/// How many tags get a digit key: `1`–`9`, `0` being the way out. The long
/// tail is what `/` is for.
pub const SCOPE_KEYS: usize = 9;

/// A screen about one note, named before the note is known.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Look {
    Log,
    Blame,
    Backlinks,
}

/// One variant per command rather than a closure, so what the browser can ask
/// for is a readable list and only the runtime knows how a call is made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Run {
    Status,
    /// Reporting only: a keystroke should not rewrite a directory.
    Doctor {
        links: bool,
        times: bool,
    },
    Readme,
    /// A name marks the notebook; no name lists what has been marked.
    Snapshot(Option<String>),
    Sync,
    Push,
    Pull,
}

impl Action {
    /// A label for a slow command, drawn before it runs; otherwise the last
    /// frame stays up with no sign of anything happening.
    pub fn working(&self) -> Option<&'static str> {
        match self {
            Action::Run(Run::Sync) => Some("syncing…"),
            Action::Run(Run::Push) => Some("pushing…"),
            Action::Run(Run::Pull) => Some("pulling…"),
            _ => None,
        }
    }
}

/// A command's answer in its own words, not summarised by the browser.
pub struct Message {
    pub text: String,
    /// A card for failures, the status line for successes: a reason has to be
    /// read, an acknowledgement only glanced at.
    pub failed: bool,
}

impl Message {
    /// The first line, for the status bar.
    pub fn line(&self) -> &str {
        self.text.lines().next().unwrap_or_default()
    }
}

/// What a backlinks screen is of: a note or a file, one walk either way, as
/// `noda backlinks` takes either.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Subject {
    Note(String),
    File(String),
}

impl Subject {
    pub fn name(&self) -> &str {
        match self {
            Subject::Note(name) | Subject::File(name) => name,
        }
    }
}

/// A screen, named in the crumb trail by a note's id or by the subcommand that
/// prints the same thing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum View {
    Notes,
    /// By id rather than row, so it survives the listing being filtered,
    /// re-sorted or reloaded.
    Note(String),
    /// Every unticked box in the notebook, soonest due first.
    Todo,
    /// Every tag, and how many notes carry it.
    Tags,
    /// What the notebook holds that is not a note.
    Files,
    /// Every notebook there is, and which one this is.
    Notebooks,
    /// The notes history holds that the notebook no longer does.
    Deleted,
    /// What is uncommitted, or what the last commit did.
    Diff,
    /// Commits, newest first: one note's, or the whole notebook's.
    Log(Option<String>),
    /// What links to a note or to a file.
    Backlinks(Subject),
    /// Which commit put each line of a note where it is.
    Blame(String),
}

impl View {
    /// What the crumb trail calls it. Which note a screen is about is on the
    /// title band instead, or a deep stack's trail outgrows the screen.
    pub fn crumb(&self) -> &str {
        match self {
            View::Notes => "notes",
            View::Note(id) => id,
            View::Todo => "todo",
            View::Tags => "tags",
            View::Files => "files",
            View::Notebooks => "notebooks",
            View::Deleted => "deleted",
            View::Diff => "diff",
            View::Log(_) => "log",
            View::Backlinks(_) => "backlinks",
            View::Blame(_) => "blame",
        }
    }
}

/// What the top screen shows, once worked out.
///
/// Only one is kept: a cache of a notebook another process writes to goes
/// stale quietly. Some the session derives itself (`App::derive`); some need
/// a repository only the runtime may open ([`App::wanted`]).
pub enum Content {
    /// As it is on disk, not rendered back from the parse.
    Note(String),
    Todo(Vec<Task>),
    Tags(Vec<Tally>),
    /// Indices into the session's notes: the ones that link to the subject.
    Backlinks(Vec<usize>),
    /// The history, and which commits the remote has not seen (empty with no
    /// upstream, as `noda log`). Apart from `Entry`, which is what a commit is
    /// rather than what a remote knows of it.
    Log(Vec<Entry>, std::collections::HashSet<git2::Oid>),
    Blame(Vec<BlameLine>),
    Deleted(Vec<Deleted>),
    /// Uncoloured; the drawing colours it.
    Diff(String),
}

/// One unticked box, and the note carrying it.
pub struct Task {
    /// An index, the list being rebuilt whenever the notes are.
    pub note: usize,
    pub item: todo::Item,
}

/// One tag, and how many notes carry it.
pub struct Tally {
    pub tag: String,
    pub notes: usize,
}

/// Three states rather than a tick: on a mixed set, leaving a tag alone differs
/// from giving it to all or taking it from all. `cmd::tag`'s own three.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    /// Each note keeps what it has.
    Leave,
    Add,
    Remove,
}

/// A tag, how it stands with the notes aimed at, and what is to be done.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    pub tag: String,
    /// How many of those notes already carry it.
    pub held: usize,
    /// How many notes in the notebook carry it, as on the tags screen.
    pub notes: usize,
    pub mark: Mark,
}

impl Choice {
    /// The states that would change something: a tag every note carries can
    /// only be removed and one none carries only added.
    fn states(&self, total: usize) -> Vec<Mark> {
        let mut out = vec![Mark::Leave];
        if self.held < total {
            out.push(Mark::Add);
        }
        if self.held > 0 {
            out.push(Mark::Remove);
        }
        out
    }

    /// The next of them, for `Tab`.
    fn next(&self, total: usize) -> Mark {
        let states = self.states(total);
        let at = states.iter().position(|state| *state == self.mark);
        states[at.map_or(0, |at| (at + 1) % states.len())]
    }

    /// `[x]` says what is true, `[+]`/`[-]` what is being done. ASCII because
    /// `☑` is ambiguous-width and can push the row's end off screen.
    pub fn tick(&self, total: usize) -> &'static str {
        match self.mark {
            Mark::Add => "[+]",
            Mark::Remove => "[-]",
            Mark::Leave if total > 0 && self.held == total => "[x]",
            Mark::Leave => "[ ]",
        }
    }
}

/// The row for typed text that is not an existing tag. A new tag can be a
/// typo, so it takes its own keystroke and shows its near neighbour.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Proposal {
    /// A tag that could be made, and an existing one a typo away.
    New {
        tag: String,
        near: Option<(String, usize)>,
    },
    /// One that could not, in `note::validate_tag`'s own words.
    Refused(String),
}

/// Whether two tags are one keystroke apart: equal ignoring case, or one
/// character added, dropped, mistyped or swapped with its neighbour. Not plain
/// edit distance, which counts a transposition, the commonest typo, as two.
fn one_edit_apart(left: &str, right: &str) -> bool {
    let left: Vec<char> = left.to_lowercase().chars().collect();
    let right: Vec<char> = right.to_lowercase().chars().collect();
    if left.len().abs_diff(right.len()) > 1 {
        return false;
    }
    // Strip the common prefix and suffix; what is left is the difference.
    let head = left.iter().zip(&right).take_while(|(a, b)| a == b).count();
    let tail = left[head..]
        .iter()
        .rev()
        .zip(right[head..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    match (left.len() - head - tail, right.len() - head - tail) {
        // Case only, one added or dropped, or one mistyped.
        (0..=1, 0..=1) => true,
        // Transposed.
        (2, 2) => left[head] == right[head + 1] && left[head + 1] == right[head],
        _ => false,
    }
}

/// Something a screen needs that only the runtime can fetch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Need {
    Note { id: String, path: PathBuf },
    Log(Option<String>),
    Blame { id: String, slug: String },
    Deleted,
    Diff,
}

/// What a screen is of, and everything about it a keystroke can move — per
/// screen, so going back lands where you left.
pub struct Screen {
    pub view: View,
    /// A listing's cursor and scroll offset.
    pub table: TableState,
    /// How far a screen of text has been scrolled.
    pub scroll: u16,
    /// Split as a shell would, this is what `noda search` takes.
    pub search: Field,
    /// Words to highlight in a title and body; not `tag:` or `id:`, which match
    /// nothing in the prose.
    pub terms: Vec<String>,
    /// Why the query as typed is not a query yet.
    pub error: Option<String>,
    /// Indices into the session's notes that this screen's query admits, in the
    /// same order. Empty on a screen that is not a listing.
    visible: Vec<usize>,
}

impl Screen {
    fn new(view: View, terms: Vec<String>) -> Screen {
        Screen {
            view,
            table: TableState::new(),
            scroll: 0,
            search: Field::default(),
            terms,
            error: None,
            visible: Vec::new(),
        }
    }
}

/// What the keyboard currently means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Browse,
    /// The list narrows on every keystroke; `Enter` only returns to the list.
    Search,
    Help,
    /// Typing the one thing a change needs said in words.
    Ask(Ask),
    /// Typed on the query's line; only one can be open at a time.
    Command,
    /// The command palette.
    Commands,
    /// Asked on screen: in raw mode, a command reading stdin would steal keys.
    Confirm(What),
    /// Reading the queue and dropping from it.
    Queue,
    /// The tag picker: a card rather than a screen, not being a place.
    Tagging,
    /// A refusal, or an answer longer than a line. Dismissed by anything.
    Alert,
}

/// What a `y` would agree to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum What {
    /// Delete the note under the cursor, now.
    Delete,
    /// Asked at the send, not the queueing: a queued delete can still be dropped.
    Send,
    /// Quit with a non-empty queue, which is written down nowhere else.
    Quit,
}

/// The one thing a change needs said in words before it can be asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ask {
    /// A title for a new note. `$EDITOR` opens on the body once it is given.
    Title,
    /// A new title for the note under the cursor.
    Retitle,
}

impl Ask {
    /// What the status line calls the field.
    pub fn prompt(self) -> &'static str {
        match self {
            Ask::Title => "new note",
            Ask::Retitle => "retitle",
        }
    }

    /// What the prompt does not say, shown at the end of the line.
    pub fn hint(self) -> &'static str {
        match self {
            Ask::Title => "Enter alone takes the title from the body",
            Ask::Retitle => "",
        }
    }
}

/// One read of the notebook, replaced whole on reload so no part describes an
/// older notebook than the rest.
pub struct Session {
    pub status: Status,
    pub notes: Vec<NoteFile>,
    /// What the notebook holds that is not a note, by name.
    pub files: Vec<String>,
    pub notebooks: Vec<String>,
    /// The local date, not UTC: east of Greenwich UTC is still yesterday in
    /// the morning, when a todo list is read.
    pub today: String,
}

pub struct App {
    /// The active notebook's name, for the header.
    pub notebook: String,
    /// The notebook's directory, for turning a note into a path.
    pub root: PathBuf,
    /// As of the last load, with no network: the drift as `noda status` has it.
    pub status: Status,
    /// In the walk's order — by slug, as `noda ls` shows without `--sort`.
    notes: Vec<NoteFile>,
    /// Read with the notes, rather than opening a repository when asked.
    files: Vec<String>,
    notebooks: Vec<String>,
    /// Once per read rather than per frame.
    today: String,
    /// Oldest first, never empty.
    stack: Vec<Screen>,
    pub mode: Mode,
    /// The prompt's field; [`Mode::Ask`] says which prompt.
    pub input: Field,
    /// What the last command that changed something had to say.
    pub message: Option<Message>,
    /// The notes picked out to be changed together, by id. Independent of the
    /// query, so marking some then searching for more keeps the first lot.
    /// Ordered, so a commit message does not depend on hash order.
    pub marks: BTreeSet<String>,
    /// Handed to `cmd::bulk` unaltered, so nothing translates a change.
    pub queue: Vec<Step>,
    /// Where the cursor is in the queue view.
    queue_at: usize,
    /// The picker's rows: every tag in the tags screen's order, then anything
    /// typed. Built when the picker opens, dropped when it closes.
    choices: Vec<Choice>,
    /// The notes the picker was opened on, fixed though the cursor moves.
    aimed_at: Vec<String>,
    /// Over the rows on screen, not the choices, so it holds while narrowed.
    tags_at: usize,
    /// Session-long, as a single key has no room for a flag; shown in the
    /// header so it is not forgotten.
    pub touch: Touch,
    /// `--sort` and `-r`, session-long for `touch`'s reason; applied by
    /// `cmd::sort_notes`.
    pub sort: Sort,
    pub reverse: bool,
    /// `ls -l`'s columns, in the same places.
    pub long: bool,
    /// Whether the crumb trail takes a row.
    pub crumbs_shown: bool,
    /// Kept with its view so a screen never draws another screen's content.
    loaded: Option<(View, Content)>,
    /// The prompt's history, never written to disk: a file of everything typed
    /// into a notebook is a privacy question, not a convenience.
    history: Vec<String>,
    /// `None` is the line being typed, where walking forward past the end lands.
    history_at: Option<usize>,
    /// Where the cursor is in the list of commands.
    commands_at: usize,
    /// What is being waited for, while it is being waited for.
    pub working: Option<&'static str>,
    /// Written back by the drawing, which alone knows the screen's height.
    page: u16,
}

impl App {
    pub fn new(notebook: String, root: PathBuf, session: Session) -> App {
        let mut listing = Screen::new(View::Notes, Vec::new());
        listing.visible = (0..session.notes.len()).collect();
        let mut app = App {
            notebook,
            root,
            status: session.status,
            notes: session.notes,
            files: session.files,
            notebooks: session.notebooks,
            today: session.today,
            stack: vec![listing],
            mode: Mode::Browse,
            input: Field::default(),
            message: None,
            marks: BTreeSet::new(),
            queue: Vec::new(),
            queue_at: 0,
            choices: Vec::new(),
            aimed_at: Vec::new(),
            tags_at: 0,
            touch: Touch::Stamp,
            sort: Sort::default(),
            reverse: false,
            long: false,
            crumbs_shown: true,
            loaded: None,
            history: Vec::new(),
            history_at: None,
            commands_at: 0,
            working: None,
            page: 10,
        };
        app.select(0);
        app
    }

    /// The screen the keyboard is on.
    fn top(&self) -> &Screen {
        self.stack.last().expect("a session is never on no screen")
    }

    fn top_mut(&mut self) -> &mut Screen {
        self.stack
            .last_mut()
            .expect("a session is never on no screen")
    }

    /// The bottom screen, never popped.
    fn listing(&self) -> &Screen {
        self.stack.first().expect("a session is never on no screen")
    }

    fn listing_mut(&mut self) -> &mut Screen {
        self.stack
            .first_mut()
            .expect("a session is never on no screen")
    }

    pub fn view(&self) -> &View {
        &self.top().view
    }

    /// The trail from the listing to where the keyboard is, outermost first.
    pub fn crumbs(&self) -> impl Iterator<Item = &str> {
        self.stack.iter().map(|screen| screen.view.crumb())
    }

    /// How deep the stack is. One is the listing alone.
    pub fn depth(&self) -> usize {
        self.stack.len()
    }

    /// The query the listing is narrowed by, as typed.
    pub fn search(&self) -> &str {
        self.listing().search.text()
    }

    /// The query up to the cursor, for placing the terminal's cursor.
    pub fn search_before(&self) -> &str {
        self.listing().search.before()
    }

    /// Why the query as typed is not a query yet.
    pub fn error(&self) -> Option<&str> {
        self.listing().error.as_deref()
    }

    /// Inherited by a screen opened from a query, so a hit stays highlighted in
    /// the note it was found in.
    pub fn terms(&self) -> &[String] {
        &self.top().terms
    }

    /// How far the note on screen has been scrolled.
    pub fn scroll(&self) -> u16 {
        self.top().scroll
    }

    fn open(&mut self, view: View) {
        let terms = self.top().terms.clone();
        self.stack.push(Screen::new(view, terms));
        self.settle();
    }

    /// `false` when there was nothing to close, so `Esc` can mean what it means
    /// on the listing.
    fn back(&mut self) -> bool {
        if self.stack.len() > 1 {
            self.stack.pop();
            self.settle();
            true
        } else {
            false
        }
    }

    /// Works out what the top screen shows, where the session can; called
    /// whenever the top screen changes or the notebook is re-read. The rest ask
    /// through [`App::wanted`] and wait a frame.
    fn settle(&mut self) {
        let view = self.top().view.clone();
        if self.content().is_none()
            && let Some(content) = self.derive(&view)
        {
            self.loaded = Some((view, content));
        }
        // Kept, not reset, since this runs on the way back too; clamped for a
        // list gone shorter.
        let at = self.top().table.selected().unwrap_or(0);
        self.cursor_to(at);
    }

    /// What a screen shows, when the session already holds the answer. These
    /// parse every body, so they run when a screen opens, not per keystroke.
    fn derive(&self, view: &View) -> Option<Content> {
        match view {
            View::Todo => {
                let mut tasks: Vec<Task> = self
                    .notes
                    .iter()
                    .enumerate()
                    .flat_map(|(at, file)| {
                        todo::items(&file.note.body)
                            .into_iter()
                            .map(move |item| Task { note: at, item })
                    })
                    .collect();
                // `todo::order`, so this matches `noda todo`.
                tasks.sort_by(|left, right| {
                    todo::order(
                        (self.notes[left.note].slug.as_str(), &left.item),
                        (self.notes[right.note].slug.as_str(), &right.item),
                    )
                });
                Some(Content::Todo(tasks))
            }
            // Counted and ordered by `notebook`, so this matches the web's tags.
            View::Tags => Some(Content::Tags(
                notebook::tag_tally(&self.notes)
                    .into_iter()
                    .map(|(tag, notes)| Tally { tag, notes })
                    .collect(),
            )),
            // `notebook` decides what counts as a link.
            View::Backlinks(subject) => {
                let found = self
                    .notes
                    .iter()
                    .enumerate()
                    .filter(|(_, file)| match subject {
                        Subject::Note(id) => notebook::links_to_note(&file.note, id),
                        Subject::File(name) => notebook::links_to_file(&file.note, name),
                    })
                    .map(|(at, _)| at)
                    .collect();
                Some(Content::Backlinks(found))
            }
            _ => None,
        }
    }

    /// What the top screen shows, when what is loaded was loaded for it.
    fn content(&self) -> Option<&Content> {
        match &self.loaded {
            Some((view, content)) if view == &self.top().view => Some(content),
            _ => None,
        }
    }

    /// What the top screen needs from the runtime; `None` once supplied, so a
    /// repository is not opened per frame.
    pub fn wanted(&self) -> Option<Need> {
        if self.content().is_some() {
            return None;
        }
        match &self.top().view {
            View::Note(id) => {
                let file = self.note_of(id)?;
                Some(Need::Note {
                    id: file.id.clone(),
                    path: self.root.join(note::file_name(&file.id, &file.slug)),
                })
            }
            View::Blame(id) => {
                let file = self.note_of(id)?;
                Some(Need::Blame {
                    id: file.id.clone(),
                    slug: file.slug.clone(),
                })
            }
            View::Log(id) => Some(Need::Log(id.clone())),
            View::Deleted => Some(Need::Deleted),
            View::Diff => Some(Need::Diff),
            _ => None,
        }
    }

    /// Dropped if its screen is no longer on top: a long blame can be escaped
    /// from while it runs.
    pub fn supply(&mut self, view: &View, content: Content) {
        if self.top().view != *view {
            return;
        }
        self.loaded = Some((view.clone(), content));
        self.top_mut().scroll = 0;
        let at = self.top().table.selected().unwrap_or(0);
        self.cursor_to(at);
    }

    /// Closes a screen the runtime could not fill, so the error card is not
    /// drawn over an empty screen.
    pub fn give_up(&mut self) {
        self.back();
    }

    pub fn note_of(&self, id: &str) -> Option<&NoteFile> {
        self.notes.iter().find(|file| file.id == id)
    }

    /// Swaps in a fresh read, keeping the query, and the cursor on its note if
    /// that is still there or else on its row (as after a delete).
    pub fn replace(&mut self, session: Session) {
        let was = self.at_cursor().map(|file| file.id.clone());
        let row = self.listing().table.selected();
        self.status = session.status;
        self.notes = session.notes;
        self.files = session.files;
        self.notebooks = session.notebooks;
        self.today = session.today;
        // A read comes back in walk order; sorted before the query picks out
        // indices into it.
        self.arrange();
        self.refilter();
        match was.and_then(|id| {
            self.listing()
                .visible
                .iter()
                .position(|&i| self.notes[i].id == id)
        }) {
            Some(at) => self.select(at),
            None => self.select(row.unwrap_or(0)),
        }
        self.loaded = None;
        // A screen about a note or file that has gone comes off the stack.
        let mut stack = std::mem::take(&mut self.stack);
        stack.retain(|screen| match &screen.view {
            View::Note(id)
            | View::Blame(id)
            | View::Log(Some(id))
            | View::Backlinks(Subject::Note(id)) => self.notes.iter().any(|file| &file.id == id),
            View::Backlinks(Subject::File(name)) => self.files.contains(name),
            _ => true,
        });
        self.stack = stack;
        // Marks on gone notes are dropped. The queue keeps its ids: `bulk`
        // reports one that has since gone.
        let ids: BTreeSet<&str> = self.notes.iter().map(|file| file.id.as_str()).collect();
        self.marks.retain(|id| ids.contains(id.as_str()));
        self.settle();
    }

    pub fn marked(&self, id: &str) -> bool {
        self.marks.contains(id)
    }

    /// In id order; ids being what a command takes and what survives a rename.
    fn marked_keys(&self) -> Vec<String> {
        self.marks.iter().cloned().collect()
    }

    /// The note the listing's cursor is on, whatever screen is on top of it.
    fn at_cursor(&self) -> Option<&NoteFile> {
        let listing = self.listing();
        let at = listing.table.selected()?;
        self.notes.get(*listing.visible.get(at)?)
    }

    /// The note the top screen is about, which lets `e`, `m`, `#` and `Ctrl-d`
    /// mean the same everywhere: the row on a screen of notes, the note on a
    /// screen about one, and nothing on a screen about the notebook.
    pub fn selected(&self) -> Option<&NoteFile> {
        let at = || self.top().table.selected();
        match &self.top().view {
            View::Notes => self.at_cursor(),
            View::Note(id) | View::Blame(id) | View::Log(Some(id)) => self.note_of(id),
            View::Todo => {
                let task = self.tasks().get(at()?)?;
                self.notes.get(task.note)
            }
            View::Backlinks(_) => self.notes.get(*self.linking().get(at()?)?),
            View::Log(None)
            | View::Tags
            | View::Files
            | View::Notebooks
            | View::Deleted
            | View::Diff => None,
        }
    }

    /// The loaded content of the top screen, empty when it shows something else.
    pub fn tasks(&self) -> &[Task] {
        match self.content() {
            Some(Content::Todo(tasks)) => tasks,
            _ => &[],
        }
    }

    pub fn tallies(&self) -> &[Tally] {
        match self.content() {
            Some(Content::Tags(tallies)) => tallies,
            _ => &[],
        }
    }

    pub fn linking(&self) -> &[usize] {
        match self.content() {
            Some(Content::Backlinks(found)) => found,
            _ => &[],
        }
    }

    pub fn entries(&self) -> &[Entry] {
        match self.content() {
            Some(Content::Log(entries, _)) => entries,
            _ => &[],
        }
    }

    /// False on every other screen and with no remote, leaving the margin blank.
    pub fn is_unpushed(&self, commit: git2::Oid) -> bool {
        match self.content() {
            Some(Content::Log(_, unpushed)) => unpushed.contains(&commit),
            _ => false,
        }
    }

    pub fn gone(&self) -> &[Deleted] {
        match self.content() {
            Some(Content::Deleted(gone)) => gone,
            _ => &[],
        }
    }

    pub fn blamed(&self) -> &[BlameLine] {
        match self.content() {
            Some(Content::Blame(lines)) => lines,
            _ => &[],
        }
    }

    /// The two screens that are a block of text rather than a list.
    pub fn text(&self) -> Option<&str> {
        match self.content() {
            Some(Content::Note(text) | Content::Diff(text)) => Some(text),
            _ => None,
        }
    }

    pub fn note_at(&self, at: usize) -> Option<&NoteFile> {
        self.notes.get(at)
    }

    pub fn files(&self) -> &[String] {
        &self.files
    }

    pub fn notebooks(&self) -> &[String] {
        &self.notebooks
    }

    /// Today, for deciding which due dates have been missed.
    pub fn today(&self) -> &str {
        &self.today
    }

    /// `None` when the screen is a block of text the keys scroll. The one place
    /// that says which kind a screen is, so every key agrees.
    fn rows_here(&self) -> Option<usize> {
        match &self.top().view {
            View::Notes => Some(self.top().visible.len()),
            View::Todo => Some(self.tasks().len()),
            View::Tags => Some(self.tallies().len()),
            View::Files => Some(self.files.len()),
            View::Notebooks => Some(self.notebooks.len()),
            View::Deleted => Some(self.gone().len()),
            View::Backlinks(_) => Some(self.linking().len()),
            View::Log(_) => Some(self.entries().len()),
            View::Note(_) | View::Blame(_) | View::Diff => None,
        }
    }

    pub fn has_rows(&self) -> bool {
        self.rows_here().is_some()
    }

    /// Which row the cursor is on, for the screens that draw one.
    pub fn row(&self) -> Option<usize> {
        self.top().table.selected()
    }

    /// The notes the listing's query admits, in listing order.
    pub fn rows(&self) -> impl Iterator<Item = &NoteFile> {
        self.listing()
            .visible
            .iter()
            .filter_map(|&i| self.notes.get(i))
    }

    pub fn shown(&self) -> usize {
        self.listing().visible.len()
    }

    pub fn total(&self) -> usize {
        self.notes.len()
    }

    /// Taken out so the rows may borrow the notes while ratatui writes the
    /// offset; the borrow checker sees one `self`.
    pub fn take_table(&mut self) -> TableState {
        std::mem::take(&mut self.top_mut().table)
    }

    pub fn put_table(&mut self, state: TableState) {
        self.top_mut().table = state;
    }

    pub fn set_page(&mut self, rows: u16) {
        self.page = rows.max(1);
    }

    /// Applies a keystroke. `None` means the state moved and nothing else.
    pub fn on_key(&mut self, key: KeyEvent) -> Option<Action> {
        // Windows sends a press and a release; both would move twice.
        if key.kind != KeyEventKind::Press {
            return None;
        }
        // Quits from anywhere without asking about the queue, as the terminal
        // would; `q` is the key that asks.
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            return Some(Action::Quit);
        }
        match self.mode {
            // Any key dismisses either card.
            Mode::Help | Mode::Alert => {
                self.mode = Mode::Browse;
                self.message = None;
                None
            }
            Mode::Search => self.searching(key),
            Mode::Ask(what) => self.asking(what, key),
            Mode::Command => self.commanding(key),
            Mode::Commands => self.listing_commands(key),
            Mode::Confirm(what) => self.confirming(what, key),
            Mode::Queue => self.queueing(key),
            Mode::Tagging => self.picking(key),
            Mode::Browse => self.browsing(key),
        }
    }

    fn browsing(&mut self, key: KeyEvent) -> Option<Action> {
        self.message = None;
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            return self.chord(key.code);
        }
        // Keys that mean the same on every screen, answered first.
        match key.code {
            KeyCode::Char('q') => return self.leaving(),
            KeyCode::Char('r') => return Some(Action::Reload),
            KeyCode::Char('?') => {
                self.mode = Mode::Help;
                return None;
            }
            // Subcommands without a key of their own are typed.
            KeyCode::Char(':') => {
                self.mode = Mode::Command;
                self.input.clear();
                self.history_at = None;
                return None;
            }
            KeyCode::Char('T') => {
                self.touch = match self.touch {
                    Touch::Stamp => Touch::Keep,
                    Touch::Keep => Touch::Stamp,
                };
                return None;
            }
            // Each aims at whatever the screen is about.
            KeyCode::Char('e') => {
                return Some(Action::Edit {
                    key: self.selected()?.id.clone(),
                    touch: self.touch,
                });
            }
            KeyCode::Char('a') => {
                self.ask(Ask::Title, String::new());
                return None;
            }
            KeyCode::Char('m') => {
                let title = self.selected()?.note.title.clone();
                self.ask(Ask::Retitle, title);
                return None;
            }
            // A toggle: the row shows which way it will go.
            KeyCode::Char('p') => {
                let Some(file) = self.selected() else {
                    self.refuse("no note on screen — open one first".to_string());
                    return None;
                };
                return Some(Action::Pin {
                    key: file.id.clone(),
                    pinned: !file.note.is_pinned(),
                    touch: self.touch,
                });
            }
            KeyCode::Char('#') => {
                let keys = if self.marks.is_empty() {
                    vec![self.selected()?.id.clone()]
                } else {
                    self.marked_keys()
                };
                self.pick(keys);
                return None;
            }
            KeyCode::Char('Q') => {
                self.mode = Mode::Queue;
                self.queue_at = self.queue_at.min(self.queue.len().saturating_sub(1));
                return None;
            }
            // Screens about the note in front of you, plus the todo list; the
            // other screens are typed.
            KeyCode::Char('t') => {
                self.open(View::Todo);
                return None;
            }
            // The notebook's or one note's, by what the screen is about, as `:log`.
            KeyCode::Char('l') => {
                let about = self.about();
                self.open(View::Log(about));
                return None;
            }
            KeyCode::Char('b') => {
                let subject = self.linkable()?;
                self.open(View::Backlinks(subject));
                return None;
            }
            KeyCode::Char('B') => {
                let id = self.aimed(None)?;
                self.open(View::Blame(id));
                return None;
            }
            // The nine commonest tags, as numbered on the tags screen, `0` the
            // way out. From any screen: it returns to the listing.
            KeyCode::Char(digit @ '0'..='9') => {
                self.scope_nth(digit as usize - '0' as usize);
                return None;
            }
            // `d` used to delete; it points at the new key rather than quietly
            // meaning something else.
            KeyCode::Char('d') => {
                self.message = Some(Message {
                    text: "delete is Ctrl-d".to_string(),
                    failed: false,
                });
                return None;
            }
            _ => {}
        }
        // A list is walked and a page scrolled.
        match (&self.top().view, self.has_rows()) {
            (View::Notes, _) => self.on_listing(key),
            (_, true) => self.on_rows(key),
            (_, false) => self.on_reading(key),
        }
    }

    /// Ctrl chords. Delete is one, being the key that cannot be taken back.
    fn chord(&mut self, code: KeyCode) -> Option<Action> {
        let half = i32::from(self.page.max(2) / 2);
        match code {
            KeyCode::Char('f') => self.step(half),
            KeyCode::Char('b') => self.step(-half),
            KeyCode::Char('d') => return self.delete(),
            KeyCode::Char('a') => {
                self.mode = Mode::Commands;
                self.input.clear();
                self.commands_at = 0;
            }
            // Only the listing has a long form.
            KeyCode::Char('w') => {
                if matches!(self.top().view, View::Notes) {
                    self.long = !self.long;
                }
            }
            KeyCode::Char('g') => self.crumbs_shown = !self.crumbs_shown,
            _ => {}
        }
        None
    }

    /// The marked notes, or the one on the screen.
    fn delete(&mut self) -> Option<Action> {
        if self.marks.is_empty() {
            self.selected()?;
            self.mode = Mode::Confirm(What::Delete);
        } else {
            // Queued unasked; the confirmation comes at the send.
            let keys = self.marked_keys();
            self.enqueue(Step {
                keys,
                change: Change::Remove,
            });
        }
        None
    }

    /// The listing: moving through it, narrowing it, marking it, and opening
    /// what the cursor is on.
    fn on_listing(&mut self, key: KeyEvent) -> Option<Action> {
        let page = i32::from(self.page);
        match key.code {
            KeyCode::Char('/') => self.mode = Mode::Search,
            KeyCode::Char('j') | KeyCode::Down => self.step(1),
            KeyCode::Char('k') | KeyCode::Up => self.step(-1),
            KeyCode::PageDown => self.step(page),
            KeyCode::PageUp => self.step(-page),
            KeyCode::Char('g') | KeyCode::Home => self.jump(Edge::First),
            KeyCode::Char('G') | KeyCode::End => self.jump(Edge::Last),
            // `--sort` and `-r`; shifted, being about the listing and `r` taken.
            KeyCode::Char('S') => {
                self.sort = self.sort.next();
                self.reorder();
            }
            KeyCode::Char('R') => {
                self.reverse = !self.reverse;
                self.reorder();
            }
            // A screen of its own, not a split, so it is read at full width.
            KeyCode::Enter => {
                let id = self.selected()?.id.clone();
                self.open(View::Note(id));
            }
            // `Space` marks the one under the cursor, `*` everything shown.
            KeyCode::Char(' ') => {
                let id = self.selected()?.id.clone();
                if !self.marks.remove(&id) {
                    self.marks.insert(id);
                }
            }
            KeyCode::Char('*') => self.mark_shown(),
            // Clears the query first, then the marks, a query being cheaper to
            // retype. Never the queue.
            KeyCode::Esc => {
                if self.top().search.is_empty() {
                    self.marks.clear();
                } else {
                    self.top_mut().search.clear();
                    self.refilter();
                }
            }
            _ => {}
        }
        None
    }

    /// A note, a patch, a blame: scrolling it and closing it again.
    fn on_reading(&mut self, key: KeyEvent) -> Option<Action> {
        let page = i32::from(self.page);
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => self.step(1),
            KeyCode::Char('k') | KeyCode::Up => self.step(-1),
            KeyCode::PageDown => self.step(page),
            KeyCode::PageUp => self.step(-page),
            KeyCode::Char('g') | KeyCode::Home => self.jump(Edge::First),
            KeyCode::Char('G') | KeyCode::End => self.jump(Edge::Last),
            KeyCode::Esc => {
                self.back();
            }
            _ => {}
        }
        None
    }

    /// The other lists: walking, closing, and what the row is for.
    fn on_rows(&mut self, key: KeyEvent) -> Option<Action> {
        let page = i32::from(self.page);
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => self.step(1),
            KeyCode::Char('k') | KeyCode::Up => self.step(-1),
            KeyCode::PageDown => self.step(page),
            KeyCode::PageUp => self.step(-page),
            KeyCode::Char('g') | KeyCode::Home => self.jump(Edge::First),
            KeyCode::Char('G') | KeyCode::End => self.jump(Edge::Last),
            KeyCode::Enter => return self.chose(),
            KeyCode::Esc => {
                self.back();
            }
            _ => {}
        }
        None
    }

    /// `Enter` on a row: a note opens, another screen opens, and anything
    /// irreversible goes onto the prompt rather than running.
    fn chose(&mut self) -> Option<Action> {
        let at = self.top().table.selected()?;
        match self.top().view.clone() {
            View::Todo | View::Backlinks(_) => {
                let id = self.selected()?.id.clone();
                self.open(View::Note(id));
            }
            // Narrows the listing, as `1`–`9` do.
            View::Tags => {
                let tag = self.tallies().get(at)?.tag.clone();
                self.scope(&tag);
            }
            View::Files => {
                let name = self.files.get(at)?.clone();
                self.open(View::Backlinks(Subject::File(name)));
            }
            View::Notebooks => {
                let name = self.notebooks.get(at)?.clone();
                return self.switch(name);
            }
            View::Deleted => {
                let gone = self.gone().get(at)?;
                let line = format!("restore {} {}", gone.id, gone.restore_from_short());
                self.compose(line);
            }
            // A commit in one note's history is a revision to restore; the
            // notebook's log names no note.
            View::Log(Some(id)) => {
                let rev = self.entries().get(at)?.short_id();
                self.compose(format!("restore {id} {rev}"));
            }
            View::Notes | View::Note(_) | View::Log(None) | View::Blame(_) | View::Diff => {}
        }
        None
    }

    /// Puts a command on the prompt, written out but not run.
    fn compose(&mut self, line: String) {
        self.mode = Mode::Command;
        self.input.set(line);
        self.history_at = None;
    }

    /// `cmd::sort_notes`, so the orders are `--sort`'s; reversed after.
    fn arrange(&mut self) {
        cmd::sort_notes(&mut self.notes, self.sort);
        if self.reverse {
            self.notes.reverse();
        }
    }

    /// Re-sorts, keeping the cursor on the note rather than the row.
    fn reorder(&mut self) {
        let was = self.at_cursor().map(|file| file.id.clone());
        self.arrange();
        self.refilter();
        if let Some(id) = was {
            self.select_id(&id);
        }
        // Derived screens hold indices, and the notes have just moved.
        self.loaded = None;
        self.settle();
    }

    /// Narrows to the tags screen's `nth` tag; `0` clears the query.
    fn scope_nth(&mut self, nth: usize) {
        if nth == 0 {
            while self.back() {}
            self.top_mut().search.clear();
            self.refilter();
            return;
        }
        let Some(Content::Tags(tallies)) = self.derive(&View::Tags) else {
            return;
        };
        let Some(tally) = tallies.get(nth - 1) else {
            return;
        };
        let tag = tally.tag.clone();
        self.scope(&tag);
    }

    /// `query::scoped` writes the query, quotes and all, as the web does.
    fn scope(&mut self, tag: &str) {
        while self.back() {}
        self.top_mut().search.set(query::scoped(tag));
        self.refilter();
    }

    /// Refused while anything is queued: an entry's ids mean nothing, or the
    /// wrong thing, in another notebook.
    fn switch(&mut self, name: String) -> Option<Action> {
        if name == self.notebook {
            self.refuse(format!("`{name}` is the notebook you are in"));
            return None;
        }
        if !self.queue.is_empty() {
            self.refuse(format!(
                "{} queued against `{}` — send it or drop it first (Q)",
                self.queue.len(),
                self.notebook
            ));
            return None;
        }
        Some(Action::Use(name))
    }

    /// Marks everything shown, or unmarks it when it is all marked. Notes the
    /// query hides are untouched.
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

    /// Checked here rather than at the send, so a bad tag is refused where it
    /// was typed.
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
            // A plain `d` is safe here: nothing in this card deletes a note.
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
        // Asked only for a deletion: a confirmation shown every time is not read.
        if self.queue.iter().any(|step| step.change == Change::Remove) {
            self.mode = Mode::Confirm(What::Send);
            return None;
        }
        self.mode = Mode::Browse;
        Some(Action::Send(self.queue.clone()))
    }

    /// How many distinct notes the queue touches.
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

    /// The queue has been carried out; clears it and the marks.
    pub fn sent(&mut self) {
        self.queue.clear();
        self.queue_at = 0;
        self.marks.clear();
    }

    /// Opens the tag picker over the given notes.
    ///
    /// It asks which tags the notes should end up with, not which `+`/`-` to
    /// apply, so a `-` aimed at a misspelling, which silently removes nothing,
    /// cannot happen. `Tab` chooses, not `Space`, since a tag may contain a
    /// space.
    fn pick(&mut self, keys: Vec<String>) {
        let tallies = match self.derive(&View::Tags) {
            Some(Content::Tags(tallies)) => tallies,
            _ => Vec::new(),
        };
        self.choices = {
            let held = |tag: &str| {
                keys.iter()
                    .filter_map(|key| self.note_of(key))
                    .filter(|file| file.note.tags.iter().any(|held| held == tag))
                    .count()
            };
            tallies
                .into_iter()
                .map(|tally| Choice {
                    held: held(&tally.tag),
                    notes: tally.notes,
                    tag: tally.tag,
                    mark: Mark::Leave,
                })
                .collect()
        };
        self.aimed_at = keys;
        self.tags_at = 0;
        self.input.clear();
        self.mode = Mode::Tagging;
    }

    fn picking(&mut self, key: KeyEvent) -> Option<Action> {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Tab => {
                self.choose();
                return None;
            }
            KeyCode::Enter => return self.tagged(),
            KeyCode::Esc => {
                self.shut();
                return None;
            }
            // Not `j`/`k`: every letter goes into the filter.
            KeyCode::Down => {
                self.tags_at = (self.tags_at + 1).min(self.picker_rows().saturating_sub(1));
                return None;
            }
            KeyCode::Up => {
                self.tags_at = self.tags_at.saturating_sub(1);
                return None;
            }
            KeyCode::Char('n') if ctrl => {
                self.tags_at = (self.tags_at + 1).min(self.picker_rows().saturating_sub(1));
                return None;
            }
            KeyCode::Char('p') if ctrl => {
                self.tags_at = self.tags_at.saturating_sub(1);
                return None;
            }
            _ => {}
        }
        // Typing and erasing only; the card shows no cursor to move. A new
        // filter is a new list, so the cursor goes to the top.
        if self.input.erasing(key).is_some() {
            self.tags_at = 0;
        }
        None
    }

    /// Closes the picker, changing nothing.
    fn shut(&mut self) {
        self.mode = Mode::Browse;
        self.input.clear();
        self.choices.clear();
        self.aimed_at.clear();
        self.tags_at = 0;
    }

    /// Walks the tag under the cursor through the states that would change
    /// something, or makes the one that is not there.
    fn choose(&mut self) {
        if let Some(&at) = self.shown_tags().get(self.tags_at) {
            let total = self.aimed_at.len();
            let choice = &mut self.choices[at];
            choice.mark = choice.next(total);
            return;
        }
        // A new tag joins the choices; it still matches the filter, so the
        // cursor stays on it.
        let Some(Proposal::New { tag, .. }) = self.proposal() else {
            return;
        };
        self.choices.push(Choice {
            tag,
            held: 0,
            notes: 0,
            mark: Mark::Add,
        });
    }

    /// The only place `+`/`-` changes are written; nothing splits them back,
    /// so a tag with a space survives as one string.
    fn tagged(&mut self) -> Option<Action> {
        let changes: Vec<String> = self
            .choices
            .iter()
            .filter_map(|choice| match choice.mark {
                Mark::Leave => None,
                Mark::Add => Some(format!("+{}", choice.tag)),
                Mark::Remove => Some(format!("-{}", choice.tag)),
            })
            .collect();
        let keys = std::mem::take(&mut self.aimed_at);
        self.shut();
        // Nothing chosen is a way out, not an error.
        if changes.is_empty() {
            return None;
        }
        // A marked set queues; a single note runs now.
        if self.marks.is_empty() {
            return Some(Action::Tag {
                key: keys.into_iter().next()?,
                changes,
                touch: self.touch,
            });
        }
        self.enqueue(Step {
            keys,
            change: Change::Tag {
                changes,
                touch: self.touch,
            },
        });
        None
    }

    /// Indices of the choices the filter admits, commonest first, typed ones last.
    pub fn shown_tags(&self) -> Vec<usize> {
        let filter = self.input.text().trim().to_lowercase();
        self.choices
            .iter()
            .enumerate()
            .filter(|(_, choice)| filter.is_empty() || choice.tag.to_lowercase().contains(&filter))
            .map(|(at, _)| at)
            .collect()
    }

    /// The row after the last tag, when what has been typed is not one of them.
    pub fn proposal(&self) -> Option<Proposal> {
        let typed = self.input.text().trim();
        if typed.is_empty() || self.choices.iter().any(|choice| choice.tag == typed) {
            return None;
        }
        // Refused where typed, by the check the command itself would fail on.
        if let Err(e) = note::validate_tag(typed) {
            return Some(Proposal::Refused(e.to_string()));
        }
        Some(Proposal::New {
            tag: typed.to_string(),
            near: self.nearest(typed),
        })
    }

    /// The commonest existing tag a keystroke away, the likeliest one meant.
    /// Tags made in this picker are not candidates.
    fn nearest(&self, tag: &str) -> Option<(String, usize)> {
        self.choices
            .iter()
            .filter(|choice| choice.notes > 0 && one_edit_apart(&choice.tag, tag))
            .max_by_key(|choice| choice.notes)
            .map(|choice| (choice.tag.clone(), choice.notes))
    }

    pub fn picker_rows(&self) -> usize {
        self.shown_tags().len() + usize::from(self.proposal().is_some())
    }

    pub fn choices(&self) -> &[Choice] {
        &self.choices
    }

    pub fn tags_at(&self) -> usize {
        self.tags_at
    }

    /// What a row's `12/40` is out of.
    pub fn picking_notes(&self) -> usize {
        self.aimed_at.len()
    }

    /// The one note, when the picker is not over a marked set.
    pub fn picking_note(&self) -> Option<&NoteFile> {
        match self.aimed_at.as_slice() {
            [only] if self.marks.is_empty() => self.note_of(only),
            _ => None,
        }
    }

    fn searching(&mut self, key: KeyEvent) -> Option<Action> {
        // The field handles editing (and knows a chord is not a character);
        // what it leaves unbound falls through.
        match self.top_mut().search.key(key) {
            Some(Edit::Typed) => {
                self.refilter();
                return None;
            }
            Some(Edit::Moved) => return None,
            None => {}
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Enter => self.mode = Mode::Browse,
            // Clears the query, unlike `Enter`.
            KeyCode::Esc => {
                self.top_mut().search.clear();
                self.refilter();
                self.mode = Mode::Browse;
            }
            // The listing can be walked while typing. Ctrl-N/P walk it too, as
            // in other narrowing lists, not readline's history. The modifier is
            // checked though only chords reach here, that being another
            // module's invariant.
            KeyCode::Down => self.step(1),
            KeyCode::Up => self.step(-1),
            KeyCode::Char('n') if ctrl => self.step(1),
            KeyCode::Char('p') if ctrl => self.step(-1),
            _ => {}
        }
        None
    }

    /// Opens the prompt holding `start`, e.g. the current title for a retitle.
    fn ask(&mut self, what: Ask, start: String) {
        self.mode = Mode::Ask(what);
        self.input.set(start);
    }

    fn asking(&mut self, what: Ask, key: KeyEvent) -> Option<Action> {
        if self.input.key(key).is_some() {
            return None;
        }
        match key.code {
            KeyCode::Enter => return self.answered(what),
            KeyCode::Esc => {
                self.mode = Mode::Browse;
                self.input.clear();
            }
            _ => {}
        }
        None
    }

    /// An empty answer is a way out, not an error — except for a new note,
    /// where it is `noda add` with no title.
    fn answered(&mut self, what: Ask) -> Option<Action> {
        let answer = self.input.text().trim().to_string();
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
        }
    }

    /// Quits, or asks first if anything is queued.
    fn leaving(&mut self) -> Option<Action> {
        if self.queue.is_empty() {
            return Some(Action::Quit);
        }
        self.mode = Mode::Confirm(What::Quit);
        None
    }

    fn commanding(&mut self, key: KeyEvent) -> Option<Action> {
        if self.input.key(key).is_some() {
            return None;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Enter => {
                let line = self.input.take();
                self.mode = Mode::Browse;
                self.history_at = None;
                return self.run(&line);
            }
            KeyCode::Esc => {
                self.mode = Mode::Browse;
                self.input.clear();
                self.history_at = None;
            }
            // History, as a shell has it, under both readline spellings.
            KeyCode::Up => self.recall(true),
            KeyCode::Down => self.recall(false),
            KeyCode::Char('p') if ctrl => self.recall(true),
            KeyCode::Char('n') if ctrl => self.recall(false),
            _ => {}
        }
        None
    }

    /// Walks the history. Forward past the newest entry gives an empty line,
    /// so something new can be typed without clearing the field.
    fn recall(&mut self, back: bool) {
        if self.history.is_empty() {
            return;
        }
        let last = self.history.len() - 1;
        self.history_at = match (self.history_at, back) {
            (None, true) => Some(last),
            (Some(at), true) => Some(at.saturating_sub(1)),
            (None, false) => None,
            (Some(at), false) if at >= last => None,
            (Some(at), false) => Some(at + 1),
        };
        self.input.set(match self.history_at {
            Some(at) => self.history[at].clone(),
            None => String::new(),
        });
    }

    /// Runs a `:` line. The rest of the line is kept as typed, not rebuilt from
    /// tokens, which would break `tag:"12.34 foo bar"`.
    fn run(&mut self, line: &str) -> Option<Action> {
        let line = line.trim();
        if line.is_empty() {
            return None;
        }
        self.remember(line);
        let (name, rest) = match line.find(char::is_whitespace) {
            Some(at) => (&line[..at], line[at..].trim_start()),
            None => (line, ""),
        };
        let Some(spec) = command::find(name) else {
            self.refuse(format!("no command `{name}` — Ctrl-a lists them"));
            return None;
        };
        let args = query::split(rest);

        match spec.name {
            "quit" => return self.leaving(),
            "reload" => return Some(Action::Reload),
            "keys" => self.mode = Mode::Help,
            // Back to the listing, narrowed on the way if a query came too.
            "notes" => {
                while self.back() {}
                self.top_mut().search.set(rest.to_string());
                self.refilter();
            }
            "open" => {
                let Some(key) = args.first() else {
                    return self.wants(spec);
                };
                return Some(Action::Open(key.clone()));
            }
            "edit" => {
                let key = self.aimed(args.first())?;
                return Some(Action::Edit {
                    key,
                    touch: self.touch,
                });
            }
            "add" => {
                return Some(Action::Add((!rest.is_empty()).then(|| rest.to_string())));
            }
            // A title's first word cannot be told from a key, so this retitles
            // the note on screen.
            "mv" => {
                if rest.is_empty() {
                    return self.wants(spec);
                }
                let key = self.aimed(None)?;
                return Some(Action::Retitle {
                    key,
                    title: rest.to_string(),
                    touch: self.touch,
                });
            }
            "tag" => {
                // A key cannot begin with `+` or `-`, so one that does is a
                // change rather than a note.
                let (key, changes) = match args.split_first() {
                    Some((first, rest)) if !first.starts_with(['+', '-']) => {
                        (self.aimed(Some(first))?, rest.to_vec())
                    }
                    _ => (self.aimed(None)?, args.clone()),
                };
                if changes.is_empty() {
                    return self.wants(spec);
                }
                return Some(Action::Tag {
                    key,
                    changes,
                    touch: self.touch,
                });
            }
            "pin" | "unpin" => {
                let key = self.aimed(args.first())?;
                return Some(Action::Pin {
                    key,
                    pinned: spec.name == "pin",
                    touch: self.touch,
                });
            }
            "rm" => return self.delete(),
            "restore" => {
                let (Some(key), Some(rev)) = (args.first(), args.get(1)) else {
                    return self.wants(spec);
                };
                return Some(Action::Restore {
                    key: key.clone(),
                    rev: rev.clone(),
                    touch: self.touch,
                });
            }
            "use" => {
                let Some(name) = args.first() else {
                    return self.wants(spec);
                };
                return self.switch(name.clone());
            }
            "todo" => self.open(View::Todo),
            "tags" => self.open(View::Tags),
            "files" => self.open(View::Files),
            "notebooks" => self.open(View::Notebooks),
            "deleted" => self.open(View::Deleted),
            "diff" => self.open(View::Diff),
            // A named key goes to the runtime to be resolved, as `open` does.
            "log" => {
                if let Some(key) = args.first() {
                    return Some(Action::Show {
                        key: key.clone(),
                        look: Look::Log,
                    });
                }
                let about = self.about();
                self.open(View::Log(about));
            }
            "blame" => {
                if let Some(key) = args.first() {
                    return Some(Action::Show {
                        key: key.clone(),
                        look: Look::Blame,
                    });
                }
                let id = self.aimed(None)?;
                self.open(View::Blame(id));
            }
            // A named key is a note; a file's backlinks come from the files
            // screen, which has already said which kind it is.
            "backlinks" => {
                if let Some(key) = args.first() {
                    return Some(Action::Show {
                        key: key.clone(),
                        look: Look::Backlinks,
                    });
                }
                let subject = self.linkable()?;
                self.open(View::Backlinks(subject));
            }
            "status" => return Some(Action::Run(Run::Status)),
            "doctor" => {
                let mut links = false;
                let mut times = false;
                for arg in &args {
                    match arg.as_str() {
                        "--links" => links = true,
                        "--times" => times = true,
                        _ => return self.wants(spec),
                    }
                }
                return Some(Action::Run(Run::Doctor { links, times }));
            }
            "snapshot" => return Some(Action::Run(Run::Snapshot(args.first().cloned()))),
            "readme" => return Some(Action::Run(Run::Readme)),
            "sync" => return Some(Action::Run(Run::Sync)),
            "push" => return Some(Action::Run(Run::Push)),
            "pull" => return Some(Action::Run(Run::Pull)),
            _ => {}
        }
        None
    }

    /// The note a command is aimed at: the one it named, or the one the screen
    /// is about.
    fn aimed(&mut self, given: Option<&String>) -> Option<String> {
        if let Some(key) = given {
            return Some(key.clone());
        }
        let Some(file) = self.selected() else {
            self.refuse("no note on screen — name one, or open one first".to_string());
            return None;
        };
        Some(file.id.clone())
    }

    /// The note the screen is about, if any — for `log`, the one command that
    /// takes a note or the whole notebook.
    fn about(&self) -> Option<String> {
        match &self.top().view {
            View::Note(id)
            | View::Blame(id)
            | View::Log(Some(id))
            | View::Backlinks(Subject::Note(id)) => Some(id.clone()),
            _ => None,
        }
    }

    /// The file under the cursor on the files screen, otherwise the aimed note.
    fn linkable(&mut self) -> Option<Subject> {
        if matches!(self.top().view, View::Files)
            && let Some(at) = self.top().table.selected()
            && let Some(name) = self.files.get(at)
        {
            return Some(Subject::File(name.clone()));
        }
        Some(Subject::Note(self.aimed(None)?))
    }

    /// Refuses with the command's usage.
    fn wants(&mut self, spec: &command::Spec) -> Option<Action> {
        self.refuse(format!("{} — {}", spec.usage(), spec.what));
        None
    }

    /// On the status line, not a card: cards are for what a command said when
    /// it ran, and this never ran.
    fn refuse(&mut self, text: String) {
        self.message = Some(Message { text, failed: true });
    }

    fn remember(&mut self, line: &str) {
        if self.history.last().map(String::as_str) == Some(line) {
            return;
        }
        self.history.push(line.to_string());
    }

    /// The command palette, narrowed as you type.
    fn listing_commands(&mut self, key: KeyEvent) -> Option<Action> {
        // Typing and erasing only; see `Field::erasing`.
        if self.input.erasing(key).is_some() {
            self.commands_at = 0;
            return None;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let shown = command::matching(self.input.text()).count();
        match key.code {
            KeyCode::Esc => {
                self.mode = Mode::Browse;
                self.input.clear();
            }
            KeyCode::Down => {
                self.commands_at = (self.commands_at + 1).min(shown.saturating_sub(1));
            }
            KeyCode::Up => self.commands_at = self.commands_at.saturating_sub(1),
            KeyCode::Char('n') if ctrl => {
                self.commands_at = (self.commands_at + 1).min(shown.saturating_sub(1));
            }
            KeyCode::Char('p') if ctrl => {
                self.commands_at = self.commands_at.saturating_sub(1);
            }
            // Onto the prompt rather than run: most take arguments, and `push`
            // should not go off by landing on it.
            KeyCode::Enter => {
                let Some(spec) = command::matching(self.input.text()).nth(self.commands_at) else {
                    self.mode = Mode::Browse;
                    self.input.clear();
                    return None;
                };
                self.input.set(if spec.takes.is_empty() {
                    spec.name.to_string()
                } else {
                    format!("{} ", spec.name)
                });
                self.mode = Mode::Command;
                self.history_at = None;
            }
            _ => {}
        }
        None
    }

    pub fn commands_at(&self) -> usize {
        self.commands_at
    }

    /// Opens a note the runtime resolved through `Notebook::resolve`.
    pub fn open_note(&mut self, id: String) {
        self.open(View::Note(id));
    }

    /// Opens a screen about a note the runtime has resolved.
    pub fn look_at(&mut self, look: Look, id: String) {
        match look {
            Look::Log => self.open(View::Log(Some(id))),
            Look::Blame => self.open(View::Blame(id)),
            Look::Backlinks => self.open(View::Backlinks(Subject::Note(id))),
        }
    }

    /// `y` agrees; any other key cancels.
    fn confirming(&mut self, what: What, key: KeyEvent) -> Option<Action> {
        self.mode = Mode::Browse;
        if !matches!(key.code, KeyCode::Char('y' | 'Y')) {
            return None;
        }
        match what {
            What::Delete => Some(Action::Remove(self.selected()?.id.clone())),
            What::Send => Some(Action::Send(self.queue.clone())),
            What::Quit => Some(Action::Quit),
        }
    }

    /// How a command the runtime ran went.
    pub fn report(&mut self, outcome: Result<String>) {
        self.message = Some(match outcome {
            Ok(text) => {
                let text = plain(&text).trim_end().to_string();
                // One line goes on the status bar; more gets the card.
                if text.lines().count() > 1 {
                    self.mode = Mode::Alert;
                }
                Message {
                    text,
                    failed: false,
                }
            }
            Err(e) => {
                // In full: e.g. a broken frontmatter error ends by saying where
                // the file was left.
                self.mode = Mode::Alert;
                Message {
                    text: plain(&e.to_string()),
                    failed: true,
                }
            }
        });
    }

    /// For the runtime to find a note just made, by diffing ids.
    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.notes.iter().map(|file| file.id.as_str())
    }

    /// Puts the listing's cursor on a note, e.g. one just made with `a`.
    pub fn select_id(&mut self, id: &str) {
        if let Some(at) = self
            .listing()
            .visible
            .iter()
            .position(|&i| self.notes[i].id == id)
        {
            self.select(at);
        }
    }

    /// Moves the cursor, or scrolls the page, by `delta` rows.
    fn step(&mut self, delta: i32) {
        let Some(rows) = self.rows_here() else {
            let max = self.reading_height();
            let screen = self.top_mut();
            screen.scroll = scrolled(screen.scroll, delta, max);
            return;
        };
        let Some(at) = self.top().table.selected() else {
            return;
        };
        let last = rows.saturating_sub(1);
        let moved = match usize::try_from(delta) {
            Ok(down) => at.saturating_add(down).min(last),
            Err(_) => at.saturating_sub(delta.unsigned_abs() as usize),
        };
        self.cursor_to(moved);
    }

    fn jump(&mut self, edge: Edge) {
        let Some(rows) = self.rows_here() else {
            let max = self.reading_height();
            self.top_mut().scroll = match edge {
                Edge::First => 0,
                Edge::Last => max,
            };
            return;
        };
        match edge {
            Edge::First => self.cursor_to(0),
            Edge::Last => self.cursor_to(rows.saturating_sub(1)),
        }
    }

    /// The furthest scroll: the last line at the top of the screen. Counted
    /// before wrapping, which under-shoots on long lines — better than
    /// scrolling into blank space.
    fn reading_height(&self) -> u16 {
        let lines = match self.content() {
            Some(Content::Note(text) | Content::Diff(text)) => text.lines().count(),
            Some(Content::Blame(lines)) => lines.len(),
            _ => 0,
        };
        lines.saturating_sub(1) as u16
    }

    /// Clamped to the screen's rows; an empty list has no cursor at all.
    fn cursor_to(&mut self, at: usize) {
        let rows = self.rows_here().unwrap_or(0);
        let screen = self.top_mut();
        if rows == 0 {
            screen.table.select(None);
        } else {
            screen.table.select(Some(at.min(rows - 1)));
        }
    }

    fn select(&mut self, at: usize) {
        let listing = self.listing_mut();
        if listing.visible.is_empty() {
            listing.table.select(None);
        } else {
            let last = listing.visible.len() - 1;
            listing.table.select(Some(at.min(last)));
        }
    }

    /// Reruns the query over every note — whole, not incremental, since a
    /// keystroke can widen a query too, and it is well under a frame.
    ///
    /// Split as a shell would, since `Query::parse` takes shell arguments and
    /// no shell stands in front of this field; otherwise `tag:"12.34 foo bar"`
    /// would be three terms.
    fn refilter(&mut self) {
        let tokens = query::split(self.listing().search.text());

        if tokens.is_empty() {
            let all = (0..self.notes.len()).collect();
            let listing = self.listing_mut();
            listing.error = None;
            listing.terms.clear();
            listing.visible = all;
            self.select(0);
            return;
        }

        match Query::parse(&tokens) {
            Ok(query) => {
                let visible: Vec<usize> = self
                    .notes
                    .iter()
                    .enumerate()
                    .filter(|(_, file)| query.matches(&file.id, &file.note))
                    .map(|(at, _)| at)
                    .collect();
                let terms = query.excerpt_terms();
                let listing = self.listing_mut();
                listing.error = None;
                listing.terms = terms;
                listing.visible = visible;
                self.select(0);
            }
            // Half a query is normal while typing: say so and keep the last
            // good result rather than emptying the list.
            Err(e) => self.listing_mut().error = Some(e.to_string()),
        }
    }
}

enum Edge {
    First,
    Last,
}

/// A command's answer with its colour taken out. `style` paints
/// unconditionally for `anstream` to strip off a stream, but here the answer
/// is drawn into a buffer, where an escape shows as `[2m` and throws off the
/// card's measured width.
fn plain(text: &str) -> String {
    anstream::adapter::strip_str(text).to_string()
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

    /// What the runtime does between the keystroke and the frame.
    fn read_it(app: &mut App, text: &str) {
        app.on_key(key(KeyCode::Enter));
        let view = app.view().clone();
        assert!(
            matches!(app.wanted(), Some(Need::Note { .. })),
            "a note screen wants its file"
        );
        app.supply(&view, Content::Note(text.to_string()));
    }

    /// Supplies content as `refresh` would.
    fn supplied(app: &mut App, content: Content) {
        let view = app.view().clone();
        assert!(app.wanted().is_some(), "{view:?} asked for nothing");
        app.supply(&view, content);
    }

    /// Nothing but these notes, and a fixed date so "overdue" holds still.
    fn a_session(status: Status, notes: Vec<NoteFile>) -> Session {
        Session {
            status,
            notes,
            files: Vec::new(),
            notebooks: vec!["personal".to_string()],
            today: "2026-08-09".to_string(),
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
                pinned: None,
                extra: Vec::new(),
                body: body.to_string(),
            },
        }
    }

    // Fixed ids, so assertions are stable.
    fn an_app() -> App {
        App::new(
            "personal".to_string(),
            PathBuf::from("/notebook"),
            a_session(
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
            ),
        )
    }

    #[test]
    fn it_opens_on_the_listing_and_the_first_note() {
        let app = an_app();
        assert_eq!(app.view(), &View::Notes);
        assert_eq!(app.depth(), 1);
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
        // The title of one and the body of another: a bare word is `text:`.
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

        typing(&mut app, " O");
        let last_good = app.shown();
        assert!(app.error().is_none());

        // `OR` with nothing after it breaks the grammar.
        typing(&mut app, "R");
        assert!(app.error().is_some());
        assert_eq!(
            app.shown(),
            last_good,
            "an unfinished query leaves the list where it was"
        );

        typing(&mut app, " tag:q3");
        assert!(app.error().is_none());
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
        assert!(app.search().is_empty());
    }

    #[test]
    fn enter_keeps_the_query_and_hands_back_the_keyboard() {
        let mut app = an_app();
        app.on_key(key(KeyCode::Char('/')));
        typing(&mut app, "tag:q3");
        app.on_key(key(KeyCode::Enter));

        assert_eq!(app.mode, Mode::Browse);
        assert_eq!(app.depth(), 1, "leaving the field is not opening a note");
        assert_eq!(app.shown(), 1);
    }

    #[test]
    fn a_query_that_matches_nothing_leaves_no_cursor() {
        let mut app = an_app();
        app.on_key(key(KeyCode::Char('/')));
        typing(&mut app, "tag:nothing");
        assert_eq!(app.shown(), 0);
        assert!(app.selected().is_none());
        app.on_key(key(KeyCode::Enter));
        app.on_key(key(KeyCode::Enter));
        assert_eq!(app.depth(), 1);
        assert!(app.wanted().is_none());
    }

    #[test]
    fn enter_opens_the_note_and_escape_closes_it_again() {
        let mut app = an_app();
        app.on_key(key(KeyCode::Char('j')));

        app.on_key(key(KeyCode::Enter));
        assert_eq!(app.view(), &View::Note("bbbb2222".to_string()));
        assert_eq!(app.depth(), 2);
        assert_eq!(
            app.wanted(),
            Some(Need::Note {
                id: "bbbb2222".to_string(),
                path: PathBuf::from("/notebook/bbbb2222-meeting-notes.md"),
            })
        );
        supplied(&mut app, Content::Note("agenda\n".to_string()));
        assert!(app.wanted().is_none());

        app.on_key(key(KeyCode::Esc));
        assert_eq!(app.view(), &View::Notes);
        assert_eq!(app.depth(), 1);
        assert_eq!(app.selected().map(|f| f.id.as_str()), Some("bbbb2222"));
    }

    #[test]
    fn the_listing_is_never_popped() {
        let mut app = an_app();
        for _ in 0..5 {
            app.on_key(key(KeyCode::Esc));
        }
        assert_eq!(app.depth(), 1);
        assert_eq!(app.view(), &View::Notes);
    }

    #[test]
    fn a_note_opened_from_a_query_keeps_what_the_query_picked_out() {
        let mut app = an_app();
        app.on_key(key(KeyCode::Char('/')));
        typing(&mut app, "budget");
        app.on_key(key(KeyCode::Enter));
        assert_eq!(app.terms(), ["budget"]);

        app.on_key(key(KeyCode::Enter));
        assert_eq!(app.depth(), 2);
        assert_eq!(app.terms(), ["budget"]);
    }

    #[test]
    fn a_note_on_its_own_screen_scrolls_and_stops_at_the_end() {
        let mut app = an_app();
        read_it(&mut app, "one\ntwo\nthree\nfour\n");
        assert_eq!(app.scroll(), 0);

        app.on_key(key(KeyCode::Char('j')));
        assert_eq!(app.scroll(), 1);
        for _ in 0..20 {
            app.on_key(key(KeyCode::Char('j')));
        }
        assert_eq!(app.scroll(), 3, "it stops rather than running into blanks");

        app.on_key(key(KeyCode::Char('g')));
        assert_eq!(app.scroll(), 0);
    }

    #[test]
    fn the_listing_keeps_its_cursor_while_a_note_is_being_read() {
        let mut app = an_app();
        read_it(&mut app, "one\ntwo\nthree\nfour\n");
        for _ in 0..3 {
            app.on_key(key(KeyCode::Char('j')));
        }
        assert_eq!(app.scroll(), 3, "the note scrolled");

        app.on_key(key(KeyCode::Esc));
        assert_eq!(
            app.selected().map(|f| f.id.as_str()),
            Some("aaaa1111"),
            "and the listing did not move underneath it"
        );
    }

    #[test]
    fn a_note_starts_at_its_top() {
        let mut app = an_app();
        read_it(&mut app, "one\ntwo\nthree\n");
        app.on_key(key(KeyCode::Char('j')));
        assert_eq!(app.scroll(), 1);

        let view = app.view().clone();
        app.supply(&view, Content::Note("agenda\n".to_string()));
        assert_eq!(app.scroll(), 0);
    }

    #[test]
    fn ctrl_f_moves_by_half_of_what_is_on_screen() {
        let mut app = an_app();
        app.set_page(4);
        app.on_key(ctrl('f'));
        assert_eq!(app.selected().map(|f| f.id.as_str()), Some("cccc3333"));
        app.on_key(ctrl('b'));
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
    fn the_keys_that_change_a_note_mean_the_same_on_the_note_itself() {
        let mut app = an_app();
        read_it(&mut app, "the q3 budget\n");

        assert_eq!(
            app.on_key(key(KeyCode::Char('e'))),
            Some(Action::Edit {
                key: "aaaa1111".to_string(),
                touch: Touch::Stamp,
            })
        );
        app.on_key(key(KeyCode::Char('m')));
        assert_eq!(app.mode, Mode::Ask(Ask::Retitle));
        assert_eq!(app.input.text(), "Budget review");
        app.on_key(key(KeyCode::Esc));

        app.on_key(ctrl('d'));
        assert_eq!(app.mode, Mode::Confirm(What::Delete));
        assert_eq!(
            app.on_key(key(KeyCode::Char('y'))),
            Some(Action::Remove("aaaa1111".to_string()))
        );
    }

    #[test]
    fn deleting_the_note_you_are_reading_closes_its_screen() {
        let mut app = an_app();
        read_it(&mut app, "the q3 budget\n");
        assert_eq!(app.depth(), 2);

        // As after a delete: the re-read notebook lacks the note on screen.
        app.replace(a_session(
            a_status(),
            vec![
                a_note("bbbb2222", "meeting-notes", "Meeting notes", &["work"], "x"),
                a_note("cccc3333", "reading-list", "Reading list", &[], "a book"),
            ],
        ));
        assert_eq!(app.depth(), 1, "a screen with nothing behind it is closed");
        assert_eq!(app.view(), &View::Notes);
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

        // Ctrl-R arrives as `Char('r')` with a modifier; an unbound chord
        // neither types nor reloads.
        app.on_key(ctrl('r'));
        app.on_key(ctrl('o'));
        assert_eq!(app.search(), "budget");
        assert_eq!(app.mode, Mode::Search, "and neither of them left the field");

        // Shift is not a chord.
        app.on_key(KeyEvent::new(KeyCode::Char('Q'), KeyModifiers::SHIFT));
        assert_eq!(app.search(), "budgetQ");
    }

    #[test]
    fn the_query_answers_the_keys_a_shell_prompt_answers() {
        let mut app = an_app();
        app.on_key(key(KeyCode::Char('/')));
        typing(&mut app, "tag:work budget");
        assert_eq!(app.shown(), 1, "one note has both");

        // The keys are tested in `field`; this checks the query is re-run.
        app.on_key(ctrl('w'));
        assert_eq!(app.search(), "tag:work ");
        assert_eq!(app.shown(), 2, "and the listing is what the query now says");

        // Including an edit mid-line.
        app.on_key(ctrl('a'));
        typing(&mut app, "id:aaaa1111 ");
        assert_eq!(app.search(), "id:aaaa1111 tag:work ");
        assert_eq!(app.shown(), 1);
    }

    #[test]
    fn the_cursor_still_walks_the_listing_while_a_query_is_typed() {
        let mut app = an_app();
        app.on_key(key(KeyCode::Char('/')));
        typing(&mut app, "tag:work");
        assert_eq!(
            app.selected().map(|note| note.id.as_str()),
            Some("aaaa1111")
        );
        app.on_key(key(KeyCode::Down));
        assert_eq!(
            app.selected().map(|note| note.id.as_str()),
            Some("bbbb2222")
        );
        app.on_key(ctrl('p'));
        assert_eq!(
            app.selected().map(|note| note.id.as_str()),
            Some("aaaa1111")
        );
        assert_eq!(app.search(), "tag:work");
    }

    #[test]
    fn q_is_a_letter_while_a_query_is_being_typed() {
        let mut app = an_app();
        app.on_key(key(KeyCode::Char('/')));
        assert_eq!(app.on_key(key(KeyCode::Char('q'))), None);
        assert_eq!(app.search(), "q");
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

        let notes = vec![
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
            a_note("dddd4444", "trip-plan", "Trip plan", &["work"], "flights"),
        ];
        app.replace(a_session(a_status(), notes));

        assert_eq!(app.search(), "tag:work");
        assert_eq!(app.shown(), 3);
        assert_eq!(app.selected().map(|f| f.id.as_str()), Some("bbbb2222"));
    }

    #[test]
    fn a_reload_drops_the_copy_of_the_note_that_was_on_screen() {
        let mut app = an_app();
        read_it(&mut app, "the q3 budget\n");
        assert!(app.wanted().is_none(), "it has been read");

        app.replace(a_session(
            a_status(),
            vec![a_note(
                "aaaa1111",
                "budget-review",
                "Budget review",
                &["work"],
                "the q3 budget, revised",
            )],
        ));
        assert!(app.wanted().is_some());
    }

    #[test]
    fn a_reload_that_removes_the_selected_note_lands_somewhere_real() {
        let mut app = an_app();
        app.on_key(key(KeyCode::Char('G')));
        assert_eq!(app.selected().map(|f| f.id.as_str()), Some("cccc3333"));

        app.replace(a_session(
            a_status(),
            vec![a_note(
                "aaaa1111",
                "budget-review",
                "Budget review",
                &["work"],
                "q3",
            )],
        ));
        assert_eq!(app.selected().map(|f| f.id.as_str()), Some("aaaa1111"));
    }

    #[test]
    fn an_empty_notebook_has_nowhere_to_put_the_cursor() {
        let mut app = App::new(
            "personal".to_string(),
            PathBuf::from("/notebook"),
            a_session(a_status(), Vec::new()),
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
        assert_eq!(app.mode, Mode::Browse);
    }

    #[test]
    fn a_new_note_is_titled_at_the_prompt() {
        let mut app = an_app();
        assert_eq!(app.on_key(key(KeyCode::Char('a'))), None);
        assert_eq!(app.mode, Mode::Ask(Ask::Title));

        typing(&mut app, "Trip plan");
        assert_eq!(app.input.text(), "Trip plan");
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
        assert_eq!(app.on_key(key(KeyCode::Enter)), Some(Action::Add(None)));
    }

    #[test]
    fn a_retitle_starts_from_the_title_it_has() {
        let mut app = an_app();
        app.on_key(key(KeyCode::Char('m')));
        assert_eq!(app.mode, Mode::Ask(Ask::Retitle));
        assert_eq!(app.input.text(), "Budget review");

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
    fn the_picker_opens_on_every_tag_with_the_notes_own_ticked() {
        let mut app = an_app();
        app.on_key(key(KeyCode::Char('#')));
        assert_eq!(app.mode, Mode::Tagging);

        // The tags screen's list and order, whether or not this note has them.
        let names: Vec<&str> = app.choices().iter().map(|c| c.tag.as_str()).collect();
        assert_eq!(names, vec!["work", "q3"]);
        assert_eq!(app.picking_notes(), 1);
        assert_eq!(app.picking_note().map(|f| f.id.as_str()), Some("aaaa1111"));

        assert_eq!(app.choices()[0].tick(1), "[x]");
        assert_eq!(app.choices()[0].notes, 2);
        assert_eq!(app.choices()[1].tick(1), "[ ]");
    }

    #[test]
    fn tab_walks_a_tag_through_the_states_that_would_change_something() {
        let mut app = an_app();
        app.on_key(key(KeyCode::Char('#')));

        // Two states over one note: a tag it carries cannot be given again.
        app.on_key(key(KeyCode::Tab));
        assert_eq!(app.choices()[0].mark, Mark::Remove);
        app.on_key(key(KeyCode::Tab));
        assert_eq!(app.choices()[0].mark, Mark::Leave);

        app.on_key(key(KeyCode::Down));
        app.on_key(key(KeyCode::Tab));
        assert_eq!(app.choices()[1].mark, Mark::Add);
        assert_eq!(app.choices()[1].tick(1), "[+]");
    }

    #[test]
    fn what_the_picker_sends_is_the_notation_the_command_takes() {
        let mut app = an_app();
        app.on_key(key(KeyCode::Char('#')));
        app.on_key(key(KeyCode::Tab));
        app.on_key(key(KeyCode::Down));
        app.on_key(key(KeyCode::Tab));

        assert_eq!(
            app.on_key(key(KeyCode::Enter)),
            Some(Action::Tag {
                key: "aaaa1111".to_string(),
                changes: vec!["-work".to_string(), "+q3".to_string()],
                touch: Touch::Stamp,
            })
        );
        assert_eq!(app.mode, Mode::Browse);
        assert!(app.choices().is_empty(), "and nothing is left behind");
    }

    #[test]
    fn choosing_nothing_is_a_way_out_rather_than_a_command() {
        let mut app = an_app();
        app.on_key(key(KeyCode::Char('#')));
        assert_eq!(app.on_key(key(KeyCode::Enter)), None);
        assert_eq!(app.mode, Mode::Browse);
        assert!(app.message.is_none(), "and nothing is said about it");
    }

    #[test]
    fn esc_leaves_the_picker_with_nothing_asked_of_the_notebook() {
        let mut app = an_app();
        app.on_key(key(KeyCode::Char('#')));
        app.on_key(key(KeyCode::Tab));
        assert_eq!(app.on_key(key(KeyCode::Esc)), None);
        assert_eq!(app.mode, Mode::Browse);
        assert!(app.choices().is_empty());
        assert!(app.queue.is_empty());
    }

    #[test]
    fn a_tag_the_notebook_does_not_have_is_a_row_of_its_own() {
        let mut app = an_app();
        app.on_key(key(KeyCode::Char('#')));
        typing(&mut app, "urgent");

        assert!(app.shown_tags().is_empty());
        assert_eq!(
            app.proposal(),
            Some(Proposal::New {
                tag: "urgent".to_string(),
                near: None,
            })
        );
        assert_eq!(app.picker_rows(), 1);

        app.on_key(key(KeyCode::Tab));
        assert_eq!(app.proposal(), None, "it is one of the tags now");
        assert_eq!(app.choices().last().map(|c| c.mark), Some(Mark::Add));
        assert_eq!(
            app.on_key(key(KeyCode::Enter)),
            Some(Action::Tag {
                key: "aaaa1111".to_string(),
                changes: vec!["+urgent".to_string()],
                touch: Touch::Stamp,
            })
        );
    }

    #[test]
    fn a_new_tag_a_keystroke_from_an_old_one_says_which_one() {
        let mut app = an_app();
        app.on_key(key(KeyCode::Char('#')));
        for typed in ["wrok", "Work", "wrk", "workk"] {
            typing(&mut app, typed);
            assert_eq!(
                app.proposal(),
                Some(Proposal::New {
                    tag: typed.to_string(),
                    near: Some(("work".to_string(), 2)),
                }),
                "{typed}"
            );
            for _ in 0..typed.len() {
                app.on_key(key(KeyCode::Backspace));
            }
        }

        // No warning for a tag that is nothing like one.
        typing(&mut app, "budget");
        assert_eq!(
            app.proposal(),
            Some(Proposal::New {
                tag: "budget".to_string(),
                near: None,
            })
        );
    }

    #[test]
    fn a_tag_the_notebook_would_refuse_is_refused_where_it_is_typed() {
        let mut app = an_app();
        app.on_key(key(KeyCode::Char('#')));
        typing(&mut app, "a,b");
        let Some(Proposal::Refused(why)) = app.proposal() else {
            panic!("a comma is not a tag");
        };
        assert!(why.contains(','), "in the words `cmd` uses: {why}");

        app.on_key(key(KeyCode::Tab));
        assert_eq!(app.on_key(key(KeyCode::Enter)), None);
    }

    #[test]
    fn over_a_marked_set_a_tag_some_of_them_carry_has_three_states() {
        let mut app = an_app();
        app.on_key(key(KeyCode::Char('*')));
        app.on_key(key(KeyCode::Char('#')));
        assert_eq!(app.picking_notes(), 3);
        assert!(
            app.picking_note().is_none(),
            "it is about a set, not a note"
        );

        assert_eq!(app.choices()[0].held, 2);
        assert_eq!(app.choices()[0].tick(3), "[ ]");
        app.on_key(key(KeyCode::Tab));
        assert_eq!(app.choices()[0].mark, Mark::Add);
        app.on_key(key(KeyCode::Tab));
        assert_eq!(app.choices()[0].mark, Mark::Remove);
        app.on_key(key(KeyCode::Tab));
        assert_eq!(app.choices()[0].mark, Mark::Leave);

        // Over a marked set it queues rather than runs.
        app.on_key(key(KeyCode::Tab));
        assert_eq!(app.on_key(key(KeyCode::Enter)), None);
        assert_eq!(app.queue.len(), 1);
        assert_eq!(
            app.queue[0].change,
            Change::Tag {
                changes: vec!["+work".to_string()],
                touch: Touch::Stamp,
            }
        );
        assert_eq!(app.queue[0].keys.len(), 3);
    }

    #[test]
    fn a_tick_over_a_set_means_every_one_of_them_carries_it() {
        let mut app = an_app();
        // The two notes that have `work`.
        mark(&mut app, &["aaaa1111", "bbbb2222"]);
        app.on_key(key(KeyCode::Char('#')));

        assert_eq!(app.choices()[0].tick(2), "[x]");
        app.on_key(key(KeyCode::Tab));
        assert_eq!(app.choices()[0].mark, Mark::Remove);
    }

    #[test]
    fn the_filter_narrows_the_list_and_leaves_the_choices_alone() {
        let mut app = an_app();
        app.on_key(key(KeyCode::Char('#')));
        app.on_key(key(KeyCode::Tab));
        assert_eq!(app.choices()[0].mark, Mark::Remove);

        typing(&mut app, "q");
        assert_eq!(app.shown_tags(), vec![1], "only `q3` answers to that");
        assert_eq!(app.tags_at(), 0, "and the cursor is on the list it can see");
        // A choice filtered out of sight is kept.
        assert_eq!(app.choices()[0].mark, Mark::Remove);

        app.on_key(key(KeyCode::Tab));
        assert_eq!(app.choices()[1].mark, Mark::Add);
        assert_eq!(
            app.on_key(key(KeyCode::Enter)),
            Some(Action::Tag {
                key: "aaaa1111".to_string(),
                changes: vec!["-work".to_string(), "+q3".to_string()],
                touch: Touch::Stamp,
            })
        );
    }

    #[test]
    fn a_tag_with_a_space_in_it_can_be_named_here_like_any_other() {
        let mut app = an_app();
        app.on_key(key(KeyCode::Char('#')));
        // As an import leaves behind. The space bar types a space.
        typing(&mut app, "24.04 Dark patterns");
        assert_eq!(app.input.text(), "24.04 Dark patterns");
        assert_eq!(
            app.proposal(),
            Some(Proposal::New {
                tag: "24.04 Dark patterns".to_string(),
                near: None,
            })
        );

        app.on_key(key(KeyCode::Tab));
        assert_eq!(
            app.on_key(key(KeyCode::Enter)),
            Some(Action::Tag {
                key: "aaaa1111".to_string(),
                changes: vec!["+24.04 Dark patterns".to_string()],
                touch: Touch::Stamp,
            })
        );
    }

    #[test]
    fn one_keystroke_apart_is_the_four_shapes_a_slip_takes() {
        assert!(one_edit_apart("Work", "work"));
        assert!(one_edit_apart("wor", "work"));
        assert!(one_edit_apart("works", "work"));
        assert!(one_edit_apart("wprk", "work"));
        assert!(one_edit_apart("wrok", "work"));
        assert!(one_edit_apart("q4", "q3"));
        assert!(!one_edit_apart("wrko", "work"));
        assert!(!one_edit_apart("workshop", "work"));
        assert!(!one_edit_apart("q3", "budget"));
    }

    #[test]
    fn a_tag_with_a_space_in_it_can_be_searched_for() {
        let mut app = App::new(
            "personal".to_string(),
            PathBuf::from("/notebook"),
            a_session(
                a_status(),
                vec![
                    a_note(
                        "aaaa1111",
                        "ubuntu-notes",
                        "Ubuntu notes",
                        &["12.34 foo bar"],
                        "body",
                    ),
                    a_note("bbbb2222", "other-note", "Other note", &["work"], "foo bar"),
                ],
            ),
        );

        app.on_key(key(KeyCode::Char('/')));
        // Unquoted it is three terms ANDed, as a shell would split it.
        typing(&mut app, "tag:12.34 foo bar");
        assert_eq!(app.shown(), 0);

        app.on_key(key(KeyCode::Esc));
        app.on_key(key(KeyCode::Char('/')));
        typing(&mut app, "tag:\"12.34 foo bar\"");
        assert_eq!(app.shown(), 1);
        assert_eq!(app.selected().map(|f| f.id.as_str()), Some("aaaa1111"));
    }

    #[test]
    fn quoting_leaves_an_ordinary_query_exactly_as_it_was() {
        let mut app = an_app();
        app.on_key(key(KeyCode::Char('/')));
        typing(&mut app, "tag:work OR tag:q3");
        assert_eq!(app.shown(), 2);
        assert!(app.error().is_none());
    }

    #[test]
    fn a_queue_is_not_left_behind_without_being_asked_about() {
        let mut app = an_app();
        assert_eq!(app.on_key(key(KeyCode::Char('q'))), Some(Action::Quit));

        mark(&mut app, &["aaaa1111"]);
        tag_with(&mut app, "archive");

        assert_eq!(app.on_key(key(KeyCode::Char('q'))), None);
        assert_eq!(app.mode, Mode::Confirm(What::Quit));
        assert_eq!(app.on_key(key(KeyCode::Char('n'))), None);
        assert_eq!(app.mode, Mode::Browse);
        assert_eq!(app.queue.len(), 1);

        app.on_key(key(KeyCode::Char('q')));
        assert_eq!(
            app.on_key(key(KeyCode::Char('y'))),
            Some(Action::Quit),
            "and saying so leaves"
        );
    }

    #[test]
    fn ctrl_c_still_means_now() {
        let mut app = an_app();
        mark(&mut app, &["aaaa1111"]);
        app.on_key(key(KeyCode::Char('#')));
        typing(&mut app, "+archive");
        app.on_key(key(KeyCode::Enter));

        assert_eq!(app.on_key(ctrl('c')), Some(Action::Quit));
    }

    #[test]
    fn t_leaves_updated_alone_for_as_long_as_it_is_on() {
        let mut app = an_app();
        assert_eq!(app.touch, Touch::Stamp, "stamping is what a change means");

        assert_eq!(app.on_key(key(KeyCode::Char('T'))), None);
        assert_eq!(app.touch, Touch::Keep);

        // Every change made while it is on carries the command's `--no-touch`.
        assert_eq!(
            app.on_key(key(KeyCode::Char('e'))),
            Some(Action::Edit {
                key: "aaaa1111".to_string(),
                touch: Touch::Keep,
            })
        );
        app.on_key(key(KeyCode::Char('#')));
        typing(&mut app, "urgent");
        app.on_key(key(KeyCode::Tab));
        assert_eq!(
            app.on_key(key(KeyCode::Enter)),
            Some(Action::Tag {
                key: "aaaa1111".to_string(),
                changes: vec!["+urgent".to_string()],
                touch: Touch::Keep,
            })
        );

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
        assert_eq!(app.search(), "T");
        assert_eq!(app.touch, Touch::Stamp);

        app.on_key(key(KeyCode::Esc));
        app.on_key(key(KeyCode::Char('m')));
        typing(&mut app, "T");
        assert!(app.input.text().ends_with('T'));
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

        app.on_key(key(KeyCode::Char('a')));
        typing(&mut app, "A trip");
        assert_eq!(app.on_key(key(KeyCode::Esc)), None);
        assert_eq!(app.mode, Mode::Browse);
        assert!(app.input.is_empty());
    }

    #[test]
    fn a_chord_does_not_type_its_own_letter_into_a_prompt() {
        let mut app = an_app();
        app.on_key(key(KeyCode::Char('a')));
        typing(&mut app, "Trip");
        app.on_key(ctrl('r'));
        app.on_key(ctrl('o'));
        assert_eq!(app.input.text(), "Trip");
        assert_eq!(app.mode, Mode::Ask(Ask::Title));
    }

    #[test]
    fn a_title_being_edited_is_edited_and_not_only_added_to() {
        let mut app = an_app();
        app.on_key(key(KeyCode::Char('m')));
        assert_eq!(app.input.text(), "Budget review");
        app.on_key(ctrl('a'));
        app.on_key(ctrl('d'));
        typing(&mut app, "F");
        assert_eq!(app.input.text(), "Fudget review");
        assert_eq!(
            app.on_key(key(KeyCode::Enter)),
            Some(Action::Retitle {
                key: "aaaa1111".to_string(),
                title: "Fudget review".to_string(),
                touch: Touch::Stamp,
            })
        );
    }

    #[test]
    fn the_prompt_takes_a_word_back_and_puts_it_back_again() {
        let mut app = an_app();
        app.on_key(key(KeyCode::Char('a')));
        typing(&mut app, "Budget review");
        app.on_key(ctrl('w'));
        assert_eq!(app.input.text(), "Budget ");
        app.on_key(ctrl('y'));
        assert_eq!(app.input.text(), "Budget review", "and Ctrl-W is undoable");
    }

    #[test]
    fn a_delete_is_asked_about_first() {
        let mut app = an_app();
        assert_eq!(app.on_key(ctrl('d')), None);
        assert_eq!(app.mode, Mode::Confirm(What::Delete));

        // Anything but `y` keeps the note, `d` included.
        assert_eq!(app.on_key(key(KeyCode::Char('d'))), None);
        assert_eq!(app.mode, Mode::Browse);

        app.on_key(ctrl('d'));
        assert_eq!(
            app.on_key(key(KeyCode::Char('y'))),
            Some(Action::Remove("aaaa1111".to_string()))
        );
        assert_eq!(app.mode, Mode::Browse);
    }

    #[test]
    fn the_key_that_used_to_delete_says_where_the_deleting_went() {
        let mut app = an_app();
        assert_eq!(app.on_key(key(KeyCode::Char('d'))), None);
        assert_eq!(app.mode, Mode::Browse);
        assert!(app.queue.is_empty());
        assert_eq!(
            app.message.as_ref().map(Message::line),
            Some("delete is Ctrl-d")
        );
    }

    #[test]
    fn nothing_under_the_cursor_is_nothing_to_change() {
        let mut app = App::new(
            "personal".to_string(),
            PathBuf::from("/notebook"),
            a_session(a_status(), Vec::new()),
        );
        for pressed in ['e', 'm', '#'] {
            assert_eq!(app.on_key(key(KeyCode::Char(pressed))), None);
            assert_eq!(app.mode, Mode::Browse, "`{pressed}` opened something");
        }
        assert_eq!(app.on_key(ctrl('d')), None);
        assert_eq!(app.mode, Mode::Browse, "Ctrl-d opened something");

        // A note can still be made.
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
    fn a_command_that_colours_its_answer_is_quoted_without_the_colour() {
        let mut app = an_app();
        // As `noda status` returns it, painted for `anstream` to strip; a card
        // does not go through `anstream`.
        app.report(Ok(format!(
            "notebook  personal  {}\nnotes     3",
            crate::style::paint(crate::style::MUTED, "(main)")
        )));
        let said = app.message.as_ref().expect("an answer");
        assert_eq!(said.text, "notebook  personal  (main)\nnotes     3");
        assert!(!said.text.contains('\u{1b}'), "{:?}", said.text);
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
        // Both lines: the second says what to do about the first.
        assert!(
            said.text.contains("was left as you saved it"),
            "{}",
            said.text
        );

        app.on_key(key(KeyCode::Char('x')));
        assert_eq!(app.mode, Mode::Browse);
        assert!(app.message.is_none());
    }

    #[test]
    fn a_deleted_note_leaves_the_cursor_where_it_was() {
        let mut app = an_app();
        app.on_key(key(KeyCode::Char('j')));
        assert_eq!(app.selected().map(|f| f.id.as_str()), Some("bbbb2222"));

        // As after a delete: the re-read notebook lacks the note under the cursor.
        app.replace(a_session(
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
        ));
        assert_eq!(
            app.selected().map(|f| f.id.as_str()),
            Some("cccc3333"),
            "the row is kept, so the next note is the one under the cursor"
        );
    }

    /// Adds a tag the notebook lacks through the picker: one `Tab` is a `+`.
    fn tag_with(app: &mut App, tag: &str) {
        app.on_key(key(KeyCode::Char('#')));
        typing(app, tag);
        app.on_key(key(KeyCode::Tab));
        app.on_key(key(KeyCode::Enter));
    }

    /// Marks the notes with `Space`, as a user would.
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

        app.on_key(key(KeyCode::Char('/')));
        typing(&mut app, "tag:q3");
        app.on_key(key(KeyCode::Enter));
        assert_eq!(app.shown(), 1, "the query answers to itself alone");
        assert!(
            app.marked("aaaa1111"),
            "a note the query is hiding is still marked"
        );

        // Marking under a query adds to the earlier marks.
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
        assert!(app.search().is_empty(), "the query goes first");
        assert_eq!(app.marks.len(), 1, "and the marks are still there");

        app.on_key(key(KeyCode::Esc));
        assert!(app.marks.is_empty());
    }

    #[test]
    fn escape_closes_the_note_before_it_touches_the_query() {
        let mut app = an_app();
        app.on_key(key(KeyCode::Char('/')));
        typing(&mut app, "tag:work");
        app.on_key(key(KeyCode::Enter));
        read_it(&mut app, "the q3 budget\n");

        app.on_key(key(KeyCode::Esc));
        assert_eq!(app.depth(), 1);
        assert_eq!(app.search(), "tag:work");

        app.on_key(key(KeyCode::Esc));
        assert!(app.search().is_empty());
    }

    #[test]
    fn with_notes_marked_a_tag_is_queued_over_all_of_them() {
        let mut app = an_app();
        mark(&mut app, &["aaaa1111", "bbbb2222"]);

        // Both carry `work`, so the first `Tab` removes it.
        app.on_key(key(KeyCode::Char('#')));
        assert_eq!(app.choices()[0].tick(2), "[x]");
        app.on_key(key(KeyCode::Tab));
        typing(&mut app, "archive");
        app.on_key(key(KeyCode::Tab));
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
        // Queueing keeps the marks, for the next change to the same set.
        assert_eq!(app.marks.len(), 2);
    }

    #[test]
    fn with_nothing_marked_the_same_key_still_acts_at_once() {
        let mut app = an_app();
        app.on_key(key(KeyCode::Char('#')));
        typing(&mut app, "archive");
        app.on_key(key(KeyCode::Tab));
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

        assert_eq!(app.on_key(ctrl('d')), None);
        assert_eq!(app.mode, Mode::Browse, "queueing a delete deletes nothing");
        assert_eq!(app.queued_deletions(), 2);

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
        tag_with(&mut app, "archive");

        app.on_key(key(KeyCode::Char('Q')));
        assert!(matches!(
            app.on_key(key(KeyCode::Enter)),
            Some(Action::Send(_))
        ));
    }

    #[test]
    fn an_entry_can_be_dropped_from_the_queue() {
        let mut app = an_app();
        mark(&mut app, &["aaaa1111"]);
        for tag in ["one", "two", "three"] {
            tag_with(&mut app, tag);
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
        // Refused where typed, before `cmd::check` at the queue could see it.
        typing(&mut app, "q3,urgent");
        let Some(Proposal::Refused(why)) = app.proposal() else {
            panic!("a comma is not a tag");
        };
        assert!(why.contains("cannot contain"), "{why}");

        app.on_key(key(KeyCode::Tab));
        assert_eq!(app.on_key(key(KeyCode::Enter)), None);
        assert!(app.queue.is_empty());
        assert_eq!(app.mode, Mode::Browse);
    }

    #[test]
    fn sending_spends_the_queue_and_the_marks() {
        let mut app = an_app();
        mark(&mut app, &["aaaa1111"]);
        tag_with(&mut app, "archive");

        app.sent();
        assert!(app.queue.is_empty());
        assert!(app.marks.is_empty());
    }

    #[test]
    fn a_mark_on_a_note_that_has_gone_goes_with_it() {
        let mut app = an_app();
        mark(&mut app, &["aaaa1111", "bbbb2222"]);
        app.replace(a_session(
            a_status(),
            vec![a_note(
                "bbbb2222",
                "meeting-notes",
                "Meeting notes",
                &["work"],
                "agenda",
            )],
        ));
        assert_eq!(app.marks.len(), 1);
        assert!(app.marked("bbbb2222"));
    }

    #[test]
    fn a_note_just_made_can_be_found_by_id() {
        let mut app = an_app();
        assert!(!app.ids().any(|id| id == "dddd4444"));

        // By slug it lands away from the cursor.
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
        app.replace(a_session(a_status(), notes));
        assert_eq!(app.selected().map(|f| f.id.as_str()), Some("aaaa1111"));
        app.select_id("dddd4444");
        assert_eq!(app.selected().map(|f| f.id.as_str()), Some("dddd4444"));
    }

    /// Types a line at the `:` prompt and presses Enter.
    fn command(app: &mut App, line: &str) -> Option<Action> {
        app.on_key(key(KeyCode::Char(':')));
        assert_eq!(app.mode, Mode::Command, "`:` did not open the prompt");
        typing(app, line);
        app.on_key(key(KeyCode::Enter))
    }

    #[test]
    fn a_command_aims_at_the_note_on_screen_when_it_names_none() {
        let mut app = an_app();
        app.on_key(key(KeyCode::Char('j')));
        assert_eq!(
            command(&mut app, "edit"),
            Some(Action::Edit {
                key: "bbbb2222".to_string(),
                touch: Touch::Stamp,
            })
        );
        assert_eq!(app.mode, Mode::Browse);
    }

    #[test]
    fn a_command_can_name_the_note_it_is_aimed_at() {
        let mut app = an_app();
        assert_eq!(
            command(&mut app, "edit cccc3333"),
            Some(Action::Edit {
                key: "cccc3333".to_string(),
                touch: Touch::Stamp,
            })
        );
    }

    #[test]
    fn a_tag_change_is_told_from_a_note_by_the_sign_in_front_of_it() {
        let mut app = an_app();
        assert_eq!(
            command(&mut app, "tag +urgent"),
            Some(Action::Tag {
                key: "aaaa1111".to_string(),
                changes: vec!["+urgent".to_string()],
                touch: Touch::Stamp,
            })
        );
        // A key cannot begin with `+` or `-`, so here the first is a note.
        assert_eq!(
            command(&mut app, "tag cccc3333 -work +q3"),
            Some(Action::Tag {
                key: "cccc3333".to_string(),
                changes: vec!["-work".to_string(), "+q3".to_string()],
                touch: Touch::Stamp,
            })
        );
        // Quoting works here too, with no shell in front of the field.
        assert_eq!(
            command(&mut app, "tag -\"24.04 Dark patterns\""),
            Some(Action::Tag {
                key: "aaaa1111".to_string(),
                changes: vec!["-24.04 Dark patterns".to_string()],
                touch: Touch::Stamp,
            })
        );
    }

    #[test]
    fn a_query_typed_at_the_command_line_keeps_its_quotes() {
        let mut app = App::new(
            "personal".to_string(),
            PathBuf::from("/notebook"),
            a_session(
                a_status(),
                vec![
                    a_note(
                        "aaaa1111",
                        "ubuntu-notes",
                        "Ubuntu notes",
                        &["12.34 foo bar"],
                        "body",
                    ),
                    a_note("bbbb2222", "other-note", "Other note", &["work"], "foo bar"),
                ],
            ),
        );
        assert_eq!(command(&mut app, "notes tag:\"12.34 foo bar\""), None);
        assert_eq!(app.shown(), 1);
        assert_eq!(app.search(), "tag:\"12.34 foo bar\"");
    }

    #[test]
    fn notes_comes_back_up_the_stack_and_narrows_on_the_way() {
        let mut app = an_app();
        read_it(&mut app, "the q3 budget\n");
        assert_eq!(app.depth(), 2);

        command(&mut app, "notes tag:q3");
        assert_eq!(app.depth(), 1);
        assert_eq!(app.shown(), 1);
    }

    #[test]
    fn a_command_that_is_not_one_says_so_on_the_line_and_does_nothing() {
        let mut app = an_app();
        assert_eq!(command(&mut app, "frobnicate the notebook"), None);
        // A line, not a card: nothing ran.
        assert_eq!(app.mode, Mode::Browse);
        let said = app.message.as_ref().expect("a reason");
        assert!(said.failed);
        assert!(said.text.contains("frobnicate"), "{}", said.text);
    }

    #[test]
    fn a_command_that_needs_something_says_what_it_takes() {
        let mut app = an_app();
        assert_eq!(command(&mut app, "open"), None);
        assert!(
            app.message
                .as_ref()
                .is_some_and(|said| said.text.contains("open <note>"))
        );

        assert_eq!(command(&mut app, "doctor --fix"), None);
        assert!(app.message.as_ref().is_some_and(|said| said.failed));
    }

    #[test]
    fn opening_by_name_is_left_to_the_notebook_to_answer() {
        let mut app = an_app();
        assert_eq!(
            command(&mut app, "open meeting-notes"),
            Some(Action::Open("meeting-notes".to_string()))
        );
        assert_eq!(app.depth(), 1, "nothing opens until the notebook answers");
    }

    #[test]
    fn a_command_on_a_note_is_aimed_at_the_note_it_is_on() {
        let mut app = an_app();
        read_it(&mut app, "the q3 budget\n");
        assert_eq!(
            command(&mut app, "mv Budget revision"),
            Some(Action::Retitle {
                key: "aaaa1111".to_string(),
                title: "Budget revision".to_string(),
                touch: Touch::Stamp,
            })
        );
    }

    #[test]
    fn the_delete_command_asks_the_question_the_key_asks() {
        let mut app = an_app();
        assert_eq!(command(&mut app, "rm"), None);
        assert_eq!(app.mode, Mode::Confirm(What::Delete));
        assert_eq!(
            app.on_key(key(KeyCode::Char('y'))),
            Some(Action::Remove("aaaa1111".to_string()))
        );
    }

    #[test]
    fn quitting_by_name_asks_about_the_queue_too() {
        let mut app = an_app();
        mark(&mut app, &["aaaa1111"]);
        tag_with(&mut app, "archive");

        assert_eq!(command(&mut app, "quit"), None);
        assert_eq!(app.mode, Mode::Confirm(What::Quit));
    }

    #[test]
    fn doctor_from_here_reports_and_changes_nothing() {
        let mut app = an_app();
        assert_eq!(
            command(&mut app, "doctor"),
            Some(Action::Run(Run::Doctor {
                links: false,
                times: false,
            }))
        );
        assert_eq!(
            command(&mut app, "doctor --links --times"),
            Some(Action::Run(Run::Doctor {
                links: true,
                times: true,
            }))
        );
    }

    #[test]
    fn the_network_commands_say_what_is_being_waited_for() {
        let mut app = an_app();
        let sync = command(&mut app, "sync").expect("a command to run");
        assert_eq!(sync, Action::Run(Run::Sync));
        assert_eq!(sync.working(), Some("syncing…"));
        assert_eq!(Action::Run(Run::Status).working(), None);
    }

    #[test]
    fn the_prompt_remembers_what_has_been_typed_into_it() {
        let mut app = an_app();
        command(&mut app, "status");
        command(&mut app, "push");

        app.on_key(key(KeyCode::Char(':')));
        app.on_key(key(KeyCode::Up));
        assert_eq!(app.input.text(), "push");
        app.on_key(key(KeyCode::Up));
        assert_eq!(app.input.text(), "status");
        app.on_key(key(KeyCode::Down));
        assert_eq!(app.input.text(), "push");
        // Forward past the newest gives back an empty line.
        app.on_key(key(KeyCode::Down));
        assert!(app.input.is_empty());
    }

    #[test]
    fn a_chord_does_not_type_its_own_letter_into_the_command_line() {
        let mut app = an_app();
        app.on_key(key(KeyCode::Char(':')));
        typing(&mut app, "stat");
        app.on_key(ctrl('d'));
        app.on_key(ctrl('a'));
        assert_eq!(app.input.text(), "stat");
        assert_eq!(app.mode, Mode::Command, "and none of them left the field");
    }

    #[test]
    fn the_command_list_puts_what_you_pick_on_the_prompt() {
        let mut app = an_app();
        app.on_key(ctrl('a'));
        assert_eq!(app.mode, Mode::Commands);

        typing(&mut app, "snap");
        app.on_key(key(KeyCode::Enter));
        // Onto the prompt, not run, with a space as it takes an argument.
        assert_eq!(app.mode, Mode::Command);
        assert_eq!(app.input.text(), "snapshot ");
    }

    #[test]
    fn the_command_list_leaves_nothing_behind_when_it_is_escaped() {
        let mut app = an_app();
        app.on_key(ctrl('a'));
        typing(&mut app, "push");
        app.on_key(key(KeyCode::Esc));
        assert_eq!(app.mode, Mode::Browse);
        assert!(app.input.is_empty());
    }

    #[test]
    fn the_crumbs_say_how_far_down_you_are() {
        let mut app = an_app();
        assert_eq!(app.crumbs().collect::<Vec<_>>(), ["notes"]);

        read_it(&mut app, "the q3 budget\n");
        assert_eq!(app.crumbs().collect::<Vec<_>>(), ["notes", "aaaa1111"]);

        app.on_key(key(KeyCode::Esc));
        assert_eq!(app.crumbs().collect::<Vec<_>>(), ["notes"]);
    }

    /// A notebook whose notes carry checkboxes and links.
    fn a_working_app() -> App {
        App::new(
            "personal".to_string(),
            PathBuf::from("/notebook"),
            a_session(
                a_status(),
                vec![
                    a_note(
                        "aaaa1111",
                        "budget-review",
                        "Budget review",
                        &["work"],
                        "- [ ] chase finance due:2026-08-01\n- [x] done already\n",
                    ),
                    a_note(
                        "bbbb2222",
                        "meeting-notes",
                        "Meeting notes",
                        &["work", "q3"],
                        "see [the budget](aaaa1111-budget-review.md)\n\n- [ ] book a room\n",
                    ),
                    a_note(
                        "cccc3333",
                        "reading-list",
                        "Reading list",
                        &["q3"],
                        "nothing links from here",
                    ),
                ],
            ),
        )
    }

    fn a_commit(hex: &str, seconds: i64, summary: &str) -> Entry {
        Entry {
            id: git2::Oid::from_str(hex).expect("an oid"),
            seconds,
            offset_minutes: 0,
            summary: summary.to_string(),
        }
    }

    #[test]
    fn t_lists_every_unticked_box_soonest_first() {
        let mut app = a_working_app();
        app.on_key(key(KeyCode::Char('t')));
        assert_eq!(app.view(), &View::Todo);

        // The ticked one is not listed, as in `noda todo`.
        let said: Vec<&str> = app
            .tasks()
            .iter()
            .map(|task| task.item.text.as_str())
            .collect();
        assert_eq!(said, vec!["chase finance", "book a room"]);
        // Dated before undated, by `todo::order`.
        assert_eq!(app.tasks()[0].item.due.as_deref(), Some("2026-08-01"));
        assert!(app.tasks()[1].item.due.is_none());
    }

    #[test]
    fn a_row_that_names_a_note_is_the_note_the_keys_aim_at() {
        let mut app = a_working_app();
        app.on_key(key(KeyCode::Char('t')));
        assert_eq!(app.selected().map(|f| f.id.as_str()), Some("aaaa1111"));
        app.on_key(key(KeyCode::Char('j')));
        assert_eq!(app.selected().map(|f| f.id.as_str()), Some("bbbb2222"));

        assert_eq!(
            app.on_key(key(KeyCode::Char('e'))),
            Some(Action::Edit {
                key: "bbbb2222".to_string(),
                touch: Touch::Stamp,
            })
        );

        app.on_key(key(KeyCode::Enter));
        assert_eq!(app.view(), &View::Note("bbbb2222".to_string()));
        assert_eq!(
            app.crumbs().collect::<Vec<_>>(),
            ["notes", "todo", "bbbb2222"]
        );
    }

    #[test]
    fn a_screen_about_the_notebook_has_no_note_for_the_keys_that_need_one() {
        let mut app = a_working_app();
        app.on_key(key(KeyCode::Char(':')));
        typing(&mut app, "tags");
        app.on_key(key(KeyCode::Enter));
        assert_eq!(app.view(), &View::Tags);
        assert!(app.selected().is_none());
        // Not reaching past the screen for the listing's note.
        assert_eq!(app.on_key(key(KeyCode::Char('e'))), None);
        assert_eq!(app.on_key(ctrl('d')), None);
        assert_eq!(app.mode, Mode::Browse);
    }

    #[test]
    fn the_tags_are_counted_commonest_first_and_enter_narrows_the_listing() {
        let mut app = a_working_app();
        app.on_key(key(KeyCode::Char(':')));
        typing(&mut app, "tags");
        app.on_key(key(KeyCode::Enter));

        let counted: Vec<(&str, usize)> = app
            .tallies()
            .iter()
            .map(|tally| (tally.tag.as_str(), tally.notes))
            .collect();
        assert_eq!(counted, vec![("q3", 2), ("work", 2)]);

        app.on_key(key(KeyCode::Enter));
        assert_eq!(app.view(), &View::Notes);
        assert_eq!(app.depth(), 1);
        assert_eq!(app.search(), "tag:q3");
        assert_eq!(app.shown(), 2);
    }

    #[test]
    fn a_tag_with_a_space_in_it_is_quoted_on_its_way_to_the_query() {
        let mut app = App::new(
            "personal".to_string(),
            PathBuf::from("/notebook"),
            a_session(
                a_status(),
                vec![
                    a_note(
                        "aaaa1111",
                        "budget-review",
                        "Budget review",
                        &["24.04 Dark patterns"],
                        "",
                    ),
                    a_note("bbbb2222", "meeting-notes", "Meeting notes", &[], ""),
                ],
            ),
        );
        app.on_key(key(KeyCode::Char(':')));
        typing(&mut app, "tags");
        app.on_key(key(KeyCode::Enter));
        app.on_key(key(KeyCode::Enter));

        // Unquoted this would be three terms and find nothing.
        assert_eq!(app.search(), "tag:\"24.04 Dark patterns\"");
        assert_eq!(app.shown(), 1);
    }

    #[test]
    fn b_shows_what_links_to_the_note_in_front_of_you() {
        let mut app = a_working_app();
        app.on_key(key(KeyCode::Char('b')));
        assert_eq!(
            app.view(),
            &View::Backlinks(Subject::Note("aaaa1111".to_string()))
        );
        let found: Vec<&str> = app
            .linking()
            .iter()
            .filter_map(|&at| app.note_at(at))
            .map(|file| file.id.as_str())
            .collect();
        assert_eq!(found, vec!["bbbb2222"]);

        // A note nothing links to opens on an empty list.
        app.on_key(key(KeyCode::Esc));
        app.on_key(key(KeyCode::Char('G')));
        app.on_key(key(KeyCode::Char('b')));
        assert!(app.linking().is_empty());
        assert!(app.selected().is_none(), "no row, so nothing to aim at");
    }

    #[test]
    fn l_follows_the_screen_the_way_log_itself_does() {
        let mut app = a_working_app();
        app.on_key(key(KeyCode::Char('l')));
        assert_eq!(app.view(), &View::Log(None));
        assert_eq!(app.wanted(), Some(Need::Log(None)));

        app.on_key(key(KeyCode::Esc));
        read_it(&mut app, "the q3 budget\n");
        app.on_key(key(KeyCode::Char('l')));
        assert_eq!(app.view(), &View::Log(Some("aaaa1111".to_string())));
        assert_eq!(app.wanted(), Some(Need::Log(Some("aaaa1111".to_string()))));
    }

    #[test]
    fn a_commit_in_one_notes_history_writes_the_restore_rather_than_running_it() {
        let mut app = a_working_app();
        read_it(&mut app, "the q3 budget\n");
        app.on_key(key(KeyCode::Char('l')));
        supplied(
            &mut app,
            Content::Log(
                vec![
                    a_commit(
                        "1111111111111111111111111111111111111111",
                        1_770_000_000,
                        "edit",
                    ),
                    a_commit(
                        "2222222222222222222222222222222222222222",
                        1_769_000_000,
                        "add",
                    ),
                ],
                std::collections::HashSet::new(),
            ),
        );

        app.on_key(key(KeyCode::Char('j')));
        assert_eq!(app.on_key(key(KeyCode::Enter)), None, "nothing runs yet");
        // On the prompt, waiting for a second Enter.
        assert_eq!(app.mode, Mode::Command);
        assert_eq!(app.input.text(), "restore aaaa1111 2222222");

        assert_eq!(
            app.on_key(key(KeyCode::Enter)),
            Some(Action::Restore {
                key: "aaaa1111".to_string(),
                rev: "2222222".to_string(),
                touch: Touch::Stamp,
            })
        );
    }

    #[test]
    fn the_notebooks_own_log_has_no_note_to_restore() {
        let mut app = a_working_app();
        app.on_key(key(KeyCode::Char('l')));
        supplied(
            &mut app,
            Content::Log(
                vec![a_commit(
                    "1111111111111111111111111111111111111111",
                    1_770_000_000,
                    "edit",
                )],
                std::collections::HashSet::new(),
            ),
        );
        app.on_key(key(KeyCode::Enter));
        assert_eq!(app.mode, Mode::Browse, "there is nothing to put it against");
        assert!(app.input.is_empty());
    }

    #[test]
    fn a_deleted_note_offers_the_revision_restore_has_to_be_given() {
        let mut app = a_working_app();
        app.on_key(key(KeyCode::Char(':')));
        typing(&mut app, "deleted");
        app.on_key(key(KeyCode::Enter));
        assert_eq!(app.wanted(), Some(Need::Deleted));

        supplied(
            &mut app,
            Content::Deleted(vec![Deleted {
                id: "dddd4444".to_string(),
                slug: "trip-plan".to_string(),
                title: "Trip plan".to_string(),
                removed_in: git2::Oid::from_str("3333333333333333333333333333333333333333")
                    .expect("an oid"),
                restore_from: git2::Oid::from_str("4444444444444444444444444444444444444444")
                    .expect("an oid"),
                removed_at: 1_770_000_000,
                offset_minutes: 0,
            }]),
        );
        app.on_key(key(KeyCode::Enter));
        // The commit before the deletion, which is what `restore` wants.
        assert_eq!(app.input.text(), "restore dddd4444 4444444");
    }

    #[test]
    fn a_file_leads_to_what_uses_it() {
        let mut session = a_session(a_status(), vec![]);
        session.files = vec!["diagram.png".to_string()];
        session.notes = vec![a_note(
            "aaaa1111",
            "budget-review",
            "Budget review",
            &[],
            "![the shape of it](diagram.png)\n",
        )];
        let mut app = App::new("personal".to_string(), PathBuf::from("/notebook"), session);

        app.on_key(key(KeyCode::Char(':')));
        typing(&mut app, "files");
        app.on_key(key(KeyCode::Enter));
        assert_eq!(app.files(), ["diagram.png"]);

        app.on_key(key(KeyCode::Enter));
        assert_eq!(
            app.view(),
            &View::Backlinks(Subject::File("diagram.png".to_string()))
        );
        let found: Vec<&str> = app
            .linking()
            .iter()
            .filter_map(|&at| app.note_at(at))
            .map(|file| file.id.as_str())
            .collect();
        assert_eq!(found, vec!["aaaa1111"], "an image counts as a use");
    }

    #[test]
    fn moving_to_another_notebook_waits_for_the_queue() {
        let mut session = a_session(
            a_status(),
            vec![a_note("aaaa1111", "a", "A", &["work"], "")],
        );
        session.notebooks = vec!["personal".to_string(), "work".to_string()];
        let mut app = App::new("personal".to_string(), PathBuf::from("/notebook"), session);

        app.on_key(key(KeyCode::Char(':')));
        typing(&mut app, "notebooks");
        app.on_key(key(KeyCode::Enter));
        // Not the one you are in.
        assert_eq!(app.on_key(key(KeyCode::Enter)), None);
        app.on_key(key(KeyCode::Char('j')));
        assert_eq!(
            app.on_key(key(KeyCode::Enter)),
            Some(Action::Use("work".to_string()))
        );

        app.queue.push(Step {
            keys: vec!["aaaa1111".to_string()],
            change: Change::Remove,
        });
        assert_eq!(app.on_key(key(KeyCode::Enter)), None);
        let said = app.message.as_ref().expect("a refusal");
        assert!(said.failed, "{}", said.text);
        assert!(said.text.contains("personal"), "{}", said.text);
    }

    #[test]
    fn going_back_to_a_screen_lands_where_it_was_left() {
        let mut app = a_working_app();
        app.on_key(key(KeyCode::Char('t')));
        app.on_key(key(KeyCode::Char('j')));
        assert_eq!(app.selected().map(|f| f.id.as_str()), Some("bbbb2222"));

        // The todo list is derived again on the way back; the cursor survives.
        app.on_key(key(KeyCode::Enter));
        assert_eq!(app.view(), &View::Note("bbbb2222".to_string()));
        app.on_key(key(KeyCode::Esc));
        assert_eq!(app.view(), &View::Todo);
        assert_eq!(app.row(), Some(1));
        assert_eq!(app.selected().map(|f| f.id.as_str()), Some("bbbb2222"));
    }

    #[test]
    fn a_screen_about_a_note_that_has_gone_is_closed_like_the_note_itself() {
        let mut app = a_working_app();
        read_it(&mut app, "the q3 budget\n");
        app.on_key(key(KeyCode::Char('B')));
        assert_eq!(app.view(), &View::Blame("aaaa1111".to_string()));
        assert_eq!(app.depth(), 3);

        app.replace(a_session(
            a_status(),
            vec![a_note("cccc3333", "reading-list", "Reading list", &[], "")],
        ));
        assert_eq!(app.depth(), 1);
        assert_eq!(app.view(), &View::Notes);
    }

    #[test]
    fn a_reload_works_the_screen_out_again_rather_than_leaving_it_stale() {
        let mut app = a_working_app();
        app.on_key(key(KeyCode::Char('t')));
        assert_eq!(app.tasks().len(), 2);

        app.replace(a_session(
            a_status(),
            vec![a_note(
                "aaaa1111",
                "budget-review",
                "Budget review",
                &["work"],
                "- [ ] chase finance\n- [ ] and the other thing\n",
            )],
        ));
        assert_eq!(app.view(), &View::Todo);
        assert_eq!(
            app.tasks().len(),
            2,
            "the new notebook's boxes, not the old"
        );
        assert_eq!(app.tasks()[1].item.text, "and the other thing");
    }

    #[test]
    fn an_answer_that_arrives_after_the_reader_has_moved_on_is_dropped() {
        let mut app = a_working_app();
        app.on_key(key(KeyCode::Char('l')));
        let asked = app.view().clone();
        // Escape while the walk is still going.
        app.on_key(key(KeyCode::Esc));

        app.supply(
            &asked,
            Content::Log(
                vec![a_commit(
                    "1111111111111111111111111111111111111111",
                    1_770_000_000,
                    "edit",
                )],
                std::collections::HashSet::new(),
            ),
        );
        assert_eq!(app.view(), &View::Notes);
        assert!(app.entries().is_empty(), "it landed on the wrong screen");
    }

    #[test]
    fn a_named_note_is_resolved_by_the_notebook_and_not_here() {
        let mut app = a_working_app();
        app.on_key(key(KeyCode::Char(':')));
        typing(&mut app, "blame meeting-notes");
        assert_eq!(
            app.on_key(key(KeyCode::Enter)),
            Some(Action::Show {
                key: "meeting-notes".to_string(),
                look: Look::Blame,
            })
        );
        app.look_at(Look::Blame, "bbbb2222".to_string());
        assert_eq!(app.view(), &View::Blame("bbbb2222".to_string()));
    }

    #[test]
    fn restore_takes_a_note_and_a_revision_and_says_so_when_it_has_neither() {
        let mut app = a_working_app();
        app.on_key(key(KeyCode::Char(':')));
        typing(&mut app, "restore aaaa1111");
        assert_eq!(app.on_key(key(KeyCode::Enter)), None);
        let said = app.message.as_ref().expect("a refusal");
        assert!(said.text.contains("<note> <rev>"), "{}", said.text);
    }

    #[test]
    fn a_page_of_text_scrolls_and_a_list_of_rows_does_not() {
        let mut app = a_working_app();
        app.on_key(key(KeyCode::Char('B')));
        supplied(
            &mut app,
            Content::Blame(vec![
                a_blame_line("one"),
                a_blame_line("two"),
                a_blame_line("three"),
            ]),
        );
        assert!(!app.has_rows());
        app.on_key(key(KeyCode::Char('j')));
        assert_eq!(app.scroll(), 1);
        app.on_key(key(KeyCode::Char('G')));
        assert_eq!(app.scroll(), 2, "its last line, and no further");

        app.on_key(key(KeyCode::Esc));
        app.on_key(key(KeyCode::Char('t')));
        assert!(app.has_rows());
        app.on_key(key(KeyCode::Char('j')));
        assert_eq!(app.scroll(), 0, "a list moves a cursor, not a page");
        assert_eq!(app.row(), Some(1));
    }

    /// The note with `created` and `updated` set.
    fn dated(mut file: NoteFile, created: &str, updated: &str) -> NoteFile {
        file.note.created = Some(created.to_string());
        file.note.updated = Some(updated.to_string());
        file
    }

    /// Three notes whose four orders all differ, so a test can tell which is in
    /// force.
    fn a_dated_app() -> App {
        App::new(
            "personal".to_string(),
            PathBuf::from("/notebook"),
            a_session(
                a_status(),
                vec![
                    dated(
                        a_note("aaaa1111", "budget-review", "Zebra", &["work"], ""),
                        "2026-01-01T00:00:00Z",
                        "2026-03-01T00:00:00Z",
                    ),
                    dated(
                        a_note("bbbb2222", "meeting-notes", "Apple", &["work", "q3"], ""),
                        "2026-02-01T00:00:00Z",
                        "2026-01-01T00:00:00Z",
                    ),
                    dated(
                        a_note("cccc3333", "reading-list", "Mango", &["q3"], ""),
                        "2026-03-01T00:00:00Z",
                        "2026-02-01T00:00:00Z",
                    ),
                ],
            ),
        )
    }

    fn listed(app: &App) -> Vec<&str> {
        app.rows().map(|file| file.id.as_str()).collect()
    }

    #[test]
    fn s_walks_the_orders_that_sort_names() {
        let mut app = a_dated_app();
        assert_eq!(app.sort, Sort::Slug);
        assert_eq!(listed(&app), ["aaaa1111", "bbbb2222", "cccc3333"]);

        // Newest first, as `--sort created`.
        app.on_key(key(KeyCode::Char('S')));
        assert_eq!(app.sort, Sort::Created);
        assert_eq!(listed(&app), ["cccc3333", "bbbb2222", "aaaa1111"]);

        app.on_key(key(KeyCode::Char('S')));
        assert_eq!(app.sort, Sort::Updated);
        assert_eq!(listed(&app), ["aaaa1111", "cccc3333", "bbbb2222"]);

        app.on_key(key(KeyCode::Char('S')));
        assert_eq!(app.sort, Sort::Title);
        assert_eq!(listed(&app), ["bbbb2222", "cccc3333", "aaaa1111"]);

        app.on_key(key(KeyCode::Char('S')));
        assert_eq!(app.sort, Sort::Slug, "round to where it started");
    }

    #[test]
    fn r_turns_whichever_order_is_in_force() {
        let mut app = a_dated_app();
        app.on_key(key(KeyCode::Char('R')));
        assert_eq!(listed(&app), ["cccc3333", "bbbb2222", "aaaa1111"]);

        // Kept across a change of order, as `ls -r`.
        app.on_key(key(KeyCode::Char('S')));
        assert_eq!(listed(&app), ["aaaa1111", "bbbb2222", "cccc3333"]);
    }

    #[test]
    fn reordering_keeps_the_cursor_on_the_note_it_was_on() {
        let mut app = a_dated_app();
        app.on_key(key(KeyCode::Char('G')));
        assert_eq!(app.selected().map(|f| f.id.as_str()), Some("cccc3333"));

        app.on_key(key(KeyCode::Char('S')));
        assert_eq!(app.selected().map(|f| f.id.as_str()), Some("cccc3333"));
        assert_eq!(app.row(), Some(0), "it is the newest, so it is first now");
    }

    #[test]
    fn the_order_survives_reading_the_notebook_again() {
        let mut app = a_dated_app();
        app.on_key(key(KeyCode::Char('S')));
        app.on_key(key(KeyCode::Char('R')));
        assert_eq!(listed(&app), ["aaaa1111", "bbbb2222", "cccc3333"]);

        // A read comes back in walk order.
        app.replace(a_session(
            a_status(),
            vec![
                dated(
                    a_note("aaaa1111", "budget-review", "Zebra", &["work"], ""),
                    "2026-01-01T00:00:00Z",
                    "2026-03-01T00:00:00Z",
                ),
                dated(
                    a_note("bbbb2222", "meeting-notes", "Apple", &["work", "q3"], ""),
                    "2026-02-01T00:00:00Z",
                    "2026-01-01T00:00:00Z",
                ),
                dated(
                    a_note("cccc3333", "reading-list", "Mango", &["q3"], ""),
                    "2026-03-01T00:00:00Z",
                    "2026-02-01T00:00:00Z",
                ),
            ],
        ));
        assert_eq!(app.sort, Sort::Created);
        assert!(app.reverse);
        assert_eq!(listed(&app), ["aaaa1111", "bbbb2222", "cccc3333"]);
    }

    #[test]
    fn the_query_narrows_whatever_order_is_in_force() {
        let mut app = a_dated_app();
        app.on_key(key(KeyCode::Char('S')));
        app.on_key(key(KeyCode::Char('/')));
        typing(&mut app, "tag:work");
        assert_eq!(listed(&app), ["bbbb2222", "aaaa1111"], "still newest first");
    }

    #[test]
    fn ctrl_w_is_the_listings_own_density_and_no_other_screens() {
        let mut app = a_dated_app();
        assert!(!app.long);
        app.on_key(ctrl('w'));
        assert!(app.long);
        app.on_key(ctrl('w'));
        assert!(!app.long);

        app.on_key(key(KeyCode::Char('t')));
        app.on_key(ctrl('w'));
        assert!(!app.long, "{:?} has no wide row", app.view());
    }

    #[test]
    fn a_digit_narrows_to_one_of_the_commonest_tags_and_zero_lets_go() {
        let mut app = a_dated_app();
        // Commonest first, ties alphabetical: `q3` and `work` both have two.
        app.on_key(key(KeyCode::Char('1')));
        assert_eq!(app.search(), "tag:q3");
        assert_eq!(app.shown(), 2);

        app.on_key(key(KeyCode::Char('2')));
        assert_eq!(app.search(), "tag:work");

        // Past the end of the list: nothing.
        app.on_key(key(KeyCode::Char('9')));
        assert_eq!(app.search(), "tag:work");

        app.on_key(key(KeyCode::Char('0')));
        assert_eq!(app.search(), "");
        assert_eq!(app.shown(), 3);
    }

    #[test]
    fn a_digit_comes_back_down_to_the_listing_from_wherever_it_is_pressed() {
        let mut app = a_dated_app();
        read_it(&mut app, "the q3 budget\n");
        app.on_key(key(KeyCode::Char('B')));
        assert_eq!(app.depth(), 3);

        app.on_key(key(KeyCode::Char('1')));
        assert_eq!(app.view(), &View::Notes);
        assert_eq!(app.depth(), 1);
        assert_eq!(app.search(), "tag:q3");
    }

    #[test]
    fn the_digits_are_the_numbers_the_tags_screen_puts_beside_them() {
        let mut app = a_dated_app();
        app.on_key(key(KeyCode::Char(':')));
        typing(&mut app, "tags");
        app.on_key(key(KeyCode::Enter));

        let second = app.tallies()[1].tag.clone();
        app.on_key(key(KeyCode::Char('2')));
        assert_eq!(app.search(), format!("tag:{second}"));
    }

    #[test]
    fn ctrl_g_gives_the_crumb_row_back() {
        let mut app = a_dated_app();
        assert!(app.crumbs_shown);
        app.on_key(ctrl('g'));
        assert!(!app.crumbs_shown);
        app.on_key(ctrl('g'));
        assert!(app.crumbs_shown);
    }

    fn a_blame_line(text: &str) -> BlameLine {
        BlameLine {
            commit: git2::Oid::from_str("1111111111111111111111111111111111111111").ok(),
            seconds: 1_770_000_000,
            offset_minutes: 0,
            text: text.to_string(),
        }
    }
}
