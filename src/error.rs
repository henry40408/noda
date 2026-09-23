//! One error type for the crate — I/O, libgit2 or a human-readable message —
//! without pulling in an error library.

use std::fmt;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Git(git2::Error),
    /// A message written for the person at the terminal.
    Msg(String),
}

impl Error {
    pub fn msg(text: impl Into<String>) -> Self {
        Error::Msg(text.into())
    }

    /// Rust ignores `SIGPIPE`, so `noda log | head` surfaces as a write error;
    /// nothing is wrong and nobody is left to tell.
    pub fn is_broken_pipe(&self) -> bool {
        matches!(self, Error::Io(e) if e.kind() == std::io::ErrorKind::BrokenPipe)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "{e}"),
            Error::Git(e) => write!(f, "{}", e.message()),
            Error::Msg(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(e) => Some(e),
            Error::Git(e) => Some(e),
            Error::Msg(_) => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

impl From<git2::Error> for Error {
    fn from(e: git2::Error) -> Self {
        Error::Git(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_closed_reader_is_not_a_failure_worth_reporting() {
        let closed = Error::Io(std::io::Error::from(std::io::ErrorKind::BrokenPipe));
        assert!(closed.is_broken_pipe());

        let denied = Error::Io(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
        assert!(!denied.is_broken_pipe());
        assert!(!Error::msg("no").is_broken_pipe());
    }
}
