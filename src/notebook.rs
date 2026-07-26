//! A notebook is a git repository of Markdown files. Every mutation is a commit.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use git2::{Repository, Signature};

use crate::note::{self, Note};
use crate::paths::Paths;
use crate::{Error, Result};

/// Directory holding noda's own bookkeeping inside a notebook.
const META_DIR: &str = ".noda";
/// Committed `id\tslug` lookup. Rebuildable from the notes' frontmatter.
const INDEX_FILE: &str = ".noda/index.tsv";

pub struct Notebook {
    pub name: String,
    pub path: PathBuf,
    repo: Repository,
}

impl Notebook {
    /// Creates the notebook repo and commits an empty index.
    pub fn create(paths: &Paths, name: &str) -> Result<Self> {
        validate_name(name)?;
        let path = paths.notebook_dir(name);
        if path.exists() {
            return Err(Error::msg(format!("notebook already exists: {name}")));
        }
        std::fs::create_dir_all(&path)?;
        let repo = Repository::init(&path)?;
        let notebook = Notebook {
            name: name.to_string(),
            path,
            repo,
        };
        notebook.write_index(&[])?;
        notebook.commit(&[Path::new(INDEX_FILE)], "chore: initialize notebook")?;
        Ok(notebook)
    }

    pub fn open(paths: &Paths, name: &str) -> Result<Self> {
        validate_name(name)?;
        let path = paths.notebook_dir(name);
        if !path.join(".git").exists() {
            return Err(Error::msg(format!(
                "notebook not found: {name} — run `noda init` or `noda notebook add {name}`"
            )));
        }
        let repo = Repository::open(&path)?;
        Ok(Notebook {
            name: name.to_string(),
            path,
            repo,
        })
    }

    pub fn open_active(paths: &Paths) -> Result<Self> {
        Notebook::open(paths, &paths.active_notebook()?)
    }

    pub fn exists(paths: &Paths, name: &str) -> bool {
        paths.notebook_dir(name).join(".git").exists()
    }

    pub fn note_path(&self, slug: &str) -> PathBuf {
        self.path.join(format!("{slug}.md"))
    }

    /// Every `(id, slug)` pair, read from the committed index.
    pub fn index(&self) -> Result<Vec<(String, String)>> {
        let path = self.path.join(INDEX_FILE);
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };
        Ok(text
            .lines()
            .filter_map(|line| line.split_once('\t'))
            .map(|(id, slug)| (id.to_string(), slug.to_string()))
            .collect())
    }

    pub fn write_index(&self, entries: &[(String, String)]) -> Result<()> {
        let mut sorted = entries.to_vec();
        sorted.sort();
        let body: String = sorted
            .iter()
            .map(|(id, slug)| format!("{id}\t{slug}\n"))
            .collect();
        std::fs::create_dir_all(self.path.join(META_DIR))?;
        std::fs::write(self.path.join(INDEX_FILE), body)?;
        Ok(())
    }

    pub fn taken_ids(&self) -> Result<HashSet<String>> {
        Ok(self.index()?.into_iter().map(|(id, _)| id).collect())
    }

    /// Every note in the notebook, sorted by slug, read from the working tree.
    pub fn notes(&self) -> Result<Vec<(String, Note)>> {
        let mut notes = Vec::new();
        for entry in std::fs::read_dir(&self.path)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("md") || !path.is_file() {
                continue;
            }
            let Some(slug) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            let text = std::fs::read_to_string(&path)?;
            let note =
                Note::parse(&text).map_err(|e| Error::msg(format!("{}: {e}", path.display())))?;
            notes.push((slug.to_string(), note));
        }
        notes.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(notes)
    }

    /// Resolves an id or a slug to a note file. Both are matched exactly; an
    /// unknown key is an error rather than a near miss.
    pub fn resolve(&self, key: &str) -> Result<PathBuf> {
        if key.is_empty() || key.contains('/') || key.contains('\\') || key.contains("..") {
            return Err(Error::msg(format!("invalid note reference: {key}")));
        }
        let by_slug = self.note_path(key);
        if by_slug.is_file() {
            return Ok(by_slug);
        }
        let wanted = note::normalize_id(key);
        for (id, slug) in self.index()? {
            if note::normalize_id(&id) == wanted {
                let path = self.note_path(&slug);
                if path.is_file() {
                    return Ok(path);
                }
            }
        }
        Err(Error::msg(format!("note not found: {key}")))
    }

    /// Stages `files` (paths relative to the notebook root) and commits them.
    pub fn commit(&self, files: &[&Path], message: &str) -> Result<()> {
        let mut index = self.repo.index()?;
        for file in files {
            index.add_path(file)?;
        }
        index.write()?;
        let tree = self.repo.find_tree(index.write_tree()?)?;
        let signature = self.signature()?;
        let parent = match self.repo.head() {
            Ok(head) => Some(head.peel_to_commit()?),
            Err(_) => None,
        };
        let parents: Vec<&git2::Commit> = parent.iter().collect();
        self.repo.commit(
            Some("HEAD"),
            &signature,
            &signature,
            message,
            &tree,
            &parents,
        )?;
        Ok(())
    }

    /// The user's git identity, or a neutral one when git is unconfigured.
    fn signature(&self) -> Result<Signature<'static>> {
        match self.repo.signature() {
            Ok(sig) => Ok(sig),
            Err(_) => Ok(Signature::now("noda", "noda@localhost")?),
        }
    }
}

fn validate_name(name: &str) -> Result<()> {
    if name.is_empty() || name.contains('/') || name.contains('\\') || name.contains("..") {
        return Err(Error::msg(format!("invalid notebook name: {name}")));
    }
    Ok(())
}
