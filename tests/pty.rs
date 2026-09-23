//! The browser in a real terminal: the built binary on a `portable-pty`, read
//! back through a `vt100` emulator.
//!
//! `tests/tui.rs` draws into a character buffer, which is blind to *layout* bugs —
//! a cell skipped so later columns slide left, a card taller than twenty-four
//! rows, a key dropped at eighty columns. Each of those passed there.
//!
//! `sign = false` is required: libgit2 reads the developer's real git config.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use noda::cmd;
use noda::paths::Paths;
use portable_pty::{CommandBuilder, PtyPair, PtySize, native_pty_system};

/// Generous, because a test flaky on a loaded machine gets deleted.
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

/// Three notes, in listing order. `Paths::rooted` lays out the same four XDG
/// roots that `Browser::open` hands the child.
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

/// An editor that appends a line and exits.
///
/// A script, because `run_editor` splits on whitespace. Set in config, which wins
/// over the environment — setting only `$EDITOR` once opened the developer's vim.
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

struct Browser {
    writer: Box<dyn Write + Send>,
    screen: Arc<Mutex<vt100::Parser>>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
    // Dropping it closes the terminal.
    _master: Box<dyn portable_pty::MasterPty + Send>,
}

impl Browser {
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
        // All four: the active pointer lives in state, so missing that one
        // rewrites the developer's real pointer.
        command.env("XDG_CONFIG_HOME", root.0.join("config"));
        command.env("XDG_DATA_HOME", root.0.join("data"));
        command.env("XDG_STATE_HOME", root.0.join("state"));
        command.env("XDG_CACHE_HOME", root.0.join("cache"));
        command.env("TERM", "xterm-256color");

        let child = slave.spawn_command(command).expect("spawn noda tui");
        // Our copy must close, or the reader never sees end of stream.
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

    fn send(&mut self, keys: &str) {
        self.writer
            .write_all(keys.as_bytes())
            .expect("write to the terminal");
        self.writer.flush().expect("flush");
    }

    fn now(&self) -> String {
        self.screen.lock().expect("screen").screen().contents()
    }

    /// Leading blanks survive, so a column is measurable.
    fn rows(&self) -> Vec<String> {
        let parser = self.screen.lock().expect("screen");
        parser.screen().rows(0, u16::MAX).collect()
    }

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

    /// `q`, then `ctrl-c`: an open card swallows the `q` that dismisses it.
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
        // A failed assertion must not leave the child running.
        let _ = self.child.kill();
    }
}

/// By slug, because the id is minted.
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

fn column_of(rows: &[String], needle: &str) -> usize {
    rows.iter()
        .find_map(|row| row.find(needle))
        .unwrap_or_else(|| panic!("{needle:?} is not on the screen:\n{}", rows.join("\n")))
}

/// Terminal column of the count beside a tag.
///
/// Searched from the checkbox, because the listing shows beside the card and
/// `Meeting notes` contains ` note`; converted to columns, because `find` answers
/// in bytes.
fn count_column(row: &str) -> Option<usize> {
    let boxed = row.find("[x] ").or_else(|| row.find("[ ] "))?;
    let at = boxed + row[boxed..].find(" note")?;
    Some(cmd::display_width(&row[..at]))
}

/// `專案管理` is four characters but eight columns; `tests/tui.rs` cannot tell.
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

    // Wait on the card's footer; the tag is on the listing too.
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

    // The filter takes every letter, `q` included.
    browser.send("\x1b");
    browser.wait_until_gone("tab  choose");
    browser.quit();
}

/// Other layers hand the state machine `KeyCode::Tab`; only a real terminal shows
/// that `\t` arrives as `Tab` and not as a filter character.
#[test]
fn the_tab_that_chooses_arrives_as_tab_and_not_as_a_character() {
    let (root, _paths) = a_notebook();
    let mut browser = Browser::open(&root, 90, 28);
    browser.wait_for("Budget review");

    // The note carries `work`, so tab takes it off.
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
    // The screen's own title.
    assert!(screen.contains("Notes"), "{screen}");

    browser.quit();
}

/// The card has outgrown twenty-four rows three times.
#[test]
fn the_help_card_fits_a_twenty_four_row_terminal() {
    let (root, _paths) = a_notebook();
    let mut browser = Browser::open(&root, 90, 24);
    browser.wait_for("Budget review");

    // Not `keys`, which is also on the grid underneath.
    browser.send("?");
    let screen = browser.wait_for("half a screen");

    // What a card that measured itself wrong cut.
    assert!(
        screen.contains("tag:work OR tag:q3 budget"),
        "the search example was cut:\n{screen}"
    );
    // The last row.
    assert!(
        screen.contains("readline: ctrl-a/e/w/u/k/y"),
        "the card lost its last row:\n{screen}"
    );

    browser.quit();
}

/// At eighty columns the grid drops keys from the right; `:` must survive.
#[test]
fn the_command_key_survives_eighty_columns() {
    let (root, _paths) = a_notebook();
    let browser = Browser::open(&root, 80, 28);
    let screen = browser.wait_for("Budget review");

    // `<:>`, not `command`: `ctrl-a  commands` would pass without it.
    assert!(screen.contains("<:>"), "the prompt key went:\n{screen}");
    assert!(screen.contains("<?>"), "the help key went:\n{screen}");

    browser.quit();
}

/// A skipped blank cell slides every later cell left. The note screen has
/// blanks mid-grid.
#[test]
fn a_blank_cell_holds_its_column_open() {
    let (root, _paths) = a_notebook();
    // At ninety columns the note screen's fourth column is gone.
    let mut browser = Browser::open(&root, 110, 28);
    browser.wait_for("Budget review");

    // `scroll` is the only grid word the listing lacks.
    browser.send("\r");
    browser.wait_for("scroll");
    let rows = browser.rows();

    // One column; three of these sit on rows with a blank cell to their left.
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

/// A `Terminal::clear()` here once waited forever for a cursor-position reply,
/// which no in-process test could see.
#[test]
fn coming_back_from_the_editor_redraws_the_screen() {
    let (root, paths) = a_notebook();
    an_editor_that_edits(&root, &paths);
    let mut browser = Browser::open(&root, 90, 28);
    browser.wait_for("Budget review");

    // Wait for the file and for the browser to be back; a key sent between them
    // races the editor for the terminal.
    browser.send("e");
    wait_for_file(&note_file(&paths, "budget-review"), "edited by the test");
    browser.wait_for("budget-review");

    browser.send("\r");
    browser.wait_for("edited by the test");

    browser.quit();
}
