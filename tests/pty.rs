//! The browser in a real terminal: a real pty, the real binary, real bytes.
//!
//! `tests/tui.rs` draws into `ratatui`'s test backend, which is a buffer of
//! characters. That is the right place for "which notes are on the screen", and
//! it is blind to a whole class of bug that has bitten this browser repeatedly —
//! the ones where the *layout* is wrong rather than the content. A padding on
//! the wrong side of a description, a blank cell skipped instead of filled so
//! every column after it slid left, a card that outgrew a twenty-four row
//! terminal, a key dropped at eighty columns that could not be looked up any
//! other way: each of those passed every assertion in that file and was found by
//! driving the built binary through a pty.
//!
//! That driver was written three times and thrown away three times. This is it,
//! kept — `portable-pty` opens the terminal and `vt100` is the emulator on the
//! other end, so what a test asserts on is the screen the bytes actually leave
//! behind, columns and all.
//!
//! The harness is `tests/cli.rs`'s and `tests/tui.rs`'s, restated rather than
//! shared: an integration test is its own crate. The `sign = false` is not
//! optional — libgit2 reads the developer's real git config, so a machine that
//! signs its commits would send every commit here to gpg.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use noda::cmd;
use noda::paths::Paths;
use portable_pty::{CommandBuilder, PtyPair, PtySize, native_pty_system};

/// How long a screen is given to say what it is asked about.
///
/// Generous on purpose: what is being waited for is a process starting, a
/// repository opening and a frame reaching the far end of a pty, and a test that
/// is flaky on a loaded machine is a test that gets deleted.
const PATIENCE: Duration = Duration::from_secs(20);

struct TempRoot(PathBuf);

impl TempRoot {
    fn new() -> Self {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("noda-pty-test-{}-{n}", std::process::id()));
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
///
/// Built in this process rather than through the binary: what is under test is
/// the browser, and a notebook assembled by `cmd::` is the same notebook either
/// way. `Paths::rooted` lays the four XDG roots out under one directory, which
/// is exactly what the four variables handed to the child point at.
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

/// An "editor" that edits: it appends a line and exits, which is the whole of
/// what the round trip needs from it.
///
/// A script on disk rather than a command line, because `run_editor` splits the
/// configured editor on whitespace and hands each piece to `Command` — so
/// anything with a quoted argument in it cannot survive the trip. Written into
/// the notebook's config because that is what wins: config beats `$VISUAL` and
/// `$EDITOR`, the way git's `core.editor` does, and a run that set only the
/// environment once opened the developer's vim and hung there.
fn an_editor_that_edits(root: &TempRoot, paths: &Paths) {
    let script = root.0.join("editor.sh");
    std::fs::write(
        &script,
        "#!/bin/sh\nprintf '\\nedited by the test\\n' >> \"$1\"\n",
    )
    .expect("write editor");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))
            .expect("chmod editor");
    }
    let config = paths.config_dir().join("config.toml");
    let existing = std::fs::read_to_string(&config).expect("read config");
    std::fs::write(
        &config,
        format!("{existing}editor = \"{}\"\n", script.display()),
    )
    .expect("write config");
}

/// The browser, running in a terminal of a stated size.
struct Browser {
    writer: Box<dyn Write + Send>,
    screen: Arc<Mutex<vt100::Parser>>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
    // The master end is what the reader and the writer were taken from; dropping
    // it closes the terminal under the child.
    _master: Box<dyn portable_pty::MasterPty + Send>,
}

impl Browser {
    /// Opens `noda tui` on the notebook under `root`, in a terminal `cols` wide
    /// and `rows` tall.
    ///
    /// The size is the point of most of these tests, so it is always stated.
    fn open(root: &TempRoot, cols: u16, rows: u16) -> Self {
        let PtyPair { master, slave } = native_pty_system()
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .expect("open a pty");

        let mut command = CommandBuilder::new(env!("CARGO_BIN_EXE_noda"));
        command.arg("tui");
        // All four, and `XDG_STATE_HOME` above all: the active-notebook pointer
        // lives in state rather than in data, so a run that overrode only config
        // and data would reach past this notebook and rewrite the real one.
        command.env("XDG_CONFIG_HOME", root.0.join("config"));
        command.env("XDG_DATA_HOME", root.0.join("data"));
        command.env("XDG_STATE_HOME", root.0.join("state"));
        command.env("XDG_CACHE_HOME", root.0.join("cache"));
        command.env("TERM", "xterm-256color");

        let child = slave.spawn_command(command).expect("spawn noda tui");
        // The child holds its own handle on the slave now. Ours has to go, or
        // the reader below never sees the end of the stream.
        drop(slave);

        let mut reader = master.try_clone_reader().expect("clone the reader");
        let writer = master.take_writer().expect("take the writer");
        let screen = Arc::new(Mutex::new(vt100::Parser::new(rows, cols, 0)));

        let feed = Arc::clone(&screen);
        std::thread::spawn(move || {
            let mut buffer = [0u8; 8192];
            while let Ok(read) = reader.read(&mut buffer) {
                if read == 0 {
                    break;
                }
                feed.lock().expect("screen").process(&buffer[..read]);
            }
        });

        Browser {
            writer,
            screen,
            child,
            _master: master,
        }
    }

    /// Types, as a person would.
    fn send(&mut self, keys: &str) {
        self.writer
            .write_all(keys.as_bytes())
            .expect("write to the terminal");
        self.writer.flush().expect("flush");
    }

    /// What is on the screen right now.
    fn now(&self) -> String {
        self.screen.lock().expect("screen").screen().contents()
    }

    /// The screen row by row, so an assertion can be about *where* something is.
    ///
    /// Leading blanks survive, which is what makes a column measurable; only the
    /// run of blanks at the end of a row is dropped.
    fn rows(&self) -> Vec<String> {
        let parser = self.screen.lock().expect("screen");
        parser.screen().rows(0, u16::MAX).collect()
    }

    /// Waits until the screen says `needle`, and answers with the whole screen.
    ///
    /// Polled rather than slept against: a fixed sleep is either slower than it
    /// needs to be or shorter than a loaded machine needs, and usually both in
    /// the same test run.
    fn wait_for(&self, needle: &str) -> String {
        let deadline = Instant::now() + PATIENCE;
        loop {
            let screen = self.now();
            if screen.contains(needle) {
                return screen;
            }
            assert!(
                Instant::now() < deadline,
                "waited {PATIENCE:?} for {needle:?}; the screen was:\n{screen}"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Waits until the screen has stopped saying `needle`.
    fn wait_until_gone(&self, needle: &str) -> String {
        let deadline = Instant::now() + PATIENCE;
        loop {
            let screen = self.now();
            if !screen.contains(needle) {
                return screen;
            }
            assert!(
                Instant::now() < deadline,
                "waited {PATIENCE:?} for {needle:?} to go; the screen was:\n{screen}"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Leaves, and insists that leaving worked.
    ///
    /// `q` first and `ctrl-c` after, because a card swallows whatever dismisses
    /// it: a run that sent only `q` while the help was up dismissed the help and
    /// then waited forever for a process that was still perfectly happy.
    fn quit(mut self) {
        self.send("q");
        if self.wait_for_exit(Duration::from_secs(2)).is_none() {
            self.send("\x03");
        }
        let status = self
            .wait_for_exit(PATIENCE)
            .expect("the browser should have left by now");
        assert!(status.success(), "the browser left with {status:?}");
    }

    fn wait_for_exit(&mut self, patience: Duration) -> Option<portable_pty::ExitStatus> {
        let deadline = Instant::now() + patience;
        loop {
            match self.child.try_wait() {
                Ok(Some(status)) => return Some(status),
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                Ok(None) => return None,
                Err(e) => panic!("waiting on the browser: {e}"),
            }
        }
    }
}

impl Drop for Browser {
    fn drop(&mut self) {
        // A test that failed mid-screen would otherwise leave a browser sitting
        // in a terminal nobody is reading.
        let _ = self.child.kill();
    }
}

/// The file a note was written to, found by the slug in its name.
///
/// The id in front of the slug is minted, so the name cannot be spelled out in
/// a test — see the standing rule about fixtures built from minted ids.
fn note_file(paths: &Paths, slug: &str) -> PathBuf {
    let ending = format!("-{slug}.md");
    let notebooks = std::fs::read_dir(paths.notebooks_dir()).expect("read the notebooks dir");
    for notebook in notebooks.flatten() {
        let Ok(entries) = std::fs::read_dir(notebook.path()) else {
            continue;
        };
        for entry in entries.flatten() {
            if entry.file_name().to_string_lossy().ends_with(&ending) {
                return entry.path();
            }
        }
    }
    panic!("no note called {slug}");
}

/// Waits until a file on disk says `needle`.
fn wait_for_file(path: &std::path::Path, needle: &str) {
    let deadline = Instant::now() + PATIENCE;
    loop {
        let text = std::fs::read_to_string(path).unwrap_or_default();
        if text.contains(needle) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "waited {PATIENCE:?} for {needle:?} in {}; it held:\n{text}",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Which column of the screen `needle` starts at, and on which row.
///
/// The point of measuring a column at all: the grid's cells are padded into
/// place, and the bugs worth catching here are the ones where a cell was skipped
/// rather than padded, so everything after it on that row moved left.
fn column_of(rows: &[String], needle: &str) -> usize {
    rows.iter()
        .find_map(|row| row.find(needle))
        .unwrap_or_else(|| panic!("{needle:?} is not on the screen:\n{}", rows.join("\n")))
}

/// Where the count beside a tag starts, in columns of the terminal.
///
/// Measured from the box rather than from the left of the row: a card lies over
/// the listing rather than replacing it, so what is to the left of the card on
/// that row is still a note — and `Meeting notes` holds the very word being
/// looked for. And measured in columns rather than in bytes, because `find`
/// answers in bytes and a tag in Chinese is three of them a character.
fn count_column(row: &str) -> Option<usize> {
    let boxed = row.find("[x] ").or_else(|| row.find("[ ] "))?;
    let at = boxed + row[boxed..].find(" note")?;
    Some(cmd::display_width(&row[..at]))
}

/// A name padded out by characters lines up in a buffer and not on a terminal.
/// `專案管理` is four characters and eight columns, so a picker that counted
/// characters would leave the count beside every ASCII tag four columns short of
/// the count beside this one — and `tests/tui.rs`, which is a buffer of
/// characters, would go on passing.
#[test]
fn the_tag_picker_lines_its_counts_up_in_columns_and_not_characters() {
    let (root, paths) = a_notebook();
    cmd::add(
        &paths,
        Some("Ubuntu notes"),
        Some("body"),
        &["專案管理".to_string()],
    )
    .expect("add");

    let mut browser = Browser::open(&root, 90, 28);
    browser.wait_for("Budget review");

    // Waited for on the card's own footer: the tag itself is on the listing
    // underneath as well, so waiting on that returns before the card is drawn.
    browser.send("#");
    let screen = browser.wait_for("tab  choose");
    assert!(screen.contains("專案管理"), "{screen}");
    let rows = browser.rows();

    let counts: Vec<usize> = rows.iter().filter_map(|row| count_column(row)).collect();
    assert!(
        counts.len() >= 3,
        "three tags, three counts, and there were {}:\n{}",
        counts.len(),
        rows.join("\n")
    );
    assert!(
        counts.windows(2).all(|pair| pair[0] == pair[1]),
        "the counts are at {counts:?}:\n{}",
        rows.join("\n")
    );

    // Out of the card before leaving: every letter in it is a letter the filter
    // takes, `q` included.
    browser.send("\x1b");
    browser.wait_until_gone("tab  choose");
    browser.quit();
}

/// The key that chooses is the one key in the browser that is not a character
/// and not a chord, and every test below this layer hands `KeyCode::Tab` to the
/// state machine directly. What a terminal actually sends is `\t`, and that it
/// arrives as `Tab` rather than as a character in the filter is a link only a
/// real terminal can test — and the whole design rests on it, because the filter
/// takes every character there is.
#[test]
fn the_tab_that_chooses_arrives_as_tab_and_not_as_a_character() {
    let (root, _paths) = a_notebook();
    let mut browser = Browser::open(&root, 90, 28);
    browser.wait_for("Budget review");

    // The note under the cursor carries `work`, so the box is ticked and the
    // only state left to walk to is the one that takes it off.
    browser.send("#");
    let screen = browser.wait_for("tab  choose");
    assert!(screen.contains("[x] work"), "{screen}");

    browser.send("\t");
    let screen = browser.wait_for("[-] work");
    assert!(
        !screen.contains("[x] work"),
        "the tab went into the filter:\n{screen}"
    );

    browser.send("\x1b");
    browser.wait_until_gone("tab  choose");
    browser.quit();
}

#[test]
fn the_listing_arrives_through_a_real_terminal() {
    let (root, _paths) = a_notebook();
    let browser = Browser::open(&root, 90, 28);

    let screen = browser.wait_for("Budget review");
    assert!(screen.contains("Meeting notes"), "{screen}");
    assert!(screen.contains("Reading list"), "{screen}");
    // The title band, which is the one thing on the screen that is about the
    // screen rather than about a note.
    assert!(screen.contains("Notes"), "{screen}");

    browser.quit();
}

/// The card has outgrown a twenty-four row terminal three times — once when the
/// write keys arrived, once when the screens did, and once when readline did —
/// and each time the fix was to consolidate rows rather than to assume a taller
/// terminal. Nothing in the command layer can see this, and the test backend
/// only sees it if somebody thinks to ask at exactly the wrong height.
#[test]
fn the_help_card_fits_a_twenty_four_row_terminal() {
    let (root, _paths) = a_notebook();
    let mut browser = Browser::open(&root, 90, 24);
    browser.wait_for("Budget review");

    // Not `keys`: that is on the grid underneath as `<?>  keys`, so waiting on
    // it returns before the card has been drawn at all and the assertions below
    // pass or fail against the wrong screen.
    browser.send("?");
    let screen = browser.wait_for("half a screen");

    // The search example is the one thing on the card that cannot be guessed
    // from the key it is next to, and a card that measured itself wrong cut it.
    assert!(
        screen.contains("tag:work OR tag:q3 budget"),
        "the search example was cut:\n{screen}"
    );
    // The last row of the card. If the card ran off the bottom this is what went
    // with it.
    assert!(
        screen.contains("readline: ctrl-a/e/w/u/k/y"),
        "the card lost its last row:\n{screen}"
    );

    browser.quit();
}

/// Eighty columns is where the grid starts dropping columns from the right, and
/// `:` is the key that cannot be looked up when it is not shown — every other
/// way of reaching a command is itself named on the prompt.
#[test]
fn the_command_key_survives_eighty_columns() {
    let (root, _paths) = a_notebook();
    let browser = Browser::open(&root, 80, 28);
    let screen = browser.wait_for("Budget review");

    // `<:>` and not `command`: the column beside it holds `ctrl-a  commands`,
    // so an assertion on the word alone passes while the key itself is gone.
    assert!(screen.contains("<:>"), "the prompt key went:\n{screen}");
    assert!(screen.contains("<?>"), "the help key went:\n{screen}");

    browser.quit();
}

/// The grid is columns of pairs, and a cell with nothing in it has to be padded
/// to its width rather than skipped — a skipped one slides every cell after it
/// on that row to the left, and a key ends up sitting under the wrong heading.
///
/// The note screen is where that bug lived: its third column runs out after two
/// entries, so three of its five rows are blank in the middle of a grid that
/// keeps going.
#[test]
fn a_blank_cell_holds_its_column_open() {
    let (root, _paths) = a_notebook();
    // Wide enough for the column being measured to be drawn at all: the grid
    // drops columns from the right, and at ninety the note's fourth one is
    // already gone. A test about where a column starts has to be run where the
    // column is.
    let mut browser = Browser::open(&root, 110, 28);
    browser.wait_for("Budget review");

    // Into the note, where the grid has the blanks. `scroll` is the marker that
    // this is the note and not the listing: every other word on the grid is on
    // both, including the four this test is about.
    browser.send("\r");
    browser.wait_for("scroll");
    let rows = browser.rows();

    // The four screens worth a letter are one column, so their keys start in one
    // place. Three of them sit on rows where the column to their left is blank,
    // which is the whole point — measured on the key rather than on the word,
    // because the key is what a slid column takes with it.
    let todo = column_of(&rows, "<t>");
    for key in ["<l>", "<b>", "<B>"] {
        assert_eq!(
            column_of(&rows, key),
            todo,
            "{key} slid out of its column:\n{}",
            rows.join("\n")
        );
    }

    browser.quit();
}

/// Every intermediate keystroke of a query is itself a query, so this asserts
/// across the whole word rather than in the middle of it.
#[test]
fn typing_a_query_narrows_the_listing() {
    let (root, _paths) = a_notebook();
    let mut browser = Browser::open(&root, 90, 28);
    browser.wait_for("Reading list");

    browser.send("/budget");
    let screen = browser.wait_until_gone("Reading list");
    assert!(screen.contains("Budget review"), "{screen}");

    browser.quit();
}

/// Handing the terminal to `$EDITOR` and taking it back is the one thing the
/// browser does that leaves its own screen entirely, and it is where a
/// `Terminal::clear()` once asked the terminal where its cursor was and waited
/// for an answer that a pty had nobody to give — fatal, and invisible to every
/// test that never left the process.
///
/// The proof that the round trip worked is the edit arriving: the editor ran,
/// the browser came back, the reload saw the change, and the note screen drew it.
#[test]
fn coming_back_from_the_editor_redraws_the_screen() {
    let (root, paths) = a_notebook();
    an_editor_that_edits(&root, &paths);
    let mut browser = Browser::open(&root, 90, 28);
    browser.wait_for("Budget review");

    // Three waits and not one. The file says the editor has been and gone; the
    // slug on the status line says the browser is back and has drawn something,
    // which is the moment it can take a keystroke again. Sending into the gap
    // between the two is racing the editor for the terminal.
    browser.send("e");
    wait_for_file(&note_file(&paths, "budget-review"), "edited by the test");
    browser.wait_for("budget-review");

    browser.send("\r");
    browser.wait_for("edited by the test");

    browser.quit();
}
