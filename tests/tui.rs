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

fn typing(app: &mut App, text: &str) {
    for c in text.chars() {
        app.on_key(key(KeyCode::Char(c)));
    }
}

/// One frame, as the lines a reader would see. Trailing blanks are cut so an
/// assertion is about what was written, not about how wide the terminal was.
fn screen(app: &mut App) -> Vec<String> {
    tui::refresh_preview(app);
    let mut terminal = Terminal::new(TestBackend::new(90, 14)).expect("test terminal");
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
fn the_header_says_which_notebook_and_which_branch() {
    let (_root, paths) = a_notebook();
    let mut app = tui::load(&paths).expect("load");
    let screen = screen(&mut app);
    assert!(has_line_with(
        &screen[..1],
        &[cmd::DEFAULT_NOTEBOOK, "3 notes"]
    ));
}

#[test]
fn the_preview_shows_the_note_the_cursor_is_on() {
    let (_root, paths) = a_notebook();
    let mut app = tui::load(&paths).expect("load");

    let first = screen(&mut app);
    assert!(has_line_with(&first, &["the q3 budget is late"]));
    // The frontmatter is on screen too — dimmed, not hidden, exactly as
    // `noda show` prints it.
    assert!(has_line_with(&first, &["title: Budget review"]));

    app.on_key(key(KeyCode::Char('j')));
    let moved = screen(&mut app);
    assert!(has_line_with(&moved, &["# Agenda"]));
    assert!(
        !has_line_with(&moved, &["the q3 budget is late"]),
        "the previous note's body is gone"
    );
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
    // The query stays visible while it is being typed, and the count says what
    // it left.
    assert!(has_line_with(&screen, &["/tag:q3"]));
    assert!(has_line_with(&screen, &["1/3"]));
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
    assert!(has_line_with(&screen, &["0/3"]));
}

#[test]
fn the_help_card_lists_the_keys_and_goes_away_again() {
    let (_root, paths) = a_notebook();
    let mut app = tui::load(&paths).expect("load");

    app.on_key(key(KeyCode::Char('?')));
    let with_help = screen(&mut app);
    assert!(has_line_with(&with_help, &["keys"]));
    assert!(has_line_with(&with_help, &["quit"]));
    // The card is as wide as its longest line. The search example is the one
    // entry that says something the key beside it cannot, so a card that cut it
    // off would be dropping the only part worth reading twice.
    assert!(has_line_with(
        &with_help,
        &["search: tag:work OR tag:q3 budget"]
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
