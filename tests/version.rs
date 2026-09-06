//! `noda --version` against the real binary, because what the version is gets
//! decided in build.rs and baked in at compile time — there is nothing for a
//! library test to call.

use std::process::Command;

#[test]
fn version_is_the_tag_the_build_came_from() {
    // Not skipped when `NODA_VERSION` is set: Cargo puts every `rustc-env` into
    // the test's own environment too, so its presence says nothing about whether
    // anybody overrode the version. A build that did override it fails here, and
    // should — the assertion is that a build from this repository reports this
    // repository's tag.
    let Some(described) = describe() else {
        eprintln!("skipped: no git, so the binary carries Cargo.toml's version");
        return;
    };
    let expected = described.strip_prefix('v').unwrap_or(&described);

    let output = Command::new(env!("CARGO_BIN_EXE_noda"))
        .arg("--version")
        .output()
        .expect("run the built binary");

    let printed = String::from_utf8(output.stdout).expect("--version is utf-8");
    assert_eq!(printed.trim(), format!("noda {expected}"));
}

fn describe() -> Option<String> {
    let output = Command::new("git")
        .args(["describe", "--tags", "--always", "--match", "v[0-9]*"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8(output.stdout).ok())
        .flatten()
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
}
