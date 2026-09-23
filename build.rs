//! Stamps the version `noda --version` prints.
//!
//! `Cargo.toml`'s version is a placeholder; releases are git tags. It is stamped
//! at compile time because an installed binary is nowhere near its repository and
//! spawning `git` would cost a quick `noda ls` its startup. Sources, in order:
//!
//! 1. `NODA_VERSION`, for the Docker build, whose `.dockerignore` excludes `.git`;
//!    the workflow describes the tag and passes it in.
//! 2. `git describe` (`0.2.0`, `0.2.0-3-gabc1234`, or a bare commit before any tag).
//! 3. The manifest version, for a source tarball with no git.

use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo::rerun-if-env-changed=NODA_VERSION");
    // A tag or commit changes the version without touching a source file. Naming
    // any path disables Cargo's default rerun-on-package-change, so HEAD and all
    // of `refs/` are listed: a commit moves only a branch ref.
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

/// `v0.2.0` -> `0.2.0`.
fn number(tag: &str) -> String {
    tag.strip_prefix('v').unwrap_or(tag).to_string()
}

/// Only paths that exist: Cargo treats a missing one as always changed.
fn git_inputs() -> Vec<PathBuf> {
    // In a worktree, HEAD is in its own git dir but refs are in the common one.
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
