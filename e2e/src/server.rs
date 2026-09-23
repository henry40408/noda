//! The server under test and the notebook it serves.
//!
//! The fixture is built by running the binary (`init`, then `add`) rather than
//! committed, since a git repository inside this one confuses every tool that
//! walks the tree, and so a change to what `add` writes reaches these tests.
//! **Nothing here asserts on an id**: ids are minted, so features name notes by
//! title.
//!
//! The binary is spawned directly, not via `cargo run`, so killing the held PID
//! kills the server and frees the port.

use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

/// Fixed, so a server can be left running between runs and features can name URLs.
pub const PORT: u16 = 8799;

pub const BASE_URL: &str = "http://127.0.0.1:8799";

pub const NOTEBOOK: &str = "default";

const STARTUP_TIMEOUT: Duration = Duration::from_mins(1);

/// The fixture notebook, its starting commit, and the bare repository standing
/// in for a git host. A static because [`reset`] runs in a cucumber hook that is
/// not handed the world's state.
struct Fixture {
    notebook: PathBuf,
    head: String,
    remote: PathBuf,
}

static FIXTURE: std::sync::OnceLock<Fixture> = std::sync::OnceLock::new();

/// Restores the fixture notebook and remote; called after every scenario.
///
/// **The suite is wrong without it**: scenarios share one notebook and some write
/// to it, so the second pass would otherwise read what the first one edited.
/// Cheaper than a notebook per scenario.
pub fn reset() -> Result<()> {
    let Some(fixture) = FIXTURE.get() else {
        // An adopted server's notebook is not ours to put back.
        return Ok(());
    };
    let notebook = &fixture.notebook;
    for args in [
        vec!["reset", "--hard", fixture.head.as_str()],
        // Untracked files, e.g. a note left by a step that failed part way.
        vec!["clean", "-fd"],
    ] {
        git(notebook, &args)?;
    }

    // **The remote is fixture too**: a sync leaves commits on the bare repository
    // and tracking refs, and "never synced" would then describe the previous
    // scenario.
    for refname in git(
        notebook,
        &["for-each-ref", "--format=%(refname)", "refs/remotes"],
    )?
    .lines()
    .map(str::to_string)
    .collect::<Vec<_>>()
    {
        git(notebook, &["update-ref", "-d", &refname])?;
    }
    bare_remote(&fixture.remote)?;
    Ok(())
}

/// A fresh bare repository standing in for a git host. libgit2's local transport
/// shares the push and fetch machinery of HTTPS and SSH.
fn bare_remote(path: &Path) -> Result<()> {
    if path.exists() {
        std::fs::remove_dir_all(path).with_context(|| format!("clearing {}", path.display()))?;
    }
    std::fs::create_dir_all(path)?;
    let done = Command::new("git")
        .args(["init", "--bare", "-q"])
        .arg(path)
        .output()
        .context("running git init --bare")?;
    if !done.status.success() {
        bail!(
            "git init --bare failed: {}",
            String::from_utf8_lossy(&done.stderr)
        );
    }
    Ok(())
}

/// Runs git in `repo`, returning stdout.
fn git(repo: &Path, args: &[&str]) -> Result<String> {
    let done = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .with_context(|| format!("running git {args:?}"))?;
    if !done.status.success() {
        bail!(
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&done.stderr)
        );
    }
    Ok(String::from_utf8_lossy(&done.stdout).into_owned())
}

/// A running server and its temporary notebook, killed and removed on drop —
/// unless the port was already open, in which case that server is left alone.
pub struct Server {
    child: Option<Child>,
    root: Option<PathBuf>,
}

impl Server {
    /// Starts a server on a fresh notebook, or adopts one already listening.
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
        // Bound first, so a half-built fixture is cleaned up on error.
        let mut server = Self {
            child: None,
            root: Some(root.clone()),
        };

        write_notebook(&binary, &root)?;

        let notebook = root.join("data/noda/notebooks/default");
        let head = git(&notebook, &["rev-parse", "HEAD"])?.trim().to_string();
        let _ = FIXTURE.set(Fixture {
            notebook,
            head,
            remote: root.join("origin.git"),
        });

        server.child = Some(
            Command::new(&binary)
                .args(["web", "--listen", &format!("127.0.0.1:{PORT}")])
                .envs(xdg(&root))
                // Inherited, so a refusal to start shows in the test output.
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

/// The four notes the features read. One body holds markup and one tag has a
/// space, because both come out of a real `noda import tiddlywiki`.
fn write_notebook(binary: &Path, root: &Path) -> Result<()> {
    let config = root.join("config/noda");
    std::fs::create_dir_all(&config)?;
    // libgit2 reads the developer's real git config, so a machine that signs
    // its commits would send every commit here to gpg.
    std::fs::write(config.join("config.toml"), "sign = false\n")?;

    run(binary, root, &["init"])?;
    // Two open todos and a done one. The absurd dates keep "overdue" independent
    // of when the suite runs.
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
    // A link for a backlink, its filename asked for since ids are minted. It
    // targets the meeting notes, not the budget: the filename is in the body, so
    // `…-budget-review.md` would make this note match `searching.feature`'s
    // search for "budget".
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
    // A file and a note pointing at it, for the files screen's backlink count.
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

    // A remote set but never synced: a state the listing's chip must show, and
    // the starting point for the sync scenarios.
    let remote = root.join("origin.git");
    bare_remote(&remote)?;
    run(binary, root, &["remote", "set", &remote.to_string_lossy()])?;
    Ok(())
}

fn run(binary: &Path, root: &Path, args: &[&str]) -> Result<()> {
    capture(binary, root, args).map(|_| ())
}

/// Runs noda, returning stdout. Only for `noda path`, whose output is meant for
/// programs; other commands' output is prose, not an interface.
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

/// All four, `XDG_STATE_HOME` included: the active-notebook pointer lives there,
/// and missing it would rewrite the developer's real one.
fn xdg(root: &Path) -> [(&'static str, PathBuf); 4] {
    [
        ("XDG_CONFIG_HOME", root.join("config")),
        ("XDG_DATA_HOME", root.join("data")),
        ("XDG_STATE_HOME", root.join("state")),
        ("XDG_CACHE_HOME", root.join("cache")),
    ]
}

/// The debug binary, built if missing: the release profile's fat LTO costs
/// minutes and changes nothing a page says.
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

fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("e2e/ always has a parent")
}
