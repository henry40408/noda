//! The server under test, and the notebook it serves.
//!
//! Both are built here rather than committed. A notebook is a git repository,
//! and a git repository inside this one is a thing to have to explain to every
//! tool that walks the tree — so the fixture is made the way a person makes one,
//! by running the binary: `init`, then `add` a few times. That also keeps the
//! fixture honest, because anything that changes what `add` writes changes what
//! these tests read.
//!
//! **Nothing here asserts on an id.** Ids are minted, so a fixture built by
//! running `add` cannot know them in advance; the features name notes by their
//! titles, which is what a reader sees anyway.
//!
//! The binary is spawned directly rather than through `cargo run`, so the PID
//! held here is the server's own — killing `cargo` would leave the server it
//! spawned holding the port.

use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

/// Fixed, so a developer can leave a server running between runs and so the
/// features can name URLs without asking anything at runtime.
pub const PORT: u16 = 8799;

/// What every navigation is relative to.
pub const BASE_URL: &str = "http://127.0.0.1:8799";

/// The notebook the features are written against.
pub const NOTEBOOK: &str = "default";

const STARTUP_TIMEOUT: Duration = Duration::from_mins(1);

/// The notebook the fixture was built in, and the commit it was built to.
///
/// Set once at start-up and read by [`reset`] after every scenario. A pair of
/// statics rather than something threaded through the world, because the hook
/// that resets is cucumber's and takes what cucumber gives it.
static FIXTURE: std::sync::OnceLock<(PathBuf, String)> = std::sync::OnceLock::new();

/// Puts the notebook back the way the fixture left it.
///
/// **Called after every scenario, and the suite is wrong without it.** The
/// scenarios share one notebook and some of them now write to it: one renames a
/// note, one takes a tag off another, one rewrites a body. Run once they happen
/// to pass, because the features that read come before the features that write;
/// run twice — which is what the scripted and script-less passes are — the
/// second pass opens a notebook the first one edited and fails on notes that are
/// no longer called what they were called.
///
/// Resetting is cheaper than a notebook per scenario and says the same thing: a
/// scenario's `Given` is either true or the scenario is not testing what it
/// says. `git` is shelled out to rather than linked, because this is a test
/// harness restoring a fixture and `reset --hard` is exactly the sentence.
///
/// # Errors
///
/// Fails when git refuses, which means the fixture is not recoverable and every
/// scenario after this one would be reporting on the wrong notebook.
pub fn reset() -> Result<()> {
    let Some((notebook, head)) = FIXTURE.get() else {
        // No fixture was built: a server was adopted rather than started, so
        // whatever it is serving is not ours to put back.
        return Ok(());
    };
    for args in [
        vec!["reset", "--hard", head.as_str()],
        // Untracked files a scenario made and did not commit — a half-written
        // note left by a step that failed part way.
        vec!["clean", "-fd"],
    ] {
        let done = Command::new("git")
            .arg("-C")
            .arg(notebook)
            .args(&args)
            .output()
            .with_context(|| format!("running git {args:?}"))?;
        if !done.status.success() {
            bail!(
                "git {args:?} failed: {}",
                String::from_utf8_lossy(&done.stderr)
            );
        }
    }
    Ok(())
}

/// A running server, and the temporary notebook it is reading.
///
/// Killed and removed when dropped — unless the port was already open before
/// the suite started, in which case somebody else's server is left alone.
pub struct Server {
    child: Option<Child>,
    root: Option<PathBuf>,
}

impl Server {
    /// Starts a server on a fresh notebook, or adopts one already listening.
    ///
    /// # Errors
    ///
    /// Fails when the binary cannot be built or spawned, when the fixture
    /// notebook cannot be written, or when the server does not start listening.
    pub fn start() -> Result<Self> {
        if port_is_open() {
            return Ok(Self {
                child: None,
                root: None,
            });
        }

        let binary = ensure_binary()?;
        let root = std::env::temp_dir().join(format!("noda-e2e-{}", std::process::id()));
        std::fs::create_dir_all(&root)?;
        // Bound before anything else can fail, so a half-built fixture is still
        // cleaned up when the error propagates.
        let mut server = Self {
            child: None,
            root: Some(root.clone()),
        };

        write_notebook(&binary, &root)?;

        // Where the fixture stands before any scenario has touched it. Every
        // scenario is put back to exactly this.
        let notebook = root.join("data/noda/notebooks/default");
        let head = Command::new("git")
            .arg("-C")
            .arg(&notebook)
            .args(["rev-parse", "HEAD"])
            .output()
            .context("reading the fixture's HEAD")?;
        if !head.status.success() {
            bail!(
                "git rev-parse failed: {}",
                String::from_utf8_lossy(&head.stderr)
            );
        }
        let _ = FIXTURE.set((
            notebook,
            String::from_utf8_lossy(&head.stdout).trim().to_string(),
        ));

        server.child = Some(
            Command::new(&binary)
                .args(["web", "--listen", &format!("127.0.0.1:{PORT}")])
                .envs(xdg(&root))
                // Inherited, so a refusal to start is visible in the test output
                // rather than swallowed into a pipe nobody reads.
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit())
                .spawn()
                .with_context(|| format!("spawning {} web", binary.display()))?,
        );

        wait_until_listening()?;
        Ok(server)
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
        if let Some(root) = self.root.as_ref() {
            let _ = std::fs::remove_dir_all(root);
        }
    }
}

/// The four notebook-worth of notes the features read.
///
/// One of them holds markup, because `noda import tiddlywiki` deliberately
/// leaves such bodies alone: a note that is markup is an ordinary note here and
/// not a hypothetical. One carries a tag with a space in it, for the same
/// reason — that shape comes out of a real import.
fn write_notebook(binary: &Path, root: &Path) -> Result<()> {
    let config = root.join("config/noda");
    std::fs::create_dir_all(&config)?;
    // libgit2 reads the developer's real git config, so a machine that signs
    // its commits would send every commit here to gpg.
    std::fs::write(config.join("config.toml"), "sign = false\n")?;

    run(binary, root, &["init"])?;
    // Two unticked boxes and a ticked one, so the todo screen has something to
    // order and something to leave out. The dates are absurd on purpose:
    // `due:2000-01-01` is late whenever the suite runs and `due:2999-12-31` is
    // not, so what the scenarios read is the comparison and not the clock.
    run(
        binary,
        root,
        &[
            "add",
            "Budget review",
            "-c",
            "the q3 budget is late\n\n\
             - [ ] chase the marketing line due:2000-01-01\n\
             - [ ] send the draft to Ana due:2999-12-31\n\
             - [x] pull the ledger export\n",
            "--tag",
            "work",
        ],
    )?;
    run(
        binary,
        root,
        &[
            "add",
            "Meeting notes",
            "-c",
            "the budget, again",
            "--tag",
            "work",
            "--tag",
            "24.04 Dark patterns",
        ],
    )?;
    // One note pointing at another, which is the only way a backlink can exist.
    // The destination is asked for rather than spelled out: an id is minted, so
    // a fixture that wrote one down would be a fixture that could not be built
    // twice — the standing rule about test ids, one layer up.
    //
    // It points at the meeting notes and not at the budget, and that is not
    // arbitrary: a destination is a *filename*, so a note linking to
    // `…-budget-review.md` holds the word "budget" in its body and turns up in
    // a search for it. One scenario in `searching.feature` asks for exactly the
    // note that would then be wrong.
    let notes = capture(binary, root, &["path", "meeting-notes"])?;
    let notes = Path::new(notes.trim())
        .file_name()
        .context("noda path did not answer with a filename")?
        .to_string_lossy()
        .to_string();
    run(
        binary,
        root,
        &[
            "add",
            "Reading list",
            "-c",
            &format!("a book, and [the notes]({notes})"),
        ],
    )?;
    // A file, and a note that points at it. The files screen counts what points
    // at each one and that count is the only door to a file's backlinks — a file
    // has no page of its own — so a notebook with no files at all could not
    // exercise it.
    let png = root.join("rack.png");
    std::fs::write(&png, b"\x89PNG\r\n\x1a\nnot really")?;
    run(binary, root, &["file", "add", &png.to_string_lossy()])?;
    run(
        binary,
        root,
        &[
            "add",
            "Markup import",
            "-c",
            "a <b>bold</b> here, and ![the rack](rack.png)",
            "--tag",
            "ops",
        ],
    )?;
    Ok(())
}

fn run(binary: &Path, root: &Path, args: &[&str]) -> Result<()> {
    capture(binary, root, args).map(|_| ())
}

/// The same, kept for what it printed.
///
/// Only the fixture needs this, and only to ask noda where it just put
/// something. Reading a command's output is otherwise a thing to avoid — what
/// `cmd` prints is written for a person — but `noda path` exists precisely to be
/// read by another program, and its whole answer is one path.
fn capture(binary: &Path, root: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new(binary)
        .args(args)
        .envs(xdg(root))
        .output()
        .with_context(|| format!("running {} {args:?}", binary.display()))?;
    if !output.status.success() {
        bail!(
            "{} {args:?} failed: {}",
            binary.display(),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// All four, `XDG_STATE_HOME` included: the active-notebook pointer lives in
/// state rather than in data, and a run that missed it would reach past this
/// fixture and rewrite the real one.
fn xdg(root: &Path) -> [(&'static str, PathBuf); 4] {
    [
        ("XDG_CONFIG_HOME", root.join("config")),
        ("XDG_DATA_HOME", root.join("data")),
        ("XDG_STATE_HOME", root.join("state")),
        ("XDG_CACHE_HOME", root.join("cache")),
    ]
}

/// Path to the binary, building it when it is not there.
///
/// The debug one on purpose. The release profile is tuned for size and cold
/// start — fat LTO, one codegen unit — which costs minutes and answers a
/// question no browser is asking. What is under test here is what the pages
/// say, and both profiles say the same thing.
fn ensure_binary() -> Result<PathBuf> {
    let binary = repo_root().join("target/debug/noda");
    if binary.is_file() {
        return Ok(binary);
    }

    eprintln!("e2e: {} is missing — building it", binary.display());
    let status = Command::new("cargo")
        .current_dir(repo_root())
        .arg("build")
        .status()
        .context("running `cargo build`")?;
    if !status.success() {
        bail!("`cargo build` failed with {status}");
    }
    if !binary.is_file() {
        bail!("`cargo build` did not produce {}", binary.display());
    }
    Ok(binary)
}

fn wait_until_listening() -> Result<()> {
    let deadline = Instant::now() + STARTUP_TIMEOUT;
    while Instant::now() < deadline {
        if port_is_open() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    bail!("noda web did not start listening on 127.0.0.1:{PORT} within {STARTUP_TIMEOUT:?}")
}

fn port_is_open() -> bool {
    TcpStream::connect(("127.0.0.1", PORT)).is_ok()
}

/// The repository root — the parent of this crate's directory.
fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("e2e/ always has a parent")
}
