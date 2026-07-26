//! A notebook is a git repository of Markdown files. Every mutation is a commit.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use git2::{Repository, Signature};

use crate::note::{self, Note};
use crate::paths::Paths;
use crate::remote;
use crate::{Error, Result};

/// Directory holding noda's own bookkeeping inside a notebook.
const META_DIR: &str = ".noda";
/// Committed `id\tslug` lookup. Rebuildable from the notes' frontmatter.
const INDEX_FILE: &str = ".noda/index.tsv";
/// noda configures exactly one remote per notebook.
const REMOTE_NAME: &str = "origin";

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
        validate_name(name).is_ok() && paths.notebook_dir(name).join(".git").exists()
    }

    /// Every notebook under the data dir, sorted. A directory that is not a git
    /// repo is not a notebook and is skipped rather than reported.
    pub fn list(paths: &Paths) -> Result<Vec<String>> {
        let dir = paths.notebooks_dir();
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };
        let mut names = Vec::new();
        for entry in entries {
            let path = entry?.path();
            if !path.join(".git").exists() {
                continue;
            }
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                names.push(name.to_string());
            }
        }
        names.sort();
        Ok(names)
    }

    /// Points the notebook at `url`, replacing any remote already configured.
    pub fn set_remote(&self, url: &str) -> Result<()> {
        if self.repo.find_remote(REMOTE_NAME).is_ok() {
            self.repo.remote_set_url(REMOTE_NAME, url)?;
        } else {
            self.repo.remote(REMOTE_NAME, url)?;
        }
        Ok(())
    }

    /// The configured remote, if there is one whose URL is valid UTF-8.
    pub fn remote_url(&self) -> Option<String> {
        let remote = self.repo.find_remote(REMOTE_NAME).ok()?;
        remote.url().ok().map(str::to_string)
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
    /// A path that no longer exists is staged as a deletion, so a rename is one
    /// commit rather than an add followed by a stray leftover.
    pub fn commit(&self, files: &[&Path], message: &str) -> Result<()> {
        let mut index = self.repo.index()?;
        for file in files {
            if self.path.join(file).exists() {
                index.add_path(file)?;
            } else {
                index.remove_path(file)?;
            }
        }
        self.commit_index(&mut index, message)
    }

    /// Commits everything in the working tree. Used by `noda sync`, which has to
    /// deal with notes edited outside noda. Returns `false` when there was
    /// nothing to commit.
    pub fn commit_all(&self, message: &str) -> Result<bool> {
        if !self.is_dirty()? {
            return Ok(false);
        }
        let mut index = self.repo.index()?;
        index.add_all(["*"], git2::IndexAddOption::DEFAULT, None)?;
        self.commit_index(&mut index, message)?;
        Ok(true)
    }

    /// Whether the working tree differs from `HEAD`, untracked files included.
    pub fn is_dirty(&self) -> Result<bool> {
        let mut options = git2::StatusOptions::new();
        options.include_untracked(true).include_ignored(false);
        Ok(!self.repo.statuses(Some(&mut options))?.is_empty())
    }

    fn commit_index(&self, index: &mut git2::Index, message: &str) -> Result<()> {
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

    /// Clones a remote notebook into `name`. A clone that fails partway leaves
    /// no half-written directory behind for the next attempt to trip over.
    pub fn clone(paths: &Paths, url: &str, name: &str) -> Result<Self> {
        validate_name(name)?;
        let path = paths.notebook_dir(name);
        if path.exists() {
            return Err(Error::msg(format!("notebook already exists: {name}")));
        }
        std::fs::create_dir_all(paths.notebooks_dir())?;

        let mut builder = git2::build::RepoBuilder::new();
        builder.fetch_options(remote::fetch_options());
        let repo = builder.clone(url, &path).map_err(|e| {
            let _ = std::fs::remove_dir_all(&path);
            remote::explain(e, url)
        })?;

        let notebook = Notebook {
            name: name.to_string(),
            path,
            repo,
        };
        if let Err(e) = notebook.adopt_remote_branch() {
            let path = notebook.path.clone();
            drop(notebook);
            let _ = std::fs::remove_dir_all(&path);
            return Err(e);
        }
        Ok(notebook)
    }

    /// A clone whose `HEAD` names a branch the remote does not actually carry
    /// checks out nothing: the notebook would read as empty rather than as
    /// broken. Two machines disagreeing about `init.defaultBranch` is enough to
    /// cause it. When the remote has exactly one branch, take it; otherwise say
    /// what is there rather than hand back an unusable notebook.
    fn adopt_remote_branch(&self) -> Result<()> {
        if self.repo.head().is_ok() {
            return Ok(());
        }
        let prefix = format!("refs/remotes/{REMOTE_NAME}/");
        let mut branches = Vec::new();
        for reference in self.repo.references()? {
            let reference = reference?;
            let (Ok(name), Some(oid)) = (reference.name(), reference.target()) else {
                continue;
            };
            match name.strip_prefix(&prefix) {
                Some("HEAD") | None => continue,
                Some(branch) => branches.push((branch.to_string(), oid)),
            }
        }

        match branches.as_slice() {
            [(branch, oid)] => {
                let refname = format!("refs/heads/{branch}");
                self.repo.reference(
                    &refname,
                    *oid,
                    true,
                    "noda clone: adopt the remote's branch",
                )?;
                self.repo.set_head(&refname)?;
                self.repo
                    .checkout_head(Some(git2::build::CheckoutBuilder::new().force()))?;
                Ok(())
            }
            [] => Err(Error::msg("the remote has no commits yet")),
            many => Err(Error::msg(format!(
                "the remote's default branch is missing, and it has more than one to choose from: {}",
                many.iter()
                    .map(|(branch, _)| branch.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ))),
        }
    }

    /// The branch `HEAD` points at.
    pub fn branch(&self) -> Result<String> {
        let head = self.repo.head()?;
        Ok(head.shorthand()?.to_string())
    }

    fn remote(&self) -> Result<git2::Remote<'_>> {
        self.repo.find_remote(REMOTE_NAME).map_err(|_| {
            Error::msg(format!(
                "notebook `{}` has no remote — set one with `noda remote set <url>`",
                self.name
            ))
        })
    }

    /// Fetches the current branch and returns the commit it now points at, or
    /// `None` when the remote does not carry that branch yet — pushing to an
    /// empty repository is a normal first sync, not a failure.
    fn fetch(&self) -> Result<Option<git2::Oid>> {
        let branch = self.branch()?;
        let mut remote = self.remote()?;
        let url = remote.url().unwrap_or_default().to_string();
        let refspec = format!("+refs/heads/{branch}:refs/remotes/{REMOTE_NAME}/{branch}");

        match remote.fetch(&[&refspec], Some(&mut remote::fetch_options()), None) {
            Ok(()) => {}
            Err(e) if e.code() == git2::ErrorCode::NotFound => return Ok(None),
            Err(e) => return Err(remote::explain(e, &url)),
        }

        let tracking = format!("refs/remotes/{REMOTE_NAME}/{branch}");
        match self.repo.refname_to_id(&tracking) {
            Ok(oid) => Ok(Some(oid)),
            Err(e) if e.code() == git2::ErrorCode::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Fetches and integrates the remote branch: fast-forward where possible, a
    /// merge commit where the histories diverged. A merge that conflicts is
    /// rolled back rather than left half-applied — noda has no `--continue`.
    pub fn pull(&self) -> Result<String> {
        if self.is_dirty()? {
            return Err(Error::msg(format!(
                "notebook `{}` has uncommitted changes — commit them, or use `noda sync`, \
                 which commits before pulling",
                self.name
            )));
        }
        let branch = self.branch()?;
        let Some(incoming) = self.fetch()? else {
            return Ok(format!("pull: the remote has no `{branch}` branch yet"));
        };

        let annotated = self.repo.find_annotated_commit(incoming)?;
        let (analysis, _) = self.repo.merge_analysis(&[&annotated])?;

        if analysis.is_up_to_date() {
            return Ok("pull: already up to date".to_string());
        }

        if analysis.is_fast_forward() {
            let refname = format!("refs/heads/{branch}");
            self.repo
                .find_reference(&refname)?
                .set_target(incoming, "noda pull: fast-forward")?;
            self.repo.set_head(&refname)?;
            self.repo
                .checkout_head(Some(git2::build::CheckoutBuilder::new().force()))?;
            return Ok(format!("pull: fast-forwarded to {}", short(incoming)));
        }

        self.repo.merge(&[&annotated], None, None)?;
        let mut index = self.repo.index()?;
        if index.has_conflicts() {
            let conflicted: Vec<String> = index
                .conflicts()?
                .filter_map(|c| c.ok())
                .filter_map(|c| c.our.or(c.their).or(c.ancestor))
                .map(|entry| String::from_utf8_lossy(&entry.path).into_owned())
                .collect();

            // Two notebooks that each added a note both appended to the id ↔ slug
            // index, which conflicts nearly every time. It is derived data, so
            // noda rebuilds it from the merged notes instead of asking anyone to
            // resolve a machine-written file. A conflict in a note is different:
            // only its author can settle it.
            let notes: Vec<&String> = conflicted.iter().filter(|p| *p != INDEX_FILE).collect();
            if !notes.is_empty() {
                self.abort_merge()?;
                return Err(Error::msg(format!(
                    "pull: `{branch}` conflicts with the remote in {} — the merge was rolled back; \
                     resolve it with git in {}",
                    notes
                        .iter()
                        .map(|p| p.as_str())
                        .collect::<Vec<_>>()
                        .join(", "),
                    self.path.display()
                )));
            }

            index.conflict_remove(Path::new(INDEX_FILE))?;
            let rebuilt: Vec<(String, String)> = self
                .notes()?
                .into_iter()
                .map(|(slug, note)| (note.id, slug))
                .collect();
            self.write_index(&rebuilt)?;
            index.add_path(Path::new(INDEX_FILE))?;
        }

        // Persist the resolved index before turning it into a tree, or the
        // commit is right while `git status` still reports the merge as pending.
        index.write()?;
        let tree = self.repo.find_tree(index.write_tree()?)?;
        let signature = self.signature()?;
        let ours = self.repo.head()?.peel_to_commit()?;
        let theirs = self.repo.find_commit(incoming)?;
        self.repo.commit(
            Some("HEAD"),
            &signature,
            &signature,
            &format!("merge: {REMOTE_NAME}/{branch}"),
            &tree,
            &[&ours, &theirs],
        )?;
        self.repo.cleanup_state()?;
        Ok(format!("pull: merged {REMOTE_NAME}/{branch}"))
    }

    /// Pushes the current branch. A rejection is reported as advice to pull
    /// rather than as a libgit2 error, because that is always the next step.
    pub fn push(&self) -> Result<String> {
        let branch = self.branch()?;
        let mut remote = self.remote()?;
        let url = remote.url().unwrap_or_default().to_string();
        let refspec = format!("refs/heads/{branch}:refs/heads/{branch}");

        // The callbacks borrow `rejections`, so they have to be dropped before it
        // can be read back — hence the block.
        let rejections = std::cell::RefCell::new(Vec::new());
        let pushed = {
            let mut callbacks = remote::callbacks();
            callbacks.push_update_reference(|refname, status| {
                if let Some(reason) = status {
                    rejections.borrow_mut().push(format!("{refname}: {reason}"));
                }
                Ok(())
            });
            let mut options = git2::PushOptions::new();
            options.remote_callbacks(callbacks);
            remote.push(&[&refspec], Some(&mut options))
        };
        if let Err(e) = pushed {
            // libgit2 refuses a non-fast-forward before sending anything; a
            // server that refuses one reports it through the callback below.
            if e.code() == git2::ErrorCode::NotFastForward
                || e.message().contains("not present locally")
            {
                return Err(rejected(&[e.message().to_string()]));
            }
            return Err(remote::explain(e, &url));
        }

        let rejections = rejections.into_inner();
        if !rejections.is_empty() {
            return Err(rejected(&rejections));
        }
        Ok(format!("push: {branch} -> {url}"))
    }

    /// Undoes a conflicted merge, leaving the notebook exactly as it was.
    fn abort_merge(&self) -> Result<()> {
        let head = self.repo.head()?.peel_to_commit()?;
        self.repo
            .reset(head.as_object(), git2::ResetType::Hard, None)?;
        self.repo.cleanup_state()?;
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

/// A refused push, phrased as the next thing to do about it.
fn rejected(reasons: &[String]) -> Error {
    Error::msg(format!(
        "push rejected — {}\nthe remote has commits you do not: run `noda pull` first",
        reasons.join("; ")
    ))
}

/// An abbreviated object id, as git prints it.
fn short(oid: git2::Oid) -> String {
    oid.to_string()[..7].to_string()
}

/// Notebook names become directory names, so they must not escape the data dir.
pub fn validate_name(name: &str) -> Result<()> {
    if name.is_empty() || name.contains('/') || name.contains('\\') || name.contains("..") {
        return Err(Error::msg(format!("invalid notebook name: {name}")));
    }
    Ok(())
}
