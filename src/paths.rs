//! XDG base directory resolution.
//!
//! noda honors the XDG variables on every platform, including macOS. A variable is
//! only honored when it holds an absolute path, per the spec; otherwise the default
//! under `$HOME` applies.

use std::path::{Path, PathBuf};

use crate::{Error, Result};

/// The four XDG roots, each already suffixed with `noda/`.
#[derive(Debug, Clone)]
pub struct Paths {
    config: PathBuf,
    data: PathBuf,
    state: PathBuf,
    cache: PathBuf,
}

impl Paths {
    pub fn from_env() -> Result<Self> {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .filter(|h| !h.as_os_str().is_empty())
            .ok_or_else(|| Error::msg("HOME is not set"))?;
        Ok(Self {
            config: xdg("XDG_CONFIG_HOME", &home, ".config"),
            data: xdg("XDG_DATA_HOME", &home, ".local/share"),
            state: xdg("XDG_STATE_HOME", &home, ".local/state"),
            cache: xdg("XDG_CACHE_HOME", &home, ".cache"),
        })
    }

    /// All four roots under one directory. Used by tests, which cannot safely
    /// mutate process-wide environment variables in parallel.
    pub fn rooted(root: impl AsRef<Path>) -> Self {
        let root = root.as_ref();
        Self {
            config: root.join("config/noda"),
            data: root.join("data/noda"),
            state: root.join("state/noda"),
            cache: root.join("cache/noda"),
        }
    }

    pub fn config_dir(&self) -> &Path {
        &self.config
    }

    pub fn cache_dir(&self) -> &Path {
        &self.cache
    }

    pub fn notebooks_dir(&self) -> PathBuf {
        self.data.join("notebooks")
    }

    pub fn notebook_dir(&self, name: &str) -> PathBuf {
        self.notebooks_dir().join(name)
    }

    /// Pointer to the active notebook. Deliberately in state, not in the synced data.
    pub fn active_file(&self) -> PathBuf {
        self.state.join("active")
    }

    /// Create every directory noda writes to.
    pub fn create_dirs(&self) -> Result<()> {
        for dir in [&self.config, &self.state, &self.cache] {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::create_dir_all(self.notebooks_dir())?;
        Ok(())
    }

    pub fn active_notebook(&self) -> Result<String> {
        let file = self.active_file();
        let name = std::fs::read_to_string(&file)
            .map_err(|_| Error::msg("no active notebook — run `noda init` first"))?;
        let name = name.trim().to_string();
        if name.is_empty() {
            return Err(Error::msg("no active notebook — run `noda init` first"));
        }
        Ok(name)
    }

    pub fn set_active_notebook(&self, name: &str) -> Result<()> {
        std::fs::create_dir_all(&self.state)?;
        std::fs::write(self.active_file(), format!("{name}\n"))?;
        Ok(())
    }
}

fn xdg(var: &str, home: &Path, default: &str) -> PathBuf {
    match std::env::var_os(var) {
        Some(value) if Path::new(&value).is_absolute() => PathBuf::from(value).join("noda"),
        _ => home.join(default).join("noda"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_xdg_value_falls_back_to_home_default() {
        // The spec says a relative XDG_* value must be ignored.
        let home = Path::new("/home/someone");
        assert_eq!(
            xdg("NODA_TEST_UNSET_VAR", home, ".config"),
            Path::new("/home/someone/.config/noda")
        );
    }

    #[test]
    fn rooted_keeps_the_four_roles_apart() {
        let p = Paths::rooted("/tmp/x");
        assert_eq!(p.notebooks_dir(), Path::new("/tmp/x/data/noda/notebooks"));
        assert_eq!(p.active_file(), Path::new("/tmp/x/state/noda/active"));
        assert_eq!(p.config_dir(), Path::new("/tmp/x/config/noda"));
        assert_eq!(p.cache_dir(), Path::new("/tmp/x/cache/noda"));
    }
}
