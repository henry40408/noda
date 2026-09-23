//! Credentials for the network commands.
//!
//! The bundled libgit2 cannot lean on the system `git` to authenticate. libgit2
//! calls back until one method succeeds, so each is offered at most once.

use std::borrow::Cow;

use git2::{Config, Cred, CredentialType, FetchOptions, RemoteCallbacks};

use crate::Error;

/// The order methods are offered in. `USERNAME` may come after the key: libgit2
/// asks for it alone first when the URL carries none.
const METHODS: [CredentialType; 4] = [
    CredentialType::SSH_KEY,
    CredentialType::USER_PASS_PLAINTEXT,
    CredentialType::USERNAME,
    CredentialType::DEFAULT,
];

/// `config` comes from the caller, since the default config would leave out
/// the repository's own `.git/config` and any helper set there.
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

/// SSH by way of an agent, HTTPS by way of git's credential helper. A notebook
/// passes its own repository's `config`.
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

/// Only SSH gets its own error class; an empty HTTPS credential lookup, the
/// commonest failure, is a generic error recognisable only by its wording.
fn is_authentication(error: &git2::Error) -> bool {
    if error.class() == git2::ErrorClass::Ssh || error.code() == git2::ErrorCode::Auth {
        return true;
    }
    let message = error.message().to_ascii_lowercase();
    ["authentication", "username/password", "credential"]
        .iter()
        .any(|needle| message.contains(needle))
}

/// Adds a hint to an authentication failure; anything else passes through.
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
/// A token in the URL is the usual setup where no credential helper can run
/// (the container image has no shell); `.git/config` keeps it as configured.
///
/// The whole userinfo goes, since Gitea and Forgejo take the token as the
/// *username*.
pub fn redact(url: &str) -> Cow<'_, str> {
    // scp syntax: `git@` hides nothing.
    let Some(mark) = url.find("://") else {
        return Cow::Borrowed(url);
    };
    let (scheme, rest) = url.split_at(mark);
    let rest = &rest["://".len()..];
    let authority = &rest[..rest.find(['/', '?', '#']).unwrap_or(rest.len())];
    // The last `@` within the authority: a password may hold one, and a path
    // may too.
    let Some(at) = authority.rfind('@') else {
        return Cow::Borrowed(url);
    };
    // Over SSH a bare username is no secret; over HTTP(S) it may be a token.
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

    /// Scoped to a URL so the machine's own configuration cannot answer.
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

    #[test]
    fn credentials_come_out_of_a_url_before_anyone_reads_it() {
        assert_eq!(
            redact("https://x-access-token:ghp_secret@github.com/me/notes.git"),
            "https://***@github.com/me/notes.git"
        );
        // Gitea and Forgejo take the token as the username.
        assert_eq!(
            redact("https://tok_secret@codeberg.org/me/notes.git"),
            "https://***@codeberg.org/me/notes.git"
        );
        assert_eq!(
            redact("https://me:p@ssw0rd@git.example.com/notes.git"),
            "https://***@git.example.com/notes.git"
        );

        for url in [
            "https://github.com/me/notes.git",
            "git@github.com:me/notes.git",
            "ssh://git@github.com/me/notes.git",
            "https://git.example.com/me/a@b.git",
            "/srv/backups/notes.git",
        ] {
            assert_eq!(redact(url), url);
        }

        // A password goes whatever the scheme.
        assert_eq!(
            redact("ssh://me:pw@git.example.com/notes.git"),
            "ssh://***@git.example.com/notes.git"
        );
    }

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

        // noda's own callback error.
        let https = explain(
            git2::Error::from_str("no usable credentials"),
            "https://example.com/notes.git",
        )
        .to_string();
        assert!(https.contains("credential helper"), "{https}");
        // A helper only `git` can see looks like none, so name the files read.
        assert!(https.contains("/etc/gitconfig"), "{https}");
    }

    #[test]
    fn unrelated_failures_keep_libgit2s_own_message() {
        let error = git2::Error::from_str("the remote hung up unexpectedly");
        assert!(!is_authentication(&error));

        let explained = explain(error, "https://example.com/notes.git").to_string();
        assert!(!explained.contains("credential helper"), "{explained}");
        assert!(explained.contains("hung up"), "{explained}");
    }
}
