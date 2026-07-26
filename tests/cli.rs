//! End-to-end tests for the command layer. Each test gets its own XDG root, so
//! nothing here reads or writes the developer's real notebooks.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use noda::cmd;
use noda::paths::Paths;

/// A self-deleting directory; enough for tests without adding a dev-dependency.
struct TempRoot(PathBuf);

impl TempRoot {
    fn new() -> Self {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("noda-test-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&path).expect("create temp root");
        TempRoot(path)
    }

    fn paths(&self) -> Paths {
        Paths::rooted(&self.0)
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn initialized() -> (TempRoot, Paths) {
    let root = TempRoot::new();
    let paths = root.paths();
    cmd::init(&paths).expect("init");
    (root, paths)
}

fn commit_count(notebook: &Path) -> usize {
    let repo = git2::Repository::open(notebook).expect("open repo");
    let mut walk = repo.revwalk().expect("revwalk");
    walk.push_head().expect("push head");
    walk.count()
}

#[test]
fn init_creates_the_xdg_layout_and_is_idempotent() {
    let (_root, paths) = initialized();

    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);
    assert!(notebook.join(".git").is_dir(), "notebook is a git repo");
    assert!(
        notebook.join(".noda/index.tsv").is_file(),
        "index committed"
    );
    assert!(paths.config_dir().is_dir(), "config dir created");
    assert_eq!(paths.active_notebook().unwrap(), cmd::DEFAULT_NOTEBOOK);
    assert_eq!(commit_count(&notebook), 1);

    cmd::init(&paths).expect("second init");
    assert_eq!(
        commit_count(&notebook),
        1,
        "re-running init commits nothing"
    );
}

#[test]
fn add_writes_frontmatter_and_commits() {
    let (_root, paths) = initialized();

    let out = cmd::add(&paths, Some("Meeting Notes"), Some("agenda\n"), &[]).unwrap();
    let (id, slug) = out.split_once("  ").expect("id and slug");
    assert_eq!(slug, "meeting-notes");

    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);
    let text = std::fs::read_to_string(notebook.join("meeting-notes.md")).unwrap();
    assert!(text.contains(&format!("id: {id}")), "{text}");
    assert!(text.contains("title: Meeting Notes"), "{text}");
    assert!(text.ends_with("agenda\n"), "{text}");

    let index = std::fs::read_to_string(notebook.join(".noda/index.tsv")).unwrap();
    assert_eq!(index, format!("{id}\tmeeting-notes\n"));

    assert_eq!(commit_count(&notebook), 2, "the note is one commit");
    let repo = git2::Repository::open(&notebook).unwrap();
    assert!(
        repo.statuses(None).unwrap().is_empty(),
        "nothing is left uncommitted"
    );
}

#[test]
fn add_derives_the_title_from_the_body_when_omitted() {
    let (_root, paths) = initialized();

    let out = cmd::add(&paths, None, Some("# Reading Log\n\nsome book\n"), &[]).unwrap();
    assert!(out.ends_with("  reading-log"), "{out}");

    let text = cmd::show(&paths, "reading-log").unwrap();
    assert!(text.contains("title: Reading Log"), "{text}");
}

#[test]
fn add_rejects_an_empty_note() {
    let (_root, paths) = initialized();
    let err = cmd::add(&paths, None, Some("   \n\n"), &[]).unwrap_err();
    assert!(err.to_string().contains("empty"), "{err}");
}

#[test]
fn add_disambiguates_colliding_slugs_but_keeps_ids_distinct() {
    let (_root, paths) = initialized();

    let first = cmd::add(&paths, Some("Notes"), Some("one\n"), &[]).unwrap();
    let second = cmd::add(&paths, Some("Notes"), Some("two\n"), &[]).unwrap();

    let (first_id, first_slug) = first.split_once("  ").unwrap();
    let (second_id, second_slug) = second.split_once("  ").unwrap();
    assert_eq!(first_slug, "notes");
    assert_eq!(second_slug, "notes-2");
    assert_ne!(first_id, second_id);
}

#[test]
fn show_resolves_by_slug_and_by_id_including_confusable_characters() {
    let (_root, paths) = initialized();
    let out = cmd::add(&paths, Some("Meeting Notes"), Some("agenda\n"), &[]).unwrap();
    let id = out.split_once("  ").unwrap().0;

    let by_slug = cmd::show(&paths, "meeting-notes").unwrap();
    assert_eq!(cmd::show(&paths, id).unwrap(), by_slug);

    // Crockford folds case and the I/L/O confusables; a mistyped id still lands.
    let mistyped: String = id
        .chars()
        .map(|c| match c {
            '1' => 'I',
            '0' => 'O',
            other => other.to_ascii_uppercase(),
        })
        .collect();
    assert_eq!(cmd::show(&paths, &mistyped).unwrap(), by_slug);
}

#[test]
fn show_reports_unknown_and_unsafe_references() {
    let (_root, paths) = initialized();
    cmd::add(&paths, Some("Meeting Notes"), Some("agenda\n"), &[]).unwrap();

    assert!(
        cmd::show(&paths, "meeting")
            .unwrap_err()
            .to_string()
            .contains("not found"),
        "prefixes must not resolve"
    );
    assert!(cmd::show(&paths, "../../etc/passwd").is_err());
}

#[test]
fn ls_lists_notes_and_filters_by_tag() {
    let (_root, paths) = initialized();
    cmd::add(&paths, Some("Alpha"), Some("a\n"), &["work".to_string()]).unwrap();
    cmd::add(&paths, Some("Beta"), Some("b\n"), &[]).unwrap();

    let all = cmd::ls(&paths, None, None).unwrap();
    assert_eq!(all.lines().count(), 2);
    assert!(all.lines().next().unwrap().contains("alpha"), "{all}");
    assert!(all.contains("[work]"), "{all}");

    let tagged = cmd::ls(&paths, None, Some("work")).unwrap();
    assert_eq!(tagged.lines().count(), 1);
    assert!(tagged.contains("alpha"), "{tagged}");

    assert!(cmd::ls(&paths, None, Some("nope")).unwrap().is_empty());
}

#[test]
fn ls_can_target_another_notebook() {
    let (_root, paths) = initialized();
    cmd::add(&paths, Some("Alpha"), Some("a\n"), &[]).unwrap();
    noda::notebook::Notebook::create(&paths, "work").unwrap();

    assert!(cmd::ls(&paths, Some("work"), None).unwrap().is_empty());
    assert!(cmd::ls(&paths, Some("missing"), None).is_err());
}

#[test]
fn commands_refuse_to_run_before_init() {
    let root = TempRoot::new();
    let paths = root.paths();
    let err = cmd::ls(&paths, None, None).unwrap_err();
    assert!(err.to_string().contains("noda init"), "{err}");
}
