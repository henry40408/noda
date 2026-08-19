//! Credentials for the network commands.
//!
//! The binary carries its own libgit2, libssh2 and OpenSSL, so it cannot lean on
//! the system `git` to authenticate — it has to find credentials itself. libgit2
//! calls back repeatedly until one succeeds, so every method is offered at most
//! once and the callback then gives up rather than looping.

use std::borrow::Cow;

use git2::{Config, Cred, CredentialType, FetchOptions, RemoteCallbacks};

use crate::Error;

/// The order the methods are offered in. `USERNAME` comes after the key because
/// libgit2 asks for it before the key itself when the URL carries no username.
const METHODS: [CredentialType; 4] = [
    CredentialType::SSH_KEY,
    CredentialType::USER_PASS_PLAINTEXT,
    CredentialType::USERNAME,
    CredentialType::DEFAULT,
];

/// The one credential a given method can produce.
///
/// Only the HTTPS method consults configuration, and it reads whatever `config`
/// layers together — which is why the caller supplies it. Opening the default
/// config here would have left out the repository's own `.git/config`, so a
/// helper set for one notebook alone was invisible to the very commands that
/// needed it.
fn credential(
    config: &Config,
    url: &str,
    username: Option<&str>,
    kind: CredentialType,
) -> Result<Cred, git2::Error> {
    if kind == CredentialType::SSH_KEY {
        Cred::ssh_key_from_agent(username.unwrap_or("git"))
    } else if kind == CredentialType::USER_PASS_PLAINTEXT {
        Cred::credential_helper(config, url, username)
    } else if kind == CredentialType::USERNAME {
        Cred::username(username.unwrap_or("git"))
    } else {
        Cred::default()
    }
}

/// Credential callbacks covering the two transports noda ships: SSH by way of an
/// agent, and HTTPS by way of git's credential helper. `config` is where the
/// helpers are read from — a notebook passes its repository's own.
pub fn callbacks<'a>(config: Config) -> RemoteCallbacks<'a> {
    let mut offered = CredentialType::empty();
    let mut callbacks = RemoteCallbacks::new();
    callbacks.credentials(move |url, username, allowed| {
        for kind in METHODS {
            if allowed.contains(kind) && !offered.contains(kind) {
                offered |= kind;
                return credential(&config, url, username, kind);
            }
        }
        Err(git2::Error::from_str("no usable credentials"))
    });
    callbacks
}

pub fn fetch_options<'a>(config: Config) -> FetchOptions<'a> {
    let mut options = FetchOptions::new();
    options.remote_callbacks(callbacks(config));
    options
}

/// Whether an error is really about credentials. Only the SSH transport gets its
/// own error class; a credential lookup that comes back empty — the commonest
/// HTTPS failure, and the one the hint is written for — surfaces as an untyped
/// generic error, so its wording is the only thing left to go on.
fn is_authentication(error: &git2::Error) -> bool {
    if error.class() == git2::ErrorClass::Ssh || error.code() == git2::ErrorCode::Auth {
        return true;
    }
    let message = error.message().to_ascii_lowercase();
    ["authentication", "username/password", "credential"]
        .iter()
        .any(|needle| message.contains(needle))
}

/// Turns libgit2's authentication failures into advice. Everything else is
/// passed through untouched — libgit2's own message is usually the better one.
pub fn explain(error: git2::Error, url: &str) -> Error {
    if !is_authentication(&error) {
        return Error::Git(error);
    }
    let hint = if url.starts_with("http") {
        "noda reads HTTPS credentials from git's credential helper — check `git config credential.helper`, \
         or store a token with `git credential approve`. noda carries its own libgit2, which reads only \
         `.git/config`, `~/.gitconfig`, `~/.config/git/config` and `/etc/gitconfig`: a helper that `git` \
         picks up from its own installation directory is invisible here and has to be repeated in one of those"
    } else {
        "noda reads SSH keys from ssh-agent — check `ssh-add -l` and add your key with `ssh-add`"
    };
    Error::msg(format!("{}: {}\n{hint}", redact(url), error.message()))
}

/// A remote URL with its credentials taken out, for anything a person reads.
///
/// A token in the URL is not an exotic setup, it is the ordinary one wherever
/// the credential helper cannot be reached: the container image carries no
/// shell, so the helper never runs and the URL is the only place left to put
/// the secret. That makes a remote URL something to assume is carrying one, and
/// every screen that shows a remote — `noda status`, `noda remote show`, the
/// notebook listing, the TUI, the web status page, and the error a failed sync
/// prints — shows it through here.
///
/// The whole userinfo goes, not the password alone. Gitea and Forgejo take the
/// token as the *username*, so a redaction that kept the username would leak
/// the secret on exactly the hosts that ask for it there.
///
/// What is left still names the host and the path, which is what a remote is
/// read for. The URL as configured stays in `.git/config`, where it was put.
pub fn redact(url: &str) -> Cow<'_, str> {
    // `git@github.com:me/notes.git` is scp syntax rather than a URL: it has no
    // scheme, and the `git@` in front of it is a username with nothing to hide.
    let Some(mark) = url.find("://") else {
        return Cow::Borrowed(url);
    };
    let (scheme, rest) = url.split_at(mark);
    let rest = &rest["://".len()..];
    let authority = &rest[..rest.find(['/', '?', '#']).unwrap_or(rest.len())];
    // `rfind`, because a password may hold an `@` and the userinfo ends at the
    // last one. The search stops at the authority because a *path* may hold one
    // too — `https://host/a@b.git` has no credentials in it at all.
    let Some(at) = authority.rfind('@') else {
        return Cow::Borrowed(url);
    };
    // Over SSH a bare username is not a secret: the key authenticates and never
    // travels in the URL. A password does, and over HTTPS so does anything at
    // all, because that is where a token is put.
    let secret = authority[..at].contains(':') || matches!(scheme, "http" | "https");
    if !secret {
        return Cow::Borrowed(url);
    }
    Cow::Owned(format!("{scheme}://***@{}", &rest[at + 1..]))
}

/// The notebook name implied by a clone URL: the last path segment, minus `.git`.
pub fn name_from_url(url: &str) -> Option<String> {
    let tail = url
        .trim_end_matches('/')
        .rsplit(['/', ':'])
        .next()?
        .trim_end_matches(".git");
    if tail.is_empty() {
        return None;
    }
    Some(tail.to_string())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    /// A repository on disk: the point of the test is the config file inside it,
    /// and an in-memory config has no `.git/config` to be layered over.
    struct TempRepo(PathBuf, git2::Repository);

    impl TempRepo {
        fn new() -> Self {
            static COUNTER: AtomicUsize = AtomicUsize::new(0);
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!("noda-remote-{}-{n}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            let repo = git2::Repository::init(&path).expect("init repo");
            TempRepo(path, repo)
        }
    }

    impl Drop for TempRepo {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// A helper set for one notebook alone has to count. The lookup reads the
    /// config it is handed, and `Repository::config` layers `.git/config` over
    /// the global and system files — which opening the default config did not.
    ///
    /// The helper is scoped to a URL so that whatever the machine running the
    /// test already configures cannot answer in its place.
    #[test]
    fn a_helper_in_the_repositorys_own_config_is_reached() {
        const URL: &str = "https://example.invalid/notes.git";

        let repo = TempRepo::new();
        let mut config = repo.1.config().expect("config");
        config
            .set_str(
                "credential.https://example.invalid.helper",
                "!printf 'username=u\\npassword=p\\n'",
            )
            .expect("set helper");

        let found = credential(&config, URL, None, CredentialType::USER_PASS_PLAINTEXT);
        assert!(found.is_ok(), "{:?}", found.err());

        // The same lookup with nothing configured anywhere: this is the failure
        // the hint in `explain` is written for.
        let bare = git2::Config::new().expect("config");
        let missing = credential(&bare, URL, None, CredentialType::USER_PASS_PLAINTEXT);
        assert!(missing.is_err(), "a config with no helper produced one");
    }

    #[test]
    fn clone_names_come_from_the_last_url_segment() {
        assert_eq!(
            name_from_url("git@github.com:me/work-notes.git").as_deref(),
            Some("work-notes")
        );
        assert_eq!(
            name_from_url("https://github.com/me/notes").as_deref(),
            Some("notes")
        );
        assert_eq!(
            name_from_url("/srv/backups/notes.git/").as_deref(),
            Some("notes")
        );
        assert_eq!(name_from_url("/"), None);
    }

    /// What a remote may be carrying, and what is safe to leave on a screen.
    #[test]
    fn credentials_come_out_of_a_url_before_anyone_reads_it() {
        // GitHub and GitLab put the token where the password goes.
        assert_eq!(
            redact("https://x-access-token:ghp_secret@github.com/me/notes.git"),
            "https://***@github.com/me/notes.git"
        );
        // Gitea and Forgejo take it as the username, so the username goes too.
        assert_eq!(
            redact("https://tok_secret@codeberg.org/me/notes.git"),
            "https://***@codeberg.org/me/notes.git"
        );
        // A password may hold an `@`, so the userinfo ends at the last one.
        assert_eq!(
            redact("https://me:p@ssw0rd@git.example.com/notes.git"),
            "https://***@git.example.com/notes.git"
        );

        // Nothing to hide: these come back exactly as they went in, because a
        // remote that reads differently from the one you configured is its own
        // kind of confusing.
        for url in [
            "https://github.com/me/notes.git",
            // scp syntax, where `git@` is a username and the key does the
            // authenticating somewhere else entirely.
            "git@github.com:me/notes.git",
            "ssh://git@github.com/me/notes.git",
            // The `@` is in the path here — the authority ended before it.
            "https://git.example.com/me/a@b.git",
            // A remote is allowed to be a directory.
            "/srv/backups/notes.git",
        ] {
            assert_eq!(redact(url), url);
        }

        // ...but a password travels whatever the scheme is, so it still goes.
        assert_eq!(
            redact("ssh://me:pw@git.example.com/notes.git"),
            "ssh://***@git.example.com/notes.git"
        );
    }

    /// The URL goes into the message, and a sync that failed to authenticate is
    /// exactly the moment the URL is most likely to be carrying a token.
    #[test]
    fn a_failure_does_not_print_the_token_it_failed_with() {
        let explained = explain(
            git2::Error::from_str("no usable credentials"),
            "https://x-access-token:ghp_secret@github.com/me/notes.git",
        )
        .to_string();
        assert!(!explained.contains("ghp_secret"), "{explained}");
        assert!(explained.contains("***@github.com"), "{explained}");
    }

    /// The message libgit2 actually produces when no credential helper answers.
    /// It carries no error class and no `Auth` code, so matching on the class
    /// alone left the commonest HTTPS failure without its hint.
    #[test]
    fn an_empty_credential_lookup_counts_as_authentication() {
        let error =
            git2::Error::from_str("failed to acquire username/password from local configuration");
        assert_eq!(error.class(), git2::ErrorClass::None);
        assert_eq!(error.code(), git2::ErrorCode::GenericError);
        assert!(is_authentication(&error));

        let explained = explain(error, "https://example.com/notes.git").to_string();
        assert!(explained.contains("credential helper"), "{explained}");
    }

    #[test]
    fn the_hint_follows_the_transport() {
        let ssh = explain(
            git2::Error::new(
                git2::ErrorCode::Auth,
                git2::ErrorClass::Ssh,
                "no auth sock variable",
            ),
            "git@github.com:me/notes.git",
        )
        .to_string();
        assert!(ssh.contains("ssh-agent"), "{ssh}");

        // noda's own callback error, when every method has been offered once.
        let https = explain(
            git2::Error::from_str("no usable credentials"),
            "https://example.com/notes.git",
        )
        .to_string();
        assert!(https.contains("credential helper"), "{https}");
        // The config files noda actually reads. A helper that only `git` can see
        // — one in its own installation's `etc/gitconfig` — looks identical to
        // no helper at all from here, so the hint has to name them.
        assert!(https.contains("/etc/gitconfig"), "{https}");
    }

    /// A hint about credentials on an error that has nothing to do with them
    /// sends the reader looking in the wrong place.
    #[test]
    fn unrelated_failures_keep_libgit2s_own_message() {
        let error = git2::Error::from_str("the remote hung up unexpectedly");
        assert!(!is_authentication(&error));

        let explained = explain(error, "https://example.com/notes.git").to_string();
        assert!(!explained.contains("credential helper"), "{explained}");
        assert!(explained.contains("hung up"), "{explained}");
    }
}
