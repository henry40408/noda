//! noda — a git-native notebook for your terminal.
//!
//! This is a spec-first project: the user-facing contract lives in `README.md` and
//! `docs/PRFAQ.md`. The CLI is not implemented yet. libgit2 (vendored, with HTTPS/SSH)
//! is already linked and validated for cross-compiling to linux musl/arm64.

fn main() {
    let (major, minor, patch) = git2::Version::get().libgit2_version();
    println!("noda (pre-implementation) — linked against libgit2 {major}.{minor}.{patch}");
    println!("See README.md and docs/PRFAQ.md for the v1 design.");
}
