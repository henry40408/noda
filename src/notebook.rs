//! A notebook is a git repository of Markdown files. Every mutation is a commit.
//!
//! A note's identity is its filename: `<id>-<slug>.md`. Nothing derived is
//! committed alongside the notes, so there is no bookkeeping file to conflict on
//! and nothing that can fall out of step with what the files say. Two machines
//! that each add a note write two different filenames, and git merges them
//! without anyone being asked to resolve anything.

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

/// How `resolve` says a key matched nothing. Named because `cmd::path` widens
/// exactly this case — it was asked about a file too — and passes every other
/// failure through untouched.
pub const NOT_FOUND: &str = "note not found";

/// The file a git host renders on the notebook's front page, written by `noda
/// readme`. Spelled exactly, because that is the one spelling noda writes and a
/// wider match would start excusing files nobody meant as a front page.
pub const README_FILE: &str = "README.md";

pub struct Notebook {
    pub name: String,
    pub path: PathBuf,
    repo: Repository,
    /// Who to commit as, when `config.toml` says. Resolved on open, because a
    /// notebook is opened once per command and read many times.
    author: Option<(String, String)>,
    /// Whether to sign, when `config.toml` says; `None` leaves the answer to
    /// git's `commit.gpgsign`. Read on open like `author`, but *resolved* at the
    /// commit — a misconfigured `gpg.format` is an error for `noda add` to
    /// report, not something that should stop `noda ls` from listing.
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
    /// Files the notebook holds that are not notes. Free to count: the walk that
    /// finds the notes passes them anyway.
    pub files: usize,
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
    /// Everything else the notebook holds: an attachment, a README, a file
    /// parked here on purpose.
    ///
    /// Counting them costs nothing — the walk that finds the notes passes them
    /// anyway. Saying which note *uses* one is the expensive question, because
    /// under this model the answer lives in the notes' prose; that is
    /// `audit_links`, and it is why nothing here needs it.
    pub files: Vec<String>,
}

/// What following every link in every note turned up.
///
/// Deliberately not part of `Scan`: building it reads and parses the body of
/// every note, so `status` must never reach for it.
pub struct Audit {
    /// Files in the notebook that no note links to, except the `README_FILE`,
    /// which is addressed to a reader outside the notebook rather than linked
    /// from inside it.
    pub orphans: Vec<String>,
    /// `(note filename, destination, the note that destination still names)`
    /// where the file is gone but its id is one the notebook still holds — a
    /// note that was retitled after something linked to it.
    pub stale: Vec<(String, String, String)>,
    /// `(note filename, destination)` where the destination names nothing the
    /// notebook holds, and nothing says what it was meant to name.
    pub broken: Vec<(String, String)>,
}

/// A note the notebook used to hold, as it stood the moment before it went.
///
/// The name and title are read from the commit that still had it: there is no
/// file left to read them from, which is the whole reason this exists.
pub struct Deleted {
    pub id: String,
    pub slug: String,
    pub title: String,
    /// The commit that removed it.
    pub removed_in: git2::Oid,
    /// Its parent: the last commit that still held the note, and so the one
    /// `noda restore` has to be pointed at.
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

/// One snapshot — a git tag — as `noda snapshot` lists it.
pub struct Snapshot {
    pub name: String,
    /// The commit it marks, not the tag object: what gets cited is the state of
    /// the notebook, and that is what `restore` resolves a snapshot name to.
    pub target: git2::Oid,
    /// The marked commit's time and offset, as `Entry` carries them and for the
    /// same reason.
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
    /// The commit's time and offset, as `Entry` carries them. Both zero for a
    /// line nobody has committed — there is no moment to report.
    pub seconds: i64,
    pub offset_minutes: i32,
    pub text: String,
}

impl BlameLine {
    /// The commit, abbreviated — or git's own spelling for a line that is not in
    /// one yet.
    pub fn short_commit(&self) -> String {
        self.commit.map_or_else(|| "0".repeat(7), short)
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

    /// `(ahead, behind)` against what the last fetch left behind, or `None` when
    /// there has never been one.
    ///
    /// Split out of `status` because it is the cheap half. `status` also walks
    /// the working tree twice over — once for the notes and once for a full
    /// `git status` — and a caller that only wants to say "2 to push" should not
    /// pay for either. Nothing here goes to the network: this is a comparison of
    /// two refs that are already on the disk.
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

    /// When the notebook was last written to — `(seconds, offset_minutes)`, the
    /// pair `Entry` carries, so the caller renders it through `cmd::format_time`
    /// like every other stamp noda prints.
    ///
    /// One commit read rather than a walk, which is what makes it worth showing
    /// on a page that already lists every notebook: `status` is the expensive
    /// half of that page and this must not add to it.
    ///
    /// It is deliberately not part of `Status`. `noda status` does not report a
    /// last-commit day, and a field on that struct would either change what the
    /// command prints or sit there unread.
    pub fn last_commit(&self) -> Result<(i64, i32)> {
        let commit = self.repo.head()?.peel_to_commit()?;
        Ok((commit.time().seconds(), commit.time().offset_minutes()))
    }

    /// Walks the working tree and sorts every `*.md` into the four cases.
    ///
    /// Tolerant where `notes` is strict: one malformed file must not stop the
    /// notebook being described.
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
            // A name that is not UTF-8 cannot be compared against a link
            // destination, which always is; and a dotfile is the repository's
            // own configuration rather than anything the notebook holds.
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
                // Neither a name nor a declaration: not a note, so it is one
                // more file the notebook happens to hold.
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

    /// Follows every link in every note, and reports both directions in which a
    /// link and a file can fail to meet.
    ///
    /// This is the expensive walk: it reads and parses the body of every note,
    /// which is the cost of `search` rather than the cost of `ls`. Nothing calls
    /// it unless asked to.
    ///
    /// A link is checked against the filesystem rather than against the scan, so
    /// a destination reaching into a subdirectory resolves. The reverse does not
    /// hold — only files at the notebook's root can be reported as orphans,
    /// because the root is the whole of the notebook noda models.
    ///
    /// A destination that resolves to nothing on disk is split in two, because
    /// only one of the halves is a question for the author. `noda mv` moves the
    /// slug half of a filename, so `[the meeting](v62b8rfa-meeting-notes.md)`
    /// can name a path that is gone and still name `v62b8rfa`, which is still
    /// exactly one note: **stale**, and noda knows what it should have said. A
    /// destination naming no id the notebook holds is **broken**, and only its
    /// author knows whether it is a typo or a file not copied in yet. The same
    /// distinction `backlinks_to_note` is built on, reported rather than acted
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

        // The README is the notebook's own entrance, not a resource a note was
        // supposed to link to. Reporting it would put a line in every `--links`
        // run that the author can never clear, since the only way to clear it is
        // to link the front page from a note — which reads backwards.
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
    /// Matched on the id in the destination rather than on the whole filename,
    /// which is what makes an answer survive a retitle. `noda mv` moves the slug
    /// half of a note's filename and says nothing to the notes that linked to
    /// it, so `[the meeting](v62b8rfa-meeting-notes.md)` is left naming a path
    /// that no longer exists — and yet it still names `v62b8rfa`, which is still
    /// exactly one note. Every Markdown renderer sees a broken link there; noda
    /// sees an unambiguous one, because the id is the half that never moves.
    ///
    /// Matching on the filename instead would be the easier build and the wrong
    /// feature: backlinks would go quiet after every retitle, which is precisely
    /// when somebody is looking for what points at a note.
    ///
    /// A note that links to itself is listed. It is what the file says, and
    /// leaving it out would be noda deciding the author did not mean it.
    pub fn backlinks_to_note(&self, id: &str) -> Result<Vec<NoteFile>> {
        Ok(self
            .notes()?
            .into_iter()
            .filter(|file| links_to_note(&file.note, id))
            .collect())
    }

    /// The notes whose bodies link to one of the notebook's files, by name.
    ///
    /// No id to fall back on here: an attachment's name is the whole of its
    /// identity, which is why `file mv` has to offer `--update-links` at the
    /// moment it renames one.
    pub fn backlinks_to_file(&self, name: &str) -> Result<Vec<NoteFile>> {
        Ok(self
            .notes()?
            .into_iter()
            .filter(|file| links_to_file(&file.note, name))
            .collect())
    }

    /// The hooks the repository holds that will never fire.
    ///
    /// noda carries its own libgit2 rather than calling `git`, and libgit2 runs
    /// no hooks at all. The same `pre-commit` is therefore live under
    /// `git commit` and dead under `noda add`, with nothing on screen to say
    /// which — that silence is the only reason this is reported.
    ///
    /// Exactly the set git itself would reach for: `core.hooksPath` when it is
    /// set, the executable bit because that is what git checks, and never the
    /// `*.sample` files a fresh repository ships, which were not going to run
    /// under either.
    ///
    /// A directory that cannot be read is not a finding. Nothing here is a
    /// problem with the notebook, so an unreadable one has nothing to report.
    pub fn hooks(&self) -> Result<Vec<String>> {
        let dir = match self.repo.config()?.get_path("core.hooksPath") {
            // A relative path is taken from the working tree, as git takes it.
            // An absolute one replaces the join outright.
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
            // Followed rather than skipped: a symlinked hook is a hook, and
            // `metadata` resolves the link where `file_type` does not.
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

    /// Every `(id, slug)` a filename in the notebook spells out, whether or not
    /// the file behind it can be read.
    ///
    /// The name is the whole record, so this opens nothing — and the file type
    /// comes back with the directory entry, so it does not `stat` either. It is deliberately
    /// more forgiving than `scan`: a note whose frontmatter has gone still says
    /// which note it is, and the commands that only need to know *which* — `rm`,
    /// `log`, `diff`, `restore` — must keep working on exactly the file someone
    /// is reaching for those commands to fix.
    /// Public for `web`, which needs the same thing for the same reason: to
    /// turn `[the plan](k3f9m2p1-the-plan.md)` in a body into a link to that
    /// note, it has to know which filenames are notes — and rendering one note
    /// is not a reason to open and parse every other one.
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

    /// The configured remote, if there is one whose URL is valid UTF-8, with any
    /// credentials in it redacted.
    ///
    /// Redacted here and not at each screen, because there are five screens and
    /// the sixth one added later would be a leak nobody would think to look for.
    /// Nothing that talks to the network reads this: `remote()` is what fetch
    /// and push open, and it carries the URL as it was configured.
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

    /// Every adopted note, sorted by slug, read from the working tree.
    /// Reads each file once. Going through `scan` would parse the whole notebook
    /// to decide which files are notes and then parse it again to read them,
    /// which is the dominant cost of `ls` and `search` — both open every note.
    /// A file that will not parse is not a note and is skipped here, exactly as
    /// `scan` classifies it.
    pub fn notes(&self) -> Result<Vec<NoteFile>> {
        Ok(self.inventory()?.0)
    }

    /// Every note the notebook holds, and every file it holds that is not one,
    /// from a single walk.
    ///
    /// `ls` wants both, and reading the directory twice to answer one command is
    /// what this exists to avoid. The classification is `scan`'s, so a file is
    /// counted here exactly as it is reported there: the `*.md` that declare
    /// themselves and carry an id are notes, the ones still waiting to be
    /// adopted or missing their frontmatter are neither notes nor files —
    /// `scan` already reports them as problems, and listing them as attachments
    /// would name them twice.
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
                // Named but unreadable, or readable but unnamed: a problem for
                // `scan` to report, and not a file the notebook is holding.
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
    /// An exact slug is tried first, then an id prefix — the same bargain git
    /// makes with object ids, so `noda show k3f9` keeps working while the id
    /// itself is long enough not to need policing. An ambiguous key is an error
    /// naming the candidates, never a guess.
    ///
    /// Resolution reads no file. Whether the note behind the name can be parsed
    /// is the caller's problem, and only some callers have it.
    ///
    /// The directory is walked once and only matches are kept: on the notebook
    /// sizes this has to be quick at, building a list of every name first costs
    /// more than the comparison it feeds.
    pub fn resolve(&self, key: &str) -> Result<(String, String)> {
        if key.is_empty() || key.contains('/') || key.contains('\\') || key.contains("..") {
            return Err(Error::msg(format!("invalid note reference: {key}")));
        }
        let wanted = note::normalize_id(key);

        // An exact slug wins outright, so an id prefix that happens to match as
        // well never gets a say — collected separately rather than sorted out
        // afterwards.
        let mut by_slug = Vec::new();
        let mut by_id = Vec::new();
        for entry in std::fs::read_dir(&self.path)? {
            let entry = entry?;
            // `file_type` comes back with the directory entry on every platform
            // noda ships to; `Path::is_file` would be a `stat` per candidate, and
            // there is one candidate per note.
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
        // Directory order is whatever the filesystem hands back; the candidate
        // list a person has to choose from must not be.
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
        let parent = match self.repo.head() {
            Ok(head) => Some(head.peel_to_commit()?),
            Err(_) => None,
        };
        let parents: Vec<&git2::Commit> = parent.iter().collect();
        self.write_commit(message, &tree, &parents)?;
        Ok(())
    }

    /// Writes the commit, GPG-signing it when the configuration asks, and moves
    /// `HEAD` onto it.
    ///
    /// The unsigned path is libgit2's one-call `commit`. The signed one cannot
    /// be: signing needs the commit's text *before* it is an object, so the
    /// buffer is built, handed to gpg, and written back with the signature —
    /// three steps where there was one, and the last of them does not move the
    /// branch, which is why [`Self::move_head`] exists.
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
        // A commit object is UTF-8 by the time noda has built it: the message is
        // a Rust `&str` and so is every name and email in the signature.
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

    /// Points whatever `HEAD` names at `oid`.
    ///
    /// `commit_signed` writes an object and stops there — unlike `commit`, which
    /// also moves the branch. Without this the notebook would gain a commit that
    /// nothing refers to, and the next `gc` would collect the note with it.
    ///
    /// Symbolic when there is a branch, direct when `HEAD` is detached; the
    /// unborn `HEAD` of a just-created notebook is symbolic too, which is what
    /// makes the root commit land on the branch `init.defaultBranch` named.
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
        // No repository to read a config from yet, so the global and system
        // files are all there is — which is what `git clone` itself works from.
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

    /// Marks the current commit with an annotated tag.
    ///
    /// Annotated rather than lightweight because the point of a snapshot is that
    /// somebody closed a chapter at a particular moment: a lightweight tag is a
    /// bare pointer with nobody and no time attached, and would list as an empty
    /// row.
    ///
    /// Never moves one that already exists. A snapshot whose meaning can be
    /// reassigned is not something anything else can cite — and `restore` takes
    /// a snapshot name precisely so it can be cited.
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

    /// Every snapshot, newest first — by the time of the commit each marks
    /// rather than the time it was taken, because that is the moment the
    /// snapshot is *of*, and it is what `log` and `deleted` are ordered by.
    ///
    /// A lightweight tag made with git outside noda is listed too. noda does not
    /// make them, but a notebook is a normal git repository and a tag somebody
    /// else put there is still a place to restore from.
    pub fn snapshots(&self) -> Result<Vec<Snapshot>> {
        let mut found = Vec::new();
        // A tag whose name is not UTF-8 is skipped rather than refused: it is
        // not one noda made, and one unreadable name must not take the listing
        // down with it.
        let names = self.repo.tag_names(None)?;
        for name in names.iter().filter_map(|name| name.ok().flatten()) {
            let reference = self.repo.find_reference(&format!("refs/tags/{name}"))?;
            let commit = reference.peel_to_commit()?;
            // An annotated tag carries its own message; a lightweight one is
            // only a pointer, so the commit it marks has to speak for it.
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

        let config = self.repo.config()?;
        // Tags come down with the branch, or a snapshot taken on the other
        // machine would be invisible here — and `noda restore <note> <snapshot>`
        // would fail on a name the notebook is meant to share.
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
        let ours = self.repo.head()?.peel_to_commit()?;
        let theirs = self.repo.find_commit(incoming)?;
        self.write_commit(
            &format!("merge: {REMOTE_NAME}/{branch}"),
            &tree,
            &[&ours, &theirs],
        )?;
        self.repo.cleanup_state()?;
        Ok(format!("pull: merged {REMOTE_NAME}/{branch}"))
    }

    /// Pushes the current branch, and the snapshots the remote does not have. A
    /// rejection is reported as advice to pull rather than as a libgit2 error,
    /// because that is always the next step.
    pub fn push(&self) -> Result<String> {
        let branch = self.branch()?;
        let mut remote = self.remote()?;
        let url = remote.url().unwrap_or_default().to_string();
        // Snapshots go with the branch. One that stayed on the machine it was
        // taken on could not be cited from anywhere else, which is most of what
        // a snapshot is for.
        //
        // Named one by one rather than as `refs/tags/*`: libgit2 refuses a
        // wildcard on the push side, because it wants references it can resolve
        // and a glob is not one.
        let mut refspecs = vec![format!("refs/heads/{branch}:refs/heads/{branch}")];
        let mut held_back = Vec::new();
        let local = self.local_tags()?;
        if !local.is_empty() {
            let theirs = self.remote_tags(&mut remote, &url)?;
            for (name, oid) in local {
                match theirs.get(&name) {
                    // Already there, and meaning the same thing.
                    Some(other) if *other == oid => {}
                    // Two machines that each made a `q3`. Sending it would
                    // either overwrite theirs or — since libgit2 checks a tag
                    // for fast-forward as if it were a branch — abort the whole
                    // push, taking the notes down with it. Neither is a trade
                    // worth making for a name, so the name is what gives way.
                    Some(_) => held_back.push(name),
                    None => refspecs.push(format!("refs/tags/{name}:refs/tags/{name}")),
                }
            }
        }

        // The callbacks borrow `rejections`, so they have to be dropped before it
        // can be read back — hence the block.
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

        let mut out = format!("push: {branch} -> {url}");
        // Said out loud rather than swallowed: the notebook now holds a snapshot
        // name that means one thing here and another everywhere else, and only
        // its author can decide which one keeps the name.
        for name in held_back {
            let _ = write!(
                out,
                "\nsnapshot `{name}` was not sent — the remote already has that name for another \
                 commit; rename yours, or drop it with `git tag -d {name}`"
            );
        }
        Ok(out)
    }

    /// Every tag the notebook holds, by name, pointing at whatever object the
    /// ref names — the tag object for an annotated tag, so it compares against
    /// what a remote advertises without peeling either side.
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

    /// What the remote already carries under `refs/tags/`, from the reference
    /// advertisement — one extra round trip, and only when the notebook has a
    /// snapshot to send at all, so a notebook without any pushes exactly as it
    /// did before.
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
            // `refs/tags/q3^{}` is the same tag with the commit it resolves to,
            // advertised alongside the tag itself. It is not a name anything can
            // be pushed to.
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

    /// Which commit put each line of a note where it is.
    ///
    /// libgit2's own blame is not used, and could not be: every one of its
    /// `GIT_BLAME_TRACK_COPIES_*` options is documented as "not yet implemented",
    /// so it follows a path and stops dead at a rename. `noda mv` renames a note
    /// whenever its title changes, which would make a wrong answer the normal one
    /// — and wrong in the way that is hardest to notice, since every line before
    /// the rename would be credited to the rename.
    ///
    /// So this is computed from the diffs instead, which costs nothing extra
    /// here: the note is picked out of each commit *by id* rather than by path,
    /// exactly as `deleted` and `last_changed` do it, and a rename simply never
    /// comes up. What is followed backwards is a line, not a filename.
    ///
    /// The walk is the one `log` uses. A commit whose version of the note matches
    /// any of its parents' changed nothing about it and is skipped, which is what
    /// keeps a `sync` merge from being credited with the note it merely carried
    /// across. Where a merge did change the note — which noda itself never
    /// produces, because a conflicting pull is rolled back — the first parent is
    /// compared against, the same simplification `log` and `last_changed` make.
    ///
    /// Only the body is reported. `updated` is rewritten on every edit, so every
    /// frontmatter line would be credited to the latest commit and the first
    /// thing on screen would be a block of noise that looks like a bug.
    pub fn blame(&self, id: &str, slug: &str) -> Result<Vec<BlameLine>> {
        let text = std::fs::read_to_string(self.note_path(id, slug))?;
        let lines: Vec<&str> = text.lines().collect();
        // Where each line still being traced sits in the version under
        // examination; `None` once the line has been accounted for.
        let mut origin: Vec<Option<usize>> = (0..lines.len()).map(Some).collect();
        let mut found: Vec<Option<git2::Oid>> = vec![None; lines.len()];
        let mut when: HashMap<git2::Oid, (i64, i32)> = HashMap::new();

        // The working tree first. A line that is on disk and in no commit is
        // nobody's yet, and saying so is nearly free once the machinery is here.
        let head = self.repo.head()?.peel_to_commit()?;
        match note_blob(&head, id)? {
            Some((_, oid)) => {
                let blob = self.repo.find_blob(oid)?;
                let map = line_map(blob.content(), text.as_bytes())?;
                attribute(&mut origin, &mut found, &map, None);
            }
            // The notebook holds the file and no commit does — a note written by
            // hand, or one `doctor` has not adopted yet. Every line is somebody's
            // and nobody's, and there is no history to walk for it.
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
                // The commit that created the note: everything in it is new,
                // which is what a diff against nothing says.
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

    /// When each note last changed according to git, by id, as a Unix time.
    ///
    /// One walk for the whole notebook rather than one per note. `log` filters a
    /// walk down to a single note, which is the right shape when the question is
    /// about one; asking it once per note would multiply the walk by the size of
    /// the notebook. Here every commit is asked what it changed instead, and the
    /// answers are collected on the way past.
    ///
    /// Newest first, so the first commit that mentions a note is the last one
    /// that touched it. A note git has never seen is simply absent.
    ///
    /// This is a full walk of history and is only reached through
    /// `doctor --times`.
    pub fn last_changed(&self) -> Result<HashMap<String, i64>> {
        let mut walk = self.repo.revwalk()?;
        walk.push_head()?;
        walk.set_sorting(git2::Sort::TIME | git2::Sort::TOPOLOGICAL)?;

        let mut last: HashMap<String, i64> = HashMap::new();
        for oid in walk {
            let commit = self.repo.find_commit(oid?)?;
            let now = note_blobs(&commit)?;
            // Against the first parent, the same simplification `git log` makes
            // for a merge — and the same one `touches` already makes.
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
    /// The tree of a commit is a complete list of filenames, not a diff, and a
    /// note's identity is its filename — so which notes existed at a commit is
    /// read straight off it, without opening a single blob. That is the whole
    /// mechanism: the ids in a commit's tree, minus the ids in its parent's,
    /// against the ids the notebook holds now.
    ///
    /// Three things fall out of using ids rather than filenames. A rename is not
    /// a deletion, because `mv` changes the slug and leaves the id where it was.
    /// A note deleted and later restored is not reported, because the check is
    /// against what is on disk now rather than against what history did. And a
    /// deletion made outside noda is found like any other, because nothing here
    /// reads a commit message — `git rm` and `noda rm` leave the same trace in
    /// the tree, which is the only place this looks.
    ///
    /// Newest first, so the first disappearance found for an id is its last one.
    /// A full walk of history, reached only through `noda deleted`.
    pub fn deleted(&self) -> Result<Vec<Deleted>> {
        let present = self.taken_ids()?;
        let mut walk = self.repo.revwalk()?;
        walk.push_head()?;
        walk.set_sorting(git2::Sort::TIME | git2::Sort::TOPOLOGICAL)?;

        let mut found = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        for oid in walk {
            let commit = self.repo.find_commit(oid?)?;
            // Against the first parent, as `log` and `last_changed` both do. A
            // root commit deletes nothing.
            let Ok(parent) = commit.parent(0) else {
                continue;
            };
            let now = note_blobs(&commit)?;
            for id in note_blobs(&parent)?.keys() {
                if now.contains_key(id) || present.contains(id) || !seen.insert(id.clone()) {
                    continue;
                }
                // The parent still had it, which is both where the name comes
                // from and what `restore` should be pointed at.
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

        // Most recently lost first: the one you are looking for is nearly always
        // the one you just lost.
        found.sort_by(|a, b| {
            b.removed_at
                .cmp(&a.removed_at)
                .then_with(|| a.id.cmp(&b.id))
        });
        Ok(found)
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

/// What `config.toml` has to say about committing: who as, and whether signed.
/// Read together because a notebook is opened once per command and the file
/// should be too.
///
/// A malformed author is left to `noda config` to complain about — a commit is
/// not the place to discover it.
fn commit_settings(paths: &Paths) -> (Option<(String, String)>, Option<bool>) {
    let Ok(config) = Config::load(paths) else {
        return (None, None);
    };
    let author = config.get("author").and_then(config::author_parts);
    (author, config.sign())
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

/// Every note a commit's tree holds, by id, with the path and blob that
/// `note_blob` would have returned for it one note at a time.
///
/// Comparing the path as well as the content is what catches a rename, which
/// moves a note without changing a byte of it.
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

/// Moves every line still being traced one version further back, and settles the
/// ones that go no further.
///
/// A line with a counterpart in the older version was not written by this
/// commit; it carries on with the number it had there. A line with none is where
/// the search ends, and the commit under examination is the answer — `None` for
/// the working tree, which is not a commit and belongs to nobody.
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

/// For each line of `new`, the line of `old` it came from — `None` where it is
/// new here.
///
/// The context is set past the length of both sides on purpose: it makes the
/// whole file one hunk, so every unchanged line is reported rather than only
/// those near a change, and the correspondence comes out complete. Diffing a few
/// kilobytes of prose this way costs nothing worth saving.
fn line_map(old: &[u8], new: &[u8]) -> Result<Vec<Option<usize>>> {
    let count = line_count(new);
    // libgit2 emits no hunks at all when the two sides are equal, which would
    // read as "every line is new" — the one case the loop below cannot tell
    // apart from a file created whole.
    if old == new {
        return Ok((0..count).map(Some).collect());
    }

    let mut options = git2::DiffOptions::new();
    options
        .context_lines(u32::try_from(count + line_count(old)).unwrap_or(u32::MAX))
        // A note is prose. If a stray byte makes libgit2 call it binary there
        // are no lines to map, and every line would be credited to whichever
        // commit was being examined.
        .force_text(true);
    let path = Path::new("note");
    let patch = git2::Patch::from_buffers(old, Some(path), new, Some(path), Some(&mut options))?;

    let mut map = vec![None; count];
    for hunk in 0..patch.num_hunks() {
        for index in 0..patch.num_lines_in_hunk(hunk)? {
            let line = patch.line_in_hunk(hunk, index)?;
            // ` ` is context and `+` is an addition; everything else is either
            // the old side or one of the "\ No newline" markers, and neither
            // names a line of `new`.
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

/// Lines the way git counts them: a trailing line without a newline still
/// counts, and an empty file has none. Matches `str::lines`, which is what the
/// text being reported is split with.
fn line_count(bytes: &[u8]) -> usize {
    bytes.split_inclusive(|byte| *byte == b'\n').count()
}

/// The line the body starts on, past the frontmatter and the blank line after
/// it. Zero when there is no frontmatter to skip.
fn body_start(text: &str) -> usize {
    let Some((_, body)) = note::split_frontmatter(text) else {
        return 0;
    };
    // `Note::parse` trims the same leading newlines, so "the body" means one
    // thing across noda rather than one thing here and another there.
    let body = body.trim_start_matches('\n');
    text[..text.len() - body.len()].lines().count()
}

/// The note a link destination names, if it names one — the id out of its
/// filename, folded the way `resolve` folds one.
///
/// A destination reaching into a subdirectory is some other file: only the
/// notebook's root holds notes, which is the same boundary `audit_links` draws
/// when it decides what can be an orphan.
pub fn linked_note_id(target: &str) -> Option<String> {
    if target.contains('/') {
        return None;
    }
    let (id, _) = note::split_stem(target.strip_suffix(".md")?)?;
    Some(note::normalize_id(id))
}

/// Every tag these notes carry, commonest first, with how many carry it.
///
/// A free function for the reason the two below are: the browser already holds
/// the notes and the web has just read them, and neither should walk a
/// directory again to count what is in its hand.
///
/// **The order is here rather than in whoever draws it.** Commonest first is a
/// judgement about what a tag list is for — sorted by name alone, the four tags
/// a notebook actually runs on are buried under every one-off ever typed — and
/// two screens showing the same list in two orders would read as a bug in
/// whichever was opened second. Alphabetical within a count, so it does not
/// reshuffle between visits.
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
/// A free function rather than a method, and this is the reason: the browser
/// holds every note in memory for the length of a session, so asking it to walk
/// the directory again to answer "what links here" would be reading the notebook
/// twice — and writing the test itself a second time in the browser is how two
/// answers to one question get into a program. This is the answer;
/// [`Notebook::backlinks_to_note`] is the walk that feeds it from disk.
///
/// `targets` is a set, so a note that links to the same place three times is one
/// backlink rather than three.
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

/// Whether git would run this file: the executable bit, which is the whole of
/// what git looks at. Elsewhere there is no bit to look at, so the name is taken
/// at its word rather than every hook being declared dead.
#[cfg(unix)]
fn is_executable(metadata: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;

    metadata.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn is_executable(_: &std::fs::Metadata) -> bool {
    true
}

/// Now, and how far the machine's clock sits from UTC — `(seconds, offset)`,
/// the same pair every commit carries.
///
/// Asked of libgit2 rather than worked out here, because noda has no timezone
/// database: jiff is compiled without one on purpose, and bundling one to answer
/// "what day is it here" would be a large dependency for a small question.
/// libgit2 gets the offset from the C library, which is where `git commit` gets
/// the one it stamps on every commit.
///
/// So this is not a workaround — it is the *same* source every timestamp noda
/// prints already comes from. `format_time` renders a commit in the zone it was
/// made in; a date compared against today has to mean the same "here", or the
/// two would disagree on screen.
///
/// The identity is thrown away. `Signature::now` needs one and the clock does
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
            files: Vec::new(),
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
