//! Regenerates the four screenshots `README.md` embeds, into `../screenshots/`.
//!
//!   cargo build && cd e2e && cargo run --bin screenshots
//!
//! The notebook is its own, written by the binary like the features' fixture,
//! so what the pictures show is what `add` really writes. Font rendering and
//! the Chrome version differ by machine: an unrelated image changing in the
//! diff means the browser changed, not the page.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use noda_e2e::Server;
use noda_e2e::browser::{Browser, MONITOR, PHONE, Scripting};
use noda_e2e::server::{BASE_URL, NOTEBOOK, capture, port_is_open, run};

/// One note with its tags and body. A body names a note it links to by title,
/// resolved to a filename once that note exists.
struct Note {
    title: &'static str,
    tags: &'static [&'static str],
    body: &'static str,
}

/// Ordered so every note a body links to is written before it. The open one,
/// "Notes on TAOCP", is linked from three others, so its margin has company.
const NOTES: &[Note] = &[
    Note {
        title: "Notes on TAOCP",
        tags: &["books", "algorithms"],
        body: "## Volume 1: Fundamental Algorithms\n\n\
               Knuth builds everything from a small machine, **MIX**, so that the cost of \
               an algorithm is a number of instructions and not a guess.\n\n\
               - Algorithm E, Euclid's: `gcd(m, n)` ends when the remainder is zero\n\
               - Linked allocation beats a contiguous array when the size is unknown\n\
               - Every analysis starts by counting what the inner loop does\n\n\
               ```\n\
               E1. [Find remainder.] Divide m by n; let r be the remainder.\n\
               E2. [Is it zero?] If r = 0, the algorithm terminates; n is the answer.\n\
               E3. [Reduce.] Set m <- n, n <- r, and go back to E1.\n\
               ```\n\n\
               > Premature optimization is the root of all evil.\n",
    },
    Note {
        title: "Reading list",
        tags: &["books"],
        body: "What is next, and what has been read.\n\n\
               - [Notes on TAOCP]({Notes on TAOCP}), volume 1, partway\n\
               - Designing Data-Intensive Applications\n\
               - The Pragmatic Programmer\n",
    },
    Note {
        title: "Algorithms cheat sheet",
        tags: &["algorithms"],
        body: "Sorting, searching and the bounds to remember. The long form is in \
               [Notes on TAOCP]({Notes on TAOCP}).\n\n\
               | Operation | Cost |\n|---|---|\n| binary search | O(log n) |\n| heap push | O(log n) |\n",
    },
    Note {
        title: "Weekly planning",
        tags: &["work", "planning"],
        body: "The week of the Q3 review.\n\n\
               - [ ] send the budget draft to Ana due:2999-06-14\n\
               - [ ] book the offsite venue due:2999-06-20\n\
               - [ ] chase the marketing line due:2000-01-01\n\
               - [x] pull the ledger export\n\n\
               Reading for the train: [Notes on TAOCP]({Notes on TAOCP}).\n",
    },
    Note {
        title: "Trip to Kyoto",
        tags: &["travel"],
        body: "Late autumn, five days.\n\n\
               - [ ] reserve the ryokan due:2999-09-01\n\
               - [ ] buy a rail pass\n\
               - [x] renew the passport\n\n\
               Fushimi Inari early, before the crowds.\n",
    },
    Note {
        title: "Home server runbook",
        tags: &["ops", "home"],
        body: "How the box in the closet is kept alive.\n\n\
               - Backups run nightly to the second disk\n\
               - `noda sync` pushes this notebook to the private remote\n",
    },
];

/// The note the seed pins.
const PINNED: &str = "weekly-planning";

/// The two screens: a note beside its listing and backlinks on a monitor, and
/// the listing alone on a phone, which is what `noda web` is for.
const SCREENS: [(&str, &str, (u32, u32)); 2] = [
    ("web-note", "/n/notes-on-taocp", MONITOR),
    ("web-phone", "", PHONE),
];

#[tokio::main]
async fn main() -> Result<()> {
    if port_is_open() {
        bail!(
            "something already listens on {BASE_URL}; stop it, or the pictures show its notebook"
        );
    }
    let output = screenshot_dir();
    std::fs::create_dir_all(&output).with_context(|| format!("creating {}", output.display()))?;

    // Killed, and its notebook removed, on drop.
    let _server = Server::start_with(write_demo)?;

    for scheme in ["light", "dark"] {
        take(scheme, &output).await?;
    }
    println!("screenshots: wrote four images to {}", output.display());
    Ok(())
}

/// Where `README.md` looks for the images.
fn screenshot_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("e2e/ always has a parent")
        .join("screenshots")
}

fn write_demo(binary: &Path, root: &Path) -> Result<()> {
    let config = root.join("config/noda");
    std::fs::create_dir_all(&config)?;
    // libgit2 reads the developer's real git config, so a machine that signs its
    // commits would send every commit here to gpg.
    std::fs::write(config.join("config.toml"), "sign = false\n")?;
    run(binary, root, &["init"])?;

    let mut files: Vec<(&str, String)> = Vec::new();
    for note in NOTES {
        let mut body = note.body.to_string();
        for (title, file) in &files {
            body = body.replace(&format!("{{{title}}}"), file);
        }
        let mut args = vec!["add", note.title, "-c", &body];
        for tag in note.tags {
            args.extend(["--tag", tag]);
        }
        run(binary, root, &args)?;

        let path = capture(binary, root, &["path", &slug(note.title)])?;
        let file = Path::new(path.trim())
            .file_name()
            .context("noda path did not answer with a filename")?
            .to_string_lossy()
            .into_owned();
        files.push((note.title, file));
    }
    run(binary, root, &["pin", PINNED])?;
    Ok(())
}

/// What `noda path` takes: the title as the filename spells it.
fn slug(title: &str) -> String {
    title
        .to_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

/// Both screens, in one colour scheme.
async fn take(scheme: &str, output: &Path) -> Result<()> {
    let browser = Browser::open(Scripting::Enabled).await?;
    browser.prefer_scheme(scheme).await?;
    let suffix = if scheme == "dark" { "-dark" } else { "" };

    for (name, path, viewport) in SCREENS {
        browser.resize(viewport).await?;
        browser
            .driver()
            .goto(format!("{BASE_URL}/nb/{NOTEBOOK}{path}"))
            .await?;
        // The panes and stamps are scripts that run after the page has loaded.
        tokio::time::sleep(Duration::from_millis(400)).await;
        browser
            .screenshot(&output.join(format!("{name}{suffix}.png")))
            .await?;
    }
    browser.quit().await
}
