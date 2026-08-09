//! End-to-end tests for the browser: a real notebook on disk, real keystrokes,
//! and the frame that comes out the other side.
//!
//! What is checked here is what a person would see. The state machine's own
//! tests live next to it in `src/tui/app.rs` and never draw anything; these draw
//! into `ratatui`'s test backend, which is a buffer of characters rather than a
//! terminal — so the assertions are about the screen without any terminal being
//! involved.
//!
//! The harness is `tests/cli.rs`'s, restated rather than shared: an integration
//! test is its own crate, and the two are only a few lines each. The `unsigned`
//! one is not optional — libgit2 reads the developer's real git config, so a
//! machine that signs its commits would send every test in this file to gpg.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use noda::notebook::Notebook;
use noda::paths::Paths;
use noda::tui::app::App;
use noda::{cmd, tui};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

struct TempRoot(PathBuf);

impl TempRoot {
    fn new() -> Self {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("noda-tui-test-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&path).expect("create temp root");
        TempRoot(path)
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A notebook holding three notes, in the order the listing puts them:
/// `budget-review`, `meeting-notes`, `reading-list`.
fn a_notebook() -> (TempRoot, Paths) {
    let root = TempRoot::new();
    let paths = Paths::rooted(&root.0);
    std::fs::create_dir_all(paths.config_dir()).expect("config dir");
    std::fs::write(paths.config_dir().join("config.toml"), "sign = false\n").expect("config");
    cmd::init(&paths).expect("init");

    cmd::add(
        &paths,
        Some("Budget review"),
        Some("the q3 budget is late"),
        &["work".to_string()],
    )
    .expect("add");
    cmd::add(
        &paths,
        Some("Meeting notes"),
        Some("# Agenda\n\nthe budget, again"),
        &["work".to_string(), "q3".to_string()],
    )
    .expect("add");
    cmd::add(&paths, Some("Reading list"), Some("a book"), &[]).expect("add");
    (root, paths)
}

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

/// How many rows the standing header takes, which is where everything about the
/// notebook and the session is said.
const HEADER: usize = 5;

/// One frame, as the lines a reader would see. Trailing blanks are cut so an
/// assertion is about what was written, not about how wide the terminal was.
///
/// Tall enough for the standing header: on a shorter terminal it collapses to
/// one line, and a test drawn there would be asserting about the fallback.
fn screen(paths: &Paths, app: &mut App) -> Vec<String> {
    screen_at(paths, app, 90, 28)
}

fn screen_at(paths: &Paths, app: &mut App, width: u16, height: u16) -> Vec<String> {
    // The step the runtime takes between a keystroke and a frame: a screen that
    // has just been opened does not know what it is of until somebody goes and
    // looks.
    tui::refresh(paths, app);
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("test terminal");
    terminal
        .draw(|frame| tui::view::draw(frame, app))
        .expect("draw");
    let buffer = terminal.backend().buffer().clone();
    (0..buffer.area.height)
        .map(|y| {
            let line: String = (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect();
            line.trim_end().to_string()
        })
        .collect()
}

/// The note under the cursor, opened and drawn — what `Enter` gets you. The
/// session is left on the note, the way pressing the key leaves it.
fn opened(paths: &Paths, app: &mut App) -> Vec<String> {
    app.on_key(key(KeyCode::Enter));
    screen(paths, app)
}

fn has_line_with(screen: &[String], needles: &[&str]) -> bool {
    screen
        .iter()
        .any(|line| needles.iter().all(|needle| line.contains(needle)))
}

#[test]
fn the_listing_names_a_note_the_way_every_other_listing_does() {
    let (_root, paths) = a_notebook();
    let mut app = tui::load(&paths).expect("load");
    let screen = screen(&paths, &mut app);

    // The id first, the title, then the tags — the row `ls`, `search` and
    // `backlinks` all print. The id is minted, so it is the id of the note the
    // cursor is on that has to appear, not a literal.
    let id = app.selected().expect("a note is selected").id.clone();
    assert!(has_line_with(&screen, &[&id, "Budget review", "[work]"]));
    assert!(has_line_with(&screen, &["Meeting notes", "[work, q3]"]));
    // A note with no tags ends after its title rather than showing empty
    // brackets, the way `ls` ends the row.
    assert!(has_line_with(&screen, &["Reading list"]));
    assert!(!has_line_with(&screen, &["Reading list", "[]"]));
}

#[test]
fn the_header_says_where_the_notebook_stands() {
    let (_root, paths) = a_notebook();
    let mut app = tui::load(&paths).expect("load");
    let screen = screen(&paths, &mut app);

    // One fact to a line, always the same five in the same order, so the eye
    // learns where each one is rather than reading a strip left to right.
    //
    // The branch is read back out of the status rather than named: what a fresh
    // notebook's branch is called comes from whoever's `init.defaultBranch` is
    // in force, so a literal here passes on the machine it was written on and
    // fails on the next one.
    let branch = app.status.branch.clone();
    let head = &screen[..HEADER];
    assert!(has_line_with(head, &["Notebook:", cmd::DEFAULT_NOTEBOOK]));
    assert!(has_line_with(head, &["Branch:", &branch]));
    assert!(has_line_with(head, &["Remote:", "none"]));
    assert!(has_line_with(head, &["Notes:", "3 notes"]));
    assert!(has_line_with(head, &["Changes:", "none"]));
    // And the keys for the screen you are on, beside them.
    assert!(has_line_with(head, &["<enter>", "read"]));
    assert!(has_line_with(head, &["<ctrl-d>", "delete"]));
}

#[test]
fn the_header_holds_still_while_the_session_changes_underneath_it() {
    let (_root, paths) = a_notebook();
    let mut app = tui::load(&paths).expect("load");
    let before = screen(&paths, &mut app)[..HEADER].to_vec();

    // Marking, queueing and the flag are the only things that move while you
    // sit there, and they are said on the title band for exactly this reason:
    // in the block they widened it, which pushed the keys along and dropped the
    // rightmost column — so marking a note hid the keys about marking.
    app.on_key(key(KeyCode::Char('*')));
    app.on_key(key(KeyCode::Char('T')));
    app.on_key(key(KeyCode::Char('#')));
    typing(&mut app, "+archive");
    app.on_key(key(KeyCode::Enter));

    let after = screen(&paths, &mut app);
    assert_eq!(after[..HEADER], before[..], "the header moved");
    assert!(has_line_with(&after, &["3 marks", "1 queued"]));
    assert!(has_line_with(&after[..HEADER], &["<space>", "mark"]));
}

#[test]
fn the_way_to_everything_else_is_on_the_header_of_every_screen() {
    let (_root, paths) = a_notebook();
    let mut app = tui::load(&paths).expect("load");

    // Eighty columns, which is the narrow end of what anybody has and the width
    // at which the key grid starts dropping its rightmost column. `:` is the
    // one key that cannot be looked up if it is not shown — everything it
    // reaches is reached by knowing it exists.
    let listing = screen_at(&paths, &mut app, 80, 28);
    assert!(has_line_with(&listing[..HEADER], &["<:>", "command"]));
    assert!(has_line_with(&listing[..HEADER], &["<ctrl-a>", "commands"]));

    app.on_key(key(KeyCode::Enter));
    let note = screen_at(&paths, &mut app, 80, 28);
    assert!(has_line_with(&note[..HEADER], &["<:>", "command"]));
    assert!(has_line_with(&note[..HEADER], &["<ctrl-a>", "commands"]));
    // And the way back out, which is the other thing a screen must never hide.
    assert!(has_line_with(&note[..HEADER], &["<esc>", "back"]));
    assert!(has_line_with(&note[..HEADER], &["<q>", "quit"]));
}

#[test]
fn the_title_band_says_what_the_screen_is_of() {
    let (_root, paths) = a_notebook();
    let mut app = tui::load(&paths).expect("load");
    assert!(has_line_with(&screen(&paths, &mut app), &["Notes(all)[3]"]));

    // A note names itself the way every other listing names it: the id, then
    // the title.
    let id = app.selected().expect("a note").id.clone();
    let opened = opened(&paths, &mut app);
    assert!(has_line_with(
        &opened,
        &[&format!("Note({id})"), "Budget review"]
    ));
}

#[test]
fn a_note_opens_on_a_screen_of_its_own_and_escape_comes_back() {
    let (_root, paths) = a_notebook();
    let mut app = tui::load(&paths).expect("load");

    // Nothing of the note is on the listing: the row is the row `noda ls`
    // prints, and reading it is a screen you go into.
    let listing = screen(&paths, &mut app);
    assert!(!has_line_with(&listing, &["the q3 budget is late"]));

    let id = app.selected().expect("a note").id.clone();
    let first = opened(&paths, &mut app);
    assert!(has_line_with(&first, &["the q3 budget is late"]));
    // The frontmatter is on screen too — dimmed, not hidden, exactly as
    // `noda show` prints it.
    assert!(has_line_with(&first, &["title: Budget review"]));
    // And the trail says how far down you are, naming the note by its id.
    assert!(has_line_with(&first, &["notes", &id]));

    app.on_key(key(KeyCode::Esc));
    let back = screen(&paths, &mut app);
    assert!(has_line_with(&back, &["Meeting notes"]));
    assert!(
        !has_line_with(&back, &["the q3 budget is late"]),
        "the note's body went with its screen"
    );

    // The next note down, and it is the one that opens.
    app.on_key(key(KeyCode::Char('j')));
    let moved = opened(&paths, &mut app);
    assert!(has_line_with(&moved, &["# Agenda"]));
    assert!(!has_line_with(&moved, &["the q3 budget is late"]));
}

#[test]
fn a_query_narrows_the_listing_as_it_is_typed() {
    let (_root, paths) = a_notebook();
    let mut app = tui::load(&paths).expect("load");

    app.on_key(key(KeyCode::Char('/')));
    typing(&mut app, "tag:q3");
    let screen = screen(&paths, &mut app);

    assert!(has_line_with(&screen, &["Meeting notes"]));
    assert!(!has_line_with(&screen, &["Budget review"]));
    assert!(!has_line_with(&screen, &["Reading list"]));
    // The query stays visible while it is being typed, and the title band says
    // what it narrowed to and how much it left.
    assert!(has_line_with(&screen, &["/tag:q3"]));
    assert!(has_line_with(&screen, &["Notes(tag:q3)[1]"]));
}

#[test]
fn an_unfinished_query_says_why_instead_of_emptying_the_screen() {
    let (_root, paths) = a_notebook();
    let mut app = tui::load(&paths).expect("load");

    app.on_key(key(KeyCode::Char('/')));
    // Still a query: a bare `O` is a `text:` term, and one note's title has one.
    typing(&mut app, "tag:work O");
    let good = screen(&paths, &mut app);
    assert!(has_line_with(&good, &["Meeting notes"]));

    // And now it is not one — the state every alternative passes through.
    typing(&mut app, "R");
    let unfinished = screen(&paths, &mut app);
    assert!(has_line_with(&unfinished, &["needs a term on both sides"]));
    assert!(
        has_line_with(&unfinished, &["Meeting notes"]),
        "the last good result is still on screen"
    );
}

#[test]
fn a_query_that_matches_nothing_says_so() {
    let (_root, paths) = a_notebook();
    let mut app = tui::load(&paths).expect("load");

    app.on_key(key(KeyCode::Char('/')));
    typing(&mut app, "tag:archived");
    let screen = screen(&paths, &mut app);

    assert!(has_line_with(&screen, &["nothing matches"]));
    assert!(has_line_with(&screen, &["Notes(tag:archived)[0]"]));
}

#[test]
fn the_help_card_lists_the_keys_and_goes_away_again() {
    let (_root, paths) = a_notebook();
    let mut app = tui::load(&paths).expect("load");

    app.on_key(key(KeyCode::Char('?')));
    let with_help = screen(&paths, &mut app);
    assert!(has_line_with(&with_help, &["keys"]));
    assert!(has_line_with(&with_help, &["quit"]));
    // The card is as wide as its longest line. The filter example is the one
    // entry that says something the key beside it cannot, so a card that cut it
    // off would be dropping the only part worth reading twice.
    assert!(has_line_with(
        &with_help,
        &["filter: tag:work OR tag:q3 budget"]
    ));

    app.on_key(key(KeyCode::Esc));
    let without = screen(&paths, &mut app);
    assert!(!has_line_with(&without, &["first / last"]));
}

#[test]
fn a_reload_picks_up_a_note_written_from_somewhere_else() {
    let (_root, paths) = a_notebook();
    let mut app = tui::load(&paths).expect("load");
    assert_eq!(app.total(), 3);

    cmd::add(&paths, Some("Trip plan"), Some("flights"), &[]).expect("add");
    // The state machine says what to do; the runtime is what does it, so the
    // test does what the runtime would.
    assert_eq!(
        app.on_key(key(KeyCode::Char('r'))),
        Some(tui::Action::Reload)
    );
    tui::reload(&paths, &mut app).expect("reload");

    let screen = screen(&paths, &mut app);
    assert!(has_line_with(&screen, &["Trip plan"]));
}

/// What the runtime does with an action, for the ones that do not want a
/// terminal of their own: run the command it names, take down what it said, and
/// read the notebook again. `Edit` and `Add` are left out — both hand the
/// terminal to `$EDITOR`, and there is no terminal here to hand over.
fn perform(paths: &Paths, app: &mut App, action: tui::Action) {
    let outcome = match action {
        tui::Action::Tag {
            key,
            changes,
            touch,
        } => cmd::tag(paths, &key, &changes, touch),
        tui::Action::Retitle { key, title, touch } => cmd::mv(paths, &key, &title, false, touch),
        tui::Action::Remove(key) => cmd::rm(paths, &key),
        tui::Action::Restore { key, rev, touch } => cmd::restore(paths, &key, &rev, touch),
        // A screen about a note the prompt named, resolved by the notebook
        // rather than by the browser — the same call `Open` makes.
        tui::Action::Show { key, look } => {
            let notebook = Notebook::open_active(paths).expect("open the notebook");
            match notebook.resolve(&key) {
                Ok((id, _)) => {
                    app.look_at(look, id);
                    return;
                }
                Err(e) => Err(e),
            }
        }
        tui::Action::Use(name) => match cmd::use_notebook(paths, &name) {
            Ok(said) => {
                *app = tui::load(paths).expect("load the notebook moved to");
                app.report(Ok(said));
                return;
            }
            Err(e) => Err(e),
        },
        tui::Action::Send(steps) => {
            let sent = cmd::bulk(paths, &steps);
            if sent.is_ok() {
                app.sent();
            }
            sent
        }
        // What a key names is the notebook's question, and this is where the
        // runtime asks it — the same call `noda show` makes, so an id prefix
        // that names two notes is refused in the same words here as there.
        tui::Action::Open(key) => {
            let notebook = Notebook::open_active(paths).expect("open the notebook");
            match notebook.resolve(&key) {
                Ok((id, _)) => {
                    tui::reload(paths, app).expect("reload");
                    app.open_note(id);
                    return;
                }
                Err(e) => Err(e),
            }
        }
        tui::Action::Run(run) => match run {
            tui::Run::Status => cmd::status(paths),
            tui::Run::Doctor { links, times } => cmd::doctor(paths, true, links, times),
            tui::Run::Readme => cmd::readme(paths, false),
            tui::Run::Snapshot(Some(name)) => cmd::snapshot(paths, &name, None),
            tui::Run::Snapshot(None) => cmd::snapshot_ls(paths),
            // The three that go to the network are left out: there is no remote
            // here, and what they do is `cmd`'s to be tested.
            other => panic!("{other:?} wants a remote"),
        },
        other => panic!("{other:?} wants a terminal of its own"),
    };
    app.report(outcome);
    tui::reload(paths, app).expect("reload");
}

/// How many commits the notebook has. One line per commit is what `log` prints.
fn commits(paths: &Paths) -> usize {
    cmd::log(paths, None, None).expect("log").lines().count()
}

/// Marks the note under the cursor and every one after it, `Space` by `Space`.
fn mark_all_shown(app: &mut App) {
    app.on_key(key(KeyCode::Char('*')));
}

#[test]
fn a_queue_arrives_in_the_history_as_one_commit() {
    let (_root, paths) = a_notebook();
    let mut app = tui::load(&paths).expect("load");
    let before = commits(&paths);

    mark_all_shown(&mut app);
    assert!(has_line_with(&screen(&paths, &mut app), &["3 marks"]));

    for tags in ["+archive", "-work"] {
        app.on_key(key(KeyCode::Char('#')));
        typing(&mut app, tags);
        assert_eq!(
            app.on_key(key(KeyCode::Enter)),
            None,
            "a queued change is not run"
        );
    }
    assert_eq!(app.queue.len(), 2);
    assert!(has_line_with(&screen(&paths, &mut app), &["2 queued"]));

    app.on_key(key(KeyCode::Char('Q')));
    let queued = screen(&paths, &mut app);
    // The queue reads in the words the commit message will use.
    assert!(has_line_with(&queued, &["tag: +archive (3 notes)"]));
    assert!(has_line_with(&queued, &["tag: -work (3 notes)"]));

    let action = app.on_key(key(KeyCode::Enter)).expect("a queue to send");
    perform(&paths, &mut app, action);

    assert_eq!(
        commits(&paths) - before,
        1,
        "two changes over three notes, and one thing was done"
    );
    let after = screen(&paths, &mut app);
    assert!(has_line_with(&after, &["2 changes over 3 notes"]));
    assert!(
        has_line_with(&after, &["[q3, archive]"]),
        "and the listing says so"
    );
    // Read out of the file itself, on the screen that shows the file.
    let file = opened(&paths, &mut app);
    assert!(has_line_with(&file, &["tags: [archive]"]));
    app.on_key(key(KeyCode::Esc));
    // Spent: the queue was carried out and the notes are no longer picked out.
    assert!(app.queue.is_empty());
    assert!(app.marks.is_empty());
}

#[test]
fn marks_made_under_one_query_survive_the_next() {
    let (_root, paths) = a_notebook();
    let mut app = tui::load(&paths).expect("load");

    // One note from one search, marked under a query that the next search will
    // not show.
    app.on_key(key(KeyCode::Char('/')));
    typing(&mut app, "tag:q3");
    app.on_key(key(KeyCode::Enter));
    mark_all_shown(&mut app);
    let first = app.selected().expect("a note").id.clone();

    app.on_key(key(KeyCode::Esc));
    app.on_key(key(KeyCode::Char('/')));
    typing(&mut app, "book");
    app.on_key(key(KeyCode::Enter));
    assert_eq!(app.shown(), 1);
    assert_ne!(
        app.selected().map(|f| f.id.clone()),
        Some(first.clone()),
        "the first note is not in this result"
    );
    mark_all_shown(&mut app);

    // One from each search, and the first was never in sight for the second.
    assert_eq!(app.marks.len(), 2);

    app.on_key(key(KeyCode::Char('#')));
    typing(&mut app, "+seen");
    app.on_key(key(KeyCode::Enter));
    app.on_key(key(KeyCode::Char('Q')));
    let action = app.on_key(key(KeyCode::Enter)).expect("a queue to send");
    perform(&paths, &mut app, action);

    app.on_key(key(KeyCode::Esc));
    // Read out of the notebook rather than off the screen: which notes were
    // changed is what this is about, and the listing's columns give way to each
    // other at whatever width the terminal happens to be.
    let seen: Vec<&str> = app
        .rows()
        .filter(|file| file.note.tags.iter().any(|tag| tag == "seen"))
        .map(|file| file.note.title.as_str())
        .collect();
    // One from each search, and nothing else: a mark is a note picked out, not
    // a query re-run at the moment of sending.
    assert_eq!(seen, vec!["Meeting notes", "Reading list"]);
}

#[test]
fn a_tag_long_enough_to_fill_the_listing_does_not_take_the_title_with_it() {
    let (_root, paths) = a_notebook();
    // The shape an import leaves behind, and wide enough to swallow a narrow
    // listing whole.
    cmd::add(
        &paths,
        Some("Ubuntu notes"),
        Some("body"),
        &["24.04 Dark patterns".to_string()],
    )
    .expect("add");
    let mut app = tui::load(&paths).expect("load");
    // Narrow on purpose. The listing has the whole width now, so a tag list
    // this long no longer starves the title at eighty columns — but a terminal
    // is whatever size it is given, and the cap is what holds at the size where
    // it still would.
    let screen = screen_at(&paths, &mut app, 46, 28);

    // Without the cap, a tag list this long takes the row whole and leaves the
    // title column nothing at all. The title keeps its floor — enough to tell
    // the notes apart — and the tags are what gets cut.
    for title in ["Budget rev", "Meeting no", "Reading li"] {
        assert!(has_line_with(&screen, &[title]), "{title}");
    }
    assert!(
        has_line_with(&screen, &["[24.04 Dark"]),
        "and the long tag list still says it is there"
    );
}

#[test]
fn a_tag_with_a_space_can_be_filtered_for_from_the_screen_it_is_on() {
    let (_root, paths) = a_notebook();
    cmd::add(
        &paths,
        Some("Ubuntu notes"),
        Some("body"),
        &["24.04 Dark patterns".to_string()],
    )
    .expect("add");
    let mut app = tui::load(&paths).expect("load");

    app.on_key(key(KeyCode::Char('/')));
    // The shell would keep this in one piece, and so does the field.
    typing(&mut app, "tag:\"24.04 Dark patterns\"");
    let screen = screen(&paths, &mut app);
    assert!(has_line_with(&screen, &["[1]"]));
    assert_eq!(
        app.selected().map(|f| f.note.title.clone()),
        Some("Ubuntu notes".to_string())
    );
}

#[test]
fn leaving_with_a_queue_in_hand_is_asked_about() {
    let (_root, paths) = a_notebook();
    let mut app = tui::load(&paths).expect("load");

    app.on_key(key(KeyCode::Char('*')));
    app.on_key(key(KeyCode::Char('#')));
    typing(&mut app, "+archive");
    app.on_key(key(KeyCode::Enter));

    assert_eq!(app.on_key(key(KeyCode::Char('q'))), None, "not yet");
    let asked = screen(&paths, &mut app);
    assert!(has_line_with(&asked, &["leave the queue behind?"]));
    assert!(has_line_with(&asked, &["1 change over 3 notes"]));
    assert!(has_line_with(&asked, &["written down anywhere"]));

    // Staying keeps it, and it can still be sent.
    app.on_key(key(KeyCode::Esc));
    assert_eq!(app.queue.len(), 1);
    app.on_key(key(KeyCode::Char('Q')));
    assert!(matches!(
        app.on_key(key(KeyCode::Enter)),
        Some(tui::Action::Send(_))
    ));
}

#[test]
fn a_queued_delete_takes_every_note_it_was_aimed_at() {
    let (_root, paths) = a_notebook();
    let mut app = tui::load(&paths).expect("load");
    let before = commits(&paths);

    app.on_key(key(KeyCode::Char('/')));
    typing(&mut app, "tag:work");
    app.on_key(key(KeyCode::Enter));
    mark_all_shown(&mut app);
    app.on_key(ctrl('d'));

    app.on_key(key(KeyCode::Char('Q')));
    let queued = screen(&paths, &mut app);
    assert!(has_line_with(&queued, &["rm: 2 notes"]));

    // The send is where the question is asked, and only because of the delete.
    assert_eq!(app.on_key(key(KeyCode::Enter)), None);
    let asked = screen(&paths, &mut app);
    assert!(has_line_with(&asked, &["send the queue?"]));
    assert!(has_line_with(&asked, &["2 notes to be deleted"]));

    let action = app
        .on_key(key(KeyCode::Char('y')))
        .expect("a queue to send");
    perform(&paths, &mut app, action);

    app.on_key(key(KeyCode::Esc));
    let after = screen(&paths, &mut app);
    assert_eq!(app.total(), 1);
    assert!(has_line_with(&after, &["Reading list"]));
    assert!(!has_line_with(&after, &["Budget review"]));
    assert_eq!(commits(&paths) - before, 1);
}

#[test]
fn a_tag_typed_at_the_prompt_is_on_the_note_afterwards() {
    let (_root, paths) = a_notebook();
    let mut app = tui::load(&paths).expect("load");

    app.on_key(key(KeyCode::Char('#')));
    typing(&mut app, "+urgent");
    // The prompt is on the same line the query uses, and says which it is.
    let prompt = screen(&paths, &mut app);
    assert!(has_line_with(&prompt, &["tags", "+urgent"]));
    assert!(
        has_line_with(&prompt, &["+work -q3"]),
        "the syntax is shown"
    );

    let action = app.on_key(key(KeyCode::Enter)).expect("a tag to apply");
    perform(&paths, &mut app, action);

    // The status line is `noda tag`'s own answer, not a sentence noda wrote
    // twice. It is read first, because the next key is what takes it away.
    let after = screen(&paths, &mut app);
    let id = app.selected().expect("a note").id.clone();
    assert!(has_line_with(&after, &[&id, "budget-review", "urgent"]));

    // And read out of the file the command wrote, on the screen that shows it.
    let file = opened(&paths, &mut app);
    assert!(has_line_with(&file, &["tags: [work, urgent]"]));
}

#[test]
fn a_tag_with_a_space_in_it_can_be_removed_from_the_screen_it_is_on() {
    let (_root, paths) = a_notebook();
    // The shape an import leaves behind: a tag is allowed a space, and the one
    // that has one is the one you most want to be rid of.
    cmd::add(
        &paths,
        Some("Ubuntu notes"),
        Some("body"),
        &["24.04 Dark patterns".to_string(), "work".to_string()],
    )
    .expect("add");
    let mut app = tui::load(&paths).expect("load");
    app.on_key(key(KeyCode::Char('G')));
    assert_eq!(
        app.selected().map(|f| f.note.title.clone()),
        Some("Ubuntu notes".to_string())
    );

    app.on_key(key(KeyCode::Char('#')));
    typing(&mut app, "-\"24.04 Dark patterns\"");
    let action = app.on_key(key(KeyCode::Enter)).expect("a tag to remove");
    perform(&paths, &mut app, action);

    let after = opened(&paths, &mut app);
    assert!(
        has_line_with(&after, &["tags: [work]"]),
        "the spaced tag is gone and the other one is not"
    );
    assert!(!has_line_with(&after, &["Dark patterns"]));
}

#[test]
fn a_retitle_renames_the_note_and_keeps_the_cursor_on_it() {
    let (_root, paths) = a_notebook();
    let mut app = tui::load(&paths).expect("load");
    let id = app.selected().expect("a note").id.clone();

    app.on_key(key(KeyCode::Char('m')));
    for _ in 0.."Budget review".len() {
        app.on_key(key(KeyCode::Backspace));
    }
    typing(&mut app, "Quarterly plan");
    let action = app.on_key(key(KeyCode::Enter)).expect("a retitle");
    perform(&paths, &mut app, action);

    let after = screen(&paths, &mut app);
    assert!(has_line_with(&after, &["Quarterly plan"]));
    assert!(!has_line_with(&after, &["Budget review"]));
    // The slug moved it down the listing; the id is what the cursor followed.
    assert_eq!(app.selected().map(|f| f.id.clone()), Some(id));
    assert!(
        has_line_with(&opened(&paths, &mut app), &["title: Quarterly plan"]),
        "and it is read from the file it is now in"
    );
}

#[test]
fn a_delete_is_asked_about_and_then_carried_out() {
    let (_root, paths) = a_notebook();
    let mut app = tui::load(&paths).expect("load");

    // Behind a modifier: the plain key says where the deleting went and does
    // nothing, which is what a key that used to remove a note has to do.
    app.on_key(key(KeyCode::Char('d')));
    assert!(has_line_with(
        &screen(&paths, &mut app),
        &["delete is Ctrl-d"]
    ));
    assert_eq!(app.total(), 3);

    app.on_key(ctrl('d'));
    let asked = screen(&paths, &mut app);
    assert!(has_line_with(&asked, &["delete this note?"]));
    assert!(has_line_with(&asked, &["Budget review"]));
    assert!(has_line_with(&asked, &["git revert brings it back"]));

    let action = app.on_key(key(KeyCode::Char('y'))).expect("a delete");
    perform(&paths, &mut app, action);

    let after = screen(&paths, &mut app);
    assert!(!has_line_with(&after, &["Budget review"]));
    assert_eq!(app.total(), 2);
    // The row is kept rather than the id, so the cursor lands on the note that
    // has taken its place.
    assert_eq!(
        app.selected().map(|f| f.note.title.clone()),
        Some("Meeting notes".to_string())
    );
}

/// The `updated:` line of the note under the cursor, as it stands on disk.
fn updated(paths: &Paths, app: &App) -> String {
    let id = app.selected().expect("a note").id.clone();
    let file = cmd::path(paths, Some(&id)).expect("the note's path");
    let text = std::fs::read_to_string(file.trim()).expect("read the note");
    text.lines()
        .find(|line| line.starts_with("updated:"))
        .expect("an updated field")
        .to_string()
}

/// Puts a date on the note that no clock will produce, so that "it did not move"
/// is a claim about the flag rather than about how fast the test ran.
fn backdate(paths: &Paths, app: &App) {
    let id = app.selected().expect("a note").id.clone();
    let file = cmd::path(paths, Some(&id)).expect("the note's path");
    let file = file.trim().to_string();
    let text = std::fs::read_to_string(&file).expect("read the note");
    let older: String = text
        .lines()
        .map(|line| {
            if line.starts_with("updated:") {
                "updated: 2019-03-04T05:06:07Z".to_string()
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&file, format!("{older}\n")).expect("write the note back");
}

#[test]
fn t_holds_a_notes_own_updated_through_a_change() {
    let (_root, paths) = a_notebook();
    let mut app = tui::load(&paths).expect("load");
    backdate(&paths, &app);
    assert_eq!(updated(&paths, &app), "updated: 2019-03-04T05:06:07Z");

    // Off, and the header says nothing about it.
    assert!(!has_line_with(
        &screen(&paths, &mut app),
        &["keeping updated"]
    ));

    app.on_key(key(KeyCode::Char('T')));
    let with_flag = screen(&paths, &mut app);
    assert!(
        has_line_with(&with_flag, &["keeping updated"]),
        "a setting you cannot see is one you forget you left on"
    );

    app.on_key(key(KeyCode::Char('#')));
    typing(&mut app, "+urgent");
    let action = app.on_key(key(KeyCode::Enter)).expect("a tag to apply");
    perform(&paths, &mut app, action);

    let after = opened(&paths, &mut app);
    assert!(
        has_line_with(&after, &["tags: [work, urgent]"]),
        "the change went in"
    );
    assert_eq!(
        updated(&paths, &app),
        "updated: 2019-03-04T05:06:07Z",
        "the date the note came with is still the date it carries"
    );
    app.on_key(key(KeyCode::Esc));

    // Off again, and the next change records itself as every other one does.
    app.on_key(key(KeyCode::Char('T')));
    assert!(!has_line_with(
        &screen(&paths, &mut app),
        &["keeping updated"]
    ));
    app.on_key(key(KeyCode::Char('#')));
    typing(&mut app, "-urgent");
    let action = app.on_key(key(KeyCode::Enter)).expect("a tag to apply");
    perform(&paths, &mut app, action);
    assert_ne!(updated(&paths, &app), "updated: 2019-03-04T05:06:07Z");
}

#[test]
fn a_change_the_command_refuses_is_reported_in_its_own_words() {
    let (_root, paths) = a_notebook();
    let mut app = tui::load(&paths).expect("load");

    app.on_key(key(KeyCode::Char('#')));
    // Tags written the way the listing prints them, which is the way the
    // frontmatter cannot hold them.
    typing(&mut app, "+q3,urgent");
    let action = app.on_key(key(KeyCode::Enter)).expect("a tag to apply");
    perform(&paths, &mut app, action);

    let after = screen(&paths, &mut app);
    assert!(has_line_with(&after, &["a tag cannot contain `,`"]));
    assert!(
        has_line_with(&after, &["Budget review", "[work]"]),
        "the note is as it was"
    );
}

/// Types a line at the command prompt and presses Enter, the way `:` does.
fn command(app: &mut App, line: &str) -> Option<tui::Action> {
    app.on_key(key(KeyCode::Char(':')));
    typing(app, line);
    app.on_key(key(KeyCode::Enter))
}

#[test]
fn a_command_reaches_what_no_key_does() {
    let (_root, paths) = a_notebook();
    let mut app = tui::load(&paths).expect("load");

    // `noda status` has no key and is not going to get one; this is the whole
    // reason the prompt exists.
    let action = command(&mut app, "status").expect("a command to run");
    perform(&paths, &mut app, action);

    // More than a line, so it is read on a card rather than in passing.
    let after = screen(&paths, &mut app);
    assert!(has_line_with(&after, &["3 notes"]), "{after:?}");
    assert!(has_line_with(&after, &["branch"]) || has_line_with(&after, &["clean"]));
}

#[test]
fn a_note_can_be_opened_by_name_from_the_prompt() {
    let (_root, paths) = a_notebook();
    let mut app = tui::load(&paths).expect("load");

    // By slug, without going and finding it in the listing first.
    let action = command(&mut app, "open reading-list").expect("a note to open");
    perform(&paths, &mut app, action);

    let opened = screen(&paths, &mut app);
    assert_eq!(app.depth(), 2);
    assert!(has_line_with(&opened, &["Note(", "Reading list"]));
    assert!(has_line_with(&opened, &["a book"]));
}

#[test]
fn a_name_that_names_nothing_is_refused_in_the_notebooks_own_words() {
    let (_root, paths) = a_notebook();
    let mut app = tui::load(&paths).expect("load");

    let action = command(&mut app, "open nowhere").expect("a note to open");
    perform(&paths, &mut app, action);

    // Not a sentence the browser wrote: the answer `noda show nowhere` would
    // have given, on a card because it is a refusal.
    let after = screen(&paths, &mut app);
    assert!(has_line_with(&after, &["nowhere"]), "{after:?}");
    assert_eq!(app.depth(), 1, "nothing opened");
}

#[test]
fn a_tag_can_be_changed_from_the_command_line_too() {
    let (_root, paths) = a_notebook();
    let mut app = tui::load(&paths).expect("load");

    // Naming the note, which the `#` key has no way to do.
    let action = command(&mut app, "tag reading-list +urgent").expect("a tag to apply");
    perform(&paths, &mut app, action);

    app.on_key(key(KeyCode::Char('G')));
    let after = screen(&paths, &mut app);
    assert!(
        has_line_with(&after, &["Reading list", "[urgent]"]),
        "{after:?}"
    );
}

#[test]
fn the_command_list_is_narrowed_by_what_a_command_does() {
    let (_root, paths) = a_notebook();
    let mut app = tui::load(&paths).expect("load");

    app.on_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL));
    let listed = screen(&paths, &mut app);
    assert!(has_line_with(&listed, &["commands"]));
    assert!(has_line_with(&listed, &["open <note>"]));

    // Typed at, it narrows on what the commands do rather than only on their
    // names — which is the way somebody who knows the job and not the name will
    // look for it.
    typing(&mut app, "remote");
    let narrowed = screen(&paths, &mut app);
    assert!(has_line_with(&narrowed, &["push"]));
    assert!(has_line_with(&narrowed, &["pull"]));
    assert!(!has_line_with(&narrowed, &["open <note>"]));
}

#[test]
fn a_line_that_is_not_a_command_stays_on_the_line() {
    let (_root, paths) = a_notebook();
    let mut app = tui::load(&paths).expect("load");

    assert_eq!(command(&mut app, "frobnicate"), None);
    let after = screen(&paths, &mut app);
    assert!(has_line_with(&after, &["frobnicate"]));
    // And the notebook is untouched behind it.
    assert!(has_line_with(&after, &["Budget review"]));
    assert_eq!(app.total(), 3);
}

#[test]
fn it_refuses_to_run_where_there_is_no_terminal() {
    let (_root, paths) = a_notebook();
    // Under a test runner stdout is a pipe, which is the case this guards: a
    // full-screen program writing escape sequences into `less` or a file is not
    // something to discover halfway through.
    if std::io::IsTerminal::is_terminal(&std::io::stdout()) {
        return;
    }
    let refused = tui::run(&paths).expect_err("a pipe is not a terminal");
    assert!(refused.to_string().contains("needs a terminal"));
}

/// A notebook with something for every screen to say: a note that links to
/// another by id, an unticked box that went overdue years ago, an attachment
/// nothing uses, and a deletion in the history.
///
/// Built with `cmd::add` rather than by writing files, so the link can name the
/// budget's real id — which is what a backlink is matched on, and the only way
/// to have one that survives a retitle.
fn a_worked_notebook() -> (TempRoot, Paths) {
    let root = TempRoot::new();
    let paths = Paths::rooted(&root.0);
    std::fs::create_dir_all(paths.config_dir()).expect("config dir");
    std::fs::write(paths.config_dir().join("config.toml"), "sign = false\n").expect("config");
    cmd::init(&paths).expect("init");

    let added = cmd::add(
        &paths,
        Some("Budget review"),
        Some("the q3 budget is late"),
        &["work".to_string()],
    )
    .expect("add");
    let id = added
        .split_whitespace()
        .next()
        .expect("add says the id first")
        .to_string();
    cmd::add(
        &paths,
        Some("Meeting notes"),
        Some(&format!(
            "see [the budget]({id}-budget-review.md)\n\n- [ ] book a room due:2020-01-01\n"
        )),
        &["work".to_string(), "q3".to_string()],
    )
    .expect("add");
    cmd::add(&paths, Some("Reading list"), Some("a book"), &[]).expect("add");
    cmd::add(
        &paths,
        Some("Trip plan"),
        Some("flights"),
        &["travel".to_string()],
    )
    .expect("add");
    cmd::rm(&paths, "trip-plan").expect("rm");

    std::fs::write(
        paths
            .notebook_dir(cmd::DEFAULT_NOTEBOOK)
            .join("diagram.png"),
        b"png",
    )
    .expect("an attachment");
    (root, paths)
}

/// Opens a screen by naming it at the prompt, and gives the runtime its chance
/// to go and read whatever the screen turns out to be of.
fn go(paths: &Paths, app: &mut App, line: &str) -> Vec<String> {
    if let Some(action) = command(app, line) {
        perform(paths, app, action);
    }
    screen(paths, app)
}

#[test]
fn the_todo_screen_lists_the_boxes_with_the_dates_that_have_been_missed() {
    let (_root, paths) = a_worked_notebook();
    let mut app = tui::load(&paths).expect("load");

    let todo = go(&paths, &mut app, "todo");
    assert!(has_line_with(&todo, &["Todo", "[1]"]), "{todo:#?}");
    assert!(
        has_line_with(&todo, &["meeting-notes", "2020-01-01", "book a room"]),
        "{todo:#?}"
    );

    // The key means what the name means, which is the whole reason four of them
    // have one.
    app.on_key(key(KeyCode::Esc));
    app.on_key(key(KeyCode::Char('t')));
    assert!(has_line_with(&screen(&paths, &mut app), &["book a room"]));
}

#[test]
fn the_tags_screen_counts_them_and_enter_narrows_the_listing() {
    let (_root, paths) = a_worked_notebook();
    let mut app = tui::load(&paths).expect("load");

    let tags = go(&paths, &mut app, "tags");
    assert!(has_line_with(&tags, &["work", "2 notes"]), "{tags:#?}");

    // Not a screen of its own: a tag narrows the listing, and the listing is
    // where the notes it narrows already are.
    app.on_key(key(KeyCode::Enter));
    let narrowed = screen(&paths, &mut app);
    assert!(
        has_line_with(&narrowed, &["Notes(tag:work)[2]"]),
        "{narrowed:#?}"
    );
    assert_eq!(app.crumbs().collect::<Vec<_>>(), ["notes"]);
}

#[test]
fn the_backlinks_screen_finds_the_note_that_points_here() {
    let (_root, paths) = a_worked_notebook();
    let mut app = tui::load(&paths).expect("load");

    // The cursor opens on the budget, which is what the meeting notes link to.
    let found = go(&paths, &mut app, "backlinks");
    assert!(has_line_with(&found, &["Backlinks("]), "{found:#?}");
    assert!(has_line_with(&found, &["Meeting notes"]), "{found:#?}");

    // A row that names a note opens it, here as on the listing.
    app.on_key(key(KeyCode::Enter));
    let note = screen(&paths, &mut app);
    assert!(
        has_line_with(&note, &["Note(", "Meeting notes"]),
        "{note:#?}"
    );
}

#[test]
fn the_files_screen_leads_to_what_uses_an_attachment() {
    let (_root, paths) = a_worked_notebook();
    let mut app = tui::load(&paths).expect("load");

    let files = go(&paths, &mut app, "files");
    assert!(has_line_with(&files, &["diagram.png"]), "{files:#?}");

    // Nothing uses it, which is a finding — the one `doctor --links` reports as
    // an orphan — rather than an empty screen.
    app.on_key(key(KeyCode::Enter));
    let orphan = screen(&paths, &mut app);
    assert!(
        has_line_with(&orphan, &["nothing links here"]),
        "{orphan:#?}"
    );
}

#[test]
fn the_log_screen_shows_the_notebooks_commits_and_then_one_notes_own() {
    let (_root, paths) = a_worked_notebook();
    let mut app = tui::load(&paths).expect("load");

    // On the listing, which is a screen about the notebook.
    let whole = go(&paths, &mut app, "log");
    let all = commits(&paths);
    assert!(
        has_line_with(&whole, &[&format!("Log({})", cmd::DEFAULT_NOTEBOOK)]),
        "{whole:#?}"
    );
    assert_eq!(app.entries().len(), all);

    // On a note, which is a screen about a note. Shorter, which is the whole
    // reason for being able to ask for it.
    app.on_key(key(KeyCode::Esc));
    app.on_key(key(KeyCode::Enter));
    app.on_key(key(KeyCode::Char('l')));
    let one = screen(&paths, &mut app);
    assert!(has_line_with(&one, &["Log(", "Budget review"]), "{one:#?}");
    assert!(app.entries().len() < all, "{one:#?}");
}

#[test]
fn a_commit_on_a_notes_log_writes_a_restore_that_puts_the_note_back() {
    let (_root, paths) = a_worked_notebook();
    let mut app = tui::load(&paths).expect("load");

    app.on_key(key(KeyCode::Char('j')));
    let id = app.selected().expect("the meeting notes").id.clone();
    perform(
        &paths,
        &mut app,
        tui::Action::Tag {
            key: id.clone(),
            changes: vec!["+later".to_string()],
            touch: cmd::Touch::Stamp,
        },
    );
    assert!(has_line_with(&screen(&paths, &mut app), &["later"]));

    // Into the note, then its history: `l` on the listing would be the
    // notebook's, which has nothing to restore against.
    app.on_key(key(KeyCode::Enter));
    app.on_key(key(KeyCode::Char('l')));
    let log = screen(&paths, &mut app);
    assert!(has_line_with(&log, &["tag: meeting-notes"]), "{log:#?}");

    // The row below the newest is the note as it stood before the tag.
    app.on_key(key(KeyCode::Char('j')));
    assert_eq!(app.on_key(key(KeyCode::Enter)), None, "nothing runs yet");
    let written = screen(&paths, &mut app);
    assert!(has_line_with(&written, &["restore", &id]), "{written:#?}");

    let action = app.on_key(key(KeyCode::Enter)).expect("now it runs");
    perform(&paths, &mut app, action);
    let tags = app
        .note_of(&id)
        .expect("the note is still there")
        .note
        .tags
        .clone();
    assert!(!tags.contains(&"later".to_string()), "{tags:?}");
    // Nothing was rewritten: putting it back is another commit on top.
    assert!(commits(&paths) > 7);
}

#[test]
fn the_deleted_screen_names_the_revision_that_brings_a_note_back() {
    let (_root, paths) = a_worked_notebook();
    let mut app = tui::load(&paths).expect("load");
    assert_eq!(app.total(), 3, "the trip was deleted");

    let gone = go(&paths, &mut app, "deleted");
    assert!(has_line_with(&gone, &["Deleted", "[1]"]), "{gone:#?}");
    assert!(
        has_line_with(&gone, &["trip-plan", "Trip plan"]),
        "{gone:#?}"
    );

    // Enter writes the restore; a second Enter runs it. The line is not run for
    // you, because landing on a row is not agreeing to bring a note back.
    app.on_key(key(KeyCode::Enter));
    let written = screen(&paths, &mut app);
    assert!(has_line_with(&written, &["restore "]), "{written:#?}");
    assert_eq!(app.total(), 3, "nothing has happened yet");

    let action = app.on_key(key(KeyCode::Enter)).expect("the restore runs");
    perform(&paths, &mut app, action);
    assert_eq!(app.total(), 4);
    // The screen you are on is still the deleted one, and it now has nothing on
    // it — which is the answer to whether the restore worked.
    let emptied = screen(&paths, &mut app);
    assert!(
        has_line_with(&emptied, &["nothing has been deleted"]),
        "{emptied:#?}"
    );
    app.on_key(key(KeyCode::Esc));
    let listing = screen(&paths, &mut app);
    assert!(has_line_with(&listing, &["Trip plan"]), "{listing:#?}");
}

#[test]
fn the_blame_screen_credits_the_body_and_leaves_the_frontmatter_out() {
    let (_root, paths) = a_worked_notebook();
    let mut app = tui::load(&paths).expect("load");

    app.on_key(key(KeyCode::Char('j')));
    app.on_key(key(KeyCode::Char('B')));
    let blame = screen(&paths, &mut app);
    assert!(
        has_line_with(&blame, &["Blame(", "Meeting notes"]),
        "{blame:#?}"
    );
    assert!(has_line_with(&blame, &["book a room"]), "{blame:#?}");
    // `updated` is rewritten on every edit, so every frontmatter line would be
    // credited to the latest commit — a block of noise that looks like a bug.
    assert!(!has_line_with(&blame, &["updated:"]), "{blame:#?}");
}

#[test]
fn the_diff_screen_shows_what_has_not_been_committed() {
    let (_root, paths) = a_worked_notebook();
    let mut app = tui::load(&paths).expect("load");

    // A note changed from another window, which is what a browser that watches
    // no files would otherwise have no way of noticing.
    let id = app.selected().expect("a note").id.clone();
    let file = paths
        .notebook_dir(cmd::DEFAULT_NOTEBOOK)
        .join(format!("{id}-budget-review.md"));
    let was = std::fs::read_to_string(&file).expect("the note");
    std::fs::write(&file, format!("{was}a line nobody committed\n")).expect("change it");

    let patch = go(&paths, &mut app, "diff");
    assert!(has_line_with(&patch, &["Diff"]), "{patch:#?}");
    assert!(has_line_with(&patch, &["@@"]), "{patch:#?}");
    assert!(
        has_line_with(&patch, &["+a line nobody committed"]),
        "{patch:#?}"
    );
}

#[test]
fn the_notebooks_screen_moves_the_whole_session() {
    let (_root, paths) = a_worked_notebook();
    cmd::notebook_add(&paths, "work", None).expect("a second notebook");
    let mut app = tui::load(&paths).expect("load");
    assert_eq!(app.total(), 3);

    let listed = go(&paths, &mut app, "notebooks");
    assert!(
        has_line_with(
            &listed,
            &[&format!("Notebooks({})[2]", cmd::DEFAULT_NOTEBOOK)]
        ),
        "{listed:#?}"
    );
    assert!(
        has_line_with(&listed, &["•", cmd::DEFAULT_NOTEBOOK]),
        "{listed:#?}"
    );

    // Into the other one: the header, the notes and the stack are all the new
    // notebook's, because a different notebook is a different session.
    app.on_key(key(KeyCode::Char('j')));
    let action = app.on_key(key(KeyCode::Enter)).expect("the switch");
    perform(&paths, &mut app, action);
    let moved = screen(&paths, &mut app);
    assert_eq!(app.total(), 0, "{moved:#?}");
    assert!(has_line_with(&moved, &["Notebook:", "work"]), "{moved:#?}");
    assert!(has_line_with(&moved, &["no notes yet"]), "{moved:#?}");
    assert_eq!(app.crumbs().collect::<Vec<_>>(), ["notes"]);
}

#[test]
fn a_screen_that_cannot_be_filled_closes_and_says_why() {
    let (_root, paths) = a_notebook();
    let mut app = tui::load(&paths).expect("load");

    // A blame of a note whose file has gone from under the browser: the id is
    // still on the listing, and there is nothing on disk to read.
    let id = app.selected().expect("a note").id.clone();
    std::fs::remove_file(
        paths
            .notebook_dir(cmd::DEFAULT_NOTEBOOK)
            .join(format!("{id}-budget-review.md")),
    )
    .expect("take the file away");

    app.on_key(key(KeyCode::Char('B')));
    let after = screen(&paths, &mut app);
    // Back on the listing rather than sitting on an empty screen with the
    // reason on a card that is about to be dismissed.
    assert_eq!(app.depth(), 1);
    assert!(has_line_with(&after, &[" no "]), "{after:#?}");
}

/// The titles the listing shows, top to bottom, so a test can say what order it
/// is in without depending on ids nobody chose.
fn titles(screen: &[String]) -> Vec<&str> {
    [
        "Budget review",
        "Meeting notes",
        "Reading list",
        "Trip plan",
    ]
    .into_iter()
    .filter_map(|title| {
        screen
            .iter()
            .position(|line| line.contains(title))
            .map(|at| (at, title))
    })
    .fold(Vec::new(), |mut sorted: Vec<(usize, &str)>, row| {
        sorted.push(row);
        sorted.sort_by_key(|(at, _)| *at);
        sorted
    })
    .into_iter()
    .map(|(_, title)| title)
    .collect()
}

#[test]
fn s_and_r_put_the_listing_in_the_orders_sort_and_r_already_name() {
    let (_root, paths) = a_worked_notebook();
    let mut app = tui::load(&paths).expect("load");
    let opened = screen(&paths, &mut app);
    let by_slug = titles(&opened);
    assert_eq!(
        by_slug,
        ["Budget review", "Meeting notes", "Reading list"],
        "by slug, which is what a walk produces"
    );

    // `R` on its own turns the walk's own order, which is `ls -r`'s bargain and
    // the reason it needs no `--sort` beside it.
    app.on_key(key(KeyCode::Char('R')));
    let turned = screen(&paths, &mut app);
    assert!(has_line_with(&turned, &["by slug reversed"]), "{turned:#?}");
    assert_eq!(
        titles(&turned),
        ["Reading list", "Meeting notes", "Budget review"]
    );
    app.on_key(key(KeyCode::Char('R')));

    // Which order each key lands on is `sort_notes`', and tested against fixed
    // dates next to the state machine. What is checked here is that the key
    // reaches all four and that the band says which one is in force — `S`
    // rearranges rows and leaves nothing else behind to say why.
    //
    // The row order is deliberately not asserted for the two time orders: these
    // notes were made in the same second, so their stamps tie and the order
    // falls back to ids nobody chose. A test written on those ids passes on the
    // run that wrote it.
    for order in ["by created", "by updated", "by title"] {
        app.on_key(key(KeyCode::Char('S')));
        let sorted = screen(&paths, &mut app);
        assert!(has_line_with(&sorted, &[order]), "{order}: {sorted:#?}");
        let mut still_there = titles(&sorted);
        still_there.sort_unstable();
        assert_eq!(
            still_there,
            ["Budget review", "Meeting notes", "Reading list"],
            "reordering lost a note"
        );
    }

    // And round to where it started, which is the order that says nothing.
    app.on_key(key(KeyCode::Char('S')));
    let back = screen(&paths, &mut app);
    assert!(!has_line_with(&back, &["by "]), "{back:#?}");
    assert_eq!(titles(&back), by_slug);
}

#[test]
fn ctrl_w_adds_the_columns_ls_l_adds_and_puts_them_where_it_puts_them() {
    let (_root, paths) = a_worked_notebook();
    let mut app = tui::load(&paths).expect("load");
    let short = screen(&paths, &mut app);
    assert!(!has_line_with(&short, &["budget-review"]), "{short:#?}");

    app.on_key(ctrl('w'));
    // Wide enough for the whole long row. At ninety the title is squeezed to
    // its floor, which is the give-way the next test is about.
    let wide = screen_at(&paths, &mut app, 120, 28);
    // The id and the title first in both, then what `-l` adds, then the tags:
    // the long row extends the short one rather than rearranging it.
    assert!(
        has_line_with(&wide, &["Budget review", "budget-review", "[work]"]),
        "{wide:#?}"
    );
    assert!(
        has_line_with(&wide, &["Meeting notes", "meeting-notes"]),
        "{wide:#?}"
    );
    // And the band says so, because a row that changed shape should say why.
    assert!(has_line_with(&wide, &["wide"]), "{wide:#?}");
}

#[test]
fn the_wide_row_gives_way_from_the_right_rather_than_starving_the_title() {
    let (_root, paths) = a_worked_notebook();
    let mut app = tui::load(&paths).expect("load");
    app.on_key(ctrl('w'));

    // Narrow enough that the whole long row cannot fit. The id and the title
    // are what name a note; the columns behind them are a density, and a
    // density is the thing to give up.
    let narrow = screen_at(&paths, &mut app, 46, 28);
    for title in ["Budget rev", "Meeting no", "Reading li"] {
        assert!(has_line_with(&narrow, &[title]), "{title}: {narrow:#?}");
    }
    assert!(
        !has_line_with(&narrow, &["2026-"]),
        "the timestamps went before the title did: {narrow:#?}"
    );
}

#[test]
fn a_digit_narrows_to_a_tag_and_the_tags_screen_says_which_digit() {
    let (_root, paths) = a_worked_notebook();
    let mut app = tui::load(&paths).expect("load");

    // The screen numbers its first nine rows with the keys that reach them.
    let tags = go(&paths, &mut app, "tags");
    assert!(has_line_with(&tags, &["1", "work", "2 notes"]), "{tags:#?}");
    assert!(has_line_with(&tags, &["2", "q3", "1 note"]), "{tags:#?}");

    app.on_key(key(KeyCode::Char('1')));
    let narrowed = screen(&paths, &mut app);
    assert!(
        has_line_with(&narrowed, &["Notes(tag:work)[2]"]),
        "{narrowed:#?}"
    );
    assert_eq!(app.crumbs().collect::<Vec<_>>(), ["notes"]);

    app.on_key(key(KeyCode::Char('0')));
    let all = screen(&paths, &mut app);
    assert!(has_line_with(&all, &["Notes(all)[3]"]), "{all:#?}");
}

#[test]
fn ctrl_g_gives_the_crumb_row_to_the_notes() {
    let (_root, paths) = a_worked_notebook();
    let mut app = tui::load(&paths).expect("load");
    app.on_key(key(KeyCode::Enter));
    // By position rather than by text: `notes` is a word the header says too,
    // and a needle that matches "3 notes  1 file" would pass whether the trail
    // was drawn or not. The trail is the band above the status line.
    let trail = |screen: &[String]| screen[screen.len() - 2].trim().to_string();

    let with = screen(&paths, &mut app);
    assert!(trail(&with).starts_with("notes"), "{with:#?}");

    app.on_key(ctrl('g'));
    let without = screen(&paths, &mut app);
    assert!(trail(&without).is_empty(), "{without:#?}");
    // The row went to the notes rather than being drawn blank, and the band
    // still says what screen you are on — the half of the trail that cannot be
    // worked out from anywhere else.
    assert!(has_line_with(&without, &["Note("]), "{without:#?}");
}
