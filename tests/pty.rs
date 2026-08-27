//! The browser in a real terminal: a real pty, the real binary, real bytes.
//!
//! `tests/tui.rs` draws into a buffer of characters, which is right for "which
//! notes are on the screen" and blind to the bugs where the *layout* is wrong
//! rather than the content: a padding on the wrong side, a blank cell skipped so
//! every column after slid left, a card that outgrew twenty-four rows, a key
//! dropped at eighty columns. Each passed every assertion there and was found by
//! driving the built binary through a pty.
//!
//! `portable-pty` opens the terminal and `vt100` is the emulator on the other
//! end, so what is asserted on is the screen the bytes leave behind.
//!
//! The harness is restated rather than shared: an integration test is its own
//! crate. `sign = false` is not optional — libgit2 reads the developer's real
//! git config, so a machine that signs would send every commit here to gpg.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use noda::cmd;
use noda::paths::Paths;
use portable_pty::{CommandBuilder, PtyPair, PtySize, native_pty_system};

/// Generous on purpose — a process starting, a repository opening and a frame
/// crossing a pty — because a test flaky on a loaded machine gets deleted.
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

/// Three notes, in the order the listing puts them.
///
/// Built in this process: what is under test is the browser, and a notebook
/// assembled by `cmd::` is the same notebook. `Paths::rooted` lays the four XDG
/// roots under one directory, which is what the child's four variables name.
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

/// Appends a line and exits, which is all the round trip needs.
///
/// A script on disk, because `run_editor` splits on whitespace and a quoted
/// argument cannot survive. In the notebook's config because that is what wins —
/// a run that set only the environment once opened the developer's vim and hung
/// there.
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
    // Where the reader and writer came from; dropping it closes the terminal.
    _master: Box<dyn portable_pty::MasterPty + Send>,
}

impl Browser {
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
        // All four, `XDG_STATE_HOME` above all: the active pointer lives in
        // state, so overriding only config and data rewrites the real one.
        command.env("XDG_CONFIG_HOME", root.0.join("config"));
        command.env("XDG_DATA_HOME", root.0.join("data"));
        command.env("XDG_STATE_HOME", root.0.join("state"));
        command.env("XDG_CACHE_HOME", root.0.join("cache"));
        command.env("TERM", "xterm-256color");

        let child = slave.spawn_command(command).expect("spawn noda tui");
        // Ours has to go, or the reader never sees the end of the stream.
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

    /// Row by row, so an assertion can be about *where* something is. Leading
    /// blanks survive, which is what makes a column measurable.
    fn rows(&self) -> Vec<String> {
        let parser = self.screen.lock().expect("screen");
        parser.screen().rows(0, u16::MAX).collect()
    }

    /// Polled rather than slept against: a fixed sleep is either slower than it
    /// needs to be or shorter than a loaded machine needs.
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

    /// `q` first and `ctrl-c` after, because a card swallows whatever dismisses
    /// it: `q` alone under the help card dismissed the help and then waited
    /// forever for a process that was perfectly happy.
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
        // Or a failure mid-screen leaves a browser nobody is reading.
        let _ = self.child.kill();
    }
}

/// By slug: the id in front of it is minted, so the name cannot be spelled out
/// in a test.
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

/// Why a column is measured at all: the grid's cells are padded into place, and
/// the bugs worth catching are the ones where a cell was skipped rather than
/// padded, so everything after it moved left.
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

/// Padding counted in characters lines up in a buffer and not on a terminal:
/// `專案管理` is four characters and eight columns, and `tests/tui.rs` would go
/// on passing.
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

    // On the card's footer: the tag is on the listing underneath too, so
    // waiting on that returns before the card is drawn.
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

    // Out of the card first: the filter takes every letter, `q` included.
    browser.send("\x1b");
    browser.wait_until_gone("tab  choose");
    browser.quit();
}

/// Every test below this layer hands `KeyCode::Tab` to the state machine. What a
/// terminal sends is `\t`, and that it arrives as `Tab` rather than as a
/// character in the filter is a link only a real terminal can test — and the
/// design rests on it, the filter taking every character there is.
#[test]
fn the_tab_that_chooses_arrives_as_tab_and_not_as_a_character() {
    let (root, _paths) = a_notebook();
    let mut browser = Browser::open(&root, 90, 28);
    browser.wait_for("Budget review");

    // The note carries `work`, so the only state left is taking it off.
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
    // The one thing on screen about the screen rather than about a note.
    assert!(screen.contains("Notes"), "{screen}");

    browser.quit();
}

/// The card has outgrown twenty-four rows three times, and each fix was to
/// consolidate rows rather than assume a taller terminal. The test backend only
/// sees it if somebody asks at exactly the wrong height.
#[test]
fn the_help_card_fits_a_twenty_four_row_terminal() {
    let (root, _paths) = a_notebook();
    let mut browser = Browser::open(&root, 90, 24);
    browser.wait_for("Budget review");

    // Not `keys`, which is on the grid underneath — waiting on it returns
    // before the card is drawn at all.
    browser.send("?");
    let screen = browser.wait_for("half a screen");

    // The one thing on the card that cannot be guessed from its key, and what
    // a card that measured itself wrong cut.
    assert!(
        screen.contains("tag:work OR tag:q3 budget"),
        "the search example was cut:\n{screen}"
    );
    // The last row: what goes if the card runs off the bottom.
    assert!(
        screen.contains("readline: ctrl-a/e/w/u/k/y"),
        "the card lost its last row:\n{screen}"
    );

    browser.quit();
}

/// Eighty columns is where the grid starts dropping from the right, and `:` is
/// the key that cannot be looked up when it is not shown.
#[test]
fn the_command_key_survives_eighty_columns() {
    let (root, _paths) = a_notebook();
    let browser = Browser::open(&root, 80, 28);
    let screen = browser.wait_for("Budget review");

    // `<:>` and not `command`: the column beside holds `ctrl-a  commands`, so
    // the word alone passes while the key is gone.
    assert!(screen.contains("<:>"), "the prompt key went:\n{screen}");
    assert!(screen.contains("<?>"), "the help key went:\n{screen}");

    browser.quit();
}

/// A blank cell has to be padded rather than skipped: a skipped one slides every
/// cell after it left, and a key ends up under the wrong heading.
///
/// The note screen is where that bug lived — its third column runs out after two
/// entries, so three of five rows are blank mid-grid.
#[test]
fn a_blank_cell_holds_its_column_open() {
    let (root, _paths) = a_notebook();
    // Wide enough for the column to be drawn at all: at ninety the note's
    // fourth is already gone.
    let mut browser = Browser::open(&root, 110, 28);
    browser.wait_for("Budget review");

    // Into the note, where the blanks are. `scroll` marks it as the note: every
    // other word on the grid is on both.
    browser.send("\r");
    browser.wait_for("scroll");
    let rows = browser.rows();

    // One column, so their keys start in one place — and three sit on rows whose
    // left-hand column is blank, which is the point. Measured on the key, being
    // what a slid column takes with it.
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

/// Every intermediate keystroke is itself a query, so this asserts across the
/// whole word.
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

/// The one thing the browser does that leaves its own screen, and where a
/// `Terminal::clear()` once asked the terminal for its cursor and waited for an
/// answer a pty had nobody to give — fatal, and invisible to every test that
/// never left the process.
///
/// The edit arriving is the proof: the editor ran, the browser came back, the
/// reload saw the change, and the note screen drew it.
#[test]
fn coming_back_from_the_editor_redraws_the_screen() {
    let (root, paths) = a_notebook();
    an_editor_that_edits(&root, &paths);
    let mut browser = Browser::open(&root, 90, 28);
    browser.wait_for("Budget review");

    // Three waits: the file says the editor has been and gone, and the slug on
    // the status line says the browser is back and can take a keystroke.
    // Sending into the gap between them races the editor for the terminal.
    browser.send("e");
    wait_for_file(&note_file(&paths, "budget-review"), "edited by the test");
    browser.wait_for("budget-review");

    browser.send("\r");
    browser.wait_for("edited by the test");

    browser.quit();
}
