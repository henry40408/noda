//! Credentials for the network commands.
//!
//! The binary carries its own libgit2, libssh2 and OpenSSL, so it cannot lean on
//! the system `git` to authenticate — it has to find credentials itself. libgit2
//! calls back repeatedly until one succeeds, so every method is offered at most
//! once and the callback then gives up rather than looping.

use git2::{Cred, CredentialType, FetchOptions, PushOptions, RemoteCallbacks};

use crate::Error;

/// Credential callbacks covering the two transports noda ships: SSH by way of an
/// agent, and HTTPS by way of git's credential helper.
pub fn callbacks<'a>() -> RemoteCallbacks<'a> {
    let mut offered = CredentialType::empty();
    let mut callbacks = RemoteCallbacks::new();
    callbacks.credentials(move |url, username, allowed| {
        let mut take = |kind: CredentialType| {
            if allowed.contains(kind) && !offered.contains(kind) {
                offered |= kind;
                true
            } else {
                false
            }
        };

        if take(CredentialType::SSH_KEY) {
            return Cred::ssh_key_from_agent(username.unwrap_or("git"));
        }
        if take(CredentialType::USER_PASS_PLAINTEXT) {
            let config = git2::Config::open_default()?;
            return Cred::credential_helper(&config, url, username);
        }
        // Asked before the key itself when the URL carries no username.
        if take(CredentialType::USERNAME) {
            return Cred::username(username.unwrap_or("git"));
        }
        if take(CredentialType::DEFAULT) {
            return Cred::default();
        }
        Err(git2::Error::from_str("no usable credentials"))
    });
    callbacks
}

pub fn fetch_options<'a>() -> FetchOptions<'a> {
    let mut options = FetchOptions::new();
    options.remote_callbacks(callbacks());
    options
}

pub fn push_options<'a>() -> PushOptions<'a> {
    let mut options = PushOptions::new();
    options.remote_callbacks(callbacks());
    options
}

/// Turns libgit2's authentication failures into advice. Everything else is
/// passed through untouched — libgit2's own message is usually the better one.
pub fn explain(error: git2::Error, url: &str) -> Error {
    let authentication = error.class() == git2::ErrorClass::Ssh
        || error.code() == git2::ErrorCode::Auth
        || error.message().contains("authentication");
    if !authentication {
        return Error::Git(error);
    }
    let hint = if url.starts_with("http") {
        "noda reads HTTPS credentials from git's credential helper — check `git config credential.helper`, \
         or store a token with `git credential approve`"
    } else {
        "noda reads SSH keys from ssh-agent — check `ssh-add -l` and add your key with `ssh-add`"
    };
    Error::msg(format!("{}: {}\n{hint}", url, error.message()))
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
    use super::*;

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
}
