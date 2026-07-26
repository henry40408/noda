//! `config.toml`: the three settings noda keeps, and where each value comes from.
//!
//! The file is edited by hand at least as often as by `noda config`, so it is
//! parsed by a real TOML parser and written back through the same document —
//! comments and layout survive a `noda config editor nvim`.

use std::path::PathBuf;

use toml_edit::{DocumentMut, value};

use crate::paths::Paths;
use crate::{Error, Result};

/// Everything that may be set. A typo silently doing nothing is worse than an
/// error, so anything else is refused by name.
pub const KEYS: [&str; 3] = ["editor", "author", "notebook"];

/// The notebook `noda init` creates when config says nothing.
pub const DEFAULT_NOTEBOOK: &str = "default";

/// Written by `noda init` when there is no config yet. Everything is commented
/// out, so the defaults still apply — it exists to show what can be set.
const TEMPLATE: &str = "\
# noda configuration. Every setting is optional; the defaults are shown.

# Editor for `noda add` and `noda edit`.
# Overrides $VISUAL and $EDITOR, the way git's core.editor does.
# editor = \"nvim\"

# Who commits. Falls back to your git config, then to noda <noda@localhost>.
# author = \"Your Name <you@example.com>\"

# The notebook to fall back to when no notebook is active, and the one
# `noda init` creates.
# notebook = \"default\"
";

pub struct Config {
    path: PathBuf,
    document: DocumentMut,
}

impl Config {
    /// Reads the config, or an empty one when the file is not there — an absent
    /// config is a valid config, not an error.
    pub fn load(paths: &Paths) -> Result<Self> {
        let path = paths.config_dir().join("config.toml");
        let document = match std::fs::read_to_string(&path) {
            Ok(text) => text.parse::<DocumentMut>().map_err(|e| {
                Error::msg(format!(
                    "{}: {e}\nfix it by hand, or start over with `noda config --edit`",
                    path.display()
                ))
            })?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => DocumentMut::new(),
            Err(e) => return Err(e.into()),
        };
        Ok(Config { path, document })
    }

    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    /// A configured value, or `None` when it is unset or not a string.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.document.get(key).and_then(|item| item.as_str())
    }

    pub fn set(&mut self, key: &str, text: &str) -> Result<()> {
        validate_key(key)?;
        if key == "author" && author_parts(text).is_none() {
            return Err(Error::msg(format!(
                "author must be `Name <email>`, not `{text}`"
            )));
        }
        let is_new = self.document.get(key).is_none();
        self.document[key] = value(text);
        if is_new {
            self.keep_the_header_on_top(key);
        }
        self.save()
    }

    /// A file of nothing but comments — the starter config — holds all of its
    /// text as the document's trailer, and a new key is written before that.
    /// The result reads back to front, so the first time a key is added the
    /// header is moved to sit in front of it instead.
    fn keep_the_header_on_top(&mut self, key: &str) {
        let existing_keys = self
            .document
            .iter()
            .filter(|(name, _)| *name != key)
            .count();
        if existing_keys > 0 {
            return;
        }
        let Some(header) = self.document.trailing().as_str().map(str::to_string) else {
            return;
        };
        if header.trim().is_empty() {
            return;
        }
        self.document.set_trailing("");
        if let Some(mut key) = self.document.key_mut(key) {
            // A blank line, so the first setting is not glued to the comments.
            key.leaf_decor_mut()
                .set_prefix(format!("{}\n", header.trim_end_matches('\n')));
        }
    }

    pub fn unset(&mut self, key: &str) -> Result<bool> {
        validate_key(key)?;
        let removed = self.document.remove(key).is_some();
        if removed {
            self.save()?;
        }
        Ok(removed)
    }

    /// Writes a starter config when there is none. Returns whether it wrote one.
    pub fn write_template(paths: &Paths) -> Result<bool> {
        let path = paths.config_dir().join("config.toml");
        if path.exists() {
            return Ok(false);
        }
        std::fs::create_dir_all(paths.config_dir())?;
        std::fs::write(&path, TEMPLATE)?;
        Ok(true)
    }

    fn save(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&self.path, self.document.to_string())?;
        Ok(())
    }
}

pub fn validate_key(key: &str) -> Result<()> {
    if KEYS.contains(&key) {
        return Ok(());
    }
    Err(Error::msg(format!(
        "unknown setting: {key} — noda knows {}",
        KEYS.join(", ")
    )))
}

/// Where a value came from, so `noda config` can answer "why is it using vi?".
#[derive(Debug, PartialEq, Eq)]
pub enum Source {
    File,
    Environment,
    Git,
    Default,
}

impl Source {
    pub fn label(&self) -> &'static str {
        match self {
            Source::File => "config.toml",
            Source::Environment => "environment",
            Source::Git => "git config",
            Source::Default => "built-in",
        }
    }
}

/// The editor to run, and where the choice came from.
///
/// Config wins over the environment, as git's `core.editor` does: `$EDITOR` is a
/// blanket default for every program you use, while the config file is a
/// decision made about this one.
pub fn editor(
    configured: Option<&str>,
    visual: Option<String>,
    editor: Option<String>,
) -> (String, Source) {
    if let Some(configured) = configured.map(str::trim).filter(|e| !e.is_empty()) {
        return (configured.to_string(), Source::File);
    }
    for from_env in [visual, editor] {
        if let Some(value) = from_env
            .map(|e| e.trim().to_string())
            .filter(|e| !e.is_empty())
        {
            return (value, Source::Environment);
        }
    }
    ("vi".to_string(), Source::Default)
}

/// Splits `Name <email>`. Both halves must be there and non-empty; anything else
/// is refused rather than quietly committed under half an identity.
pub fn author_parts(text: &str) -> Option<(String, String)> {
    let (name, rest) = text.trim().split_once('<')?;
    let email = rest.strip_suffix('>')?.trim();
    let name = name.trim();
    if name.is_empty() || email.is_empty() || email.contains('<') {
        return None;
    }
    Some((name.to_string(), email.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_config_file_outranks_the_environment() {
        let (chosen, source) = editor(Some("nvim"), Some("code -w".into()), Some("vi".into()));
        assert_eq!((chosen.as_str(), source), ("nvim", Source::File));
    }

    #[test]
    fn visual_outranks_editor_and_both_outrank_the_fallback() {
        let (chosen, source) = editor(None, Some("code -w".into()), Some("vi".into()));
        assert_eq!((chosen.as_str(), source), ("code -w", Source::Environment));

        let (chosen, source) = editor(None, None, Some("emacs".into()));
        assert_eq!((chosen.as_str(), source), ("emacs", Source::Environment));

        let (chosen, source) = editor(None, None, None);
        assert_eq!((chosen.as_str(), source), ("vi", Source::Default));
    }

    #[test]
    fn a_setting_that_is_present_but_blank_does_not_count() {
        // An empty $EDITOR is the shell's leftover, not a choice.
        let (chosen, source) = editor(Some("  "), Some("   ".into()), None);
        assert_eq!((chosen.as_str(), source), ("vi", Source::Default));
    }

    #[test]
    fn an_author_needs_both_halves() {
        assert_eq!(
            author_parts("Heng-Yi Wu <me@henry40408.com>"),
            Some(("Heng-Yi Wu".into(), "me@henry40408.com".into()))
        );
        assert_eq!(
            author_parts("  Spaced  < e@x >  "),
            Some(("Spaced".into(), "e@x".into()))
        );
        assert_eq!(author_parts("no email"), None);
        assert_eq!(author_parts("<only@email>"), None);
        assert_eq!(author_parts("Name <unclosed"), None);
    }

    #[test]
    fn unknown_settings_are_named_rather_than_ignored() {
        let err = validate_key("edito").unwrap_err().to_string();
        assert!(err.contains("editor, author, notebook"), "{err}");
        assert!(KEYS.iter().all(|key| validate_key(key).is_ok()));
    }
}
