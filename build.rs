//! Where the version `noda --version` prints comes from.
//!
//! `version` in `Cargo.toml` is a placeholder and stays one: releases are cut as
//! git tags through `gh release`, so the tag is the only place a real version
//! number is written down, and Cargo has no way to read it. Hence a build
//! script, which stamps one into the binary at compile time — a runtime `git`
//! call would be wrong twice over, since the installed binary is nowhere near
//! the repository it was built from and starting a process is most of what a
//! quick `noda ls` costs.
//!
//! Three sources, in order:
//!
//! 1. `NODA_VERSION`, because the Docker build cannot use the next one —
//!    `.dockerignore` excludes `.git`, and un-ignoring it would put the whole
//!    history into every build context. The workflow describes the tag on the
//!    runner and passes the answer in.
//! 2. `git describe`, which names a tagged build after its tag and any other
//!    after the commits since (`0.2.0-3-gabc1234`), or after the commit alone
//!    while no tag exists yet.
//! 3. Failing both — a source tarball with no git — the manifest's version.

use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo::rerun-if-env-changed=NODA_VERSION");
    // A tag or a commit changes the version without changing a source file, so
    // git's refs are an input to this script as much as its own text is. Naming
    // any of them switches off Cargo's default "rerun when the package changes",
    // which is why HEAD and the whole of `refs/` are listed rather than the tags
    // alone: a commit moves a branch ref and touches nothing else.
    for path in git_inputs() {
        println!("cargo::rerun-if-changed={}", path.display());
    }
    println!("cargo::rustc-env=NODA_VERSION={}", version());
}

fn version() -> String {
    if let Some(from_env) = std::env::var("NODA_VERSION")
        .ok()
        .filter(|value| !value.trim().is_empty())
    {
        return number(from_env.trim());
    }
    if let Some(described) = git(&["describe", "--tags", "--always", "--match", "v[0-9]*"]) {
        return number(&described);
    }
    std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "unknown".into())
}

/// A tag is `v0.2.0` and a version is `0.2.0`: `--version` prints a number, not
/// the name of the ref it was cut from.
fn number(tag: &str) -> String {
    tag.strip_prefix('v').unwrap_or(tag).to_string()
}

/// The files whose mtime says the answer may have changed. Only the ones that
/// exist — Cargo treats a path that is not there as changed, which would rerun
/// this on every build.
fn git_inputs() -> Vec<PathBuf> {
    // A worktree's own git dir holds HEAD; the branches and tags live in the
    // common one it shares with the checkout it was made from.
    let Some(git_dir) = git(&["rev-parse", "--absolute-git-dir"]) else {
        return Vec::new();
    };
    let common = git(&["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .unwrap_or_else(|| git_dir.clone());
    [
        PathBuf::from(git_dir).join("HEAD"),
        PathBuf::from(&common).join("refs"),
        PathBuf::from(&common).join("packed-refs"),
    ]
    .into_iter()
    .filter(|path| path.exists())
    .collect()
}

fn git(args: &[&str]) -> Option<String> {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").ok()?;
    let output = Command::new("git")
        .args(args)
        .current_dir(manifest)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let text = text.trim().to_string();
    (!text.is_empty()).then_some(text)
}
