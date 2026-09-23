//! A notebook is a git repository of Markdown files; every mutation is a commit.
//! A note's identity is its filename, `<id>-<slug>.md`, and no bookkeeping file
//! is committed alongside, so there is nothing to conflict on.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use git2::{Repository, RepositoryInitOptions, Signature};

use crate::config::{self, Config};
use crate::note::{self, Note};
use crate::paths::Paths;
use crate::remote;
use crate::sign;
use crate::{Error, Result};

const REMOTE_NAME: &str = "origin";

/// Matched by `cmd::path` and `cmd::backlinks`, which widen only this failure
/// because they were asked about a file too.
pub const NOT_FOUND: &str = "note not found";

/// Spelled exactly, so no other file is excused from being an orphan.
pub const README_FILE: &str = "README.md";

pub struct Notebook {
    pub name: String,
    pub path: PathBuf,
    repo: Repository,
    author: Option<(String, String)>,
    /// `None` defers to git's `commit.gpgsign`. Resolved at the commit, so a
    /// misconfigured `gpg.format` stops `noda add` rather than `noda ls`.
    sign: Option<bool>,
}

pub struct NoteFile {
    pub id: String,
    pub slug: String,
    pub note: Note,
}

/// Where a notebook stands, as `noda status` reports it.
pub struct Status {
    pub branch: String,
    pub notes: usize,
    pub files: usize,
    /// Files differing from `HEAD`, untracked ones included.
    pub uncommitted: usize,
    pub remote: Option<String>,
    /// `(ahead, behind)` against the remote-tracking ref; `None` (shown as
    /// `never synced`) when there is no such ref yet.
    pub drift: Option<(usize, usize)>,
    pub problems: Vec<(Problem, Vec<String>)>,
}

/// Something in the notebook that noda will not settle on its own. Reported by
/// kind, because it usually goes wrong wholesale (a directory copied in).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Problem {
    /// One id on more than one file: two machines minted the same id, and the
    /// filenames differ, so git merged them silently.
    SharedId,
    /// Frontmatter but no id in the name: a hand-written note awaiting adoption.
    Unnamed,
    /// An id over a file with no frontmatter: a note that lost its block, or
    /// never a note.
    Suspicious,
}

impl Problem {
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

/// The four cases a filename and a frontmatter block produce: the block says
/// "a note", the id prefix says "adopted", and a file with neither is left alone.
pub struct Scan {
    /// `(id, slug)`.
    pub notes: Vec<(String, String)>,
    /// Frontmatter but no id: adoptable.
    pub unnamed: Vec<String>,
    /// An id but no frontmatter: ambiguous.
    pub suspicious: Vec<String>,
    /// Everything else. Which note uses one is `audit_links`'s question.
    pub files: Vec<String>,
}

/// Not part of `Scan`: it parses every body, which `status` must not pay for.
pub struct Audit {
    /// Files no note links to, `README_FILE` exempt.
    pub orphans: Vec<String>,
    /// `(note filename, destination, current filename)`: the path is gone but
    /// its id is held — a note retitled after being linked to.
    pub stale: Vec<(String, String, String)>,
    /// `(note filename, destination)` naming nothing the notebook holds.
    pub broken: Vec<(String, String)>,
}

/// A note the notebook used to hold; name and title from the last commit that
/// had it.
pub struct Deleted {
    pub id: String,
    pub slug: String,
    pub title: String,
    /// The commit that removed it.
    pub removed_in: git2::Oid,
    /// The last commit that still held it, for `restore`.
    pub restore_from: git2::Oid,
    pub removed_at: i64,
    pub offset_minutes: i32,
}

impl Deleted {
    pub fn restore_from_short(&self) -> String {
        short(self.restore_from)
    }
}

impl Scan {
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
    /// With the offset, so a commit prints in the zone it was written in.
    pub seconds: i64,
    pub offset_minutes: i32,
    pub summary: String,
}

impl Entry {
    pub fn short_id(&self) -> String {
        short(self.id)
    }
}

/// One snapshot — a git tag — as `noda snapshot` lists it.
pub struct Snapshot {
    pub name: String,
    /// The commit, not the tag object.
    pub target: git2::Oid,
    pub seconds: i64,
    pub offset_minutes: i32,
    pub message: String,
}

impl Snapshot {
    pub fn short_target(&self) -> String {
        short(self.target)
    }
}

/// One line of a note, and the commit that put it there.
pub struct BlameLine {
    /// `None` for a line that is on disk but not committed.
    pub commit: Option<git2::Oid>,
    /// Both zero for an uncommitted line.
    pub seconds: i64,
    pub offset_minutes: i32,
    pub text: String,
}

impl BlameLine {
    /// Abbreviated, or git's `0000000` for an uncommitted line.
    pub fn short_commit(&self) -> String {
        self.commit.map_or_else(|| "0".repeat(7), short)
    }
}

impl Notebook {
    /// With an empty first commit, because `HEAD` has to name something before
    /// a branch can be pushed or compared against a remote.
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
        let (author, sign) = commit_settings(paths);
        let notebook = Notebook {
            name: name.to_string(),
            path,
            repo,
            author,
            sign,
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
        let (author, sign) = commit_settings(paths);
        Ok(Notebook {
            name: name.to_string(),
            path,
            repo,
            author,
            sign,
        })
    }

    pub fn open_active(paths: &Paths) -> Result<Self> {
        Notebook::open(paths, &active_name(paths)?)
    }

    /// Drift is measured against the remote-tracking ref, so it is as current as
    /// the last fetch: an orienting command should not need the network.
    pub fn status(&self) -> Result<Status> {
        let branch = self.branch()?;
        let scan = self.scan()?;

        let mut options = git2::StatusOptions::new();
        options.include_untracked(true).include_ignored(false);
        let uncommitted = self.repo.statuses(Some(&mut options))?.len();

        let drift = self.drift(&branch)?;

        Ok(Status {
            branch,
            notes: scan.notes.len(),
            files: scan.files.len(),
            uncommitted,
            remote: self.remote_url(),
            drift,
            problems: scan.problems(),
        })
    }

    /// `(ahead, behind)` against the remote-tracking ref, offline. Split from
    /// `status` so a caller wanting only this skips its working-tree walks.
    pub fn drift(&self, branch: &str) -> Result<Option<(usize, usize)>> {
        let tracking = format!("refs/remotes/{REMOTE_NAME}/{branch}");
        match (
            self.repo.head()?.target(),
            self.repo.refname_to_id(&tracking).ok(),
        ) {
            (Some(local), Some(upstream)) => {
                Ok(Some(self.repo.graph_ahead_behind(local, upstream)?))
            }
            _ => Ok(None),
        }
    }

    /// [`drift`](Self::drift)'s ahead count as the commits themselves. A set,
    /// because after a merge the unpushed commits are interleaved with the
    /// remote's rather than a run from `HEAD`; `push HEAD / hide upstream` is
    /// what `graph_ahead_behind` counts, so the two agree. Empty when never
    /// synced.
    pub fn unpushed(&self, branch: &str) -> Result<std::collections::HashSet<git2::Oid>> {
        let tracking = format!("refs/remotes/{REMOTE_NAME}/{branch}");
        let Ok(upstream) = self.repo.refname_to_id(&tracking) else {
            return Ok(std::collections::HashSet::new());
        };
        let mut walk = self.repo.revwalk()?;
        walk.push_head()?;
        walk.hide(upstream)?;
        Ok(walk.collect::<std::result::Result<std::collections::HashSet<_>, _>>()?)
    }

    /// `HEAD`'s time and offset: one commit read, cheap enough for a page
    /// listing every notebook.
    pub fn last_commit(&self) -> Result<(i64, i32)> {
        let commit = self.repo.head()?.peel_to_commit()?;
        Ok((commit.time().seconds(), commit.time().offset_minutes()))
    }

    /// Sorts every file into the four cases.
    pub fn scan(&self) -> Result<Scan> {
        let mut notes = Vec::new();
        let mut unnamed = Vec::new();
        let mut suspicious = Vec::new();
        let mut files = Vec::new();

        for entry in std::fs::read_dir(&self.path)? {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            let name = entry.file_name();
            // A non-UTF-8 name cannot match a link; a dotfile is repo config.
            let Some(name) = name.to_str() else { continue };
            if name.starts_with('.') {
                continue;
            }
            let Some(stem) = name.strip_suffix(".md") else {
                files.push(name.to_string());
                continue;
            };
            let file = format!("{stem}.md");
            let declared = Note::parse(&std::fs::read_to_string(entry.path())?).is_ok();
            match (note::split_stem(stem), declared) {
                (Some((id, slug)), true) => notes.push((id.to_string(), slug.to_string())),
                (Some(_), false) => suspicious.push(file),
                (None, true) => unnamed.push(file),
                (None, false) => files.push(file),
            }
        }

        notes.sort();
        unnamed.sort();
        suspicious.sort();
        files.sort();
        Ok(Scan {
            notes,
            unnamed,
            suspicious,
            files,
        })
    }

    /// Orphaned files and dangling links. Parses every body, so only on request.
    ///
    /// Links are checked against the filesystem, so one into a subdirectory
    /// resolves; orphans are reported at the root only. A dangling link is
    /// **stale** when it still names an id the notebook holds (noda knows the
    /// right name) and **broken** otherwise.
    pub fn audit_links(&self) -> Result<Audit> {
        let (notes, files) = self.inventory()?;
        let mut referenced: HashSet<String> = HashSet::new();
        let mut stale = Vec::new();
        let mut broken = Vec::new();

        let current: HashMap<String, String> = notes
            .iter()
            .map(|file| {
                (
                    note::normalize_id(&file.id),
                    note::file_name(&file.id, &file.slug),
                )
            })
            .collect();

        for file in &notes {
            let name = note::file_name(&file.id, &file.slug);
            for target in crate::link::targets(&file.note.body) {
                if self.path.join(&target).exists() {
                    referenced.insert(target);
                    continue;
                }
                match linked_note_id(&target).and_then(|id| current.get(&id)) {
                    Some(now) => stale.push((name.clone(), target, now.clone())),
                    None => broken.push((name.clone(), target)),
                }
            }
        }

        // The README addresses a reader outside; no note should have to link it.
        let orphans = files
            .into_iter()
            .filter(|file| file != README_FILE && !referenced.contains(file))
            .collect();

        stale.sort();
        broken.sort();
        Ok(Audit {
            orphans,
            stale,
            broken,
        })
    }

    /// The notes whose bodies link to the note `id` names. Matched on the id,
    /// not the filename, so a link survives `noda mv` retitling its target. A
    /// note linking to itself is listed.
    pub fn backlinks_to_note(&self, id: &str) -> Result<Vec<NoteFile>> {
        Ok(self
            .notes()?
            .into_iter()
            .filter(|file| links_to_note(&file.note, id))
            .collect())
    }

    /// Matched on the whole name: an attachment has no id.
    pub fn backlinks_to_file(&self, name: &str) -> Result<Vec<NoteFile>> {
        Ok(self
            .notes()?
            .into_iter()
            .filter(|file| links_to_file(&file.note, name))
            .collect())
    }

    /// The hooks git would run and noda will not: libgit2 runs none, silently.
    /// Found as git finds them — `core.hooksPath`, the executable bit, no
    /// `*.sample`. An unreadable directory is not a finding.
    pub fn hooks(&self) -> Result<Vec<String>> {
        let dir = match self.repo.config()?.get_path("core.hooksPath") {
            // Relative to the working tree, as git takes it.
            Ok(configured) => self.path.join(configured),
            Err(_) => self.repo.path().join("hooks"),
        };
        let Ok(entries) = std::fs::read_dir(dir) else {
            return Ok(Vec::new());
        };

        let mut found = Vec::new();
        for entry in entries.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            // `metadata` below follows a symlinked hook; `file_type` does not.
            if file_type.is_dir() {
                continue;
            }
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if name.ends_with(".sample") {
                continue;
            }
            if entry.metadata().is_ok_and(|meta| is_executable(&meta)) {
                found.push(name.to_string());
            }
        }
        found.sort();
        Ok(found)
    }

    /// Every `(id, slug)` a filename spells out, readable file or not; opens
    /// nothing. More forgiving than `scan`, so `rm`, `log`, `diff` and `restore`
    /// work on a broken file.
    pub fn named_files(&self) -> Result<Vec<(String, String)>> {
        let mut found = Vec::new();
        for entry in std::fs::read_dir(&self.path)? {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            let name = entry.file_name();
            if let Some(stem) = name.to_str().and_then(|name| name.strip_suffix(".md"))
                && let Some((id, slug)) = note::split_stem(stem)
            {
                found.push((id.to_string(), slug.to_string()));
            }
        }
        found.sort();
        Ok(found)
    }

    /// Every id spoken for, folded, from the filenames alone.
    pub fn taken_ids(&self) -> Result<HashSet<String>> {
        Ok(self
            .named_files()?
            .into_iter()
            .map(|(id, _)| note::normalize_id(&id))
            .collect())
    }

    /// The identity git itself would use here.
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

    /// Sorted. A directory that is not a git repo is skipped, not reported.
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

    /// The configured remote, credentials redacted here rather than per screen
    /// so a new screen cannot leak them. Fetch and push use `remote()` instead.
    pub fn remote_url(&self) -> Option<String> {
        let remote = self.repo.find_remote(REMOTE_NAME).ok()?;
        remote
            .url()
            .ok()
            .map(|url| remote::redact(url).into_owned())
    }

    pub fn note_path(&self, id: &str, slug: &str) -> PathBuf {
        self.path.join(note::file_name(id, slug))
    }

    /// Every adopted note, sorted by slug, each file read once. A file that
    /// will not parse is skipped.
    pub fn notes(&self) -> Result<Vec<NoteFile>> {
        Ok(self.inventory()?.0)
    }

    /// Notes and other files from one walk, classified as `scan` does; unnamed
    /// and suspicious files are in neither list, since `scan` reports them.
    pub fn inventory(&self) -> Result<(Vec<NoteFile>, Vec<String>)> {
        let mut notes = Vec::new();
        let mut files = Vec::new();

        for entry in std::fs::read_dir(&self.path)? {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if name.starts_with('.') {
                continue;
            }
            let Some(stem) = name.strip_suffix(".md") else {
                files.push(name.to_string());
                continue;
            };
            let parsed = std::fs::read_to_string(entry.path())
                .ok()
                .and_then(|text| Note::parse(&text).ok());
            match (note::split_stem(stem), parsed) {
                (Some((id, slug)), Some(note)) => notes.push(NoteFile {
                    id: id.to_string(),
                    slug: slug.to_string(),
                    note,
                }),
                (Some(_), None) | (None, Some(_)) => {}
                (None, None) => files.push(name.to_string()),
            }
        }

        notes.sort_by(|a, b| a.slug.cmp(&b.slug));
        files.sort();
        Ok((notes, files))
    }

    /// Resolves a key to one note's `(id, slug)`: exact slug first, then a
    /// folded id prefix, as git abbreviates object ids. An ambiguous key is an
    /// error naming the candidates. Reads no file.
    pub fn resolve(&self, key: &str) -> Result<(String, String)> {
        if key.is_empty() || key.contains('/') || key.contains('\\') || key.contains("..") {
            return Err(Error::msg(format!("invalid note reference: {key}")));
        }
        let wanted = note::normalize_id(key);

        let mut by_slug = Vec::new();
        let mut by_id = Vec::new();
        for entry in std::fs::read_dir(&self.path)? {
            let entry = entry?;
            // `file_type` comes with the entry; `is_file` would `stat`.
            if !entry.file_type()?.is_file() {
                continue;
            }
            let name = entry.file_name();
            let Some((id, slug)) = name
                .to_str()
                .and_then(|name| name.strip_suffix(".md"))
                .and_then(note::split_stem)
            else {
                continue;
            };
            if slug == key {
                by_slug.push((id.to_string(), slug.to_string()));
            } else if note::normalize_id(id).starts_with(&wanted) {
                by_id.push((id.to_string(), slug.to_string()));
            }
        }

        let mut matched = if by_slug.is_empty() { by_id } else { by_slug };
        matched.sort();

        match matched.len() {
            1 => Ok(matched.remove(0)),
            0 => Err(Error::msg(format!("{NOT_FOUND}: {key}"))),
            n => Err(Error::msg(format!(
                "`{key}` matches {n} notes — say which:\n{}",
                matched
                    .iter()
                    .map(|(id, slug)| format!("  {id}  {slug}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            ))),
        }
    }

    /// Stages `files` and commits them. A path that no longer exists is staged
    /// as a deletion, so a rename is one commit.
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

    /// Everything in the working tree, for `noda sync`. `false` when there was
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

    /// Untracked files included.
    pub fn is_dirty(&self) -> Result<bool> {
        let mut options = git2::StatusOptions::new();
        options.include_untracked(true).include_ignored(false);
        Ok(!self.repo.statuses(Some(&mut options))?.is_empty())
    }

    fn commit_index(&self, index: &mut git2::Index, message: &str) -> Result<()> {
        index.write()?;
        let tree = self.repo.find_tree(index.write_tree()?)?;
        let parent = match self.repo.head() {
            Ok(head) => Some(head.peel_to_commit()?),
            Err(_) => None,
        };
        let parents: Vec<&git2::Commit> = parent.iter().collect();
        self.write_commit(message, &tree, &parents)?;
        Ok(())
    }

    /// Writes the commit, signing when configured, and moves `HEAD` onto it.
    /// Signing needs the commit's text before it is an object, so it takes
    /// three steps, the last of which does not move the branch.
    fn write_commit(
        &self,
        message: &str,
        tree: &git2::Tree<'_>,
        parents: &[&git2::Commit<'_>],
    ) -> Result<git2::Oid> {
        let who = self.signature()?;
        let Some(signer) = sign::resolve(self.sign, &self.repo.config()?)? else {
            return Ok(self
                .repo
                .commit(Some("HEAD"), &who, &who, message, tree, parents)?);
        };

        let buffer = self
            .repo
            .commit_create_buffer(&who, &who, message, tree, parents)?;
        let content = std::str::from_utf8(&buffer).map_err(|e| {
            Error::msg(format!(
                "the commit is not valid UTF-8 and cannot be signed: {e}"
            ))
        })?;
        let oid = self
            .repo
            .commit_signed(content, &signer.sign(content)?, None)?;
        self.move_head(oid, message)?;
        Ok(oid)
    }

    /// `commit_signed` does not move any ref; without this the next `gc`
    /// collects the commit. Follows a symbolic `HEAD` (including a new
    /// notebook's unborn one), or moves a detached one directly.
    fn move_head(&self, oid: git2::Oid, message: &str) -> Result<()> {
        let head = self.repo.find_reference("HEAD")?;
        let target = head.symbolic_target()?.map(str::to_string);
        let reflog = format!("commit: {}", message.lines().next().unwrap_or(message));
        match target {
            Some(branch) => self.repo.reference(&branch, oid, true, &reflog)?,
            None => self.repo.reference("HEAD", oid, true, &reflog)?,
        };
        Ok(())
    }

    /// A clone that fails partway leaves no half-written directory behind.
    pub fn clone(paths: &Paths, url: &str, name: &str) -> Result<Self> {
        validate_name(name)?;
        let path = paths.notebook_dir(name);
        if path.exists() {
            return Err(Error::msg(format!("notebook already exists: {name}")));
        }
        std::fs::create_dir_all(paths.notebooks_dir())?;

        let mut builder = git2::build::RepoBuilder::new();
        // No repository yet, so global and system config, as `git clone` uses.
        builder.fetch_options(remote::fetch_options(git2::Config::open_default()?));
        let repo = builder.clone(url, &path).map_err(|e| {
            let _ = std::fs::remove_dir_all(&path);
            remote::explain(e, url)
        })?;

        let (author, sign) = commit_settings(paths);
        let notebook = Notebook {
            name: name.to_string(),
            path,
            repo,
            author,
            sign,
        };
        if let Err(e) = notebook.adopt_remote_branch() {
            let path = notebook.path.clone();
            drop(notebook);
            let _ = std::fs::remove_dir_all(&path);
            return Err(e);
        }
        Ok(notebook)
    }

    /// A `HEAD` naming a branch the remote lacks (machines disagreeing on
    /// `init.defaultBranch`) checks out nothing. A sole remote branch is
    /// adopted; otherwise fail naming them.
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

    /// An annotated tag, so it carries an author, time and message. Never moves
    /// an existing one: a snapshot must mean one thing to be cited.
    pub fn snapshot(&self, name: &str, message: &str) -> Result<git2::Oid> {
        let refname = format!("refs/tags/{name}");
        if !git2::Reference::is_valid_name(&refname) {
            return Err(Error::msg(format!(
                "invalid snapshot name: {name} — no spaces, no `..`, no `~^:?*[\\`"
            )));
        }
        if self.repo.refname_to_id(&refname).is_ok() {
            return Err(Error::msg(format!(
                "snapshot already exists: {name} — pick another name, or remove it with \
                 `git tag -d {name}` in {}",
                self.path.display()
            )));
        }

        let head = self.repo.head()?.peel_to_commit()?;
        let signature = self.signature()?;
        self.repo
            .tag(name, head.as_object(), &signature, message, false)?;
        Ok(head.id())
    }

    /// Newest first by the tagged commit's time, as `log` orders. Lightweight
    /// tags made outside noda are listed too.
    pub fn snapshots(&self) -> Result<Vec<Snapshot>> {
        let mut found = Vec::new();
        // A non-UTF-8 name is skipped rather than failing the listing.
        let names = self.repo.tag_names(None)?;
        for name in names.iter().filter_map(|name| name.ok().flatten()) {
            let reference = self.repo.find_reference(&format!("refs/tags/{name}"))?;
            let commit = reference.peel_to_commit()?;
            // A lightweight tag has no message; its commit's summary stands in.
            let message = match reference.peel_to_tag() {
                Ok(tag) => tag
                    .message()
                    .ok()
                    .flatten()
                    .unwrap_or("")
                    .trim()
                    .to_string(),
                Err(_) => commit.summary().ok().flatten().unwrap_or("").to_string(),
            };
            found.push(Snapshot {
                name: name.to_string(),
                target: commit.id(),
                seconds: commit.time().seconds(),
                offset_minutes: commit.time().offset_minutes(),
                message,
            });
        }
        found.sort_by(|a, b| b.seconds.cmp(&a.seconds).then_with(|| a.name.cmp(&b.name)));
        Ok(found)
    }

    pub fn branch(&self) -> Result<String> {
        let head = self.repo.head()?;
        Ok(head.shorthand()?.to_string())
    }

    /// libgit2's error, with the way out of it.
    #[allow(clippy::map_err_ignore)]
    fn remote(&self) -> Result<git2::Remote<'_>> {
        self.repo.find_remote(REMOTE_NAME).map_err(|_| {
            Error::msg(format!(
                "notebook `{}` has no remote — set one with `noda remote set <url>`",
                self.name
            ))
        })
    }

    /// `None` when the remote does not carry the branch yet, a normal first
    /// sync.
    fn fetch(&self) -> Result<Option<git2::Oid>> {
        let branch = self.branch()?;
        let mut remote = self.remote()?;
        let url = remote.url().unwrap_or_default().to_string();
        let refspec = format!("+refs/heads/{branch}:refs/remotes/{REMOTE_NAME}/{branch}");

        let config = self.repo.config()?;
        // Tags too, so a snapshot taken elsewhere can be restored from here.
        let mut options = remote::fetch_options(config);
        options.download_tags(git2::AutotagOption::All);
        match remote.fetch(&[&refspec], Some(&mut options), None) {
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

    /// Fast-forward where possible, a merge commit where the histories
    /// diverged. A conflict (the same note edited on both sides) is rolled back
    /// rather than left half-applied: noda has no `--continue`.
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

        // Counted after the fetch and before the branch moves.
        let incoming_count = self.drift(&branch)?.map_or(0, |(_, behind)| behind);

        if analysis.is_fast_forward() {
            let refname = format!("refs/heads/{branch}");
            self.repo
                .find_reference(&refname)?
                .set_target(incoming, "noda pull: fast-forward")?;
            self.repo.set_head(&refname)?;
            self.repo
                .checkout_head(Some(git2::build::CheckoutBuilder::new().force()))?;
            return Ok(format!(
                "pull: fast-forwarded {} to {}",
                plural(incoming_count, "commit"),
                short(incoming)
            ));
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
        let ours = self.repo.head()?.peel_to_commit()?;
        let theirs = self.repo.find_commit(incoming)?;
        self.write_commit(
            &format!("merge: {REMOTE_NAME}/{branch}"),
            &tree,
            &[&ours, &theirs],
        )?;
        self.repo.cleanup_state()?;
        Ok(format!(
            "pull: merged {} from {REMOTE_NAME}/{branch}",
            plural(incoming_count, "commit")
        ))
    }

    /// The branch and the snapshots the remote lacks. A rejection is reported
    /// as advice to pull.
    pub fn push(&self) -> Result<String> {
        let branch = self.branch()?;
        // Before sending: libgit2 moves the tracking ref once the push lands.
        let ahead = self.drift(&branch)?.map(|(ahead, _)| ahead);
        let mut remote = self.remote()?;
        let url = remote.url().unwrap_or_default().to_string();
        // Snapshots go with the branch, named one by one because libgit2
        // refuses a wildcard push refspec.
        let mut refspecs = vec![format!("refs/heads/{branch}:refs/heads/{branch}")];
        let mut held_back = Vec::new();
        let local = self.local_tags()?;
        if !local.is_empty() {
            let theirs = self.remote_tags(&mut remote, &url)?;
            for (name, oid) in local {
                match theirs.get(&name) {
                    Some(other) if *other == oid => {}
                    // Two machines each made a `q3`. Sending it would
                    // overwrite theirs or abort the whole push (libgit2
                    // fast-forward-checks tags), so it is held back.
                    Some(_) => held_back.push(name),
                    None => refspecs.push(format!("refs/tags/{name}:refs/tags/{name}")),
                }
            }
        }

        // The callbacks borrow `rejections` and must drop before it is read.
        let rejections = std::cell::RefCell::new(Vec::new());
        let pushed = {
            let mut callbacks = remote::callbacks(self.repo.config()?);
            callbacks.push_update_reference(|refname, status| {
                if let Some(reason) = status {
                    rejections.borrow_mut().push(format!("{refname}: {reason}"));
                }
                Ok(())
            });
            let mut options = git2::PushOptions::new();
            options.remote_callbacks(callbacks);
            let refspecs: Vec<&str> = refspecs.iter().map(String::as_str).collect();
            remote.push(&refspecs, Some(&mut options))
        };
        if let Err(e) = pushed {
            // libgit2 refuses before sending; a server refuses via the callback.
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

        // The branch is the first refspec; the rest are snapshots.
        let snapshots = refspecs.len() - 1;
        let mut sent = Vec::new();
        if let Some(n) = ahead
            && n > 0
        {
            sent.push(plural(n, "commit"));
        }
        if snapshots > 0 {
            sent.push(plural(snapshots, "snapshot"));
        }

        let mut out = match (sent.is_empty(), ahead) {
            (false, _) => format!("push: {branch} ({}) -> {url}", sent.join(", ")),
            (true, Some(_)) => format!("push: {branch} matches {url} — nothing to send"),
            // Never synced: no count is known.
            (true, None) => format!("push: {branch} -> {url}"),
        };
        for name in held_back {
            let _ = write!(
                out,
                "\nsnapshot `{name}` was not sent — the remote already has that name for another \
                 commit; rename yours, or drop it with `git tag -d {name}`"
            );
        }
        Ok(out)
    }

    /// Unpeeled, to compare against the remote's advertisement.
    fn local_tags(&self) -> Result<Vec<(String, git2::Oid)>> {
        let names = self.repo.tag_names(None)?;
        let mut found = Vec::new();
        for name in names.iter().filter_map(|name| name.ok().flatten()) {
            if let Ok(oid) = self.repo.refname_to_id(&format!("refs/tags/{name}")) {
                found.push((name.to_string(), oid));
            }
        }
        Ok(found)
    }

    /// From the reference advertisement: one extra round trip, only when there
    /// is a snapshot to send.
    fn remote_tags(
        &self,
        remote: &mut git2::Remote<'_>,
        url: &str,
    ) -> Result<HashMap<String, git2::Oid>> {
        let callbacks = remote::callbacks(self.repo.config()?);
        let connection = remote
            .connect_auth(git2::Direction::Push, Some(callbacks), None)
            .map_err(|e| remote::explain(e, url))?;

        let mut found = HashMap::new();
        for head in connection.list()? {
            // `refs/tags/q3^{}` is the peeled form.
            let Some(name) = head.name().strip_prefix("refs/tags/") else {
                continue;
            };
            if name.ends_with("^{}") {
                continue;
            }
            found.insert(name.to_string(), head.oid());
        }
        Ok(found)
    }

    /// Commits, newest first; with `note_id`, only those that changed it,
    /// following renames by the id in the filename.
    pub fn log(&self, note_id: Option<&str>, max: Option<usize>) -> Result<Vec<Entry>> {
        let mut walk = self.repo.revwalk()?;
        walk.push_head()?;
        // noda commits several times a second; time alone leaves ties.
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

    /// Which commit put each line of a note's body where it is.
    ///
    /// Not libgit2's blame: its `GIT_BLAME_TRACK_COPIES_*` options are
    /// unimplemented, so it stops at a rename, and `noda mv` renames on every
    /// retitle. Computed from diffs instead, finding the note in each commit by
    /// id. A commit matching any parent is skipped, so a merge is not credited
    /// with what it carried; otherwise the first parent is compared.
    ///
    /// Body only: `updated` changes on every edit, so the frontmatter is noise.
    pub fn blame(&self, id: &str, slug: &str) -> Result<Vec<BlameLine>> {
        let text = std::fs::read_to_string(self.note_path(id, slug))?;
        let lines: Vec<&str> = text.lines().collect();
        // Where each traced line sits in the version being examined.
        let mut origin: Vec<Option<usize>> = (0..lines.len()).map(Some).collect();
        let mut found: Vec<Option<git2::Oid>> = vec![None; lines.len()];
        let mut when: HashMap<git2::Oid, (i64, i32)> = HashMap::new();

        // Lines on disk but not in `HEAD` are nobody's yet.
        let head = self.repo.head()?.peel_to_commit()?;
        match note_blob(&head, id)? {
            Some((_, oid)) => {
                let blob = self.repo.find_blob(oid)?;
                let map = line_map(blob.content(), text.as_bytes())?;
                attribute(&mut origin, &mut found, &map, None);
            }
            // Never committed: no history to walk.
            None => origin.fill(None),
        }

        let mut walk = self.repo.revwalk()?;
        walk.push_head()?;
        walk.set_sorting(git2::Sort::TIME | git2::Sort::TOPOLOGICAL)?;
        for oid in walk {
            if origin.iter().all(Option::is_none) {
                break;
            }
            let commit = self.repo.find_commit(oid?)?;
            let Some((_, now)) = note_blob(&commit, id)? else {
                continue;
            };
            let parents: Vec<Option<git2::Oid>> = commit
                .parents()
                .map(|parent| note_blob(&parent, id).map(|blob| blob.map(|(_, oid)| oid)))
                .collect::<Result<_>>()?;
            if parents.contains(&Some(now)) {
                continue;
            }

            let new = self.repo.find_blob(now)?;
            let old = match parents.first().copied().flatten() {
                Some(oid) => Some(self.repo.find_blob(oid)?),
                // The commit that created the note.
                None => None,
            };
            let map = line_map(
                old.as_ref().map_or(&[][..], git2::Blob::content),
                new.content(),
            )?;
            attribute(&mut origin, &mut found, &map, Some(commit.id()));
            when.insert(
                commit.id(),
                (commit.time().seconds(), commit.time().offset_minutes()),
            );
        }

        let start = body_start(&text);
        Ok(lines
            .into_iter()
            .enumerate()
            .skip(start)
            .map(|(line, text)| {
                let commit = found[line];
                let (seconds, offset_minutes) = commit
                    .and_then(|oid| when.get(&oid).copied())
                    .unwrap_or((0, 0));
                BlameLine {
                    commit,
                    seconds,
                    offset_minutes,
                    text: text.to_string(),
                }
            })
            .collect())
    }

    /// When each note last changed according to git, by id, in one walk of
    /// history rather than `log` per note. Reached only through
    /// `doctor --times`.
    pub fn last_changed(&self) -> Result<HashMap<String, i64>> {
        let mut walk = self.repo.revwalk()?;
        walk.push_head()?;
        walk.set_sorting(git2::Sort::TIME | git2::Sort::TOPOLOGICAL)?;

        let mut last: HashMap<String, i64> = HashMap::new();
        for oid in walk {
            let commit = self.repo.find_commit(oid?)?;
            let now = note_blobs(&commit)?;
            // First parent, as `touches` does.
            let before = match commit.parent(0) {
                Ok(parent) => note_blobs(&parent)?,
                Err(_) => BTreeMap::new(),
            };
            for (id, entry) in &now {
                if before.get(id) != Some(entry) {
                    last.entry(id.clone()).or_insert(commit.time().seconds());
                }
            }
        }
        Ok(last)
    }

    /// Notes history holds that the notebook no longer does: ids in a parent's
    /// tree but not the commit's, minus the ids on disk now. So a rename is not
    /// a deletion, a restored note is not reported, and a `git rm` counts like
    /// `noda rm`. A full walk of history; newest first, so an id's first
    /// disappearance found is its last.
    pub fn deleted(&self) -> Result<Vec<Deleted>> {
        let present = self.taken_ids()?;
        let mut walk = self.repo.revwalk()?;
        walk.push_head()?;
        walk.set_sorting(git2::Sort::TIME | git2::Sort::TOPOLOGICAL)?;

        let mut found = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        for oid in walk {
            let commit = self.repo.find_commit(oid?)?;
            // First parent; a root commit deletes nothing.
            let Ok(parent) = commit.parent(0) else {
                continue;
            };
            let now = note_blobs(&commit)?;
            for id in note_blobs(&parent)?.keys() {
                if now.contains_key(id) || present.contains(id) || !seen.insert(id.clone()) {
                    continue;
                }
                let Some((slug, text)) = self.note_at(&parent, id)? else {
                    continue;
                };
                found.push(Deleted {
                    id: id.clone(),
                    slug,
                    title: Note::parse(&text)
                        .map(|note| note.title)
                        .unwrap_or_default(),
                    removed_in: commit.id(),
                    restore_from: parent.id(),
                    removed_at: commit.time().seconds(),
                    offset_minutes: commit.time().offset_minutes(),
                });
            }
        }

        found.sort_by(|a, b| {
            b.removed_at
                .cmp(&a.removed_at)
                .then_with(|| a.id.cmp(&b.id))
        });
        Ok(found)
    }

    /// Uncommitted changes, or what the last commit changed when there are
    /// none (clean being the normal state).
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

        // Or `noda mv` reads as a deletion plus an unrelated new note.
        diff.find_similar(None)?;
        Ok(diff)
    }

    /// What a push would carry, offline: committed work from the merge base,
    /// `origin/<branch>...HEAD`. Two-dot would show every line the remote added
    /// as removed.
    pub fn diff_remote(&self, branch: &str, file: Option<&str>) -> Result<git2::Diff<'_>> {
        let tracking = format!("refs/remotes/{REMOTE_NAME}/{branch}");
        let Ok(upstream) = self.repo.refname_to_id(&tracking) else {
            return Err(Error::msg(format!(
                "notebook `{}` has never synced, so there is nothing to compare against — \
                 run `noda sync` first",
                self.name
            )));
        };

        let head = self.repo.head()?.peel_to_commit()?;
        let base = self.repo.merge_base(head.id(), upstream)?;

        let mut options = git2::DiffOptions::new();
        if let Some(file) = file {
            options.pathspec(file);
        }
        let mut diff = self.repo.diff_tree_to_tree(
            Some(&self.repo.find_commit(base)?.tree()?),
            Some(&head.tree()?),
            Some(&mut options),
        )?;
        diff.find_similar(None)?;
        Ok(diff)
    }

    /// Any revision git accepts, peeled to a commit.
    pub fn revision(&self, rev: &str) -> Result<git2::Commit<'_>> {
        let object = self
            .repo
            .revparse_single(rev)
            .map_err(|e| Error::msg(format!("unknown revision: {rev} — {}", e.message())))?;
        // The only failure left is a blob or a tree.
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

    /// A blob's text, for the web layer's edit lock, which carries the base
    /// version's blob id. `None` is ordinary: an uncommitted note has no blob,
    /// and non-UTF-8 bytes are unusable as a base too.
    pub fn blob_text(&self, oid: git2::Oid) -> Result<Option<String>> {
        match self.repo.find_blob(oid) {
            Ok(blob) => Ok(String::from_utf8(blob.content().to_vec()).ok()),
            Err(e) if e.code() == git2::ErrorCode::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// By slug or id prefix, so a deleted note can still be named.
    pub fn id_at(&self, commit: &git2::Commit<'_>, key: &str) -> Result<Option<String>> {
        let wanted = note::normalize_id(key);
        for (id, slug) in notes_in(&commit.tree()?) {
            if slug == key || note::normalize_id(&id).starts_with(&wanted) {
                return Ok(Some(id));
            }
        }
        Ok(None)
    }

    fn abort_merge(&self) -> Result<()> {
        let head = self.repo.head()?.peel_to_commit()?;
        self.repo
            .reset(head.as_object(), git2::ResetType::Hard, None)?;
        self.repo.cleanup_state()?;
        Ok(())
    }

    /// `config.toml` first, then git, then a neutral identity.
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

/// The notebook commands act on: the state pointer, else the configured
/// default if it exists.
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

/// Who to commit as and whether to sign, from one config read. A malformed
/// author is ignored here; `noda config` reports it.
fn commit_settings(paths: &Paths) -> (Option<(String, String)>, Option<bool>) {
    let Ok(config) = Config::load(paths) else {
        return (None, None);
    };
    let author = config.get("author").and_then(config::author_parts);
    (author, config.sign())
}

/// Against the first parent only.
fn touches(commit: &git2::Commit<'_>, id: &str) -> Result<bool> {
    let now = note_blob(commit, id)?;
    let before = match commit.parent(0) {
        Ok(parent) => note_blob(&parent, id)?,
        Err(_) => None,
    };
    // Path as well as blob, or a rename goes unnoticed.
    Ok(now != before)
}

/// The file a note occupied at `commit`, found by id, and its blob.
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

/// `note_blob` for a whole tree, keyed by folded id.
fn note_blobs(commit: &git2::Commit<'_>) -> Result<BTreeMap<String, (String, git2::Oid)>> {
    let mut found = BTreeMap::new();
    for entry in &commit.tree()? {
        let Ok(name) = entry.name() else {
            continue;
        };
        let Some((id, _)) = name.strip_suffix(".md").and_then(note::split_stem) else {
            continue;
        };
        found.insert(note::normalize_id(id), (name.to_string(), entry.id()));
    }
    Ok(found)
}

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

/// `init.defaultBranch`, which libgit2 ignores, else `git init`'s `master`.
fn initial_branch(config: &git2::Config) -> String {
    config
        .get_string("init.defaultBranch")
        .ok()
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "master".to_string())
}

fn plural(n: usize, thing: &str) -> String {
    if n == 1 {
        format!("1 {thing}")
    } else {
        format!("{n} {thing}s")
    }
}

fn rejected(reasons: &[String]) -> Error {
    Error::msg(format!(
        "push rejected — {}\nthe remote has commits you do not: run `noda pull` first",
        reasons.join("; ")
    ))
}

/// Moves every traced line one version back. A line with no counterpart is
/// credited to `commit` (`None` for the working tree).
fn attribute(
    origin: &mut [Option<usize>],
    found: &mut [Option<git2::Oid>],
    map: &[Option<usize>],
    commit: Option<git2::Oid>,
) {
    for (line, place) in origin.iter_mut().enumerate() {
        let Some(at) = *place else { continue };
        if let Some(earlier) = map.get(at).copied().flatten() {
            *place = Some(earlier);
        } else {
            found[line] = commit;
            *place = None;
        }
    }
}

/// For each line of `new`, the line of `old` it came from. Context spans both
/// files, so one hunk maps every line, not only those near a change.
fn line_map(old: &[u8], new: &[u8]) -> Result<Vec<Option<usize>>> {
    let count = line_count(new);
    // Equal sides give no hunks, indistinguishable from a file created whole.
    if old == new {
        return Ok((0..count).map(Some).collect());
    }

    let mut options = git2::DiffOptions::new();
    options
        .context_lines(u32::try_from(count + line_count(old)).unwrap_or(u32::MAX))
        // A stray byte must not make the note binary, with no lines to map.
        .force_text(true);
    let path = Path::new("note");
    let patch = git2::Patch::from_buffers(old, Some(path), new, Some(path), Some(&mut options))?;

    let mut map = vec![None; count];
    for hunk in 0..patch.num_hunks() {
        for index in 0..patch.num_lines_in_hunk(hunk)? {
            let line = patch.line_in_hunk(hunk, index)?;
            if !matches!(line.origin(), ' ' | '+') {
                continue;
            }
            let Some(number) = line.new_lineno() else {
                continue;
            };
            if let Some(slot) = map.get_mut(number as usize - 1) {
                *slot = line.old_lineno().map(|number| number as usize - 1);
            }
        }
    }
    Ok(map)
}

/// Matches `str::lines`, which splits the reported text.
fn line_count(bytes: &[u8]) -> usize {
    bytes.split_inclusive(|byte| *byte == b'\n').count()
}

/// Past the frontmatter and its blank line; zero when there is none.
fn body_start(text: &str) -> usize {
    let Some((_, body)) = note::split_frontmatter(text) else {
        return 0;
    };
    // As `Note::parse` trims.
    let body = body.trim_start_matches('\n');
    text[..text.len() - body.len()].lines().count()
}

/// The folded id in a destination's filename. Only the root holds notes, so a
/// destination into a subdirectory has none.
pub fn linked_note_id(target: &str) -> Option<String> {
    if target.contains('/') {
        return None;
    }
    let (id, _) = note::split_stem(target.strip_suffix(".md")?)?;
    Some(note::normalize_id(id))
}

/// Every tag with its note count, commonest first, then alphabetical. Decided
/// here so the TUI and web agree; a free function so a caller holding the
/// notes does not walk the directory again.
pub fn tag_tally(notes: &[NoteFile]) -> Vec<(String, usize)> {
    let mut counted: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    for file in notes {
        for tag in &file.note.tags {
            *counted.entry(tag.as_str()).or_default() += 1;
        }
    }
    let mut tallies: Vec<(String, usize)> = counted
        .into_iter()
        .map(|(tag, notes)| (tag.to_string(), notes))
        .collect();
    tallies.sort_by(|(left_tag, left), (right_tag, right)| {
        right.cmp(left).then_with(|| left_tag.cmp(right_tag))
    });
    tallies
}

/// Whether this note's body links to the note `id` names. A free function for
/// the TUI, which already holds every note; [`Notebook::backlinks_to_note`]
/// reads them from disk.
pub fn links_to_note(note: &Note, id: &str) -> bool {
    let want = note::normalize_id(id);
    crate::link::targets(&note.body)
        .iter()
        .any(|target| linked_note_id(target).as_deref() == Some(want.as_str()))
}

/// Whether this note's body links to the notebook file `name` names.
pub fn links_to_file(note: &Note, name: &str) -> bool {
    crate::link::targets(&note.body)
        .iter()
        .any(|target| target == name)
}

/// The executable bit, all git looks at. Without one, every hook counts.
#[cfg(unix)]
fn is_executable(metadata: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;

    metadata.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn is_executable(_: &std::fs::Metadata) -> bool {
    true
}

/// Now, and the local UTC offset. Asked of libgit2 because jiff is built
/// without a timezone database, and libgit2's offset comes from the C library,
/// the same source as every commit time noda prints.
pub fn local_now() -> Result<(i64, i32)> {
    let when = Signature::now("noda", "noda@localhost")?.when();
    Ok((when.seconds(), when.offset_minutes()))
}

pub(crate) fn short(oid: git2::Oid) -> String {
    oid.to_string()[..7].to_string()
}

/// Notebook names become directory names, so must not escape the data dir.
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

    /// A file, because an in-memory config has no backend to write to.
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
            files: Vec::new(),
        }
    }

    #[test]
    fn a_healthy_notebook_has_nothing_to_report() {
        let scan = scan_of(&[("k3f9m2p1", "alpha"), ("q7x2rstv", "beta")], &[], &[]);
        assert!(scan.problems().is_empty());
        assert!(scan_of(&[], &[], &[]).problems().is_empty());
    }

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
        let files: Vec<String> = (0..2_000).map(|n| format!("note-{n:04}.md")).collect();
        let scan = Scan {
            notes: Vec::new(),
            unnamed: files,
            suspicious: Vec::new(),
            files: Vec::new(),
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

    #[test]
    fn a_blank_default_branch_falls_back_rather_than_naming_nothing() {
        let mut config = TempConfig::new();
        config.set("   ");
        assert_eq!(initial_branch(&config.1), "master");
    }
}
