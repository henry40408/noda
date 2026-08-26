//! Signing a commit with GPG.
//!
//! libgit2 shells out to nothing, so a `commit.gpgsign = true` that `git commit`
//! honours does nothing here unless noda calls gpg itself. It does: build the
//! commit object, hand it to gpg as text, write it back with the signature.
//!
//! Only `OpenPGP` — a notebook configured for `ssh` or `x509` is told so rather
//! than quietly committed unsigned, because nothing downstream can tell an
//! unsigned commit from one nobody asked about.

use std::io::Write;
use std::process::{Command, Stdio};

use crate::{Error, Result};

/// gpg exiting 0 is not evidence it signed anything — a `gpg.program` that is
/// not gpg exits 0 too.
const ARMOR_HEADER: &str = "-----BEGIN PGP SIGNATURE-----";

/// Its existence is the decision: resolving to `None` means unsigned.
#[derive(Debug)]
pub struct Signer {
    /// `gpg.openpgp.program`, `gpg.program`, then `gpg` — git's own order, so a
    /// notebook signs with whatever `git commit` here would have used.
    program: String,
    /// `user.signingkey`. Absent lets gpg pick its default, as `git commit -S`
    /// without a configured key does.
    key: Option<String>,
}

/// `configured` is noda's own `sign`, outranking `commit.gpgsign` the way
/// `config.toml`'s `author` outranks `user.name`: a notebook is one program's
/// worth of decision and `commit.gpgsign` is a blanket one.
pub fn resolve(configured: Option<bool>, git: &git2::Config) -> Result<Option<Signer>> {
    let wanted = configured.unwrap_or_else(|| git.get_bool("commit.gpgsign").unwrap_or(false));
    if !wanted {
        return Ok(None);
    }

    // Unset is git's default of `openpgp`. Anything else is refused by name: a
    // user told "noda cannot do ssh" can act on it, one silently unsigned cannot.
    let format = git
        .get_string("gpg.format")
        .unwrap_or_else(|_| "openpgp".to_string());
    if format != "openpgp" {
        return Err(Error::msg(format!(
            "signing is on, but `gpg.format` is `{format}` — noda signs with OpenPGP only.\n\
             Set `gpg.format` to openpgp, or turn signing off with `noda config sign false`"
        )));
    }

    let program = git
        .get_string("gpg.openpgp.program")
        .or_else(|_| git.get_string("gpg.program"))
        .unwrap_or_else(|_| "gpg".to_string());
    let key = git
        .get_string("user.signingkey")
        .ok()
        .filter(|k| !k.is_empty());
    Ok(Some(Signer { program, key }))
}

impl Signer {
    /// stderr is inherited, not captured: gpg talks to its agent through it, and
    /// capturing turns a pinentry prompt into a hang with nothing on screen.
    pub fn sign(&self, content: &str) -> Result<String> {
        let mut command = Command::new(&self.program);
        // `-b` detached, `-s` sign, `-a` armored: the three git passes too.
        command.args(["-bsa"]);
        if let Some(key) = &self.key {
            command.args(["-u", key]);
        }
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| match e.kind() {
                std::io::ErrorKind::NotFound => Error::msg(format!(
                    "signing is on, but `{}` is not installed — install it, point \
                     `gpg.program` at it, or turn signing off with `noda config sign false`",
                    self.program
                )),
                _ => Error::msg(format!("could not run `{}`: {e}", self.program)),
            })?;

        // A few hundred bytes fits the pipe buffer, so this cannot deadlock
        // against a reader that has not started.
        child
            .stdin
            .take()
            .ok_or_else(|| Error::msg("gpg took no input"))?
            .write_all(content.as_bytes())?;

        let output = child.wait_with_output()?;
        if !output.status.success() {
            return Err(Error::msg(format!(
                "`{}` could not sign the commit — its output is above",
                self.program
            )));
        }
        let signature = String::from_utf8(output.stdout).map_err(|e| {
            Error::msg(format!(
                "`{}` returned a non-UTF-8 signature: {e}",
                self.program
            ))
        })?;
        if !signature.trim_start().starts_with(ARMOR_HEADER) {
            return Err(Error::msg(format!(
                "`{}` returned no OpenPGP signature — is it gpg?",
                self.program
            )));
        }
        Ok(signature)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// libgit2's in-memory config is read-only, so one to write into is on disk.
    struct Scratch(PathBuf);

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    /// Every lookup misses — a machine with no git configuration.
    fn empty() -> git2::Config {
        git2::Config::new().expect("empty config")
    }

    fn with(entries: &[(&str, &str)]) -> (Scratch, git2::Config) {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("noda-sign-{}-{n}.gitconfig", std::process::id()));
        let mut config = git2::Config::open(&path).expect("config on disk");
        for (key, value) in entries {
            config.set_str(key, value).expect("set");
        }
        (Scratch(path), config)
    }

    #[test]
    fn nothing_configured_anywhere_signs_nothing() {
        assert!(resolve(None, &empty()).unwrap().is_none());
    }

    #[test]
    fn git_alone_is_enough_to_turn_it_on() {
        let (_scratch, config) = with(&[("commit.gpgsign", "true")]);
        assert!(resolve(None, &config).unwrap().is_some());
    }

    #[test]
    fn nodas_own_setting_outranks_gits() {
        let (_on_file, on) = with(&[("commit.gpgsign", "true")]);
        assert!(resolve(Some(false), &on).unwrap().is_none());

        let (_off_file, off) = with(&[("commit.gpgsign", "false")]);
        assert!(resolve(Some(true), &off).unwrap().is_some());
    }

    #[test]
    fn a_format_noda_cannot_sign_is_named_rather_than_ignored() {
        let (_scratch, config) = with(&[("commit.gpgsign", "true"), ("gpg.format", "ssh")]);
        let err = resolve(None, &config).unwrap_err().to_string();
        assert!(err.contains("ssh"), "{err}");
        assert!(err.contains("OpenPGP only"), "{err}");

        // Only when it would have signed.
        assert!(resolve(Some(false), &config).unwrap().is_none());
    }

    #[test]
    fn the_format_specific_program_wins_and_the_key_is_optional() {
        let (_scratch, config) = with(&[
            ("commit.gpgsign", "true"),
            ("gpg.program", "generic"),
            ("gpg.openpgp.program", "specific"),
            ("user.signingkey", "ABCD1234"),
        ]);
        let signer = resolve(None, &config).unwrap().expect("signing");
        assert_eq!(signer.program, "specific");
        assert_eq!(signer.key.as_deref(), Some("ABCD1234"));

        let (_scratch, config) = with(&[("commit.gpgsign", "true"), ("gpg.program", "generic")]);
        let signer = resolve(None, &config).unwrap().expect("signing");
        assert_eq!(signer.program, "generic");
        assert_eq!(signer.key, None);

        let (_scratch, config) = with(&[("commit.gpgsign", "true")]);
        assert_eq!(resolve(None, &config).unwrap().unwrap().program, "gpg");
    }

    #[test]
    fn a_program_that_is_not_there_says_so_by_name() {
        let signer = Signer {
            program: "noda-no-such-gpg".to_string(),
            key: None,
        };
        let err = signer.sign("tree deadbeef\n").unwrap_err().to_string();
        assert!(err.contains("noda-no-such-gpg"), "{err}");
        assert!(err.contains("not installed"), "{err}");
    }
}
