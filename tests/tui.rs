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
fn screen(app: &mut App) -> Vec<String> {
    screen_at(app, 90, 28)
}

fn screen_at(app: &mut App, width: u16, height: u16) -> Vec<String> {
    tui::refresh_reading(app);
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
fn opened(app: &mut App) -> Vec<String> {
    app.on_key(key(KeyCode::Enter));
    screen(app)
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
    let screen = screen(&mut app);

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
    let screen = screen(&mut app);

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
    let before = screen(&mut app)[..HEADER].to_vec();

    // Marking, queueing and the flag are the only things that move while you
    // sit there, and they are said on the title band for exactly this reason:
    // in the block they widened it, which pushed the keys along and dropped the
    // rightmost column — so marking a note hid the keys about marking.
    app.on_key(key(KeyCode::Char('*')));
    app.on_key(key(KeyCode::Char('T')));
    app.on_key(key(KeyCode::Char('#')));
    typing(&mut app, "+archive");
    app.on_key(key(KeyCode::Enter));

    let after = screen(&mut app);
    assert_eq!(after[..HEADER], before[..], "the header moved");
    assert!(has_line_with(&after, &["3 marks", "1 queued"]));
    assert!(has_line_with(&after[..HEADER], &["<space>", "mark"]));
}

#[test]
fn the_title_band_says_what_the_screen_is_of() {
    let (_root, paths) = a_notebook();
    let mut app = tui::load(&paths).expect("load");
    assert!(has_line_with(&screen(&mut app), &["Notes(all)[3]"]));

    // A note names itself the way every other listing names it: the id, then
    // the title.
    let id = app.selected().expect("a note").id.clone();
    let opened = opened(&mut app);
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
    let listing = screen(&mut app);
    assert!(!has_line_with(&listing, &["the q3 budget is late"]));

    let id = app.selected().expect("a note").id.clone();
    let first = opened(&mut app);
    assert!(has_line_with(&first, &["the q3 budget is late"]));
    // The frontmatter is on screen too — dimmed, not hidden, exactly as
    // `noda show` prints it.
    assert!(has_line_with(&first, &["title: Budget review"]));
    // And the trail says how far down you are, naming the note by its id.
    assert!(has_line_with(&first, &["notes", &id]));

    app.on_key(key(KeyCode::Esc));
    let back = screen(&mut app);
    assert!(has_line_with(&back, &["Meeting notes"]));
    assert!(
        !has_line_with(&back, &["the q3 budget is late"]),
        "the note's body went with its screen"
    );

    // The next note down, and it is the one that opens.
    app.on_key(key(KeyCode::Char('j')));
    let moved = opened(&mut app);
    assert!(has_line_with(&moved, &["# Agenda"]));
    assert!(!has_line_with(&moved, &["the q3 budget is late"]));
}

#[test]
fn a_query_narrows_the_listing_as_it_is_typed() {
    let (_root, paths) = a_notebook();
    let mut app = tui::load(&paths).expect("load");

    app.on_key(key(KeyCode::Char('/')));
    typing(&mut app, "tag:q3");
    let screen = screen(&mut app);

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
    let good = screen(&mut app);
    assert!(has_line_with(&good, &["Meeting notes"]));

    // And now it is not one — the state every alternative passes through.
    typing(&mut app, "R");
    let unfinished = screen(&mut app);
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
    let screen = screen(&mut app);

    assert!(has_line_with(&screen, &["nothing matches"]));
    assert!(has_line_with(&screen, &["Notes(tag:archived)[0]"]));
}

#[test]
fn the_help_card_lists_the_keys_and_goes_away_again() {
    let (_root, paths) = a_notebook();
    let mut app = tui::load(&paths).expect("load");

    app.on_key(key(KeyCode::Char('?')));
    let with_help = screen(&mut app);
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
    let without = screen(&mut app);
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

    let screen = screen(&mut app);
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
        tui::Action::Send(steps) => {
            let sent = cmd::bulk(paths, &steps);
            if sent.is_ok() {
                app.sent();
            }
            sent
        }
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
    assert!(has_line_with(&screen(&mut app), &["3 marks"]));

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
    assert!(has_line_with(&screen(&mut app), &["2 queued"]));

    app.on_key(key(KeyCode::Char('Q')));
    let queued = screen(&mut app);
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
    let after = screen(&mut app);
    assert!(has_line_with(&after, &["2 changes over 3 notes"]));
    assert!(
        has_line_with(&after, &["[q3, archive]"]),
        "and the listing says so"
    );
    // Read out of the file itself, on the screen that shows the file.
    let file = opened(&mut app);
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
    let screen = screen_at(&mut app, 46, 28);

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
    let screen = screen(&mut app);
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
    let asked = screen(&mut app);
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
    let queued = screen(&mut app);
    assert!(has_line_with(&queued, &["rm: 2 notes"]));

    // The send is where the question is asked, and only because of the delete.
    assert_eq!(app.on_key(key(KeyCode::Enter)), None);
    let asked = screen(&mut app);
    assert!(has_line_with(&asked, &["send the queue?"]));
    assert!(has_line_with(&asked, &["2 notes to be deleted"]));

    let action = app
        .on_key(key(KeyCode::Char('y')))
        .expect("a queue to send");
    perform(&paths, &mut app, action);

    app.on_key(key(KeyCode::Esc));
    let after = screen(&mut app);
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
    let prompt = screen(&mut app);
    assert!(has_line_with(&prompt, &["tags", "+urgent"]));
    assert!(
        has_line_with(&prompt, &["+work -q3"]),
        "the syntax is shown"
    );

    let action = app.on_key(key(KeyCode::Enter)).expect("a tag to apply");
    perform(&paths, &mut app, action);

    // The status line is `noda tag`'s own answer, not a sentence noda wrote
    // twice. It is read first, because the next key is what takes it away.
    let after = screen(&mut app);
    let id = app.selected().expect("a note").id.clone();
    assert!(has_line_with(&after, &[&id, "budget-review", "urgent"]));

    // And read out of the file the command wrote, on the screen that shows it.
    let file = opened(&mut app);
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

    let after = opened(&mut app);
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

    let after = screen(&mut app);
    assert!(has_line_with(&after, &["Quarterly plan"]));
    assert!(!has_line_with(&after, &["Budget review"]));
    // The slug moved it down the listing; the id is what the cursor followed.
    assert_eq!(app.selected().map(|f| f.id.clone()), Some(id));
    assert!(
        has_line_with(&opened(&mut app), &["title: Quarterly plan"]),
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
    assert!(has_line_with(&screen(&mut app), &["delete is Ctrl-d"]));
    assert_eq!(app.total(), 3);

    app.on_key(ctrl('d'));
    let asked = screen(&mut app);
    assert!(has_line_with(&asked, &["delete this note?"]));
    assert!(has_line_with(&asked, &["Budget review"]));
    assert!(has_line_with(&asked, &["git revert brings it back"]));

    let action = app.on_key(key(KeyCode::Char('y'))).expect("a delete");
    perform(&paths, &mut app, action);

    let after = screen(&mut app);
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
    assert!(!has_line_with(&screen(&mut app), &["keeping updated"]));

    app.on_key(key(KeyCode::Char('T')));
    let with_flag = screen(&mut app);
    assert!(
        has_line_with(&with_flag, &["keeping updated"]),
        "a setting you cannot see is one you forget you left on"
    );

    app.on_key(key(KeyCode::Char('#')));
    typing(&mut app, "+urgent");
    let action = app.on_key(key(KeyCode::Enter)).expect("a tag to apply");
    perform(&paths, &mut app, action);

    let after = opened(&mut app);
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
    assert!(!has_line_with(&screen(&mut app), &["keeping updated"]));
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

    let after = screen(&mut app);
    assert!(has_line_with(&after, &["a tag cannot contain `,`"]));
    assert!(
        has_line_with(&after, &["Budget review", "[work]"]),
        "the note is as it was"
    );
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
