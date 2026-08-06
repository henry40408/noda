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
        other => panic!("{other:?} wants a terminal of its own"),
    };
    app.report(outcome);
    tui::reload(paths, app).expect("reload");
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

    let after = screen(&mut app);
    // Read out of the file the command wrote, in the pane that shows the file.
    assert!(has_line_with(&after, &["tags: [work, urgent]"]));
    // And the footer is `noda tag`'s own answer, not a sentence noda wrote
    // twice.
    let id = app.selected().expect("a note").id.clone();
    assert!(has_line_with(&after, &[&id, "budget-review", "urgent"]));
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

    let after = screen(&mut app);
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
        has_line_with(&after, &["title: Quarterly plan"]),
        "the preview was read again from the file it is now in"
    );
}

#[test]
fn a_delete_is_asked_about_and_then_carried_out() {
    let (_root, paths) = a_notebook();
    let mut app = tui::load(&paths).expect("load");

    app.on_key(key(KeyCode::Char('d')));
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
    assert!(!has_line_with(&screen(&mut app)[..1], &["keeping updated"]));

    app.on_key(key(KeyCode::Char('T')));
    let with_flag = screen(&mut app);
    assert!(
        has_line_with(&with_flag[..1], &["keeping updated"]),
        "a setting you cannot see is one you forget you left on"
    );

    app.on_key(key(KeyCode::Char('#')));
    typing(&mut app, "+urgent");
    let action = app.on_key(key(KeyCode::Enter)).expect("a tag to apply");
    perform(&paths, &mut app, action);

    let after = screen(&mut app);
    assert!(
        has_line_with(&after, &["tags: [work, urgent]"]),
        "the change went in"
    );
    assert_eq!(
        updated(&paths, &app),
        "updated: 2019-03-04T05:06:07Z",
        "the date the note came with is still the date it carries"
    );

    // Off again, and the next change records itself as every other one does.
    app.on_key(key(KeyCode::Char('T')));
    assert!(!has_line_with(&screen(&mut app)[..1], &["keeping updated"]));
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
