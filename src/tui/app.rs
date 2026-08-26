//! What a browsing session holds, and how one keystroke changes it.
//!
//! Nothing here opens a file, a repository or a terminal: a key goes in, the
//! state moves, and anything needing the world outside comes back as an
//! [`Action`]. That is what lets the whole interaction be tested with no
//! terminal — which matters more here than anywhere else, every other command
//! being a function returning a string and this one a loop.
//!
//! The keys that change a note ask a command to do it, and the answer is the
//! line that command would have printed. What a change means is written once, in
//! `cmd`; this is a second way of asking rather than a second version.
//!
//! A session is a **stack of screens**, the notes at the bottom and never
//! popped. Each keeps its own cursor, query and scroll, which is what makes
//! going back land where you left.
//!
//! The notes are held in memory for the session: `noda search` reads every body
//! on every invocation, and this is that cost paid once rather than per query.

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
/// The five that change a notebook name a command and its arguments, never an
/// edit: what a change *means* lives in `cmd`, and a second account of it must
/// not exist.
///
/// A note is named by id rather than by row — the command reopens the notebook,
/// and by then the listing is a picture of what was there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Quit,
    /// The browser's answer to another process writing a note, rather than a
    /// file watcher.
    Reload,
    /// The one action needing the terminal handed back, `$EDITOR` being
    /// full-screen too.
    Edit {
        key: String,
        touch: Touch,
    },
    /// `None` leaves the title to the body, as `add` does. No `touch`, because
    /// `add` has none: a note never changed was changed when it was made.
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
    /// Every change in the queue, in one commit. `cmd::bulk` is the same code
    /// writing the same files as the keys above, with the commit boundary out one
    /// level so a queue arrives in history as the one thing it was.
    Send(Vec<Step>),
    /// A new commit, so nothing is rewritten and nothing is asked — and two
    /// arguments, both typed on purpose.
    Restore {
        key: String,
        rev: String,
        touch: Touch,
    },
    /// Open a note the prompt named, on a screen of its own.
    ///
    /// The key is not resolved here: what a prefix or slug names is
    /// `Notebook::resolve`, and a browser holding the notes could answer it a
    /// second way without noticing.
    Open(String),
    /// A screen *about* a note named by key. A second variant rather than a flag
    /// on `Open`, and resolved by the runtime for its reason: a key is not an id
    /// until the notebook says so.
    Show {
        key: String,
        look: Look,
    },
    /// Not a `Run`: what comes back is a different notebook, so the runtime
    /// builds a new session rather than reloading this one.
    Use(String),
    /// A command that reads or changes the notebook and answers with a line.
    Run(Run),
}

/// Nine, there being nine digits that are not `0` and `0` being the way out. A
/// notebook's tags are a long tail with a short head; the one-offs are what `/`
/// is for.
pub const SCOPE_KEYS: usize = 9;

/// A screen about one note, named before the note is known.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Look {
    Log,
    Blame,
    Backlinks,
}

/// One variant apiece rather than a closure, so what the browser can ask for is
/// a list somebody can read — and the runtime stays the only part that knows how
/// a call is made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Run {
    Status,
    /// Reporting only: a browser is not where you find out that a keystroke
    /// rewrote a directory.
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
    /// The loop draws, waits for a key, then acts — so a command taking seconds
    /// leaves the last frame up with no sign of anything happening. These get a
    /// frame of their own first.
    pub fn working(&self) -> Option<&'static str> {
        match self {
            Action::Run(Run::Sync) => Some("syncing…"),
            Action::Run(Run::Push) => Some("pushing…"),
            Action::Run(Run::Pull) => Some("pulling…"),
            _ => None,
        }
    }
}

/// Its own words: a browser that summarised them would be deciding what a
/// command meant, which is the same mistake as writing the change twice.
pub struct Message {
    pub text: String,
    /// A card for failures, the status line for successes: an acknowledgement is
    /// read in passing and a reason has to be read.
    pub failed: bool,
}

impl Message {
    /// As much of it as one line of the status bar can hold.
    pub fn line(&self) -> &str {
        self.text.lines().next().unwrap_or_default()
    }
}

/// Both kinds, because it is one question answered by one walk — which is why
/// `noda backlinks` takes either.
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

/// The name is what the crumb trail shows, so it is the notebook's own word
/// rather than a heading invented for the browser: a note by its id, and every
/// other screen by the subcommand that prints the same thing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum View {
    /// The bottom of the stack, never popped: what a session is open on.
    Notes,
    /// Held by id rather than by row, so the screen survives the listing under
    /// it being filtered, re-sorted or read again.
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
/// Some the session works out for itself and some need a repository, which only
/// the runtime may open — [`App::derive`] and [`App::wanted`]. The difference
/// does not show on screen.
pub enum Content {
    /// As it is on disk, not rendered back from the parse — `noda show`'s
    /// reason.
    Note(String),
    Todo(Vec<Task>),
    Tags(Vec<Tally>),
    /// Indices into the session's notes: the ones that link to the subject.
    Backlinks(Vec<usize>),
    /// The history, and which commits the remote has not seen.
    ///
    /// Beside the entries rather than a flag on each: it is read off the refs in
    /// one walk, and an `Entry` is what a commit *is*, not what a remote knows
    /// about it. Empty with nothing to compare against, as `noda log` answers.
    Log(Vec<Entry>, std::collections::HashSet<git2::Oid>),
    Blame(Vec<BlameLine>),
    Deleted(Vec<Deleted>),
    /// Uncoloured: the drawing puts it back, as for every other listing.
    Diff(String),
}

/// One unticked box, and the note carrying it.
pub struct Task {
    /// An index rather than an id, the list being rebuilt whenever the notes
    /// are and never outliving them.
    pub note: usize,
    pub item: todo::Item,
}

/// One tag, and how many notes carry it.
pub struct Tally {
    pub tag: String,
    pub notes: usize,
}

/// Three states rather than a tick, because a change aimed at forty notes has
/// three answers: leaving a mixed set alone is a different instruction from
/// giving the tag to all or taking it from all. `cmd::tag`'s own three, so
/// nothing is translated on the way out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    /// Nothing. Whatever each note says about this tag, it goes on saying.
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
    /// The tags screen's number, which says whether a tag is one the notebook
    /// runs on or one somebody typed once.
    pub notes: usize,
    pub mark: Mark,
}

impl Choice {
    /// The states that would change something. A tag every note carries can only
    /// be taken away and one none carries can only be given, so the key walks two
    /// states on one note and three on a mixed set — one rule, not two.
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

    /// The next of them, which is what `Tab` means.
    fn next(&self, total: usize) -> Mark {
        let states = self.states(total);
        let at = states.iter().position(|state| *state == self.mark);
        states[at.map_or(0, |at| (at + 1) % states.len())]
    }

    /// A tick says what is *true* and the other three say what is being *done*.
    /// One box holds both, because a tick only appears where nothing is being
    /// done.
    ///
    /// Written out rather than drawn: `☑` is ambiguous-width, and a terminal
    /// giving it two columns takes the one back off the end of the row.
    pub fn tick(&self, total: usize) -> &'static str {
        match self.mark {
            Mark::Add => "[+]",
            Mark::Remove => "[-]",
            Mark::Leave if total > 0 && self.held == total => "[x]",
            Mark::Leave => "[ ]",
        }
    }
}

/// The row for what has been typed when it is not a tag the notebook has.
///
/// A tag that does not exist yet is the one thing here still to be spelled, and
/// so the one thing that can be a typo. It costs a keystroke of its own and says
/// what it would sit next to: `Work` under `work, 37 notes` answers itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Proposal {
    /// A tag that could be made, and the nearest thing to it that is close
    /// enough to be a slip of a finger.
    New {
        tag: String,
        near: Option<(String, usize)>,
    },
    /// One that could not, in `note::validate_tag`'s own words.
    Refused(String),
}

/// Whether two tags are one keystroke apart: the same but for case, or for a
/// character added, dropped, mistyped or swapped with its neighbour.
///
/// Not a general edit distance. What it is asked for is `Work` beside `work` and
/// `wrok` beside it too — a transposition counts as two substitutions to
/// anything counting one at a time, and is the commonest typo there is.
fn one_edit_apart(left: &str, right: &str) -> bool {
    let left: Vec<char> = left.to_lowercase().chars().collect();
    let right: Vec<char> = right.to_lowercase().chars().collect();
    if left.len().abs_diff(right.len()) > 1 {
        return false;
    }
    // What they share at each end, and so the whole of the difference — which
    // one edit leaves in one of four shapes.
    let head = left.iter().zip(&right).take_while(|(a, b)| a == b).count();
    let tail = left[head..]
        .iter()
        .rev()
        .zip(right[head..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    match (left.len() - head - tail, right.len() - head - tail) {
        // Three of the four: nothing between is the same tag but for case, one
        // on one side is added or dropped, one on each is mistyped.
        (0..=1, 0..=1) => true,
        // Two each, and the same two the other way round: transposed.
        (2, 2) => left[head] == right[head + 1] && left[head + 1] == right[head],
        _ => false,
    }
}

/// Something a screen needs that only the runtime can get. Asked for rather than
/// fetched: the state says what it wants, the runtime brings it back, and
/// nothing here touches a disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Need {
    Note { id: String, path: PathBuf },
    Log(Option<String>),
    Blame { id: String, slug: String },
    Deleted,
    Diff,
}

/// What a screen is of, and everything about it a keystroke can move.
///
/// Cursor, query and scroll are per-screen: going back has to land where you
/// left, or `Enter` becomes a key you learn not to press — and a screen that is
/// not a listing still has to scroll without the one under it losing its
/// place.
pub struct Screen {
    pub view: View,
    /// The cursor and the scroll offset of a listing, which ratatui keeps for us.
    pub table: TableState,
    /// How far a screen of text has been scrolled.
    pub scroll: u16,
    /// Split the way a shell would split it, this is what `noda search` takes.
    pub search: Field,
    /// For picking the match out of a title and a body. A `tag:` or `id:`
    /// matched something the prose does not contain.
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
    /// The list narrows on every keystroke, so there is no "run the search"
    /// step — `Enter` only puts the keyboard back on the list.
    Search,
    Help,
    /// Typing the one thing a change needs said in words.
    Ask(Ask),
    /// The same line the query uses: only one can be open at a time, and three
    /// places to type is a browser you look at to find where your keys went.
    Command,
    /// The way in for somebody who knows what they want and not what it is
    /// called.
    Commands,
    /// Asked on the screen, because the terminal is in raw mode and a command
    /// reading stdin would take keystrokes out from under the browser.
    Confirm(What),
    /// Reading the queue: what is waiting to be sent, and what to drop from it.
    Queue,
    /// A card and not a screen, for the queue's reason: what the keyboard is
    /// doing rather than somewhere the session has gone.
    Tagging,
    /// A refusal, or an answer longer than a line. Dismissed by anything.
    Alert,
}

/// What a `y` would agree to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum What {
    /// Delete the note under the cursor, now.
    Delete,
    /// Asked at the send and not at the queueing: a delete sitting in the queue
    /// has not happened and can still be dropped.
    Send,
    /// The queue is the one thing a session holds that is written down nowhere:
    /// a query can be retyped, an afternoon of queued changes cannot.
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

    /// What cannot be guessed from the prompt, at the other end of the line.
    pub fn hint(self) -> &'static str {
        match self {
            Ask::Title => "Enter alone takes the title from the body",
            Ask::Retitle => "",
        }
    }
}

/// One read of the notebook: what a session holds regardless of which screen is
/// on top.
///
/// One value because a reload has to replace all of it at once — a listing
/// rebuilt from new notes beside a file list describing the old notebook is half
/// true, and half true is the hardest kind of wrong to notice.
pub struct Session {
    pub status: Status,
    pub notes: Vec<NoteFile>,
    /// What the notebook holds that is not a note, by name.
    pub files: Vec<String>,
    /// Every notebook there is, for the screen that moves between them.
    pub notebooks: Vec<String>,
    /// The local date and not UTC: east of here an item would stay unmarked
    /// until morning, which is when a todo list is read.
    pub today: String,
}

pub struct App {
    /// The active notebook's name, for the header.
    pub notebook: String,
    /// What turns a note on a screen into a file for the runtime to read.
    pub root: PathBuf,
    /// As of the last load. Nothing touches the network: the drift is what the
    /// last sync left, exactly as `noda status` reports it.
    pub status: Status,
    /// In the walk's order — by slug, as `noda ls` shows without `--sort`.
    notes: Vec<NoteFile>,
    /// Both come with the read that produced the notes: fetching them when a
    /// screen asks would be a repository opened for a list of filenames.
    files: Vec<String>,
    notebooks: Vec<String>,
    /// Once per read rather than per frame: a browser left open overnight is
    /// rarer than a clock asked sixty times a second.
    today: String,
    /// Oldest first, never empty: popping the listing leaves nothing to be in.
    stack: Vec<Screen>,
    pub mode: Mode,
    /// One field, because only one prompt can be open at a time and
    /// [`Mode::Ask`] already says which.
    pub input: Field,
    /// What the last command that changed something had to say.
    pub message: Option<Message>,
    /// The notes picked out to be changed together, by id.
    ///
    /// Apart from the query on purpose: `/` narrows what can be *seen* and
    /// marking says what is meant to *change*, so a hidden note is still marked.
    /// Otherwise "mark these, then search for the next lot" would drop the first
    /// lot. On the session rather than a screen for the same reason.
    ///
    /// Ordered, so a commit message does not depend on a hash's order.
    pub marks: BTreeSet<String>,
    /// `cmd::Step`s, because that is what `cmd::bulk` is handed unaltered — and
    /// a translation at the end is where a second account of what a change means
    /// gets in.
    pub queue: Vec<Step>,
    /// Where the cursor is in the queue view.
    queue_at: usize,
    /// Every tag the notebook has, in the tags screen's order, with anything
    /// typed on the end. Built when the picker opens and thrown away after: a
    /// question being asked, not a thing the session holds.
    choices: Vec<Choice>,
    /// Settled when the picker opens: a cursor can move, and what a question was
    /// asked about cannot.
    aimed_at: Vec<String>,
    /// Over the rows on screen rather than the choices, so it means the same
    /// thing while the list is narrowed.
    tags_at: usize,
    /// Session-long rather than per keystroke: there is no room to qualify a
    /// single key, and the reason for wanting it lasts longer than one anyway.
    /// Shown in the header, because a setting you cannot see is one you forget
    /// you left on.
    pub touch: Touch,
    /// Session settings for `touch`'s reason: on a screen there is nothing to
    /// write an order on. `--sort`'s and `-r`'s orders, applied by
    /// `cmd::sort_notes`.
    pub sort: Sort,
    pub reverse: bool,
    /// `ls -l`'s columns, in the same places.
    pub long: bool,
    /// A row of the terminal, given back to the notes by anyone who wants it.
    pub crumbs_shown: bool,
    /// Kept with its view so a screen never draws the last screen's answer: what
    /// is loaded is loaded *for* something.
    loaded: Option<(View, Content)>,
    /// For the prompt to walk back through, and kept no longer than the session:
    /// a file of everything anyone typed into a notebook is a thing to think
    /// about rather than a convenience.
    history: Vec<String>,
    /// `None` is the line being typed, which is why walking forward past the end
    /// returns to it rather than to the newest entry.
    history_at: Option<usize>,
    /// Where the cursor is in the list of commands.
    commands_at: usize,
    /// What is being waited for, while it is being waited for.
    pub working: Option<&'static str>,
    /// Written back by the drawing: half a screen is half of what you can see,
    /// and only the drawing knows how much that is.
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

    /// Never popped, so the one screen that can always be asked about — which a
    /// mark, a reload and a cursor kept across one all need.
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

    /// What says where along the line the terminal's cursor belongs.
    pub fn search_before(&self) -> &str {
        self.listing().search.before()
    }

    /// Why the query as typed is not a query yet.
    pub fn error(&self) -> Option<&str> {
        self.listing().error.as_deref()
    }

    /// A screen opened from a query inherits its terms, so a hit is still marked
    /// in the note it was found in.
    pub fn terms(&self) -> &[String] {
        &self.top().terms
    }

    /// How far the note on screen has been scrolled.
    pub fn scroll(&self) -> u16 {
        self.top().scroll
    }

    /// The terms come with it: opening the note that matched should not lose the
    /// highlighting saying why.
    fn open(&mut self, view: View) {
        let terms = self.top().terms.clone();
        self.stack.push(Screen::new(view, terms));
        self.settle();
    }

    /// `false` when there was nothing to close, so `Esc` can go on to mean what
    /// it means on the listing.
    fn back(&mut self) -> bool {
        if self.stack.len() > 1 {
            self.stack.pop();
            // What was loaded belonged to the screen that has gone; the one
            // underneath is worked out again with its own cursor left alone.
            self.settle();
            true
        } else {
            false
        }
    }

    /// What the top screen shows, for the screens the session can answer.
    ///
    /// Called wherever the top screen changes — pushed, popped, or standing while
    /// the notebook under it was read again. The ones needing a repository ask
    /// through [`App::wanted`] and wait a frame.
    fn settle(&mut self) {
        let view = self.top().view.clone();
        if self.content().is_none()
            && let Some(content) = self.derive(&view)
        {
            self.loaded = Some((view, content));
        }
        // Kept and clamped rather than sent to the top: this runs on the way
        // back as well as in, and a stack that dropped the cursor is one you
        // learn not to press Escape in. The clamp is for a list gone shorter.
        let at = self.top().table.selected().unwrap_or(0);
        self.cursor_to(at);
    }

    /// What a screen shows, when the session already holds the answer.
    ///
    /// These three read every body, so they are worked out when a screen opens
    /// rather than as the cursor moves: parsing per keystroke is what the
    /// in-memory copy exists to avoid, not what it makes affordable.
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
                // `todo::order`, not a comparison written here: `noda todo`
                // prints this list too, and two orders read as a bug.
                tasks.sort_by(|left, right| {
                    todo::order(
                        (self.notes[left.note].slug.as_str(), &left.item),
                        (self.notes[right.note].slug.as_str(), &right.item),
                    )
                });
                Some(Content::Todo(tasks))
            }
            // Counted and ordered by `notebook`: the web's tag screen shows the
            // same list, and two orders for one list read as a bug.
            View::Tags => Some(Content::Tags(
                notebook::tag_tally(&self.notes)
                    .into_iter()
                    .map(|(tag, notes)| Tally { tag, notes })
                    .collect(),
            )),
            // Asked of `notebook`: what counts as a link is written down once,
            // applied here to the notes already in hand.
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

    /// `None` on every frame but the one after a screen opens, which keeps a
    /// repository from being opened per keystroke.
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

    /// Dropped when the screen it was for is no longer in front of you: a blame
    /// of two thousand commits takes long enough to press Escape during.
    pub fn supply(&mut self, view: &View, content: Content) {
        if self.top().view != *view {
            return;
        }
        self.loaded = Some((view.clone(), content));
        self.top_mut().scroll = 0;
        let at = self.top().table.selected().unwrap_or(0);
        self.cursor_to(at);
    }

    /// Public because only the runtime finds out a command could not fill a
    /// screen — and leaving it up puts the reason on a card over the top of it.
    pub fn give_up(&mut self) {
        self.back();
    }

    /// A note by id, for the screens that are about one.
    pub fn note_of(&self, id: &str) -> Option<&NoteFile> {
        self.notes.iter().find(|file| file.id == id)
    }

    /// Keeps the query, and the cursor when the note is still there — a reload
    /// that jumped to the top would be a reason not to press the key.
    ///
    /// Otherwise the row it was on is kept, which is the case a delete makes
    /// ordinary.
    pub fn replace(&mut self, session: Session) {
        let was = self.at_cursor().map(|file| file.id.clone());
        let row = self.listing().table.selected();
        self.status = session.status;
        self.notes = session.notes;
        self.files = session.files;
        self.notebooks = session.notebooks;
        self.today = session.today;
        // Before the query is rerun, which picks out indices into this list.
        // Reapplied, because a read brings the walk's own order back and an
        // order that came off on every `r` would not be a setting.
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
        // This copy is what the reload was pressed to get rid of.
        self.loaded = None;
        // A screen about a note the notebook no longer holds has nothing behind
        // it — the ordinary end of deleting the note you are reading — so it
        // comes off rather than showing the last thing that was true.
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
        // A mark on a note that has gone is a change aimed at nothing. The queue
        // keeps its own ids: an entry records what was asked for, and `bulk`
        // says so if one has since gone.
        let ids: BTreeSet<&str> = self.notes.iter().map(|file| file.id.as_str()).collect();
        self.marks.retain(|id| ids.contains(id.as_str()));
        // The screen now describes a notebook read again, so it is worked out
        // again; the ones needing a repository ask on the next frame.
        self.settle();
    }

    /// Whether the note is one of the ones picked out.
    pub fn marked(&self, id: &str) -> bool {
        self.marks.contains(id)
    }

    /// In listing order. Ids, being what a command takes and what survives a
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
    /// One question with one answer on every screen, which is what lets `e`, `m`,
    /// `#` and `Ctrl-d` mean the same thing everywhere without knowing where they
    /// are.
    ///
    /// A screen whose rows are notes answers with the row; a screen *about* one
    /// note answers with that note however far it has scrolled; a screen about
    /// the notebook answers with nothing, and those keys do nothing.
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

    /// Whichever of the three the screen in front of you is showing.
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

    /// False on every other screen and with no remote, which leaves the margin
    /// blank rather than marking everything.
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

    /// `None` when the screen is a block of text and the keys scroll it.
    ///
    /// The one place that says which kind a screen is: every key that moves asks
    /// here, so a screen added later cannot be a list on one key and a page on
    /// another.
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

    /// Taken out so the rows may borrow the notes while ratatui writes this
    /// frame's offset — different fields, but the borrow checker sees `self`.
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
        // Windows sends a press and a release; both would move twice.
        if key.kind != KeyEventKind::Press {
            return None;
        }
        // Leaves from wherever you are, because that is what the terminal itself
        // would have meant — and without asking about an unsent queue, for the
        // same reason. A program that argues with Ctrl-C is one you kill from
        // another window; `q` is the key that can afford to ask.
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
            Mode::Tagging => self.picking(key),
            Mode::Browse => self.browsing(key),
        }
    }

    fn browsing(&mut self, key: KeyEvent) -> Option<Action> {
        // The next key is the reader moving on, so the line goes back empty.
        self.message = None;
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            return self.chord(key.code);
        }
        // Answered first, so a screen only describes what is different about it
        // and a key cannot mean two things by being forgotten on one.
        match key.code {
            KeyCode::Char('q') => return self.leaving(),
            KeyCode::Char('r') => return Some(Action::Reload),
            KeyCode::Char('?') => {
                self.mode = Mode::Help;
                return None;
            }
            // Thirty subcommands and about a dozen letters worth spending, so
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
            // Each asks a command, and the three needing something said first
            // open the prompt. All aim at whatever the screen is about.
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
                // Nothing to tag is nothing to ask about.
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
            // The three *about the note in front of you*, where naming it would
            // be naming what you are looking at, plus the one notebook list read
            // as often as the notes. The other five arrive by being named.
            KeyCode::Char('t') => {
                self.open(View::Todo);
                return None;
            }
            // The notebook's or one note's, by what the screen is about. `:log`
            // reads the screen the same way, so the two never disagree.
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
            // The nine commonest tags, one press each, `0` the way out — the
            // short form of the tags screen's `enter`, which is why that screen
            // numbers its first nine rows.
            //
            // Answered on every screen, because the answer is a screen: it comes
            // back down to the listing, where the notes a tag narrows are.
            KeyCode::Char(digit @ '0'..='9') => {
                self.scope_nth(digit as usize - '0' as usize);
                return None;
            }
            // Not a delete, and not anything else either: this key removed a
            // note until the modifier went in front of it, so it has to say
            // where the deleting went rather than quietly mean something new.
            KeyCode::Char('d') => {
                self.message = Some(Message {
                    text: "delete is Ctrl-d".to_string(),
                    failed: false,
                });
                return None;
            }
            _ => {}
        }
        // Which kind of screen, not which: a list is walked and a page is
        // scrolled, and everything else about them is the same.
        match (&self.top().view, self.has_rows()) {
            (View::Notes, _) => self.on_listing(key),
            (_, true) => self.on_rows(key),
            (_, false) => self.on_reading(key),
        }
    }

    /// Half a screen either way, and the delete.
    ///
    /// A chord for the delete, it being the one key that cannot be taken back:
    /// a modifier is the difference between a key you reach for and one you
    /// mean.
    fn chord(&mut self, code: KeyCode) -> Option<Action> {
        let half = i32::from(self.page.max(2) / 2);
        match code {
            KeyCode::Char('f') => self.step(half),
            KeyCode::Char('b') => self.step(-half),
            KeyCode::Char('d') => return self.delete(),
            // For somebody who knows what they want and not what it is called.
            // A card, the list being where there are too many for keys.
            KeyCode::Char('a') => {
                self.mode = Mode::Commands;
                self.input.clear();
                self.commands_at = 0;
            }
            // Only where there are rows: the other screens print what their own
            // command prints and have no second density.
            KeyCode::Char('w') => {
                if matches!(self.top().view, View::Notes) {
                    self.long = !self.long;
                }
            }
            // A row given back to the notes. The trail is worth a line most of
            // the time — a stack whose depth you cannot see is one whose Escape
            // you guess at — but the title band still says where you are.
            KeyCode::Char('g') => self.crumbs_shown = !self.crumbs_shown,
            _ => {}
        }
        None
    }

    /// Aimed as every change is: the marked notes, or the one on the screen.
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
            // `--sort`'s orders and `-r`'s reverse. Shifted because they are
            // about the listing rather than a note, and `r` is already taken.
            KeyCode::Char('S') => {
                self.sort = self.sort.next();
                self.reorder();
            }
            KeyCode::Char('R') => {
                self.reverse = !self.reverse;
                self.reorder();
            }
            // A screen of its own rather than half of this one, so it is read at
            // the width it was written at.
            KeyCode::Enter => {
                let id = self.selected()?.id.clone();
                self.open(View::Note(id));
            }
            // `Space` marks the one under the cursor, `*` everything shown —
            // which is what composes a search with a selection.
            KeyCode::Char(' ') => {
                let id = self.selected()?.id.clone();
                if !self.marks.remove(&id) {
                    self.marks.insert(id);
                }
            }
            KeyCode::Char('*') => self.mark_shown(),
            // Out of whatever narrowing is in force, one layer at a time: the
            // query first, then the marks — in that order, a query being cheap
            // to retype and a selection not.
            //
            // The queue is not in this chain: losing work to one Escape too many
            // is what a queue exists to prevent.
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

    /// The obvious next question about the thing on the row, in three kinds: a
    /// note opens, another angle opens that angle, and something *irreversible*
    /// goes onto the prompt rather than running — landing on a row is not
    /// agreeing to overwrite the note it names.
    fn chose(&mut self) -> Option<Action> {
        let at = self.top().table.selected()?;
        match self.top().view.clone() {
            View::Todo | View::Backlinks(_) => {
                let id = self.selected()?.id.clone();
                self.open(View::Note(id));
            }
            // A tag narrows the listing, which is where its notes already are.
            // The key `0`–`9` is short for.
            View::Tags => {
                let tag = self.tallies().get(at)?.tag.clone();
                self.scope(&tag);
            }
            // The one question worth asking about an attachment.
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
            // A commit in one note's history is a version of it, so the row is
            // the revision `restore` wants. The notebook's log names no note.
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

    /// `cmd::sort_notes` and not a comparison written here: `--sort` offers the
    /// same four, and two spellings would be one feature wearing two names. The
    /// reverse applies after, so every order gets one free.
    fn arrange(&mut self) {
        cmd::sort_notes(&mut self.notes, self.sort);
        if self.reverse {
            self.notes.reverse();
        }
    }

    /// Keeps the cursor on the *note* and not the row: re-sorting asks where a
    /// note falls in a new order, and being thrown to the top to find out is a
    /// reason not to press the key.
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

    /// In the tags screen's order, so the number beside a tag there is the key
    /// that reaches it. `0` is the way out.
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

    /// `query::scoped` writes the query, quotes and all: the field splits the
    /// way a shell would, and the web's tag screen narrows the same way.
    fn scope(&mut self, tag: &str) {
        while self.back() {}
        self.top_mut().search.set(query::scoped(tag));
        self.refilter();
    }

    /// The queue stands in the way and has to: an entry names notes by id, and
    /// ids belong to the notebook they were minted in — against another it finds
    /// nothing, or the wrong thing. Refused rather than dropped, the queue being
    /// written down nowhere else.
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

    /// Marks everything shown, or unmarks it when it is all marked — one key,
    /// because what it does now is on the screen. It does not touch a marked
    /// note the query is hiding.
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
    /// was typed: the end of a sitting is too late to remember what was meant.
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
            // Dropping an entry is the queue's point. A plain `d` here, unlike
            // on a screen, because nothing in this card can delete a note.
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
        // A tag can be put back and a removed note only through git, so the
        // question is asked for a deletion and not otherwise: a confirmation
        // that appears every time is one nobody reads.
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

    /// The queue has been carried out, and its notes are no longer picked out.
    pub fn sent(&mut self) {
        self.queue.clear();
        self.queue_at = 0;
        self.marks.clear();
    }

    /// Opens the tag picker over whatever the screen is about.
    ///
    /// It asks *which tags these notes should end up with*, not which `+`s and
    /// `-`s to apply — the browser's tag form's question, for its reason. Every
    /// tag is already a row, so the commonest mistake the notation allowed — a
    /// `-` aimed at a misspelling, which removes nothing and says nothing — has
    /// nowhere left to happen.
    ///
    /// `Tab` chooses, not `Space`: the line being typed is a tag that may not
    /// exist yet, and a tag is allowed a space. Making this the one field where
    /// the space bar did something else would put `24.04 Dark patterns` out of
    /// reach of the card that exists to spare you spelling it.
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
            // Before the field is offered the key, which is why it is `Tab`:
            // not a character, so the filter still takes every character.
            KeyCode::Tab => {
                self.choose();
                return None;
            }
            KeyCode::Enter => return self.tagged(),
            KeyCode::Esc => {
                self.shut();
                return None;
            }
            // `j` and `k` are not among them: every letter goes into the
            // filter.
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
        // Erasing only, as the command list takes only those: what is typed is
        // shown on the card and there is no cursor for a motion key to move. The
        // cursor goes to the top, the list under it being a different list.
        if self.input.erasing(key).is_some() {
            self.tags_at = 0;
        }
        None
    }

    /// Puts the picker away with nothing asked of the notebook.
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
        // The row that is not a tag yet joins where the filter can see it, so
        // the cursor stays put and the box it grew is all that changed.
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

    /// The `+`s and `-`s are written here and nowhere else. Nothing was typed in
    /// that notation and nothing is split back out of it, which is what carries
    /// a tag with a space in it through as one string.
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
        // Nothing chosen is a way out rather than a command: a refusal would be
        // answering a question they stopped asking.
        if changes.is_empty() {
            return None;
        }
        // As the key was aimed: the marked set, where it queues, or the note the
        // screen is about, where it runs.
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

    /// In the order they are held: commonest first, with anything typed on the
    /// end.
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
        // Refused where typed rather than where sent, by the same call the
        // command itself would fail on.
        if let Err(e) = note::validate_tag(typed) {
            return Some(Proposal::Refused(e.to_string()));
        }
        Some(Proposal::New {
            tag: typed.to_string(),
            near: self.nearest(typed),
        })
    }

    /// The commonest of the tags a keystroke away, because the commonest is the
    /// one meant. Only tags the notebook already has: one made a moment ago in
    /// this picker says nothing about whether the next is a slip.
    fn nearest(&self, tag: &str) -> Option<(String, usize)> {
        self.choices
            .iter()
            .filter(|choice| choice.notes > 0 && one_edit_apart(&choice.tag, tag))
            .max_by_key(|choice| choice.notes)
            .map(|choice| (choice.tag.clone(), choice.notes))
    }

    /// How many rows the picker is showing altogether.
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

    /// Which note, when it is the one under the cursor rather than a marked set.
    pub fn picking_note(&self) -> Option<&NoteFile> {
        match self.aimed_at.as_slice() {
            [only] if self.marks.is_empty() => self.note_of(only),
            _ => None,
        }
    }

    fn searching(&mut self, key: KeyEvent) -> Option<Action> {
        // The field's business, including the rule it was built around: a chord
        // is not a character. `KeyCode::Char('d')` arrives for Ctrl-D as much as
        // for `d`, and a field taking that at face value reads `budgetd`.
        //
        // What is left are the keys the field deliberately does not bind.
        // Re-running the query is this end's job, only this end knowing there is
        // a notebook behind the line.
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
            // Already narrowed, so this only hands the keyboard back.
            KeyCode::Enter => self.mode = Mode::Browse,
            // Leaving the query leaves what it selected, which is what makes
            // this an escape rather than a commit.
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

    /// With whatever it should start out holding: the current title for a
    /// retitle, so the common edit is a few keystrokes.
    fn ask(&mut self, what: Ask, start: String) {
        self.mode = Mode::Ask(what);
        self.input.set(start);
    }

    fn asking(&mut self, what: Ask, key: KeyEvent) -> Option<Action> {
        // The query's field, so the same keys work — which matters most here,
        // where a retitle is editing a title rather than typing one.
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

    /// An empty answer is a way out rather than an error: a refusal would be
    /// answering a question they stopped asking. The exception is a new note,
    /// where nothing typed is what `noda add` means by no title.
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

    /// Leaving, or asking about the queue first — the one piece of state
    /// written down nowhere.
    fn leaving(&mut self) -> Option<Action> {
        if self.queue.is_empty() {
            return Some(Action::Quit);
        }
        self.mode = Mode::Confirm(What::Quit);
        None
    }

    /// The same field the query and the prompt use, and so the same keys — this
    /// is the one most like a shell prompt, where a hand reaches for `Ctrl-A`
    /// without deciding to.
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
            // What a shell does, for its reason: the command you want next is
            // usually one you have typed. Both spellings, as readline has two.
            KeyCode::Up => self.recall(true),
            KeyCode::Down => self.recall(false),
            KeyCode::Char('p') if ctrl => self.recall(true),
            KeyCode::Char('n') if ctrl => self.recall(false),
            _ => {}
        }
        None
    }

    /// Forward past the newest entry gives an empty line rather than sticking:
    /// stopping dead would leave no way to type something new without clearing
    /// the field by hand.
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

    /// The rest of the line is kept as typed rather than rebuilt from its
    /// tokens: `tag:"12.34 foo bar"` is the case already got wrong twice
    /// elsewhere.
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
            // A title is free text, so its first word cannot be told from a
            // key. The note on screen is the one retitled.
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
            // The key's question, aimed the same way.
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
            // Each is the subcommand's own name showing what it prints, so `:`
            // is one vocabulary.
            "todo" => self.open(View::Todo),
            "tags" => self.open(View::Tags),
            "files" => self.open(View::Files),
            "notebooks" => self.open(View::Notebooks),
            "deleted" => self.open(View::Deleted),
            "diff" => self.open(View::Diff),
            // The three that can be about a note elsewhere. A key is not an id
            // until the notebook says so, so a named one takes `open`'s route
            // back out to the runtime.
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
            // From the files screen rather than by name: `noda backlinks` has
            // to tell a note from a file because it gets a bare word, and here
            // the screen has already said which.
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

    /// For the one command happy with either answer: `log` reads a note and the
    /// notebook alike, so it is the only one that can follow the screen this
    /// way. Everything else needs a note and says so.
    fn about(&self) -> Option<String> {
        match &self.top().view {
            View::Note(id)
            | View::Blame(id)
            | View::Log(Some(id))
            | View::Backlinks(Subject::Note(id)) => Some(id.clone()),
            _ => None,
        }
    }

    /// The file under the cursor on the files screen, otherwise the note every
    /// other key aims at.
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

    /// Not a card: a card is for what a command said when it *ran*, and this
    /// never became one — the same class of thing as half a query.
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
        // Erasing and typing only: what is typed shows along the top of the
        // card and nowhere else, so there is no cursor for a motion key to move
        // — see `Field::erasing`.
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
            // The other spelling of the same two.
            KeyCode::Char('n') if ctrl => {
                self.commands_at = (self.commands_at + 1).min(shown.saturating_sub(1));
            }
            KeyCode::Char('p') if ctrl => {
                self.commands_at = self.commands_at.saturating_sub(1);
            }
            // Onto the prompt rather than run: most take something, and `push`
            // is not a thing to set off by landing on it.
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

    /// The id comes from `Notebook::resolve`, so what a key names is answered
    /// in one place.
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

    /// `y` agrees and anything else is a way out: the key that cancels a
    /// destructive question should be every key but one.
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

    /// Public because the runtime ran the command: the state asked for it and
    /// has no way of finding out how it went.
    pub fn report(&mut self, outcome: Result<String>) {
        self.message = Some(match outcome {
            Ok(text) => {
                let text = plain(&text).trim_end().to_string();
                // A line is an acknowledgement and lives on the status bar; more
                // than a line has to be read, so it gets the card.
                if text.lines().count() > 1 {
                    self.mode = Mode::Alert;
                }
                Message {
                    text,
                    failed: false,
                }
            }
            Err(e) => {
                // The whole of it: an editor that saved a broken block is told
                // where the file was left, which is the part that says what to
                // do next.
                self.mode = Mode::Alert;
                Message {
                    text: plain(&e.to_string()),
                    failed: true,
                }
            }
        });
    }

    /// For the runtime to tell a note just made from the ones already there.
    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.notes.iter().map(|file| file.id.as_str())
    }

    /// What `a` needs: a note made and then left in a list of two hundred is a
    /// note you have to go and find.
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

    /// Its last line, so the end can reach the top of the screen and no further.
    ///
    /// Counted before wrapping, so a note of long lines scrolls less far than it
    /// is tall: under-shooting leaves text reachable, over-shooting scrolls into
    /// blank space and reads like a bug.
    fn reading_height(&self) -> u16 {
        let lines = match self.content() {
            Some(Content::Note(text) | Content::Diff(text)) => text.lines().count(),
            Some(Content::Blame(lines)) => lines.len(),
            _ => 0,
        };
        lines.saturating_sub(1) as u16
    }

    /// Clamped to what the screen has. An empty list has no cursor at all, which
    /// is how the drawing knows not to highlight a row that is not there.
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
    /// narrow it, so there is no subset to refine, and the walk is well under a
    /// frame in memory.
    ///
    /// Split the way a shell would, quotes and all: `Query::parse` takes one
    /// token per argument so the shell's quoting is the only quoting, and there
    /// is no shell in front of this field. Without it `tag:"12.34 foo bar"` is
    /// three and-ed terms, and a tag you can read in the listing is one you
    /// cannot filter by.
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
            // Half a query is the ordinary state of one being typed. Say so and
            // leave the last good result up: emptying the list at every other
            // keystroke is the opposite of what filtering as you go is for.
            Err(e) => self.listing_mut().error = Some(e.to_string()),
        }
    }
}

enum Edge {
    First,
    Last,
}

/// A command's answer with its colour taken out.
///
/// `style` paints unconditionally and leaves `anstream` to strip it off a
/// stream. Here the other end is a terminal but not a stream — the answer goes
/// onto a card a character at a time, so an escape arrives as the text `[2m`.
/// Done where every answer passes rather than where each is shown.
///
/// It also matters for the card's size, measured from its longest line.
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

    /// Put into the state the way `refresh` puts it.
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
                extra: Vec::new(),
                body: body.to_string(),
            },
        }
    }

    // Fixed ids, never minted: one drawn from the notebook changes what it
    // asserts every run.
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

        // Every keystroke up to this one still spells a query; the count only
        // has to hold still across the one that breaks the grammar.
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

        // Ctrl-R arrives as `Char('r')` with a modifier: taken at face value the
        // field reads `budgetr`, and outside one the key reloads. An unbound
        // chord does neither.
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

        // The keys are the field's and tested there; what is checked here is
        // that the notebook is walked again — an edited query that was not re-run
        // leaves the listing describing a line nobody can see.
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
    fn the_picker_opens_on_every_tag_with_the_notes_own_ticked() {
        let mut app = an_app();
        app.on_key(key(KeyCode::Char('#')));
        assert_eq!(app.mode, Mode::Tagging);

        // The tags screen's list and order, both on the card whether or not this
        // note has them.
        let names: Vec<&str> = app.choices().iter().map(|c| c.tag.as_str()).collect();
        assert_eq!(names, vec!["work", "q3"]);
        assert_eq!(app.picking_notes(), 1);
        assert_eq!(app.picking_note().map(|f| f.id.as_str()), Some("aaaa1111"));

        // The box says what is true of the note, and the count says how much of
        // the notebook stands behind the tag.
        assert_eq!(app.choices()[0].tick(1), "[x]");
        assert_eq!(app.choices()[0].notes, 2);
        assert_eq!(app.choices()[1].tick(1), "[ ]");
    }

    #[test]
    fn tab_walks_a_tag_through_the_states_that_would_change_something() {
        let mut app = an_app();
        app.on_key(key(KeyCode::Char('#')));

        // Two over one note: a tag it already carries cannot be given again.
        app.on_key(key(KeyCode::Tab));
        assert_eq!(app.choices()[0].mark, Mark::Remove);
        app.on_key(key(KeyCode::Tab));
        assert_eq!(app.choices()[0].mark, Mark::Leave);

        // And the other way for one it does not carry.
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

        // Written here and typed nowhere: what came out is what `noda tag`
        // takes, and nothing was spelled to get it.
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

        // Nothing the filter admits, and one row after the end of that.
        assert!(app.shown_tags().is_empty());
        assert_eq!(
            app.proposal(),
            Some(Proposal::New {
                tag: "urgent".to_string(),
                near: None,
            })
        );
        assert_eq!(app.picker_rows(), 1);

        // Chosen like any other row, and a row like any other once it is.
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
        // The four shapes a slip takes, against a notebook whose commonest tag
        // is `work`.
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

        // And a word that is nothing like one is left alone: a warning on every
        // new tag is one nobody reads.
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

        // The row cannot be chosen, so there is nothing to send.
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

        // Two of the three carry `work`, so all three states say something.
        assert_eq!(app.choices()[0].held, 2);
        assert_eq!(app.choices()[0].tick(3), "[ ]");
        app.on_key(key(KeyCode::Tab));
        assert_eq!(app.choices()[0].mark, Mark::Add);
        app.on_key(key(KeyCode::Tab));
        assert_eq!(app.choices()[0].mark, Mark::Remove);
        app.on_key(key(KeyCode::Tab));
        assert_eq!(app.choices()[0].mark, Mark::Leave);

        // And with a set marked it queues rather than runs, exactly as the
        // prompt it replaced did.
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
        // The two notes that have `work`, and nothing else.
        mark(&mut app, &["aaaa1111", "bbbb2222"]);
        app.on_key(key(KeyCode::Char('#')));

        assert_eq!(app.choices()[0].tick(2), "[x]");
        // All of them have it, so giving it to them is not a state on offer.
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
        // Narrowing is a way of looking: a picker forgetting a choice the moment
        // it scrolls out of sight is one you cannot use twice.
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
        // What an import leaves behind. The filter is a field like any other, so
        // the space bar types a space — which is why `Tab` chooses.
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
                // One string from the card to the command's argument list, with
                // no quoting anywhere between: there is no shell here.
                changes: vec!["+24.04 Dark patterns".to_string()],
                touch: Touch::Stamp,
            })
        );
    }

    #[test]
    fn one_keystroke_apart_is_the_four_shapes_a_slip_takes() {
        // Case, a character dropped, one mistyped, and two swapped.
        assert!(one_edit_apart("Work", "work"));
        assert!(one_edit_apart("wor", "work"));
        assert!(one_edit_apart("works", "work"));
        assert!(one_edit_apart("wprk", "work"));
        assert!(one_edit_apart("wrok", "work"));
        assert!(one_edit_apart("q4", "q3"));
        // And what is not one: two edits, and words that merely start alike.
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
        tag_with(&mut app, "archive");

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
        // What `noda status` returns: the palette paints unconditionally and
        // `anstream` decides at a prompt. Nothing on a card goes through it.
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

    /// Name it, choose it, apply. Every tag named this way is one the notebook
    /// lacks, so it is the row at the end and one `Tab` is a `+`.
    fn tag_with(app: &mut App, tag: &str) {
        app.on_key(key(KeyCode::Char('#')));
        typing(app, tag);
        app.on_key(key(KeyCode::Tab));
        app.on_key(key(KeyCode::Enter));
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

        // Both of them carry `work`, so the box in front of it is ticked and
        // the first press of `Tab` is the one that takes it away.
        app.on_key(key(KeyCode::Char('#')));
        assert_eq!(app.choices()[0].tick(2), "[x]");
        app.on_key(key(KeyCode::Tab));
        typing(&mut app, "archive");
        app.on_key(key(KeyCode::Tab));
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
        tag_with(&mut app, "archive");

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
        // Refused where it was typed: `Tab` has nothing to choose, so there is
        // nothing for `cmd::check` to catch on the way into the queue.
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
        // Kept as typed rather than rebuilt from its tokens, which is the only
        // way a value with a space arrives whole.
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
        // Not resolved here: what a prefix or slug names has one implementation,
        // and a browser holding the notes could answer it a second way.
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
                // Nothing waiting to go out: this is about the restore, and a
                // margin either way does not change which row Enter lands on.
                std::collections::HashSet::new(),
            ),
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
