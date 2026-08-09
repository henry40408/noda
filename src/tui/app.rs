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
//! A session is a **stack of screens**. The notebook's notes are the one at the
//! bottom and it is never popped; `Enter` opens what the cursor is on as a
//! screen of its own and `Esc` closes it again. Each screen keeps its own
//! cursor, its own query and its own scroll, which is what makes going back
//! land where you left rather than at the top — and what lets a screen be about
//! something other than a list of notes without the one underneath it having to
//! give anything up.
//!
//! The notes are held in memory for the length of the session. `noda search`
//! already reads every body on every invocation, so this is that cost paid once
//! instead of once per query — which is the whole point of typing into a filter
//! rather than into a shell.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::widgets::TableState;

use super::command;
use super::field::{Edit, Field};
use crate::Result;
use crate::cmd::{self, Change, Sort, Step, Touch};
use crate::note;
use crate::notebook::{self, BlameLine, Deleted, Entry, NoteFile, Status};
use crate::query::Query;
use crate::todo;

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
    /// `noda restore`: a note put back the way it was at a revision, as a new
    /// commit. Nothing is rewritten, which is why it is not asked about — and
    /// why it takes two arguments, both typed on purpose.
    Restore {
        key: String,
        rev: String,
        touch: Touch,
    },
    /// Open a note the prompt named, on a screen of its own.
    ///
    /// The key is not resolved here, deliberately. What an id prefix or a slug
    /// names — and what it means for one to name two notes — is `Notebook::
    /// resolve`, and a browser holding the notes in memory could answer it a
    /// second way without noticing it had. So the runtime asks the notebook, and
    /// an ambiguous key comes back as the same refusal the prompt would print.
    Open(String),
    /// Open a screen *about* a note the prompt named by key — its history, who
    /// wrote it, what points at it.
    ///
    /// A second variant rather than a flag on `Open`, and resolved by the
    /// runtime for the same reason `Open` is: the key is not an id until the
    /// notebook has said so.
    Show {
        key: String,
        look: Look,
    },
    /// `noda use`: the whole session moved to another notebook. Not a `Run`,
    /// because what comes back is a different notebook — the runtime builds a
    /// new session rather than reloading this one.
    Use(String),
    /// A command that reads or changes the notebook and answers with a line.
    Run(Run),
}

/// How many tags get a digit of their own.
///
/// Nine, because there are nine digits that are not `0` and `0` is the way back
/// out. A notebook's tags are a long tail with a short head — the handful it
/// actually runs on are worth a keystroke apiece, and the hundred one-offs are
/// what `/` is for.
pub const SCOPE_KEYS: usize = 9;

/// A screen about one note, named before the note is known.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Look {
    Log,
    Blame,
    Backlinks,
}

/// The commands the prompt can ask for that are not about one note.
///
/// One variant apiece rather than a closure, so that what the browser is able to
/// ask for is a list somebody can read — and so the runtime, which is the only
/// part allowed to open a repository, stays the only part that knows how the
/// call is made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Run {
    Status,
    /// Reporting only. `doctor` will adopt files and write to the notebook when
    /// asked to at the prompt; from here it says what it found and stops, because
    /// a browser is not where you find out that a keystroke rewrote a directory.
    Doctor {
        links: bool,
        times: bool,
    },
    Readme,
    /// A name marks the notebook as it stands; no name lists what has been
    /// marked, which is what `noda snapshot` alone already does.
    Snapshot(Option<String>),
    Sync,
    Push,
    Pull,
}

impl Action {
    /// What to say while this runs, for the ones that go to the network.
    ///
    /// The loop draws, waits for a key, then acts — so a command that takes
    /// seconds leaves the last frame on the screen with no sign that anything is
    /// happening. These get a frame of their own first.
    pub fn working(&self) -> Option<&'static str> {
        match self {
            Action::Run(Run::Sync) => Some("syncing…"),
            Action::Run(Run::Push) => Some("pushing…"),
            Action::Run(Run::Pull) => Some("pulling…"),
            _ => None,
        }
    }
}

/// What a command said when it was asked to change something.
///
/// Its own words. A browser that summarised them would be deciding what a
/// command meant, which is the same mistake as writing the change twice.
pub struct Message {
    pub text: String,
    /// Failures get a card and successes get the status line: an acknowledgement
    /// is read in passing, a reason has to be read.
    pub failed: bool,
}

impl Message {
    /// As much of it as one line of the status bar can hold.
    pub fn line(&self) -> &str {
        self.text.lines().next().unwrap_or_default()
    }
}

/// What a `backlinks` screen is about.
///
/// Both kinds, because the question is one question: "which notes use this
/// diagram" and "which notes link to this note" are answered by the same walk,
/// which is why `noda backlinks` takes either.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Subject {
    Note(String),
    File(String),
}

impl Subject {
    /// What it is called, for the band that says what the screen is of.
    pub fn name(&self) -> &str {
        match self {
            Subject::Note(name) | Subject::File(name) => name,
        }
    }
}

/// What a screen is a screen of.
///
/// The name is what the crumb trail shows, so it is the notebook's own word for
/// the thing rather than a heading invented for the browser: a note is named by
/// its id here for the same reason it is named by its id everywhere else, and
/// every other screen is named by the subcommand that prints the same thing at
/// the prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum View {
    /// Every note the notebook holds, narrowed by this screen's own query. The
    /// bottom of the stack, always, and never popped: it is what a session is
    /// open on.
    Notes,
    /// One note, read whole and on its own. Held by id rather than by row, so a
    /// screen survives the listing underneath it being filtered, re-sorted or
    /// read again.
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
    /// What the crumb trail calls it.
    ///
    /// The subcommand's own name, so the trail reads as the path you would have
    /// typed. Which note a screen is about is not in the crumb — it is on the
    /// title band, and repeating it here would make the trail as wide as the
    /// screen on a stack three deep.
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

/// What a screen shows, once somebody has worked it out.
///
/// One at a time, because one screen is on top at a time: keeping the last five
/// would be a cache, and a cache of a notebook that another window is writing to
/// is a cache that goes wrong quietly.
///
/// Some of these the session can work out for itself — every note is in memory
/// already — and some need a repository, which only the runtime may open. Which
/// is which is [`App::derive`] and [`App::wanted`]; the difference does not show
/// on screen.
pub enum Content {
    /// A note's file as it is on disk, read rather than rendered back from the
    /// parse — the reason `noda show` reads it too.
    Note(String),
    Todo(Vec<Task>),
    Tags(Vec<Tally>),
    /// Indices into the session's notes: the ones that link to the subject.
    Backlinks(Vec<usize>),
    Log(Vec<Entry>),
    Blame(Vec<BlameLine>),
    Deleted(Vec<Deleted>),
    /// The patch, with no colour on it. Colour is put back on by the drawing,
    /// which is where every other listing's colour is decided.
    Diff(String),
}

/// One unticked box, and the note carrying it.
pub struct Task {
    /// Which note, as an index into the session's notes. An index rather than an
    /// id because the list is rebuilt whenever the notes are, and never outlives
    /// them.
    pub note: usize,
    pub item: todo::Item,
}

/// One tag, and how many notes carry it.
pub struct Tally {
    pub tag: String,
    pub notes: usize,
}

/// Something a screen needs that only the runtime can get, because getting it
/// means opening the repository.
///
/// Asked for rather than fetched, in the same shape the note's own file has
/// always been asked for: the state says what it wants, the runtime brings it
/// back, and nothing here touches a disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Need {
    Note { id: String, path: PathBuf },
    Log(Option<String>),
    Blame { id: String, slug: String },
    Deleted,
    Diff,
}

/// One screen on the stack: what it is of, and everything about it that a
/// keystroke can move.
///
/// The cursor, the query and the scroll are per-screen rather than per-session
/// on purpose. Going back has to land where you left — a stack that dropped the
/// cursor on the way down would make `Enter` a key you learn not to press — and
/// a screen that is not a list of notes still has to be able to be scrolled
/// without the listing underneath it losing its place.
pub struct Screen {
    pub view: View,
    /// The cursor and the scroll offset of a listing, which ratatui keeps for us.
    pub table: TableState,
    /// How far a screen of text has been scrolled.
    pub scroll: u16,
    /// The query as typed, and where in it the cursor is. Split the way a shell
    /// would split it, it is what `noda search` takes: one token per argument.
    pub search: Field,
    /// The text terms of the active query, for picking the match out of a title
    /// and a body. A `tag:` or an `id:` matched something the prose does not
    /// contain, so there is nothing in the note to point at.
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
    /// Typing a query. The list narrows on every keystroke, so there is no
    /// "run the search" step — `Enter` only puts the keyboard back on the list.
    Search,
    Help,
    /// Typing the one thing a change needs said in words.
    Ask(Ask),
    /// Typing a command. The same line the query and the prompt use, because
    /// only one of the three can be open at a time and a browser with three
    /// places to type would be one you had to look at to find out where your
    /// keystrokes were going.
    Command,
    /// Reading what the prompt accepts, narrowed as you type. The way in for
    /// somebody who knows what they want to do and not what it is called.
    Commands,
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
    /// Leave with a queue still in it. The queue is the one thing a session
    /// holds that is not written down anywhere: a query can be retyped and a
    /// mark remade, but an afternoon's worth of queued changes goes with the
    /// process.
    Quit,
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
    /// What the status line calls the field.
    pub fn prompt(self) -> &'static str {
        match self {
            Ask::Title => "new note",
            Ask::Retitle => "retitle",
            Ask::Tags => "tags",
        }
    }

    /// The part of the answer that cannot be guessed from the prompt, shown at
    /// the other end of the line. Nothing, where the prompt says it all.
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

/// One read of the notebook: everything a session holds that does not depend on
/// which screen is on top.
///
/// Gathered into one value because a reload has to replace all of it at once. A
/// listing rebuilt from new notes while the file list still describes the old
/// notebook is a screen that is half true, and half true is the hardest kind of
/// wrong to notice.
pub struct Session {
    pub status: Status,
    pub notes: Vec<NoteFile>,
    /// What the notebook holds that is not a note, by name — an attachment, a
    /// README, a file parked here on purpose.
    pub files: Vec<String>,
    /// Every notebook there is, for the screen that moves between them.
    pub notebooks: Vec<String>,
    /// Today where this machine is, `YYYY-MM-DD`, for deciding what is overdue.
    /// The local date and not UTC: east of here an item would otherwise stay
    /// unmarked until morning, which is exactly when a todo list is read.
    pub today: String,
}

pub struct App {
    /// The active notebook's name, for the header.
    pub notebook: String,
    /// Its directory, which is what turns a note on a screen into a file for the
    /// runtime to read.
    pub root: PathBuf,
    /// Where the notebook stands, as of the last load. Nothing here touches the
    /// network — the drift is what the last sync left behind, exactly as
    /// `noda status` reports it.
    pub status: Status,
    /// Every note the notebook holds, in the order the walk produced: by slug,
    /// which is what `noda ls` shows without `--sort`.
    notes: Vec<NoteFile>,
    /// What it holds that is not a note, and every notebook there is. Both come
    /// with the read that produced the notes, because both are cheap enough that
    /// fetching them when a screen asks would be a repository opened for a list
    /// of filenames.
    files: Vec<String>,
    notebooks: Vec<String>,
    /// Today, for the todo screen. Taken once per read rather than per frame: a
    /// browser left open overnight is a rarer thing than a clock asked sixty
    /// times a second.
    today: String,
    /// The screens, oldest first. Never empty: the listing at the bottom is what
    /// the session is open on, and popping it would leave nothing to be in.
    stack: Vec<Screen>,
    pub mode: Mode,
    /// What is being typed into the prompt, when there is one. A title, a new
    /// title or a run of tag changes — one field, because only one of them can
    /// be open at a time and [`Mode::Ask`] already says which.
    pub input: Field,
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
    /// Kept on the session rather than on a screen, for the same reason: a
    /// selection outlives the screen it was made on.
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
    /// What order the listing is in, and whether it is running the other way.
    ///
    /// Session settings for the same reason `touch` is one: at a prompt an order
    /// is written on the one `ls` it applies to, and on a screen there is
    /// nothing to write it on. They are the orders `--sort` and `-r` already
    /// name, put in the order `cmd::sort_notes` already puts them.
    pub sort: Sort,
    pub reverse: bool,
    /// Whether the listing shows the whole row — `ls -l`'s columns, which are
    /// the same columns and in the same places.
    pub long: bool,
    /// Whether the crumb trail is drawn. A row of the terminal, given back to
    /// the notes by anyone who would rather have the row.
    pub crumbs_shown: bool,
    /// What the top screen shows, and which screen it was worked out for.
    ///
    /// Kept with its view so that a screen never draws the last screen's answer:
    /// what is loaded is only loaded *for* something, and going somewhere else
    /// makes it stale rather than merely old.
    loaded: Option<(View, Content)>,
    /// The command lines this session has run, oldest first, for the prompt to
    /// walk back through. Kept for the session and no longer: a browser is not a
    /// shell, and a file of everything anyone ever typed into a notebook is a
    /// thing to have to think about rather than a convenience.
    history: Vec<String>,
    /// How far back through it the prompt has been walked. `None` is the line
    /// being typed, which is why walking forward past the end returns to it
    /// rather than to the newest entry.
    history_at: Option<usize>,
    /// Where the cursor is in the list of commands.
    commands_at: usize,
    /// What is being waited for, while it is being waited for.
    pub working: Option<&'static str>,
    /// How many rows the body last had room for, written back by the drawing
    /// code. Half a screen is half of what you can see, and only the drawing
    /// knows how much that is.
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

    /// The listing, wherever the keyboard happens to be. It is the bottom of the
    /// stack and it is never popped, so it is the one screen that can always be
    /// asked about — which is what a mark, a reload and a cursor kept across one
    /// all need.
    fn listing(&self) -> &Screen {
        self.stack.first().expect("a session is never on no screen")
    }

    fn listing_mut(&mut self) -> &mut Screen {
        self.stack
            .first_mut()
            .expect("a session is never on no screen")
    }

    /// Which screen is on top, for the drawing and for the crumb trail.
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

    /// The part of it that is to the left of the cursor, which is what says
    /// where along the line the terminal's cursor belongs.
    pub fn search_before(&self) -> &str {
        self.listing().search.before()
    }

    /// Why the query as typed is not a query yet.
    pub fn error(&self) -> Option<&str> {
        self.listing().error.as_deref()
    }

    /// What to pick out of the prose on the screen in front of you. A screen
    /// opened from a query inherits its terms, so a hit found by searching is
    /// still marked in the note it was found in.
    pub fn terms(&self) -> &[String] {
        &self.top().terms
    }

    /// How far the note on screen has been scrolled.
    pub fn scroll(&self) -> u16 {
        self.top().scroll
    }

    /// Opens a screen on top of the one the keyboard is on.
    ///
    /// The terms come with it. Searching for a word and then opening the note
    /// that matched should not lose the highlighting that said why it matched —
    /// the query is the reason the screen was opened.
    fn open(&mut self, view: View) {
        let terms = self.top().terms.clone();
        self.stack.push(Screen::new(view, terms));
        self.settle();
    }

    /// Closes the top screen, unless it is the listing. `false` when there was
    /// nothing to close, so `Esc` can go on to mean what it means down there.
    fn back(&mut self) -> bool {
        if self.stack.len() > 1 {
            self.stack.pop();
            // What was loaded belonged to the screen that has just gone, so the
            // one underneath has to be worked out again — with its own cursor
            // left where it was, which is the whole reason for coming back.
            self.settle();
            true
        } else {
            false
        }
    }

    /// Works out what the screen on top shows, for the screens whose answer is
    /// already in the session.
    ///
    /// Called wherever the top screen changes — pushed, popped, or left standing
    /// while the notebook underneath it was read again. The screens that need a
    /// repository are not answered here; they ask through [`App::wanted`] and
    /// wait a frame.
    fn settle(&mut self) {
        let view = self.top().view.clone();
        if self.content().is_none()
            && let Some(content) = self.derive(&view)
        {
            self.loaded = Some((view, content));
        }
        // Kept where it was and clamped, rather than sent back to the top: this
        // runs on the way back to a screen as well as on the way in, and a stack
        // that dropped the cursor on the way back would be a stack you learn not
        // to press Escape in. A list that has since got shorter is what the
        // clamp is for, and a list that never needed working out — the files,
        // the notebooks — still needs a cursor put on it.
        let at = self.top().table.selected().unwrap_or(0);
        self.cursor_to(at);
    }

    /// What a screen shows, when the session already holds the answer.
    ///
    /// These three read every note's body, which is why they are worked out when
    /// a screen is opened rather than as the cursor moves: parsing the notebook
    /// per keystroke is what the in-memory copy exists to avoid, not something it
    /// makes affordable.
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
                // `todo::order` and not a comparison written here: `noda todo`
                // prints this list too, and a list that came out in a different
                // order depending on which one you asked would read as a bug in
                // whichever you asked second.
                tasks.sort_by(|left, right| {
                    todo::order(
                        (self.notes[left.note].slug.as_str(), &left.item),
                        (self.notes[right.note].slug.as_str(), &right.item),
                    )
                });
                Some(Content::Todo(tasks))
            }
            View::Tags => {
                let mut counted: BTreeMap<&str, usize> = BTreeMap::new();
                for file in &self.notes {
                    for tag in &file.note.tags {
                        *counted.entry(tag.as_str()).or_default() += 1;
                    }
                }
                let mut tallies: Vec<Tally> = counted
                    .into_iter()
                    .map(|(tag, notes)| Tally {
                        tag: tag.to_string(),
                        notes,
                    })
                    .collect();
                // Commonest first, and alphabetical within a count. A tag list
                // sorted by name alone buries the four tags a notebook actually
                // runs on under every one-off ever typed.
                tallies.sort_by(|left, right| {
                    right
                        .notes
                        .cmp(&left.notes)
                        .then_with(|| left.tag.cmp(&right.tag))
                });
                Some(Content::Tags(tallies))
            }
            // Asked of `notebook` rather than worked out here. What counts as a
            // link — the id in the destination, not the filename, so an answer
            // survives a retitle — is written down once, and this is the same
            // test applied to the notes already in hand instead of to a second
            // walk of the directory.
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

    /// What the screen in front of you needs that only the runtime can get.
    ///
    /// `None` on every frame but the one after a screen is opened, which is what
    /// keeps a repository from being opened per keystroke.
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

    /// Takes what the runtime went and got.
    ///
    /// Dropped when the screen it was for is no longer the one in front of you:
    /// a blame of two thousand commits takes long enough to press Escape during,
    /// and the answer to a question nobody is asking any more must not land on
    /// whatever screen happens to be there instead.
    pub fn supply(&mut self, view: &View, content: Content) {
        if self.top().view != *view {
            return;
        }
        self.loaded = Some((view.clone(), content));
        self.top_mut().scroll = 0;
        let at = self.top().table.selected().unwrap_or(0);
        self.cursor_to(at);
    }

    /// Closes the screen a command could not fill. Public because only the
    /// runtime finds out that it could not: a `blame` of a note whose file has
    /// gone is a screen with nothing to be about, and leaving it up would leave
    /// the reason for the empty screen on a card over the top of it.
    pub fn give_up(&mut self) {
        self.back();
    }

    /// A note by id, for the screens that are about one.
    pub fn note_of(&self, id: &str) -> Option<&NoteFile> {
        self.notes.iter().find(|file| file.id == id)
    }

    /// Swaps in a freshly read notebook, keeping the query and — when the note
    /// is still there — the cursor. A reload that jumped back to the top would
    /// be a reason not to press the key.
    ///
    /// When it is not there, the row it was on is what is kept instead. That is
    /// the case a delete makes ordinary: removing the fortieth note of two
    /// hundred and being returned to the first would be a reason not to press
    /// that key either.
    pub fn replace(&mut self, session: Session) {
        let was = self.at_cursor().map(|file| file.id.clone());
        let row = self.listing().table.selected();
        self.status = session.status;
        self.notes = session.notes;
        self.files = session.files;
        self.notebooks = session.notebooks;
        self.today = session.today;
        // Before the query is rerun, because the query picks out indices into
        // this list. Reapplied rather than remembered: a read brings the
        // notebook back in the walk's own order, and an order that came off
        // every time you pressed `r` would not be a setting.
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
        // Dropped rather than kept: what is on disk is what changed, and this
        // copy of it is what the reload was pressed to get rid of.
        self.loaded = None;
        // A screen about a note the notebook no longer holds is a screen with
        // nothing behind it — which is the ordinary end of deleting the note you
        // are reading. Taken off the stack rather than left showing the last
        // thing that was true, and the same goes for every screen that is about
        // one note rather than about the notebook.
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
        // A mark on a note that is no longer there is a change aimed at nothing.
        // The queue keeps its own copy of the ids on purpose — an entry is a
        // record of what was asked for, and `bulk` is what says so if one of
        // them has since gone.
        let ids: BTreeSet<&str> = self.notes.iter().map(|file| file.id.as_str()).collect();
        self.marks.retain(|id| ids.contains(id.as_str()));
        // Whatever screen the reload left you on is now describing a notebook
        // that has been read again, so it is worked out again — and the ones
        // that need a repository ask for themselves on the next frame.
        self.settle();
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

    /// The note the listing's cursor is on, whatever screen is on top of it.
    fn at_cursor(&self) -> Option<&NoteFile> {
        let listing = self.listing();
        let at = listing.table.selected()?;
        self.notes.get(*listing.visible.get(at)?)
    }

    /// The note the screen in front of you is about.
    ///
    /// On the listing that is the row under the cursor; on a note it is that
    /// note. One question with one answer on every screen, which is what lets
    /// `e`, `m`, `#` and `Ctrl-d` mean the same thing everywhere without any of
    /// them knowing where they are.
    ///
    /// A screen whose rows are notes answers with the row — the todo list and
    /// the backlinks are listings of notes, and a key that changes a note should
    /// change the one under the cursor there for the same reason it does on the
    /// listing. A screen *about* one note answers with that note however far its
    /// own rows have been scrolled. A screen about the notebook answers with
    /// nothing, and the keys that need a note quietly do nothing.
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

    /// The unticked boxes, the tags, and the notes that link here — whichever of
    /// them the screen in front of you is showing, and nothing when it is
    /// showing something else.
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
            Some(Content::Log(entries)) => entries,
            _ => &[],
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

    /// The note's file, or the patch — the two screens that are a block of text
    /// rather than a list of anything.
    pub fn text(&self) -> Option<&str> {
        match self.content() {
            Some(Content::Note(text) | Content::Diff(text)) => Some(text),
            _ => None,
        }
    }

    /// A note by index, for the rows that name one.
    pub fn note_at(&self, at: usize) -> Option<&NoteFile> {
        self.notes.get(at)
    }

    /// What the notebook holds that is not a note.
    pub fn files(&self) -> &[String] {
        &self.files
    }

    /// Every notebook there is.
    pub fn notebooks(&self) -> &[String] {
        &self.notebooks
    }

    /// Today, for deciding which due dates have been missed.
    pub fn today(&self) -> &str {
        &self.today
    }

    /// How many rows the screen in front of you has, or `None` when it is a
    /// block of text and the keys scroll it instead of moving a cursor.
    ///
    /// The one place that says which kind a screen is. Every key that moves
    /// asks here rather than matching on the view itself, so a screen added
    /// later cannot be a list on one key and a page on another.
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

    /// Whether the screen in front of you is a list with a cursor in it.
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

    /// The cursor and scroll of the screen being drawn, taken out so the rows
    /// may borrow the notes while ratatui writes this frame's offset into it.
    /// They are different fields, but the borrow checker only sees `self`.
    pub fn take_table(&mut self) -> TableState {
        std::mem::take(&mut self.top_mut().table)
    }

    pub fn put_table(&mut self, state: TableState) {
        self.top_mut().table = state;
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
        // the terminal itself would have meant by it — and the one that does not
        // stop to ask about an unsent queue, for the same reason. A program that
        // argues with Ctrl-C is a program you end up killing from another
        // window; `q` is the key that can afford to ask.
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
            Mode::Command => self.commanding(key),
            Mode::Commands => self.listing_commands(key),
            Mode::Confirm(what) => self.confirming(what, key),
            Mode::Queue => self.queueing(key),
            Mode::Browse => self.browsing(key),
        }
    }

    fn browsing(&mut self, key: KeyEvent) -> Option<Action> {
        // Whatever the last command said has now been read, or has not been and
        // was only an acknowledgement. Either way the next key is the reader
        // moving on, and the line goes back to being empty.
        self.message = None;
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            return self.chord(key.code);
        }
        // The keys that mean the same thing on every screen. They are answered
        // first so that a screen only has to describe what is different about
        // it, and so that a key cannot come to mean two things by being added to
        // one screen and forgotten on another.
        match key.code {
            KeyCode::Char('q') => return self.leaving(),
            KeyCode::Char('r') => return Some(Action::Reload),
            KeyCode::Char('?') => {
                self.mode = Mode::Help;
                return None;
            }
            // The way to the commands a key cannot reach. There are thirty
            // subcommands and about a dozen letters worth spending on them, so
            // the rest arrive by being named.
            KeyCode::Char(':') => {
                self.mode = Mode::Command;
                self.input.clear();
                self.history_at = None;
                return None;
            }
            // Not a change of its own — a change to what the next one records.
            KeyCode::Char('T') => {
                self.touch = match self.touch {
                    Touch::Stamp => Touch::Keep,
                    Touch::Keep => Touch::Stamp,
                };
                return None;
            }
            // The keys that change a note. Each asks a command for it, and the
            // three that need something said first open the prompt instead.
            // They aim at whatever the screen is about, so they read the same on
            // a listing and on the note itself.
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
            KeyCode::Char('#') => {
                // Nothing to tag is nothing to ask about — the same reason `m`
                // and a delete do nothing over an empty list.
                if self.marks.is_empty() {
                    self.selected()?;
                }
                self.ask(Ask::Tags, String::new());
                return None;
            }
            KeyCode::Char('Q') => {
                self.mode = Mode::Queue;
                self.queue_at = self.queue_at.min(self.queue.len().saturating_sub(1));
                return None;
            }
            // The four screens worth a letter, and the rule for which four: the
            // three that are *about the note in front of you* — where you would
            // otherwise have to name a note you are already looking at — and the
            // one list of the notebook that is read as often as the notes
            // themselves. The other five arrive by being named, which is what
            // `:` is for.
            KeyCode::Char('t') => {
                self.open(View::Todo);
                return None;
            }
            // The whole notebook's, from a screen that is about the notebook;
            // one note's, from a screen that is about a note. `:log` reads the
            // screen the same way, so the key and the name never disagree.
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
            // The nine commonest tags, one press apiece, and `0` for the way
            // back out. The short version of what the tags screen's `enter`
            // does, which is why that screen numbers its first nine rows: the
            // number beside a tag there is the key that reaches it.
            //
            // Answered on every screen, because the answer is a screen: like
            // `:notes` it comes back down to the listing, which is where the
            // notes a tag narrows already are.
            KeyCode::Char(digit @ '0'..='9') => {
                self.scope_nth(digit as usize - '0' as usize);
                return None;
            }
            // Deliberately not a delete, and deliberately not anything else
            // either. This key removed a note until the modifier was put in
            // front of it, and a key that used to delete must not quietly become
            // the key that does something else instead — it has to say where the
            // deleting went and do nothing at all.
            KeyCode::Char('d') => {
                self.message = Some(Message {
                    text: "delete is Ctrl-d".to_string(),
                    failed: false,
                });
                return None;
            }
            _ => {}
        }
        // Which kind of screen this is, rather than which screen: a list is
        // walked with a cursor and a page of text is scrolled, and everything
        // else about them is the same.
        match (&self.top().view, self.has_rows()) {
            (View::Notes, _) => self.on_listing(key),
            (_, true) => self.on_rows(key),
            (_, false) => self.on_reading(key),
        }
    }

    /// The chords, which mean the same thing on every screen: half a screen
    /// either way, and the delete.
    ///
    /// A chord and not a letter, for the delete. It is the one key here that
    /// cannot be taken back by pressing something else, and one modifier is the
    /// difference between a key you reach for and a key you mean.
    fn chord(&mut self, code: KeyCode) -> Option<Action> {
        let half = i32::from(self.page.max(2) / 2);
        match code {
            KeyCode::Char('f') => self.step(half),
            KeyCode::Char('b') => self.step(-half),
            KeyCode::Char('d') => return self.delete(),
            // What the prompt accepts, for somebody who knows what they want to
            // do and not what it is called. A card and not a key apiece: the
            // list is the point at which there are too many to have keys.
            KeyCode::Char('a') => {
                self.mode = Mode::Commands;
                self.input.clear();
                self.commands_at = 0;
            }
            // `ls -l`'s columns, and only where there are rows to put them on.
            // The other screens print what their own command prints and have no
            // second density to offer.
            KeyCode::Char('w') => {
                if matches!(self.top().view, View::Notes) {
                    self.long = !self.long;
                }
            }
            // A row of the terminal, given back to the notes. The trail is worth
            // a line most of the time — a stack you cannot see the depth of is
            // one whose Escape key you have to guess at — but on a short
            // terminal a row is a row, and the title band still says what screen
            // you are on.
            KeyCode::Char('g') => self.crumbs_shown = !self.crumbs_shown,
            _ => {}
        }
        None
    }

    /// The delete, aimed the way every change is aimed: at the marked notes when
    /// there are any, and at the one on the screen when there are not.
    fn delete(&mut self) -> Option<Action> {
        if self.marks.is_empty() {
            self.selected()?;
            self.mode = Mode::Confirm(What::Delete);
        } else {
            // Not asked about here: queueing a delete deletes nothing, and the
            // question belongs at the send, where it is the last moment it can
            // still be answered no.
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
            // The orders `--sort` names, one press apiece, and the reverse `-r`
            // already spells. Shifted because they are about the listing rather
            // than about a note, and the unshifted letters near them are not:
            // `r` reads the notebook again and `s` is not a key at all.
            KeyCode::Char('S') => {
                self.sort = self.sort.next();
                self.reorder();
            }
            KeyCode::Char('R') => {
                self.reverse = !self.reverse;
                self.reorder();
            }
            // Down the stack. The note gets a screen of its own rather than half
            // of this one, which is what lets it be read at the width it was
            // written at.
            KeyCode::Enter => {
                let id = self.selected()?.id.clone();
                self.open(View::Note(id));
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
            // The way back out of whatever narrowing is in force, one layer at a
            // time: the query first, and once there is no query, the marks. Two
            // things that both make the notebook smaller than it is, undone by
            // the key that already meant "never mind" — and in that order,
            // because a query is cheap to retype and a selection is not.
            //
            // The queue is not in this chain. Dropping work by pressing Escape
            // once too often is the sort of thing a queue exists to prevent.
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

    /// A page of text — a note, a patch, a note with its history down the left:
    /// scrolling it, and closing it again.
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

    /// Any of the other lists: walking it, closing it, and doing whatever the
    /// row under the cursor is for.
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

    /// What the row under the cursor is for.
    ///
    /// Every one of these is the obvious next question about the thing on the
    /// row, and they divide into three kinds. A row that names a note opens it.
    /// A row that names something to look at from another angle opens that
    /// angle. And a row that names something *irreversible* puts the command on
    /// the prompt instead of running it — the same bargain `Ctrl-a` makes, and
    /// for a stronger reason here: landing on a row is not agreeing to overwrite
    /// the note it names.
    fn chose(&mut self) -> Option<Action> {
        let at = self.top().table.selected()?;
        match self.top().view.clone() {
            View::Todo | View::Backlinks(_) => {
                let id = self.selected()?.id.clone();
                self.open(View::Note(id));
            }
            // Not a screen of its own: a tag is a way of narrowing the listing,
            // and the listing is where the notes it narrows already live. This
            // is the key `0`–`9` will be short for.
            View::Tags => {
                let tag = self.tallies().get(at)?.tag.clone();
                self.scope(&tag);
            }
            // The one question worth asking about an attachment, and the reason
            // `backlinks` takes a file as readily as a note.
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
            // A commit in one note's history is a version of that note, so the
            // row is the revision `restore` wants. The notebook's own log names
            // no note, so there is nothing to put the commit against.
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

    /// Puts the notes in the order the session has asked for.
    ///
    /// `cmd::sort_notes` and not a comparison written here: `noda ls --sort`
    /// offers the same four orders, and an order that came out differently
    /// depending on which one you asked would be two features wearing one name.
    /// The reverse is applied after, here as it is there, so every order gets
    /// one for free.
    fn arrange(&mut self) {
        cmd::sort_notes(&mut self.notes, self.sort);
        if self.reverse {
            self.notes.reverse();
        }
    }

    /// Reorders the listing under the cursor, and leaves the cursor on the note
    /// it was on.
    ///
    /// The note and not the row: re-sorting is asking where a particular note
    /// falls in a new order, and being thrown to the top to find out would be a
    /// reason not to press the key.
    fn reorder(&mut self) {
        let was = self.at_cursor().map(|file| file.id.clone());
        self.arrange();
        self.refilter();
        if let Some(id) = was {
            self.select_id(&id);
        }
        // Every derived screen holds indices into the notes, and the notes have
        // just moved underneath them.
        self.loaded = None;
        self.settle();
    }

    /// The tag a digit stands for: one of the commonest, in the order the tags
    /// screen lists them, so the number beside a tag there is the key that
    /// reaches it. `0` is the way back out.
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

    /// Back to the listing, narrowed to one tag.
    ///
    /// Quoted when the tag has a space in it, because the listing's query field
    /// splits the way a shell would — `tag:24.04 Dark patterns` is three terms
    /// and-ed together, and would find nothing at all.
    fn scope(&mut self, tag: &str) {
        while self.back() {}
        let query = if tag.contains(char::is_whitespace) {
            format!("tag:\"{tag}\"")
        } else {
            format!("tag:{tag}")
        };
        self.top_mut().search.set(query);
        self.refilter();
    }

    /// Moves the whole session to another notebook, or says why not.
    ///
    /// The queue is what stands in the way, and it has to: an entry names notes
    /// by id, and ids belong to the notebook they were minted in. Sent against
    /// another one it would find nothing, or — with two notebooks imported from
    /// the same place — find the wrong thing. Refused rather than dropped,
    /// because the queue is the one piece of a session that is written down
    /// nowhere else.
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
            // A plain `d` here, unlike out on a screen, because nothing in this
            // card can delete a note — dropping a queued change is the opposite
            // of one.
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
        // Typing, and every key readline puts around a line, are the field's
        // business — including the rule the field was built around: a chord is
        // not a character. `KeyCode::Char('d')` is what arrives for Ctrl-D as
        // much as for `d`, and a query field that took it at face value would
        // quietly read `budgetd`.
        //
        // Only the query's own keys are left here, and they are the ones the
        // field deliberately does not bind. Re-running the query is this end's
        // job because only this end knows there is a notebook behind the line.
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
            // The list is already narrowed, so there is nothing left to run:
            // this only hands the keyboard back to the list.
            KeyCode::Enter => self.mode = Mode::Browse,
            // Leaving the query behind means leaving what it selected behind
            // too, which is what makes it an escape rather than a commit.
            KeyCode::Esc => {
                self.top_mut().search.clear();
                self.refilter();
                self.mode = Mode::Browse;
            }
            // The cursor still moves while the query is being typed, so the
            // listing can be walked without stopping to leave the field.
            //
            // Ctrl-N and Ctrl-P do the same, which is the one place the browser
            // parts company with readline: there they walk a history this field
            // does not have, and a query field is walked the way every other
            // narrowing list in a terminal is walked. Only an unbound chord can
            // arrive here as a `Char`, but the modifier is asked for anyway —
            // that invariant belongs to another module and should not be what a
            // plain `n` depends on.
            KeyCode::Down => self.step(1),
            KeyCode::Up => self.step(-1),
            KeyCode::Char('n') if ctrl => self.step(1),
            KeyCode::Char('p') if ctrl => self.step(-1),
            _ => {}
        }
        None
    }

    /// Opens the prompt, with whatever it should start out holding — the current
    /// title, for a retitle, so the common edit is a few keystrokes rather than
    /// typing it all again.
    fn ask(&mut self, what: Ask, start: String) {
        self.mode = Mode::Ask(what);
        self.input.set(start);
    }

    fn asking(&mut self, what: Ask, key: KeyEvent) -> Option<Action> {
        // The same field the query is typed into, so the same keys work in it —
        // which matters most here, where the line often arrives already written:
        // a retitle is somebody editing a title rather than typing one, and
        // editing is what a cursor is for.
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

    /// Turns a filled-in prompt into the command it stands for.
    ///
    /// An empty answer is a way out rather than an error: somebody who has
    /// cleared the field has changed their mind, and reporting "a note needs a
    /// title" at them would be answering a question they stopped asking. The one
    /// exception is a new note, where nothing typed is what `noda add` already
    /// means by no title — the body will say what the note is called.
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
            // The one prompt that answers to the marks: with a set picked out,
            // the tags it was given are queued against that set rather than run
            // against the note the screen happens to be about.
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

    /// Leaving, or asking about the queue first. The one piece of state a
    /// session holds that is written down nowhere.
    fn leaving(&mut self) -> Option<Action> {
        if self.queue.is_empty() {
            return Some(Action::Quit);
        }
        self.mode = Mode::Confirm(What::Quit);
        None
    }

    /// Typing a command.
    ///
    /// The same field the query and the prompt use, and so the same keys: this
    /// is the one of the three that is most like a shell prompt, and the one
    /// where a hand that has typed at shell prompts for twenty years is most
    /// likely to reach for `Ctrl-A` without deciding to.
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
            // What a shell does with the same keys, and for the same reason: the
            // command you want next is usually one you have already typed. Both
            // spellings of them, because readline has two.
            KeyCode::Up => self.recall(true),
            KeyCode::Down => self.recall(false),
            KeyCode::Char('p') if ctrl => self.recall(true),
            KeyCode::Char('n') if ctrl => self.recall(false),
            _ => {}
        }
        None
    }

    /// Walks the history. Forward past the newest entry gives back an empty
    /// line rather than sticking on it — walking away from what you were typing
    /// is what discarded it, and stopping dead at the end would leave no way to
    /// type something new without clearing the field by hand.
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

    /// Turns a typed line into the command it names.
    ///
    /// The rest of the line is kept as it was typed rather than rebuilt from its
    /// tokens: a query and a title both survive quoting only if nothing puts
    /// them back together, and `tag:"12.34 foo bar"` is exactly the case that
    /// has already been got wrong twice elsewhere.
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
        let args = split_quoted(rest);

        match spec.name {
            "quit" => return self.leaving(),
            "reload" => return Some(Action::Reload),
            "keys" => self.mode = Mode::Help,
            // Back to the listing from wherever you are, and narrowed on the way
            // if a query came with it.
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
            // No note may be named: a title is free text, so the first word of
            // one cannot be told from a key. The note on screen is the one that
            // gets retitled, and `open` is how you put another one there.
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
                // A key cannot begin with `+` or `-`, so a first argument that
                // does is a change rather than a note. That is the whole of the
                // ambiguity, and the notation settles it.
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
            // The same question the key asks, and aimed the same way — at the
            // marked set when there is one, and at the note on screen when there
            // is not.
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
            // The screens. Each is the subcommand's own name, showing what that
            // subcommand prints — so `:` is one vocabulary and not two.
            "todo" => self.open(View::Todo),
            "tags" => self.open(View::Tags),
            "files" => self.open(View::Files),
            "notebooks" => self.open(View::Notebooks),
            "deleted" => self.open(View::Deleted),
            "diff" => self.open(View::Diff),
            // The three that can be about a note somewhere else in the notebook.
            // A key is not an id until the notebook has said so, so a named one
            // goes back out to the runtime to be resolved — the same route
            // `open` takes, and for the same reason.
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
            // A file's backlinks are reached from the files screen rather than
            // by name. `noda backlinks` has to tell a note from a file because
            // it is handed a bare word; here the screen has already said which,
            // and a browser guessing between them would be inventing an
            // ambiguity it does not have.
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

    /// What the screen in front of you is about, for the one command that is
    /// happy with either answer.
    ///
    /// `log` is the only thing here that reads a note and the notebook alike, so
    /// it is the only thing that can follow the screen this way: on a note it is
    /// that note's history, and on any screen about the notebook as a whole it
    /// is the notebook's. Everything else needs a note and says so.
    fn about(&self) -> Option<String> {
        match &self.top().view {
            View::Note(id)
            | View::Blame(id)
            | View::Log(Some(id))
            | View::Backlinks(Subject::Note(id)) => Some(id.clone()),
            _ => None,
        }
    }

    /// What `backlinks` is about here: the file under the cursor when the files
    /// are what is on screen, and otherwise the note every other key aims at.
    fn linkable(&mut self) -> Option<Subject> {
        if matches!(self.top().view, View::Files)
            && let Some(at) = self.top().table.selected()
            && let Some(name) = self.files.get(at)
        {
            return Some(Subject::File(name.clone()));
        }
        Some(Subject::Note(self.aimed(None)?))
    }

    /// What the command takes, said back at somebody who did not give it.
    fn wants(&mut self, spec: &command::Spec) -> Option<Action> {
        self.refuse(format!("{} — {}", spec.usage(), spec.what));
        None
    }

    /// Why a line was not a command, on the line it was typed on.
    ///
    /// Not a card. A card is for what a command said when it ran; this is a
    /// sentence that never became one, which is the same class of thing as a
    /// query that is not a query yet — and that already lives here.
    fn refuse(&mut self, text: String) {
        self.message = Some(Message { text, failed: true });
    }

    fn remember(&mut self, line: &str) {
        if self.history.last().map(String::as_str) == Some(line) {
            return;
        }
        self.history.push(line.to_string());
    }

    /// Reading what the prompt accepts, narrowed as you type.
    fn listing_commands(&mut self, key: KeyEvent) -> Option<Action> {
        // Erasing only, and typing. What is being typed here is shown along the
        // top of the card and nowhere else, so there is no cursor on the screen
        // for the motion keys to move — see `Field::erasing`. The keys that
        // shorten the line from the end still work, because those are the ones
        // this list is actually narrowed with.
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
            // The other spelling of the same two, for the same hands the rest of
            // this is for.
            KeyCode::Char('n') if ctrl => {
                self.commands_at = (self.commands_at + 1).min(shown.saturating_sub(1));
            }
            KeyCode::Char('p') if ctrl => {
                self.commands_at = self.commands_at.saturating_sub(1);
            }
            // Onto the prompt rather than run outright. Most of them take
            // something, and a list that ran them blind would be no use for the
            // half that do — and `push` is not a thing to set off by landing on
            // it and pressing Enter.
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

    /// Which row the command list has its cursor on.
    pub fn commands_at(&self) -> usize {
        self.commands_at
    }

    /// Opens a note the runtime has resolved, on a screen of its own.
    ///
    /// The id comes from `Notebook::resolve` rather than from anything here, so
    /// that what a key names is answered in one place.
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
            What::Quit => Some(Action::Quit),
        }
    }

    /// Takes down what a command said, in its own words.
    ///
    /// Public because the runtime is what ran the command: the state asked for
    /// it and has no way of finding out how it went.
    pub fn report(&mut self, outcome: Result<String>) {
        self.message = Some(match outcome {
            Ok(text) => {
                let text = plain(&text).trim_end().to_string();
                // A line is an acknowledgement and lives on the status bar. More
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
                // that to the width of one line would be losing the part that
                // says what to do next.
                self.mode = Mode::Alert;
                Message {
                    text: plain(&e.to_string()),
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
        if let Some(at) = self
            .listing()
            .visible
            .iter()
            .position(|&i| self.notes[i].id == id)
        {
            self.select(at);
        }
    }

    /// Moves the cursor, or scrolls the page, by `delta` rows — whichever the
    /// screen in front of you has.
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

    /// How far the page may be scrolled: its last line, so its end can be
    /// brought to the top of the screen and no further.
    ///
    /// Counted before wrapping, so a note of long lines can be scrolled less far
    /// than it is tall. Under-shooting leaves text reachable; over-shooting
    /// would scroll into blank space, which reads like a bug.
    fn reading_height(&self) -> u16 {
        let lines = match self.content() {
            Some(Content::Note(text) | Content::Diff(text)) => text.lines().count(),
            Some(Content::Blame(lines)) => lines.len(),
            _ => 0,
        };
        lines.saturating_sub(1) as u16
    }

    /// Puts the cursor on a row of the screen in front of you, clamped to what
    /// it has. A list with nothing in it has no cursor at all, which is what the
    /// drawing needs to know not to highlight a row that is not there.
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

    /// Reruns the query over every note.
    ///
    /// Whole rather than incremental: a keystroke can widen a query as easily as
    /// narrow it (a backspace, or an `OR` completed), so there is no subset to
    /// refine. At `noda ls` speeds the notebook is walked in memory in well
    /// under a frame.
    ///
    /// Split the way the shell would split it, quotes and all. `Query::parse`
    /// takes one token per argument precisely so that the shell's quoting is the
    /// only quoting there is — and there is no shell in front of this field, so
    /// it does that part itself. Without it `tag:"12.34 foo bar"` has no
    /// spelling here at all: a value with a space in it becomes three terms
    /// and-ed together, and a tag you can read in the listing is one you cannot
    /// filter by on the screen it is on. Same reasoning as the tags prompt.
    fn refilter(&mut self) {
        let tokens = split_quoted(self.listing().search.text());

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
            // Half a query is the ordinary state of one being typed: `tag:`
            // before its value, `budget OR` before its alternative. Say so, and
            // leave the last good result under the cursor — emptying the list at
            // every other keystroke would make the query harder to type, which
            // is the opposite of what filtering as you go is for.
            Err(e) => self.listing_mut().error = Some(e.to_string()),
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

/// A command's answer with its colour taken out.
///
/// `style` paints unconditionally and leaves it to `anstream` to strip the
/// escapes when what is on the other end of the pipe is not a terminal. Here the
/// other end *is* a terminal — but not a stream: the answer is put on a card a
/// character at a time, so an escape arrives as the text `[2m` and is drawn as
/// one. Stripping here is that same decision made at the other consumer, and it
/// is done where every answer passes rather than where each one is shown.
///
/// It also matters for the size of the card: it is measured from its longest
/// line, and escapes that draw nothing would have been counted.
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

    /// Opens the note under the cursor and gives it something to show, which is
    /// what the runtime does between the keystroke and the frame.
    fn read_it(app: &mut App, text: &str) {
        app.on_key(key(KeyCode::Enter));
        let view = app.view().clone();
        assert!(
            matches!(app.wanted(), Some(Need::Note { .. })),
            "a note screen wants its file"
        );
        app.supply(&view, Content::Note(text.to_string()));
    }

    /// What the runtime brings back for a screen that needs a repository, put
    /// straight into the state the way `refresh` puts it.
    fn supplied(app: &mut App, content: Content) {
        let view = app.view().clone();
        assert!(app.wanted().is_some(), "{view:?} asked for nothing");
        app.supply(&view, content);
    }

    /// A read of a notebook holding nothing but these notes: no attachments, one
    /// notebook, and a fixed date so "overdue" means the same thing every run.
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
        assert!(app.error().is_none());

        // `OR` with nothing after it yet: the state every alternative passes
        // through on its way to being one.
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
        // And there is nothing to open, so nothing to read.
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
        // The file the runtime should read, named the way the notebook names it.
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
        // And it landed back on the note it was opened from, not at the top.
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

        // Opening the note that matched must not lose the reason it matched.
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

        // Another note in the same screen: a fresh file starts where it starts.
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

        // The screen is about one note, so there is no cursor to consult and no
        // second reading of what `e` is aimed at.
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

        // What the runtime does after a delete: the notebook is read again, and
        // the note that had a screen is not in it.
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

        // Ctrl-R arrives as `Char('r')` with a modifier, and a query field that
        // took it at face value would quietly become `budgetr` — and this one
        // reloads the notebook when it is not in a field. A chord the field does
        // not bind does nothing at all rather than either.
        app.on_key(ctrl('r'));
        app.on_key(ctrl('o'));
        assert_eq!(app.search(), "budget");
        assert_eq!(app.mode, Mode::Search, "and neither of them left the field");

        // A capital is still a capital, though: shift is not a chord.
        app.on_key(KeyEvent::new(KeyCode::Char('Q'), KeyModifiers::SHIFT));
        assert_eq!(app.search(), "budgetQ");
    }

    #[test]
    fn the_query_answers_the_keys_a_shell_prompt_answers() {
        let mut app = an_app();
        app.on_key(key(KeyCode::Char('/')));
        typing(&mut app, "tag:work budget");
        assert_eq!(app.shown(), 1, "one note has both");

        // The keys are the field's and are tested there; what is checked here is
        // that the notebook is walked again afterwards. A query that had been
        // edited but not re-run would leave the listing describing a line
        // nobody can see any more.
        app.on_key(ctrl('w'));
        assert_eq!(app.search(), "tag:work ");
        assert_eq!(app.shown(), 2, "and the listing is what the query now says");

        // Including when the edit is in the middle of the line rather than at
        // the end of it, which is the whole point of there being a cursor.
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
        // Both spellings of down, and neither of them types anything.
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
        // The copy on screen was the reason to press the key, so it goes and the
        // file is asked for again.
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
        // And the browser has not moved: the command is the runtime's to run.
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
        // Which is what `noda add` with no title of its own already means.
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
        // Unquoted it is three terms ANDed, which is the same reading the shell
        // would give it, and it finds nothing.
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
        app.on_key(key(KeyCode::Char('#')));
        typing(&mut app, "+archive");
        app.on_key(key(KeyCode::Enter));

        assert_eq!(app.on_key(key(KeyCode::Char('q'))), None);
        assert_eq!(app.mode, Mode::Confirm(What::Quit));
        // Anything but `y` stays, and the queue is still there to be sent.
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

        // The one key that does not argue: a program that talks back to Ctrl-C
        // is one you end up killing from another window.
        assert_eq!(app.on_key(ctrl('c')), Some(Action::Quit));
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
        // The same trap the query field has: Ctrl-R arrives as `Char('r')`, and
        // an unbound chord in a field is a key that does nothing rather than one
        // that types an `r` into a title.
        app.on_key(ctrl('r'));
        app.on_key(ctrl('o'));
        assert_eq!(app.input.text(), "Trip");
        assert_eq!(app.mode, Mode::Ask(Ask::Title));
    }

    #[test]
    fn a_title_being_edited_is_edited_and_not_only_added_to() {
        // The retitle case, which is what made a cursor worth having: the prompt
        // opens on the title the note already has, and the word to fix is not
        // the last one.
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
        app.on_key(key(KeyCode::Char('#')));
        typing(&mut app, "+work +q3");
        app.on_key(ctrl('w'));
        assert_eq!(app.input.text(), "+work ");
        app.on_key(ctrl('y'));
        assert_eq!(app.input.text(), "+work +q3", "and Ctrl-W is undoable");
    }

    #[test]
    fn a_delete_is_asked_about_first() {
        let mut app = an_app();
        assert_eq!(app.on_key(ctrl('d')), None);
        assert_eq!(app.mode, Mode::Confirm(What::Delete));

        // Anything but `y` keeps the note — including the key that would have
        // deleted the next one.
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
        // Nothing opened, nothing queued, nothing gone — and a line saying why
        // the key did not do what it used to.
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
    fn a_command_that_colours_its_answer_is_quoted_without_the_colour() {
        let mut app = an_app();
        // What `noda status` actually returns: the palette paints
        // unconditionally, because at a prompt `anstream` is what decides
        // whether the escapes survive. Nothing on a card goes through it.
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

        // What the runtime does after a delete: the notebook is read again, and
        // the note that was under the cursor is not in it.
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

        // The screen in front of you is what Escape is about. The narrowing
        // underneath it is the next layer, not this one.
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

        assert_eq!(app.on_key(ctrl('d')), None);
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
        app.replace(a_session(a_status(), notes));
        assert_eq!(app.selected().map(|f| f.id.as_str()), Some("aaaa1111"));
        app.select_id("dddd4444");
        assert_eq!(app.selected().map(|f| f.id.as_str()), Some("dddd4444"));
    }

    /// Types a line at the command prompt and presses Enter, the way `:` does.
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
        // A key cannot begin with `+` or `-`, so the first argument here is a
        // note and there it was a change. That is the whole ambiguity.
        assert_eq!(
            command(&mut app, "tag cccc3333 -work +q3"),
            Some(Action::Tag {
                key: "cccc3333".to_string(),
                changes: vec!["-work".to_string(), "+q3".to_string()],
                touch: Touch::Stamp,
            })
        );
        // And the quoting the tags prompt already does survives being typed
        // here instead — there is still no shell in front of the field.
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
        // The rest of the line is kept as it was typed rather than rebuilt from
        // its tokens, which is the only way a value with a space in it arrives
        // whole.
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
        // A line and not a card: this never reached the notebook, so there is
        // nothing a card would be quoting.
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

        // And a flag it does not take is refused rather than passed on.
        assert_eq!(command(&mut app, "doctor --fix"), None);
        assert!(app.message.as_ref().is_some_and(|said| said.failed));
    }

    #[test]
    fn opening_by_name_is_left_to_the_notebook_to_answer() {
        let mut app = an_app();
        // Not resolved here. What an id prefix or a slug names — and what it
        // means for one to name two notes — has one implementation, and a
        // browser holding the notes in memory could answer it a second way
        // without noticing it had.
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
        app.on_key(key(KeyCode::Char('#')));
        typing(&mut app, "+archive");
        app.on_key(key(KeyCode::Enter));

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
        // And the ones that answer at once do not.
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
        // Forward past the newest gives back the line that was being typed,
        // rather than sticking on the last thing that was run.
        app.on_key(key(KeyCode::Down));
        assert!(app.input.is_empty());
    }

    #[test]
    fn a_chord_does_not_type_its_own_letter_into_the_command_line() {
        let mut app = an_app();
        app.on_key(key(KeyCode::Char(':')));
        typing(&mut app, "stat");
        // The same trap as the query field and the tags prompt, one field on.
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
        // Onto the prompt with a space after it, because it takes something —
        // and not run, because a list that set off a `push` by being landed on
        // would be a list nobody could read.
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
        // A note is named by its id here for the same reason it is named by its
        // id in a listing, a link and a commit message.
        assert_eq!(app.crumbs().collect::<Vec<_>>(), ["notes", "aaaa1111"]);

        app.on_key(key(KeyCode::Esc));
        assert_eq!(app.crumbs().collect::<Vec<_>>(), ["notes"]);
    }

    /// A notebook whose notes carry boxes and links, which the plain one does
    /// not: the screens below are about what is *in* the notes rather than what
    /// the notes are called.
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

        // The ticked one is not here: a finished item stays where its author
        // wrote it, and `noda todo` does not list it either.
        let said: Vec<&str> = app
            .tasks()
            .iter()
            .map(|task| task.item.text.as_str())
            .collect();
        assert_eq!(said, vec!["chase finance", "book a room"]);
        // Dated before undated, which is `todo::order` and not a rule written
        // twice.
        assert_eq!(app.tasks()[0].item.due.as_deref(), Some("2026-08-01"));
        assert!(app.tasks()[1].item.due.is_none());
    }

    #[test]
    fn a_row_that_names_a_note_is_the_note_the_keys_aim_at() {
        let mut app = a_working_app();
        app.on_key(key(KeyCode::Char('t')));
        // The first box belongs to the first note; the second to the second.
        assert_eq!(app.selected().map(|f| f.id.as_str()), Some("aaaa1111"));
        app.on_key(key(KeyCode::Char('j')));
        assert_eq!(app.selected().map(|f| f.id.as_str()), Some("bbbb2222"));

        // So `e` edits the note the cursor is on, exactly as it does on the
        // listing — one question with one answer on every screen.
        assert_eq!(
            app.on_key(key(KeyCode::Char('e'))),
            Some(Action::Edit {
                key: "bbbb2222".to_string(),
                touch: Touch::Stamp,
            })
        );

        // And `enter` opens it.
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
        // Nothing to edit and nothing to delete, so both do nothing at all
        // rather than reaching past the screen for a note somewhere else.
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

        // Enter is not a screen of its own: a tag is a way of narrowing the
        // listing, and the listing is where the notes it narrows already are.
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

        // Unquoted this is three terms and-ed together, and would find nothing
        // — the same trap the tags prompt and the search field both fell into.
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

        // And a note nothing points at says so with an empty list rather than
        // by refusing to open.
        app.on_key(key(KeyCode::Esc));
        app.on_key(key(KeyCode::Char('G')));
        app.on_key(key(KeyCode::Char('b')));
        assert!(app.linking().is_empty());
        assert!(app.selected().is_none(), "no row, so nothing to aim at");
    }

    #[test]
    fn l_follows_the_screen_the_way_log_itself_does() {
        let mut app = a_working_app();
        // On the listing, which is a screen about the notebook.
        app.on_key(key(KeyCode::Char('l')));
        assert_eq!(app.view(), &View::Log(None));
        assert_eq!(app.wanted(), Some(Need::Log(None)));

        app.on_key(key(KeyCode::Esc));
        read_it(&mut app, "the q3 budget\n");
        // On a note, which is a screen about a note.
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
            Content::Log(vec![
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
            ]),
        );

        app.on_key(key(KeyCode::Char('j')));
        assert_eq!(app.on_key(key(KeyCode::Enter)), None, "nothing runs yet");
        // On the prompt, spelled out and waiting for a second Enter: landing on
        // a row is not agreeing to write over the note it names.
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
            Content::Log(vec![a_commit(
                "1111111111111111111111111111111111111111",
                1_770_000_000,
                "edit",
            )]),
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
        // The commit *before* the deletion, which is the one `restore` wants —
        // naming the deletion and leaving the `~1` to be worked out would be
        // reporting a problem without its remedy.
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
        // The one you are in is not somewhere to go.
        assert_eq!(app.on_key(key(KeyCode::Enter)), None);
        app.on_key(key(KeyCode::Char('j')));
        assert_eq!(
            app.on_key(key(KeyCode::Enter)),
            Some(Action::Use("work".to_string()))
        );

        // With a queue in hand it is refused: an entry names notes by id, and an
        // id belongs to the notebook it was minted in.
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

        // Down into the note and back out again: the todo list is worked out a
        // second time, and the cursor has to survive being worked out.
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
        // Both the blame and the note under it were about a note the notebook no
        // longer holds.
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
        // Escape while the walk is still going, which is what a slow blame or a
        // long history makes ordinary.
        app.on_key(key(KeyCode::Esc));

        app.supply(
            &asked,
            Content::Log(vec![a_commit(
                "1111111111111111111111111111111111111111",
                1_770_000_000,
                "edit",
            )]),
        );
        assert_eq!(app.view(), &View::Notes);
        assert!(app.entries().is_empty(), "it landed on the wrong screen");
    }

    #[test]
    fn a_named_note_is_resolved_by_the_notebook_and_not_here() {
        let mut app = a_working_app();
        app.on_key(key(KeyCode::Char(':')));
        typing(&mut app, "blame meeting-notes");
        // A slug is not an id until `Notebook::resolve` has said so, so the key
        // goes back out to the runtime exactly as `open` does.
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

    /// The same note with times on it, for the two orders that need them.
    fn dated(mut file: NoteFile, created: &str, updated: &str) -> NoteFile {
        file.note.created = Some(created.to_string());
        file.note.updated = Some(updated.to_string());
        file
    }

    /// Three notes whose slug, title, creation and update orders are all
    /// different, so a test can tell which one is in force.
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

        // Newest first, which is how `--sort created` runs: the question put to
        // a time is nearly always "what is recent".
        app.on_key(key(KeyCode::Char('S')));
        assert_eq!(app.sort, Sort::Created);
        assert_eq!(listed(&app), ["cccc3333", "bbbb2222", "aaaa1111"]);

        app.on_key(key(KeyCode::Char('S')));
        assert_eq!(app.sort, Sort::Updated);
        assert_eq!(listed(&app), ["aaaa1111", "cccc3333", "bbbb2222"]);

        // Alphabetical, which runs the other way to the two times.
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

        // Applied after the sort, so every order gets one — the same bargain
        // `ls -r` makes, and the reason it needs no `--sort` beside it.
        app.on_key(key(KeyCode::Char('S')));
        assert_eq!(listed(&app), ["aaaa1111", "bbbb2222", "cccc3333"]);
    }

    #[test]
    fn reordering_keeps_the_cursor_on_the_note_it_was_on() {
        let mut app = a_dated_app();
        app.on_key(key(KeyCode::Char('G')));
        assert_eq!(app.selected().map(|f| f.id.as_str()), Some("cccc3333"));

        // The note and not the row: re-sorting is asking where this note falls
        // in a new order, and being thrown to the top to find out would be a
        // reason not to press the key.
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

        // A read brings the notebook back in the walk's own order. A setting
        // that came off every time you pressed `r` would not be a setting.
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

        // The other screens print what their own command prints; there is no
        // second density for them to offer.
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

        // A digit past the end of the list is a digit with nothing to mean.
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

        // The answer to "show me this tag" is a screen, and it is the one at the
        // bottom of the stack — the same reason `:notes` comes back down.
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

        // The screen numbers its first nine rows with the very keys that reach
        // them, so the two cannot drift: whatever is second there is what `2`
        // narrows to.
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
