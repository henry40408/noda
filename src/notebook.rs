//! A notebook is a git repository of Markdown files. Every mutation is a commit.
//!
//! A note's identity is its filename, `<id>-<slug>.md`, and nothing derived is
//! committed alongside — no bookkeeping file to conflict on, nothing to fall out
//! of step. Two machines each adding a note write two filenames that git
//! merges without asking anyone to resolve anything.

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

/// noda configures exactly one remote per notebook.
const REMOTE_NAME: &str = "origin";

/// Named because `cmd::path` widens exactly this case — it was asked about a
/// file too — and passes every other failure through.
pub const NOT_FOUND: &str = "note not found";

/// Spelled exactly: a wider match would start excusing files nobody meant as a
/// front page.
pub const README_FILE: &str = "README.md";

pub struct Notebook {
    pub name: String,
    pub path: PathBuf,
    repo: Repository,
    /// Resolved on open: a notebook is opened once per command, read many
    /// times.
    author: Option<(String, String)>,
    /// `None` leaves the answer to git's `commit.gpgsign`. Read on open like
    /// `author` but *resolved* at the commit, so a misconfigured `gpg.format`
    /// stops `noda add` rather than `noda ls`.
    sign: Option<bool>,
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
    /// Free to count: the walk that finds the notes passes them anyway.
    pub files: usize,
    /// Files differing from `HEAD`, untracked ones included.
    pub uncommitted: usize,
    pub remote: Option<String>,
    /// `(ahead, behind)` against the remote-tracking ref, `None` when there is
    /// no such ref yet — which a first push builds as surely as a fetch, so the
    /// screens call it `never synced` rather than naming either half.
    pub drift: Option<(usize, usize)>,
    /// Empty is the healthy state, and the ordinary one.
    pub problems: Vec<(Problem, Vec<String>)>,
}

/// Something in the notebook that noda will not settle on its own.
///
/// Reported by kind, because the commonest way this goes wrong is wholesale — a
/// directory copied in at once — and one line of how many beats two thousand
/// naming them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Problem {
    /// One id on more than one file. Two machines can mint the same id without
    /// meeting; the filenames differ, so git merges them and only this notices.
    SharedId,
    /// Frontmatter but no id in the name: a note waiting to be adopted, which
    /// is what a hand-written file looks like.
    Unnamed,
    /// An id over a file with no frontmatter — a note that lost its block or a
    /// file that never was one, and only its author knows which.
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

/// The four cases a filename and a frontmatter block produce between them: the
/// block says "I am a note", the id prefix says "I have been adopted", and a
/// file with neither is left alone rather than reported forever.
pub struct Scan {
    /// Adopted notes, as `(id, slug)`.
    pub notes: Vec<(String, String)>,
    /// Frontmatter but no id: adoptable.
    pub unnamed: Vec<String>,
    /// An id but no frontmatter: ambiguous.
    pub suspicious: Vec<String>,
    /// Everything else the notebook holds. Counting is free; saying which note
    /// *uses* one means reading every body, which is `audit_links`.
    pub files: Vec<String>,
}

/// Deliberately not part of `Scan`: building it parses every note's body, so
/// `status` must never reach for it.
pub struct Audit {
    /// Files no note links to. `README_FILE` is exempt: it addresses a reader
    /// outside the notebook rather than being linked from inside.
    pub orphans: Vec<String>,
    /// `(note filename, destination, the note it still names)`: the file is
    /// gone but its id is still held — a note retitled after being linked to.
    pub stale: Vec<(String, String, String)>,
    /// `(note filename, destination)` naming nothing the notebook holds.
    pub broken: Vec<(String, String)>,
}

/// A note the notebook used to hold. Name and title come from the commit that
/// still had it, there being no file left to read them from.
pub struct Deleted {
    pub id: String,
    pub slug: String,
    pub title: String,
    /// The commit that removed it.
    pub removed_in: git2::Oid,
    /// The last commit that still held it — what `restore` must be pointed at.
    pub restore_from: git2::Oid,
    pub removed_at: i64,
    pub offset_minutes: i32,
}

impl Deleted {
    /// The revision to hand `noda restore`, abbreviated as git prints it.
    pub fn restore_from_short(&self) -> String {
        short(self.restore_from)
    }
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
    /// Time and offset, so a commit prints in the zone it was written in.
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
    /// The commit, not the tag object: the notebook's state is what is cited.
    pub target: git2::Oid,
    /// As `Entry` carries them, for its reason.
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
    /// As `Entry` carries them; both zero for an uncommitted line.
    pub seconds: i64,
    pub offset_minutes: i32,
    pub text: String,
}

impl BlameLine {
    /// Abbreviated, or git's own spelling for an uncommitted line.
    pub fn short_commit(&self) -> String {
        self.commit.map_or_else(|| "0".repeat(7), short)
    }
}

impl Notebook {
    /// Empty because noda commits no bookkeeping of its own, so the first note
    /// is the first content — but `HEAD` has to name something before a branch
    /// can be pushed or compared against a remote.
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

    /// `(ahead, behind)` against what the last fetch left behind.
    ///
    /// `status`'s cheap half, split out so a caller that only wants "2 to push"
    /// does not pay for its two walks of the working tree. Nothing goes to the
    /// network — both refs are already on disk.
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

    /// [`drift`](Self::drift)'s question answered with the commits themselves
    /// rather than a count.
    ///
    /// **A set, and it has to be.** After a `pull` merges, the unpushed commits
    /// are no longer a run along the top of the log — the two histories are
    /// interleaved below the merge, so walking down from `HEAD` would mark the
    /// wrong ones, and only on notebooks that had ever merged. `push HEAD /
    /// hide upstream` is what `graph_ahead_behind` counts, so this enumerates
    /// that same answer and the two cannot disagree.
    ///
    /// Empty with no remote-tracking ref: `status` calls that `never synced`.
    pub fn unpushed(&self, branch: &str) -> Result<std::collections::HashSet<git2::Oid>> {
        let tracking = format!("refs/remotes/{REMOTE_NAME}/{branch}");
        let Ok(upstream) = self.repo.refname_to_id(&tracking) else {
            return Ok(std::collections::HashSet::new());
        };
        let mut walk = self.repo.revwalk()?;
        walk.push_head()?;
        walk.hide(upstream)?;
        // A membership test; the order a set comes back in is not one.
        Ok(walk.collect::<std::result::Result<std::collections::HashSet<_>, _>>()?)
    }

    /// When the notebook was last written to, as the pair `Entry` carries.
    ///
    /// One commit read rather than a walk, which is what makes it affordable on
    /// a page already listing every notebook. Not part of `Status`: a field
    /// there would either change what `noda status` prints or sit unread.
    pub fn last_commit(&self) -> Result<(i64, i32)> {
        let commit = self.repo.head()?.peel_to_commit()?;
        Ok((commit.time().seconds(), commit.time().offset_minutes()))
    }

    /// Sorts every `*.md` into the four cases. Tolerant where `notes` is
    /// strict: one malformed file must not stop the notebook being described.
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
            // A non-UTF-8 name cannot be compared against a link destination,
            // and a dotfile is the repository's own configuration.
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
                // Neither a name nor a declaration: one more file.
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

    /// Both directions in which a link and a file can fail to meet.
    ///
    /// The expensive walk — every note's body parsed, the cost of `search` and
    /// not of `ls` — so nothing calls it unless asked.
    ///
    /// Links are checked against the filesystem, so a destination reaching into
    /// a subdirectory resolves; orphans are only reported at the root, because
    /// the root is the whole of the notebook noda models.
    ///
    /// A destination resolving to nothing splits in two, because only one half
    /// is a question for the author: **stale** still names an id the notebook
    /// holds, so noda knows what it should have said, while **broken** names
    /// none and only its author knows whether it is a typo or a file not copied
    /// in yet. `backlinks_to_note`'s distinction, reported rather than acted
    /// on.
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

        // The notebook's entrance, not a resource a note should link to. The
        // only way to clear such a finding reads backwards.
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

    /// The notes whose bodies link to the note `id` names.
    ///
    /// Matched on the id in the destination, not the whole filename, which is
    /// what makes an answer survive a retitle: `noda mv` leaves
    /// `[the meeting](v62b8rfa-meeting-notes.md)` naming a path that is gone and
    /// an id that is not. Matching the filename would go quiet after every
    /// retitle — precisely when somebody is looking.
    ///
    /// A note linking to itself is listed: it is what the file says.
    pub fn backlinks_to_note(&self, id: &str) -> Result<Vec<NoteFile>> {
        Ok(self
            .notes()?
            .into_iter()
            .filter(|file| links_to_note(&file.note, id))
            .collect())
    }

    /// No id to fall back on: an attachment's name is the whole of its
    /// identity, which is why `file mv` offers `--update-links`.
    pub fn backlinks_to_file(&self, name: &str) -> Result<Vec<NoteFile>> {
        Ok(self
            .notes()?
            .into_iter()
            .filter(|file| links_to_file(&file.note, name))
            .collect())
    }

    /// The hooks the repository holds that will never fire.
    ///
    /// libgit2 runs no hooks, so the same `pre-commit` is live under
    /// `git commit` and dead under `noda add` with nothing on screen to say
    /// which. That silence is the only reason this is reported.
    ///
    /// Exactly the set git would reach for: `core.hooksPath`, the executable
    /// bit, and never the `*.sample` files. An unreadable directory is not a
    /// finding — nothing here is a problem with the notebook.
    pub fn hooks(&self) -> Result<Vec<String>> {
        let dir = match self.repo.config()?.get_path("core.hooksPath") {
            // Relative is from the working tree, as git takes it.
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
            // A symlinked hook is a hook, and `metadata` follows where
            // `file_type` does not.
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

    /// Every `(id, slug)` a filename spells out, readable file or not.
    ///
    /// The name is the whole record, so this opens nothing and does not `stat`.
    /// Deliberately more forgiving than `scan`: `rm`, `log`, `diff` and
    /// `restore` must keep working on exactly the file somebody is reaching for
    /// them to fix. Public for `web`, which turns
    /// `[the plan](k3f9m2p1-the-plan.md)` into a link without opening every
    /// other note to do it.
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

    /// Every id spoken for, folded as `resolve` folds them. From the filenames
    /// alone: one directory listing, nothing opened.
    pub fn taken_ids(&self) -> Result<HashSet<String>> {
        Ok(self
            .named_files()?
            .into_iter()
            .map(|(id, _)| note::normalize_id(&id))
            .collect())
    }

    /// The identity git itself would use here, resolved as git resolves it.
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

    /// The configured remote, credentials redacted.
    ///
    /// Redacted here rather than at each screen, because the sixth screen added
    /// later would be a leak nobody thinks to look for. Nothing that talks to
    /// the network reads this — fetch and push open `remote()`.
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

    /// Every adopted note, sorted by slug, each file read once — going through
    /// `scan` would parse the whole notebook twice, which is the dominant cost
    /// of `ls` and `search`. A file that will not parse is skipped, as `scan`
    /// classifies it.
    pub fn notes(&self) -> Result<Vec<NoteFile>> {
        Ok(self.inventory()?.0)
    }

    /// Notes and non-notes from a single walk, because `ls` wants both.
    ///
    /// `scan`'s classification, so a file counts here exactly as it is reported
    /// there — the ones awaiting adoption or missing frontmatter are neither,
    /// since `scan` already reports them and listing them here names them
    /// twice.
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
                // `scan`'s to report, and not a file the notebook holds.
                (Some(_), None) | (None, Some(_)) => {}
                (None, None) => files.push(name.to_string()),
            }
        }

        notes.sort_by(|a, b| a.slug.cmp(&b.slug));
        files.sort();
        Ok((notes, files))
    }

    /// Resolves a key to one note's `(id, slug)`.
    ///
    /// Exact slug first, then an id prefix — git's bargain with object ids, so
    /// `noda show k3f9` works. An ambiguous key is an error naming the
    /// candidates, never a guess.
    ///
    /// Reads no file: whether the note parses is the caller's problem. The
    /// directory is walked once keeping only matches, because at these sizes
    /// building a list of every name costs more than the comparison.
    pub fn resolve(&self, key: &str) -> Result<(String, String)> {
        if key.is_empty() || key.contains('/') || key.contains('\\') || key.contains("..") {
            return Err(Error::msg(format!("invalid note reference: {key}")));
        }
        let wanted = note::normalize_id(key);

        // An exact slug wins outright, so the two are collected separately
        // rather than sorted out afterwards.
        let mut by_slug = Vec::new();
        let mut by_id = Vec::new();
        for entry in std::fs::read_dir(&self.path)? {
            let entry = entry?;
            // `file_type` comes with the entry; `is_file` would `stat` once
            // per note.
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
        // A list somebody has to choose from is not in filesystem order.
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
    /// as a deletion, so a rename is one commit rather than an add and a
    /// leftover.
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

    /// Everything in the working tree, for `noda sync`, which has to deal with
    /// notes edited outside noda. `false` when there was nothing to commit.
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
        let parent = match self.repo.head() {
            Ok(head) => Some(head.peel_to_commit()?),
            Err(_) => None,
        };
        let parents: Vec<&git2::Commit> = parent.iter().collect();
        self.write_commit(message, &tree, &parents)?;
        Ok(())
    }

    /// Writes the commit, signing when configured, and moves `HEAD` onto it.
    ///
    /// Unsigned is libgit2's one-call `commit`. Signed cannot be: signing needs
    /// the commit's text *before* it is an object, so it takes three steps and
    /// the last does not move the branch — hence [`Self::move_head`].
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
        // UTF-8 by construction: the message and signature are Rust `&str`.
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

    /// `commit_signed` writes an object and stops, unlike `commit`. Without
    /// this the notebook gains a commit nothing refers to, and the next `gc`
    /// collects the note with it.
    ///
    /// Symbolic with a branch, direct when detached. A new notebook's unborn
    /// `HEAD` is symbolic too, which is what lands the root commit on the
    /// branch `init.defaultBranch` named.
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
        // No repository yet, so the global and system files are all there is —
        // which is what `git clone` works from too.
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

    /// A `HEAD` naming a branch the remote does not carry checks out nothing,
    /// so the notebook reads as empty rather than broken — two machines
    /// disagreeing about `init.defaultBranch` is enough. One branch is taken;
    /// otherwise say what is there rather than hand back an unusable notebook.
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

    /// Annotated rather than lightweight: a snapshot says somebody closed a
    /// chapter at a moment, and a bare pointer with no author or time would list
    /// as an empty row.
    ///
    /// Never moves one that exists — a snapshot whose meaning can be reassigned
    /// cannot be cited, which is what `restore` takes a name for.
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

    /// Newest first by the marked commit's time, not the tagging time: that is
    /// the moment the snapshot is *of*, and what `log` and `deleted` order by.
    ///
    /// A lightweight tag made outside noda is listed too — a notebook is a
    /// normal git repository, and such a tag is still a place to restore from.
    pub fn snapshots(&self) -> Result<Vec<Snapshot>> {
        let mut found = Vec::new();
        // Not one noda made, and one unreadable name must not take the listing
        // down with it.
        let names = self.repo.tag_names(None)?;
        for name in names.iter().filter_map(|name| name.ok().flatten()) {
            let reference = self.repo.find_reference(&format!("refs/tags/{name}"))?;
            let commit = reference.peel_to_commit()?;
            // A lightweight tag is only a pointer, so its commit speaks for it.
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

    /// The branch `HEAD` points at.
    pub fn branch(&self) -> Result<String> {
        let head = self.repo.head()?;
        Ok(head.shorthand()?.to_string())
    }

    /// libgit2 says "remote 'origin' does not exist" — the same fact without
    /// the way out of it.
    #[allow(clippy::map_err_ignore)]
    fn remote(&self) -> Result<git2::Remote<'_>> {
        self.repo.find_remote(REMOTE_NAME).map_err(|_| {
            Error::msg(format!(
                "notebook `{}` has no remote — set one with `noda remote set <url>`",
                self.name
            ))
        })
    }

    /// `None` when the remote does not carry the branch yet: pushing to an
    /// empty repository is a normal first sync, not a failure.
    fn fetch(&self) -> Result<Option<git2::Oid>> {
        let branch = self.branch()?;
        let mut remote = self.remote()?;
        let url = remote.url().unwrap_or_default().to_string();
        let refspec = format!("+refs/heads/{branch}:refs/remotes/{REMOTE_NAME}/{branch}");

        let config = self.repo.config()?;
        // Or a snapshot taken on the other machine is invisible here, and
        // `restore <note> <snapshot>` fails on a name meant to be shared.
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
    /// diverged. A conflicting merge is rolled back rather than left
    /// half-applied — noda has no `--continue`.
    ///
    /// Two notebooks each adding a note write two paths rather than two edits to
    /// one, so what is left here is the same note edited on both sides, which
    /// only its author can settle.
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

        // The one moment both sides are known and neither has moved: the
        // tracking ref carries the remote's news and the branch is untouched.
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
    /// as advice to pull, because that is always the next step.
    pub fn push(&self) -> Result<String> {
        let branch = self.branch()?;
        // Before anything is sent: libgit2 moves the tracking ref once the push
        // lands, so this is the only moment the count exists. A push that raced
        // a moved remote is refused below rather than counted wrongly.
        let ahead = self.drift(&branch)?.map(|(ahead, _)| ahead);
        let mut remote = self.remote()?;
        let url = remote.url().unwrap_or_default().to_string();
        // Snapshots go with the branch, or they cannot be cited from anywhere
        // else. Named one by one because libgit2 refuses a wildcard on the push
        // side — it wants references it can resolve.
        let mut refspecs = vec![format!("refs/heads/{branch}:refs/heads/{branch}")];
        let mut held_back = Vec::new();
        let local = self.local_tags()?;
        if !local.is_empty() {
            let theirs = self.remote_tags(&mut remote, &url)?;
            for (name, oid) in local {
                match theirs.get(&name) {
                    // Already there, and meaning the same thing.
                    Some(other) if *other == oid => {}
                    // Two machines that each made a `q3`. Sending it either
                    // overwrites theirs or aborts the whole push — libgit2
                    // fast-forward-checks a tag like a branch — so the name
                    // gives way rather than the notes.
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
            // libgit2 refuses one before sending; a server refuses it through
            // the callback below.
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

        // The branch is always the first refspec, so the rest are snapshots.
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
            // Nothing moved, and the notebook knew before it connected. Worth
            // saying: this used to print the same line as a push of twenty.
            (true, Some(_)) => format!("push: {branch} matches {url} — nothing to send"),
            // Never synced: what the remote holds is unknown until something is
            // fetched, and a local count would be a guess dressed as a fact.
            (true, None) => format!("push: {branch} -> {url}"),
        };
        // The notebook now holds a name meaning one thing here and another
        // everywhere else, and only its author can decide which keeps it.
        for name in held_back {
            let _ = write!(
                out,
                "\nsnapshot `{name}` was not sent — the remote already has that name for another \
                 commit; rename yours, or drop it with `git tag -d {name}`"
            );
        }
        Ok(out)
    }

    /// Pointing at whatever object the ref names, so it compares against a
    /// remote's advertisement without peeling either side.
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

    /// From the reference advertisement: one extra round trip, and only when
    /// there is a snapshot to send, so a notebook without any pushes as before.
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
            // `refs/tags/q3^{}` is the peeled form, not a name to push to.
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

    /// Commits, newest first; with `note_id`, only those that changed it.
    ///
    /// Renames are followed without rename detection: the id is in the filename,
    /// so the file a note occupied at any commit is whichever tree entry carried
    /// that prefix.
    pub fn log(&self, note_id: Option<&str>, max: Option<usize>) -> Result<Vec<Entry>> {
        let mut walk = self.repo.revwalk()?;
        walk.push_head()?;
        // noda commits several times a second, so time alone leaves commits
        // sharing a timestamp in arbitrary order.
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

    /// Which commit put each line of a note where it is.
    ///
    /// Not libgit2's blame, which could not do it: every
    /// `GIT_BLAME_TRACK_COPIES_*` option is "not yet implemented", so it stops
    /// dead at a rename — and `noda mv` renames a note on every retitle, which
    /// would credit every earlier line to the rename.
    ///
    /// Computed from the diffs instead, picking the note out of each commit *by
    /// id* as `deleted` and `last_changed` do, so a rename never comes up: what
    /// is followed backwards is a line, not a filename.
    ///
    /// `log`'s walk. A commit matching any parent changed nothing and is
    /// skipped, which keeps a `sync` merge from being credited with what it
    /// merely carried across; where a merge did change the note, the first
    /// parent is compared against.
    ///
    /// Body only: `updated` is rewritten on every edit, so the frontmatter would
    /// open the screen with a block of noise that looks like a bug.
    pub fn blame(&self, id: &str, slug: &str) -> Result<Vec<BlameLine>> {
        let text = std::fs::read_to_string(self.note_path(id, slug))?;
        let lines: Vec<&str> = text.lines().collect();
        // Where each traced line sits in the version under examination.
        let mut origin: Vec<Option<usize>> = (0..lines.len()).map(Some).collect();
        let mut found: Vec<Option<git2::Oid>> = vec![None; lines.len()];
        let mut when: HashMap<git2::Oid, (i64, i32)> = HashMap::new();

        // A line on disk and in no commit is nobody's yet.
        let head = self.repo.head()?.peel_to_commit()?;
        match note_blob(&head, id)? {
            Some((_, oid)) => {
                let blob = self.repo.find_blob(oid)?;
                let map = line_map(blob.content(), text.as_bytes())?;
                attribute(&mut origin, &mut found, &map, None);
            }
            // Held on disk and in no commit — hand-written, or not yet adopted.
            // There is no history to walk.
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
                // The commit that created the note: a diff against nothing.
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

    /// When each note last changed according to git, by id.
    ///
    /// One walk for the whole notebook: `log`'s per-note filter is right for one
    /// note and would multiply the walk by the notebook's size for all of them,
    /// so every commit is asked what it changed on the way past.
    ///
    /// Newest first, so the first commit mentioning a note is the last to touch
    /// it. A full walk of history, reached only through `doctor --times`.
    pub fn last_changed(&self) -> Result<HashMap<String, i64>> {
        let mut walk = self.repo.revwalk()?;
        walk.push_head()?;
        walk.set_sorting(git2::Sort::TIME | git2::Sort::TOPOLOGICAL)?;

        let mut last: HashMap<String, i64> = HashMap::new();
        for oid in walk {
            let commit = self.repo.find_commit(oid?)?;
            let now = note_blobs(&commit)?;
            // First parent, as `git log` and `touches` both do for a merge.
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

    /// Notes history holds that the notebook no longer does.
    ///
    /// A commit's tree is a complete list of filenames and a note's identity is
    /// its filename, so which notes existed at a commit is read straight off it
    /// without opening a blob: the ids in a tree, minus its parent's, against
    /// the ids held now.
    ///
    /// Three things fall out of using ids rather than filenames. A rename is not
    /// a deletion; a note deleted and later restored is not reported, the check
    /// being against disk rather than history; and a `git rm` is found like a
    /// `noda rm`, because nothing here reads a commit message.
    ///
    /// Newest first, so the first disappearance found for an id is its last. A
    /// full walk, reached only through `noda deleted`.
    pub fn deleted(&self) -> Result<Vec<Deleted>> {
        let present = self.taken_ids()?;
        let mut walk = self.repo.revwalk()?;
        walk.push_head()?;
        walk.set_sorting(git2::Sort::TIME | git2::Sort::TOPOLOGICAL)?;

        let mut found = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        for oid in walk {
            let commit = self.repo.find_commit(oid?)?;
            // First parent, as `log` and `last_changed`. A root commit deletes
            // nothing.
            let Ok(parent) = commit.parent(0) else {
                continue;
            };
            let now = note_blobs(&commit)?;
            for id in note_blobs(&parent)?.keys() {
                if now.contains_key(id) || present.contains(id) || !seen.insert(id.clone()) {
                    continue;
                }
                // The parent still had it: the name, and what `restore` wants.
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

        // The one you are looking for is nearly always the one just lost.
        found.sort_by(|a, b| {
            b.removed_at
                .cmp(&a.removed_at)
                .then_with(|| a.id.cmp(&b.id))
        });
        Ok(found)
    }

    /// Uncommitted changes, or what the last commit changed when there are
    /// none: noda commits as it goes, so clean is the normal state and "what
    /// just happened" is the useful answer.
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

        // Without rename detection, `noda mv` reads as a note deleted and an
        // unrelated one invented.
        diff.find_similar(None)?;
        Ok(diff)
    }

    /// What a push would carry: the third layer of what `status` counts and
    /// `log` enumerates.
    ///
    /// **Measured from where the histories parted**, `origin/main...HEAD` — the
    /// three-dot form a pull request shows. Two-dot would be wrong in a way that
    /// is hard to see: every line the remote added comes back as a line removed,
    /// because it is absent from your tree. Nobody removed it.
    ///
    /// So a notebook that is behind gets the same answer as one that is level.
    /// Committed work only, because a push would carry nothing else. Nothing
    /// goes to the network — the tracking ref is the one the last sync left.
    pub fn diff_remote(&self, branch: &str, file: Option<&str>) -> Result<git2::Diff<'_>> {
        let tracking = format!("refs/remotes/{REMOTE_NAME}/{branch}");
        let Ok(upstream) = self.repo.refname_to_id(&tracking) else {
            // A never-synced notebook differs by everything it holds, so "no
            // changes" would be the wrong answer that looks right.
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
        // `diff`'s reason: a renamed note is one note.
        diff.find_similar(None)?;
        Ok(diff)
    }

    /// Anything git accepts, and nothing invented on top.
    pub fn revision(&self, rev: &str) -> Result<git2::Commit<'_>> {
        let object = self
            .repo
            .revparse_single(rev)
            .map_err(|e| Error::msg(format!("unknown revision: {rev} — {}", e.message())))?;
        // One way to fail — a blob or a tree — and the message says it.
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

    /// The text of a blob already in the object database.
    ///
    /// The web layer's optimistic lock carries a blob id, so the version an
    /// edit began from is not merely a marker to compare — it is an address,
    /// and this is what reads it back.
    ///
    /// **`None` is an ordinary answer, not a failure.** A note written by hand
    /// and not yet committed has no blob, so a caller has to have an answer for
    /// a version it cannot fetch. Bytes that are not UTF-8 come back `None` for
    /// the same reason: unusable as a base, whatever the cause.
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

    /// Undoes a conflicted merge, leaving the notebook exactly as it was.
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

/// The notebook every command acts on by default. A missing state pointer falls
/// back to the configured default: state records where you are, config records
/// where you belong. In one place because every command must agree.
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

/// Who to commit as and whether to sign, read together because a notebook is
/// opened once per command and the file should be too. A malformed author is
/// `noda config`'s to complain about — a commit is not the place to find out.
fn commit_settings(paths: &Paths) -> (Option<(String, String)>, Option<bool>) {
    let Ok(config) = Config::load(paths) else {
        return (None, None);
    };
    let author = config.get("author").and_then(config::author_parts);
    (author, config.sign())
}

/// Against the first parent, as `git log` does for merges.
fn touches(commit: &git2::Commit<'_>, id: &str) -> Result<bool> {
    let now = note_blob(commit, id)?;
    let before = match commit.parent(0) {
        Ok(parent) => note_blob(&parent, id)?,
        Err(_) => None,
    };
    // Path as well as content, or a rename goes unnoticed.
    Ok(now != before)
}

/// The file a note occupied at `commit` and the blob it held. Every commit
/// records the filenames, which is why following a note across a rename needs
/// neither rename detection nor a committed map.
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

/// `note_blob` for a whole tree at once. The path as well as the content,
/// because a rename moves a note without changing a byte of it.
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

/// libgit2 hardcodes `master`, so without this a notebook disagrees with every
/// other repository on a machine that sets `init.defaultBranch`, and pushing it
/// leaves two branches where one was asked for. The fallback matches
/// `git init`'s.
fn initial_branch(config: &git2::Config) -> String {
    config
        .get_string("init.defaultBranch")
        .ok()
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "master".to_string())
}

/// `1 commit` / `3 commits`, for the counts `push` and `pull` report.
fn plural(n: usize, thing: &str) -> String {
    if n == 1 {
        format!("1 {thing}")
    } else {
        format!("{n} {thing}s")
    }
}

/// A refused push, phrased as the next thing to do about it.
fn rejected(reasons: &[String]) -> Error {
    Error::msg(format!(
        "push rejected — {}\nthe remote has commits you do not: run `noda pull` first",
        reasons.join("; ")
    ))
}

/// Moves every traced line one version back and settles the ones that go no
/// further: a line with a counterpart carries on with the number it had there,
/// and a line with none is answered by the commit under examination — `None` for
/// the working tree, which belongs to nobody.
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

/// For each line of `new`, the line of `old` it came from.
///
/// The context is set past both lengths on purpose, making the whole file one
/// hunk so the correspondence comes out complete rather than only near changes.
fn line_map(old: &[u8], new: &[u8]) -> Result<Vec<Option<usize>>> {
    let count = line_count(new);
    // libgit2 emits no hunks when the sides are equal, which the loop below
    // cannot tell apart from a file created whole.
    if old == new {
        return Ok((0..count).map(Some).collect());
    }

    let mut options = git2::DiffOptions::new();
    options
        .context_lines(u32::try_from(count + line_count(old)).unwrap_or(u32::MAX))
        // A stray byte calling a note binary would leave no lines to map.
        .force_text(true);
    let path = Path::new("note");
    let patch = git2::Patch::from_buffers(old, Some(path), new, Some(path), Some(&mut options))?;

    let mut map = vec![None; count];
    for hunk in 0..patch.num_hunks() {
        for index in 0..patch.num_lines_in_hunk(hunk)? {
            let line = patch.line_in_hunk(hunk, index)?;
            // Everything else is the old side or a "\ No newline" marker.
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

/// Matches `str::lines`, which is what the reported text is split with.
fn line_count(bytes: &[u8]) -> usize {
    bytes.split_inclusive(|byte| *byte == b'\n').count()
}

/// Past the frontmatter and its blank line; zero when there is none.
fn body_start(text: &str) -> usize {
    let Some((_, body)) = note::split_frontmatter(text) else {
        return 0;
    };
    // `Note::parse` trims the same newlines, so "the body" means one thing.
    let body = body.trim_start_matches('\n');
    text[..text.len() - body.len()].lines().count()
}

/// The id out of a destination's filename, folded as `resolve` folds one. A
/// destination into a subdirectory is some other file — only the root holds
/// notes, the boundary `audit_links` draws for orphans.
pub fn linked_note_id(target: &str) -> Option<String> {
    if target.contains('/') {
        return None;
    }
    let (id, _) = note::split_stem(target.strip_suffix(".md")?)?;
    Some(note::normalize_id(id))
}

/// Every tag these notes carry, commonest first, with how many carry it.
///
/// A free function so a caller already holding the notes does not walk the
/// directory again. **The order is here rather than in whoever draws it**:
/// sorted by name alone, the four tags a notebook runs on are buried under every
/// one-off ever typed, and two screens in two orders read as a bug.
/// Alphabetical within a count, so it does not reshuffle between visits.
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

/// Whether this note's body links to the note `id` names.
///
/// A free function so the browser, which already holds every note, does not walk
/// the directory again — and so the test is not written a second time there.
/// [`Notebook::backlinks_to_note`] is the walk that feeds it from disk.
///
/// `targets` is a set, so three links to one place are one backlink.
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

/// The executable bit, which is the whole of what git looks at. Elsewhere there
/// is no bit, so the name is taken at its word rather than every hook declared
/// dead.
#[cfg(unix)]
fn is_executable(metadata: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;

    metadata.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn is_executable(_: &std::fs::Metadata) -> bool {
    true
}

/// Now, and how far the machine's clock sits from UTC.
///
/// Asked of libgit2 because noda has no timezone database — jiff is compiled
/// without one, and bundling one to answer "what day is it here" is a large
/// dependency for a small question. libgit2 takes the offset from the C library,
/// which is the *same* source every timestamp noda prints comes from: a date
/// compared against today has to mean the same "here" as a rendered commit.
///
/// The identity is thrown away; `Signature::now` needs one and the clock does
/// not care which.
pub fn local_now() -> Result<(i64, i32)> {
    let when = Signature::now("noda", "noda@localhost")?.when();
    Ok((when.seconds(), when.offset_minutes()))
}

/// An abbreviated object id, as git prints it.
pub(crate) fn short(oid: git2::Oid) -> String {
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

    /// An in-memory config has no backend to write to.
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

    /// The filenames differ, so git merges them without a word.
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
        // `resolve` folds case and the I/L/O confusables.
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
        // However many, it is one kind: counted, not enumerated.
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

    /// `initial_head("")` would leave the repository naming no branch at all.
    #[test]
    fn a_blank_default_branch_falls_back_rather_than_naming_nothing() {
        let mut config = TempConfig::new();
        config.set("   ");
        assert_eq!(initial_branch(&config.1), "master");
    }
}
