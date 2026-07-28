//! A notebook is a git repository of Markdown files. Every mutation is a commit.
//!
//! A note's identity is its filename: `<id>-<slug>.md`. Nothing derived is
//! committed alongside the notes, so there is no bookkeeping file to conflict on
//! and nothing that can fall out of step with what the files say. Two machines
//! that each add a note write two different filenames, and git merges them
//! without anyone being asked to resolve anything.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use git2::{Repository, RepositoryInitOptions, Signature};

use crate::config::{self, Config};
use crate::note::{self, Note};
use crate::paths::Paths;
use crate::remote;
use crate::{Error, Result};

/// noda configures exactly one remote per notebook.
const REMOTE_NAME: &str = "origin";

pub struct Notebook {
    pub name: String,
    pub path: PathBuf,
    repo: Repository,
    /// Who to commit as, when `config.toml` says. Resolved on open, because a
    /// notebook is opened once per command and read many times.
    author: Option<(String, String)>,
}

/// A note as it sits in the working tree: the identity its filename spells out,
/// and what the file holds.
pub struct NoteFile {
    pub id: String,
    pub slug: String,
    pub note: Note,
}

/// Where a notebook stands, as `noda status` reports it.
pub struct Status {
    pub branch: String,
    pub notes: usize,
    /// Files differing from `HEAD`, untracked ones included.
    pub uncommitted: usize,
    pub remote: Option<String>,
    /// `(ahead, behind)` against the remote-tracking ref, or `None` when there
    /// is nothing to compare against because the notebook has never fetched.
    pub drift: Option<(usize, usize)>,
    /// What the walk of the working tree turned up that wants attention. Empty
    /// is the healthy state, and the ordinary one.
    pub problems: Vec<(Problem, Vec<String>)>,
}

/// Something in the notebook that noda will not settle on its own.
///
/// Reported by kind rather than one occurrence at a time, because the commonest
/// way this goes wrong is wholesale — a directory of files copied in at once. One
/// line saying how many is worth more than two thousand naming them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Problem {
    /// One id carried by more than one file. Two machines can mint the same id
    /// without ever meeting; the filenames differ, so git merges them happily
    /// and the collision only shows up here.
    SharedId,
    /// A `*.md` holding frontmatter but with no id in its name — a note waiting
    /// to be adopted, which is what a file written by hand looks like.
    Unnamed,
    /// A filename that claims an id over a file with no frontmatter. Either a
    /// note that lost its frontmatter or a file that was never one, and only its
    /// author knows which.
    Suspicious,
}

impl Problem {
    /// How to say that there are `count` of this kind.
    pub fn describe(self, count: usize) -> String {
        match (self, count == 1) {
            (Problem::SharedId, true) => "1 id is carried by more than one note".to_string(),
            (Problem::SharedId, false) => {
                format!("{count} ids are carried by more than one note")
            }
            (Problem::Unnamed, true) => "1 note has no id in its filename".to_string(),
            (Problem::Unnamed, false) => format!("{count} notes have no id in their filenames"),
            (Problem::Suspicious, true) => {
                "1 file is named like a note but has no frontmatter".to_string()
            }
            (Problem::Suspicious, false) => {
                format!("{count} files are named like notes but have no frontmatter")
            }
        }
    }
}

/// What a walk of the working tree found, sorted into the four cases a filename
/// and a frontmatter block can produce between them.
///
/// The frontmatter says "I am a note"; the id prefix says "I have been adopted".
/// A file with neither is just a file — an attachment, a README, anything — and
/// noda leaves it alone rather than reporting it forever.
pub struct Scan {
    /// Adopted notes, as `(id, slug)`.
    pub notes: Vec<(String, String)>,
    /// Frontmatter but no id: adoptable.
    pub unnamed: Vec<String>,
    /// An id but no frontmatter: ambiguous.
    pub suspicious: Vec<String>,
}

impl Scan {
    /// Everything worth reporting, gathered by kind.
    pub fn problems(&self) -> Vec<(Problem, Vec<String>)> {
        let mut found: BTreeMap<Problem, Vec<String>> = BTreeMap::new();

        let mut by_id: BTreeMap<String, usize> = BTreeMap::new();
        for (id, _) in &self.notes {
            *by_id.entry(note::normalize_id(id)).or_default() += 1;
        }
        for (id, _) in by_id.iter().filter(|(_, count)| **count > 1) {
            found.entry(Problem::SharedId).or_default().push(id.clone());
        }

        if !self.unnamed.is_empty() {
            found
                .entry(Problem::Unnamed)
                .or_default()
                .extend(self.unnamed.iter().cloned());
        }
        if !self.suspicious.is_empty() {
            found
                .entry(Problem::Suspicious)
                .or_default()
                .extend(self.suspicious.iter().cloned());
        }

        found
            .into_iter()
            .map(|(kind, mut subjects)| {
                subjects.sort();
                subjects.dedup();
                (kind, subjects)
            })
            .collect()
    }
}

/// One commit, as `noda log` reports it.
pub struct Entry {
    pub id: git2::Oid,
    /// Commit time, and the offset it was made at, so a commit prints in the
    /// timezone it was written in — the way git shows it.
    pub seconds: i64,
    pub offset_minutes: i32,
    pub summary: String,
}

impl Entry {
    pub fn short_id(&self) -> String {
        short(self.id)
    }
}

impl Notebook {
    /// Creates the notebook repo with an empty root commit.
    ///
    /// Empty because there is nothing to put in it: noda commits no bookkeeping
    /// of its own, so the first note is the first content. The commit itself is
    /// still needed — `HEAD` has to name something before a branch can be pushed
    /// or compared against a remote.
    pub fn create(paths: &Paths, name: &str) -> Result<Self> {
        validate_name(name)?;
        let path = paths.notebook_dir(name);
        if path.exists() {
            return Err(Error::msg(format!("notebook already exists: {name}")));
        }
        std::fs::create_dir_all(&path)?;
        let mut options = RepositoryInitOptions::new();
        if let Ok(config) = git2::Config::open_default() {
            options.initial_head(&initial_branch(&config));
        }
        let repo = Repository::init_opts(&path, &options)?;
        let notebook = Notebook {
            name: name.to_string(),
            path,
            repo,
            author: configured_author(paths),
        };
        notebook.commit(&[], "chore: initialize notebook")?;
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
            author: configured_author(paths),
        })
    }

    pub fn open_active(paths: &Paths) -> Result<Self> {
        Notebook::open(paths, &active_name(paths)?)
    }

    /// Where the notebook stands: what is uncommitted, and how far it has
    /// drifted from the remote.
    ///
    /// The drift is measured against the remote-tracking ref, so it is only as
    /// current as the last fetch. That is deliberate — a command you run to
    /// orient yourself should not go to the network and should not fail because
    /// you are on a train.
    pub fn status(&self) -> Result<Status> {
        let branch = self.branch()?;
        let scan = self.scan()?;

        let mut options = git2::StatusOptions::new();
        options.include_untracked(true).include_ignored(false);
        let uncommitted = self.repo.statuses(Some(&mut options))?.len();

        let tracking = format!("refs/remotes/{REMOTE_NAME}/{branch}");
        let drift = match (
            self.repo.head()?.target(),
            self.repo.refname_to_id(&tracking).ok(),
        ) {
            (Some(local), Some(upstream)) => Some(self.repo.graph_ahead_behind(local, upstream)?),
            _ => None,
        };

        Ok(Status {
            branch,
            notes: scan.notes.len(),
            uncommitted,
            remote: self.remote_url(),
            drift,
            problems: scan.problems(),
        })
    }

    /// Walks the working tree and sorts every `*.md` into the four cases.
    ///
    /// Tolerant where `notes` is strict: one malformed file must not stop the
    /// notebook being described.
    pub fn scan(&self) -> Result<Scan> {
        let mut notes = Vec::new();
        let mut unnamed = Vec::new();
        let mut suspicious = Vec::new();

        for entry in std::fs::read_dir(&self.path)? {
            let path = entry?.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("md") || !path.is_file() {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            let file = format!("{stem}.md");
            let declared = Note::parse(&std::fs::read_to_string(&path)?).is_ok();
            match (note::split_stem(stem), declared) {
                (Some((id, slug)), true) => notes.push((id.to_string(), slug.to_string())),
                (Some(_), false) => suspicious.push(file),
                (None, true) => unnamed.push(file),
                // Neither a name nor a declaration: not noda's business.
                (None, false) => {}
            }
        }

        notes.sort();
        unnamed.sort();
        suspicious.sort();
        Ok(Scan {
            notes,
            unnamed,
            suspicious,
        })
    }

    /// Every `(id, slug)` a filename in the notebook spells out, whether or not
    /// the file behind it can be read.
    ///
    /// The name is the whole record, so this opens nothing. It is deliberately
    /// more forgiving than `scan`: a note whose frontmatter has gone still says
    /// which note it is, and the commands that only need to know *which* — `rm`,
    /// `log`, `diff`, `restore` — must keep working on exactly the file someone
    /// is reaching for those commands to fix.
    fn named_files(&self) -> Result<Vec<(String, String)>> {
        let mut found = Vec::new();
        for entry in std::fs::read_dir(&self.path)? {
            let path = entry?.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("md") || !path.is_file() {
                continue;
            }
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str())
                && let Some((id, slug)) = note::split_stem(stem)
            {
                found.push((id.to_string(), slug.to_string()));
            }
        }
        found.sort();
        Ok(found)
    }

    /// Every id already spoken for, folded the way `resolve` folds them.
    ///
    /// Read from the filenames alone, so this costs one directory listing and
    /// opens nothing.
    pub fn taken_ids(&self) -> Result<HashSet<String>> {
        Ok(self
            .named_files()?
            .into_iter()
            .map(|(id, _)| note::normalize_id(&id))
            .collect())
    }

    /// The identity git itself would use here — the repo's config, then the
    /// user's global one, exactly as git resolves it.
    pub fn git_author(&self) -> Option<String> {
        let signature = self.repo.signature().ok()?;
        Some(format!(
            "{} <{}>",
            signature.name().ok()?,
            signature.email().ok()?
        ))
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

    pub fn note_path(&self, id: &str, slug: &str) -> PathBuf {
        self.path.join(note::file_name(id, slug))
    }

    /// Every adopted note, sorted by slug, read from the working tree.
    pub fn notes(&self) -> Result<Vec<NoteFile>> {
        let mut notes = Vec::new();
        for (id, slug) in self.scan()?.notes {
            let path = self.note_path(&id, &slug);
            let text = std::fs::read_to_string(&path)?;
            let note =
                Note::parse(&text).map_err(|e| Error::msg(format!("{}: {e}", path.display())))?;
            notes.push(NoteFile { id, slug, note });
        }
        notes.sort_by(|a, b| a.slug.cmp(&b.slug));
        Ok(notes)
    }

    /// Resolves a key to one note's `(id, slug)`.
    ///
    /// An exact slug is tried first, then an id prefix — the same bargain git
    /// makes with object ids, so `noda show k3f9` keeps working while the id
    /// itself is long enough not to need policing. An ambiguous key is an error
    /// naming the candidates, never a guess.
    ///
    /// Resolution reads no file. Whether the note behind the name can be parsed
    /// is the caller's problem, and only some callers have it.
    pub fn resolve(&self, key: &str) -> Result<(String, String)> {
        if key.is_empty() || key.contains('/') || key.contains('\\') || key.contains("..") {
            return Err(Error::msg(format!("invalid note reference: {key}")));
        }
        let notes = self.named_files()?;

        let mut matched: Vec<&(String, String)> =
            notes.iter().filter(|(_, slug)| slug == key).collect();
        if matched.is_empty() {
            let wanted = note::normalize_id(key);
            matched = notes
                .iter()
                .filter(|(id, _)| note::normalize_id(id).starts_with(&wanted))
                .collect();
        }

        match matched.as_slice() {
            [(id, slug)] => Ok((id.clone(), slug.clone())),
            [] => Err(Error::msg(format!("note not found: {key}"))),
            many => Err(Error::msg(format!(
                "`{key}` matches {} notes — say which:\n{}",
                many.len(),
                many.iter()
                    .map(|(id, slug)| format!("  {id}  {slug}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            ))),
        }
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
            author: configured_author(paths),
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
            if let Some(branch) = name.strip_prefix(&prefix)
                && branch != "HEAD"
            {
                branches.push((branch.to_string(), oid));
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

    /// libgit2's own error here reads "remote 'origin' does not exist" — the
    /// same fact without the way out of it, so it is replaced rather than kept.
    #[allow(clippy::map_err_ignore)]
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
    ///
    /// Two notebooks that each added a note no longer collide: the notes carry
    /// their ids in their filenames, so they are two paths rather than two edits
    /// to one. What is left here is a genuine conflict — the same note edited on
    /// both sides — and only its author can settle that.
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
                .filter_map(std::result::Result::ok)
                .filter_map(|c| c.our.or(c.their).or(c.ancestor))
                .map(|entry| String::from_utf8_lossy(&entry.path).into_owned())
                .collect();
            self.abort_merge()?;
            return Err(Error::msg(format!(
                "pull: `{branch}` conflicts with the remote in {} — the merge was rolled back; \
                 resolve it with git in {}",
                conflicted.join(", "),
                self.path.display()
            )));
        }

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

    /// Commits, newest first. With `note_id`, only the commits that changed that
    /// note.
    ///
    /// A rename is followed without any rename detection: the id is in the
    /// filename, so the file a note occupied at any commit is whichever tree
    /// entry carried that prefix. Every commit records it, because every commit
    /// records the filenames.
    pub fn log(&self, note_id: Option<&str>, max: Option<usize>) -> Result<Vec<Entry>> {
        let mut walk = self.repo.revwalk()?;
        walk.push_head()?;
        // Time alone is not enough: noda commits several times a second, and
        // commits sharing a timestamp would come back in an arbitrary order.
        // The topological constraint keeps a child ahead of its parent.
        walk.set_sorting(git2::Sort::TIME | git2::Sort::TOPOLOGICAL)?;

        let mut entries = Vec::new();
        for oid in walk {
            let commit = self.repo.find_commit(oid?)?;
            if let Some(id) = note_id
                && !touches(&commit, id)?
            {
                continue;
            }
            entries.push(Entry {
                id: commit.id(),
                seconds: commit.time().seconds(),
                offset_minutes: commit.time().offset_minutes(),
                summary: commit.summary().ok().flatten().unwrap_or("").to_string(),
            });
            if max.is_some_and(|max| entries.len() >= max) {
                break;
            }
        }
        Ok(entries)
    }

    /// The uncommitted changes when there are any, and what the last commit
    /// changed when there are not — because noda commits as it goes, a clean
    /// notebook is the normal state and "what just happened" is the useful
    /// answer. `file` narrows it to one note.
    pub fn diff(&self, file: Option<&str>) -> Result<git2::Diff<'_>> {
        let mut options = git2::DiffOptions::new();
        options.include_untracked(true).recurse_untracked_dirs(true);
        if let Some(file) = file {
            options.pathspec(file);
        }

        let head = self.repo.head()?.peel_to_tree()?;
        let mut diff = if self.is_dirty()? {
            self.repo
                .diff_tree_to_workdir_with_index(Some(&head), Some(&mut options))?
        } else {
            let commit = self.repo.head()?.peel_to_commit()?;
            let parent = commit
                .parent(0)
                .ok()
                .map(|parent| parent.tree())
                .transpose()?;
            self.repo
                .diff_tree_to_tree(parent.as_ref(), Some(&head), Some(&mut options))?
        };

        // `noda mv` writes the note under its new slug and removes the old file.
        // Without rename detection that reads as a note deleted and an unrelated
        // one invented, which is not what happened to it.
        diff.find_similar(None)?;
        Ok(diff)
    }

    /// Resolves a revision the way git does: a full or abbreviated id, `HEAD~3`,
    /// a tag, a branch. Anything git accepts, and nothing invented on top.
    pub fn revision(&self, rev: &str) -> Result<git2::Commit<'_>> {
        let object = self
            .repo
            .revparse_single(rev)
            .map_err(|e| Error::msg(format!("unknown revision: {rev} — {}", e.message())))?;
        // Unlike the above, there is only one way to fail here — the revision
        // names a blob or a tree — and the message already says it.
        #[allow(clippy::map_err_ignore)]
        object
            .peel_to_commit()
            .map_err(|_| Error::msg(format!("`{rev}` is not a commit")))
    }

    /// The slug and text of a note as it stood at `commit`.
    pub fn note_at(&self, commit: &git2::Commit<'_>, id: &str) -> Result<Option<(String, String)>> {
        let Some((file, blob)) = note_blob(commit, id)? else {
            return Ok(None);
        };
        let blob = self.repo.find_blob(blob)?;
        let text = String::from_utf8_lossy(blob.content()).into_owned();
        let slug = file
            .strip_suffix(".md")
            .and_then(note::split_stem)
            .map_or_else(|| file.clone(), |(_, slug)| slug.to_string());
        Ok(Some((slug, text)))
    }

    /// The id a key referred to at `commit`, by slug or by id prefix. Lets a note
    /// that has since been deleted still be named.
    pub fn id_at(&self, commit: &git2::Commit<'_>, key: &str) -> Result<Option<String>> {
        let wanted = note::normalize_id(key);
        for (id, slug) in notes_in(&commit.tree()?) {
            if slug == key || note::normalize_id(&id).starts_with(&wanted) {
                return Ok(Some(id));
            }
        }
        Ok(None)
    }

    /// Undoes a conflicted merge, leaving the notebook exactly as it was.
    fn abort_merge(&self) -> Result<()> {
        let head = self.repo.head()?.peel_to_commit()?;
        self.repo
            .reset(head.as_object(), git2::ResetType::Hard, None)?;
        self.repo.cleanup_state()?;
        Ok(())
    }

    /// Who the commit is by: `config.toml` first, then whatever git would use,
    /// and a neutral identity when git is unconfigured.
    fn signature(&self) -> Result<Signature<'static>> {
        if let Some((name, email)) = &self.author {
            return Ok(Signature::now(name, email)?);
        }
        match self.repo.signature() {
            Ok(sig) => Ok(sig),
            Err(_) => Ok(Signature::now("noda", "noda@localhost")?),
        }
    }
}

/// The notebook every command acts on by default. When the state pointer is
/// missing — a wiped state directory, a fresh machine, a notebook restored from
/// backup — the configured default stands in for it: state records where you
/// are, config records where you belong. Resolved in one place because `ls`,
/// `notebook rm` and the note commands must all agree on the answer.
pub fn active_name(paths: &Paths) -> Result<String> {
    match paths.active_notebook() {
        Ok(name) => Ok(name),
        Err(missing) => {
            let fallback = Config::load(paths)
                .ok()
                .and_then(|config| config.get("notebook").map(str::to_string))
                .unwrap_or_else(|| config::DEFAULT_NOTEBOOK.to_string());
            if !Notebook::exists(paths, &fallback) {
                return Err(missing);
            }
            Ok(fallback)
        }
    }
}

/// The author `config.toml` asks for, if it holds a usable one. A malformed
/// value is left to `noda config` to complain about — a commit is not the place
/// to discover it.
fn configured_author(paths: &Paths) -> Option<(String, String)> {
    config::author_parts(Config::load(paths).ok()?.get("author")?)
}

/// Whether `commit` changed the note with this id, against its first parent —
/// the same simplification `git log` makes for merges.
fn touches(commit: &git2::Commit<'_>, id: &str) -> Result<bool> {
    let now = note_blob(commit, id)?;
    let before = match commit.parent(0) {
        Ok(parent) => note_blob(&parent, id)?,
        Err(_) => None,
    };
    // Comparing path as well as content catches a rename, which changes where
    // the note lives without touching a byte of it.
    Ok(now != before)
}

/// The file a note occupied at `commit` and the blob it held, or `None` when the
/// note was not in that commit.
///
/// Every commit records this, because every commit records the filenames — which
/// is why following a note across a rename needs no rename detection and no
/// committed map of its own.
fn note_blob(commit: &git2::Commit<'_>, id: &str) -> Result<Option<(String, git2::Oid)>> {
    let tree = commit.tree()?;
    let wanted = note::normalize_id(id);
    for entry in &tree {
        let Ok(name) = entry.name() else {
            continue;
        };
        let Some((entry_id, _)) = name.strip_suffix(".md").and_then(note::split_stem) else {
            continue;
        };
        if note::normalize_id(entry_id) == wanted {
            return Ok(Some((name.to_string(), entry.id())));
        }
    }
    Ok(None)
}

/// Every `(id, slug)` a tree holds, read from its filenames.
fn notes_in(tree: &git2::Tree<'_>) -> Vec<(String, String)> {
    let mut found = Vec::new();
    for entry in tree {
        let Ok(name) = entry.name() else {
            continue;
        };
        if let Some((id, slug)) = name.strip_suffix(".md").and_then(note::split_stem) {
            found.push((id.to_string(), slug.to_string()));
        }
    }
    found
}

/// The branch a fresh notebook starts on. libgit2 hardcodes `master`, so without
/// this a notebook disagrees with every other repository on a machine that sets
/// `init.defaultBranch` — and pushing it to a remote that expects `main` leaves
/// two branches where the user asked for one. `git init`'s own fallback is
/// `master`, and matching it keeps the two tools telling the same story.
fn initial_branch(config: &git2::Config) -> String {
    config
        .get_string("init.defaultBranch")
        .ok()
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "master".to_string())
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

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    /// A file-backed config: an in-memory one has no backend to write to.
    struct TempConfig(PathBuf, git2::Config);

    impl TempConfig {
        fn new() -> Self {
            static COUNTER: AtomicUsize = AtomicUsize::new(0);
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!("noda-config-{}-{n}", std::process::id()));
            let _ = std::fs::remove_file(&path);
            let config = git2::Config::open(&path).expect("open config");
            TempConfig(path, config)
        }

        fn set(&mut self, value: &str) {
            self.1
                .set_str("init.defaultBranch", value)
                .expect("set init.defaultBranch");
        }
    }

    impl Drop for TempConfig {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    fn scan_of(notes: &[(&str, &str)], unnamed: &[&str], suspicious: &[&str]) -> Scan {
        Scan {
            notes: notes
                .iter()
                .map(|(id, slug)| ((*id).to_string(), (*slug).to_string()))
                .collect(),
            unnamed: unnamed.iter().map(|f| (*f).to_string()).collect(),
            suspicious: suspicious.iter().map(|f| (*f).to_string()).collect(),
        }
    }

    #[test]
    fn a_healthy_notebook_has_nothing_to_report() {
        let scan = scan_of(&[("k3f9m2p1", "alpha"), ("q7x2rstv", "beta")], &[], &[]);
        assert!(scan.problems().is_empty());
        assert!(scan_of(&[], &[], &[]).problems().is_empty());
    }

    /// Two machines can mint one id without ever meeting. The filenames differ,
    /// so git merges them without a word — this is the only place it surfaces.
    #[test]
    fn one_id_on_two_notes_is_reported_once() {
        let scan = scan_of(&[("k3f9m2p1", "alpha"), ("k3f9m2p1", "beta")], &[], &[]);
        assert_eq!(
            scan.problems(),
            [(Problem::SharedId, vec!["k3f9m2p1".to_string()])]
        );
    }

    #[test]
    fn ids_are_compared_the_way_they_are_addressed() {
        // `resolve` folds case and the I/L/O confusables, so two spellings of one
        // id are one id here too.
        let scan = scan_of(&[("K3F9M2P1", "alpha"), ("k3f9m2p1", "beta")], &[], &[]);
        assert_eq!(scan.problems().len(), 1, "one id, spelled two ways");
    }

    #[test]
    fn each_kind_of_stray_file_is_named() {
        let scan = scan_of(&[], &["hand-written.md"], &["abcdefgh-hello.md"]);
        assert_eq!(
            scan.problems(),
            [
                (Problem::Unnamed, vec!["hand-written.md".to_string()]),
                (Problem::Suspicious, vec!["abcdefgh-hello.md".to_string()]),
            ]
        );
    }

    #[test]
    fn a_wholesale_problem_stays_one_kind() {
        // A directory of files copied in at once: however many, it is one kind
        // and it is counted, not enumerated.
        let files: Vec<String> = (0..2_000).map(|n| format!("note-{n:04}.md")).collect();
        let scan = Scan {
            notes: Vec::new(),
            unnamed: files,
            suspicious: Vec::new(),
        };

        let reported = scan.problems();
        assert_eq!(reported.len(), 1, "one kind, not two thousand problems");
        let (kind, subjects) = &reported[0];
        assert_eq!(*kind, Problem::Unnamed);
        assert_eq!(subjects.len(), 2_000);
        assert_eq!(
            kind.describe(subjects.len()),
            "2000 notes have no id in their filenames"
        );
    }

    #[test]
    fn a_notebook_starts_on_the_branch_git_would_have_used() {
        let mut config = TempConfig::new();
        assert_eq!(
            initial_branch(&config.1),
            "master",
            "unset, so `git init`'s own fallback"
        );

        config.set("main");
        assert_eq!(initial_branch(&config.1), "main");

        config.set("trunk");
        assert_eq!(initial_branch(&config.1), "trunk");
    }

    /// `initial_head("")` would leave the repository naming no branch at all.
    #[test]
    fn a_blank_default_branch_falls_back_rather_than_naming_nothing() {
        let mut config = TempConfig::new();
        config.set("   ");
        assert_eq!(initial_branch(&config.1), "master");
    }
}
