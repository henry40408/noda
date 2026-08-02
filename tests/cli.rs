//! End-to-end tests for the command layer. Each test gets its own XDG root, so
//! nothing here reads or writes the developer's real notebooks.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use noda::cmd;
use noda::note;
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

/// The id and the slug out of the `id  slug  [tags]` line every mutating
/// command prints.
fn parts(summary: &str) -> (&str, &str) {
    let (id, rest) = summary.split_once("  ").expect("id and slug");
    (id, rest.split("  ").next().expect("slug"))
}

/// The file a note lives in, from that same line.
fn note_file(summary: &str) -> String {
    let (id, slug) = parts(summary);
    format!("{id}-{slug}.md")
}

#[test]
fn init_creates_the_xdg_layout_and_is_idempotent() {
    let (_root, paths) = initialized();

    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);
    assert!(notebook.join(".git").is_dir(), "notebook is a git repo");
    assert!(
        !notebook.join(".noda").exists(),
        "noda commits no bookkeeping of its own"
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
fn add_writes_the_id_into_the_filename_and_commits() {
    let (_root, paths) = initialized();

    let out = cmd::add(&paths, Some("Meeting Notes"), Some("agenda\n"), &[]).unwrap();
    let (id, slug) = parts(&out);
    assert_eq!(slug, "meeting-notes");

    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);
    let text = std::fs::read_to_string(notebook.join(format!("{id}-meeting-notes.md"))).unwrap();
    // The identity is the filename; the frontmatter carries only what a person
    // wrote, so there is nothing in the file to fall out of step with the name.
    assert!(!text.contains("id:"), "{text}");
    assert!(text.contains("title: Meeting Notes"), "{text}");
    assert!(text.ends_with("agenda\n"), "{text}");

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
fn add_refuses_a_title_or_a_tag_the_frontmatter_cannot_carry() {
    let (_root, paths) = initialized();
    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);
    let before = commit_count(&notebook);

    // A second line in the title becomes a field of its own, which makes `render`
    // and `parse` stop being inverses.
    let err = cmd::add(&paths, Some("Meeting\ntitle: other"), Some("body\n"), &[])
        .unwrap_err()
        .to_string();
    assert!(err.contains("one line"), "{err}");

    // `,` separates tags and `]` closes the list, so neither can sit inside one.
    for tag in ["work, secret", "a]", ""] {
        assert!(
            cmd::add(&paths, Some("Alpha"), Some("body\n"), &[tag.to_string()]).is_err(),
            "tag `{tag}` should be refused"
        );
    }

    assert!(
        cmd::ls(&paths, &cmd::List::default()).unwrap().is_empty(),
        "nothing written"
    );
    assert_eq!(commit_count(&notebook), before, "and nothing committed");
}

#[test]
fn a_tag_is_stored_the_way_it_reads_back() {
    let (_root, paths) = initialized();
    cmd::add(
        &paths,
        Some("Alpha"),
        Some("a\n"),
        &["  work  ".to_string()],
    )
    .unwrap();

    // Surrounding space is dropped on the way in, because it is dropped on the
    // way out — otherwise the tag shown is not the tag `ls --tag` matches.
    assert!(cmd::show(&paths, "alpha").unwrap().contains("tags: [work]"));
    assert!(
        cmd::ls(
            &paths,
            &cmd::List {
                tag: Some("work"),
                ..Default::default()
            }
        )
        .unwrap()
        .contains("Alpha")
    );

    let err = cmd::tag(
        &paths,
        "alpha",
        &["+q3, urgent".to_string()],
        cmd::Touch::Stamp,
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains('`'), "{err}");
    // Removal stays permissive: a tag that got in before the check must still
    // have a way out.
    assert!(
        cmd::tag(
            &paths,
            "alpha",
            &["-q3, urgent".to_string()],
            cmd::Touch::Stamp
        )
        .is_ok()
    );
}

/// Two notes may share a slug: the id in front of it keeps the filenames apart.
/// The `-2` suffix this used to append was only ever a local fix — two machines
/// adding "Notes" at once could not see each other's, so both wrote `notes.md`
/// and the sync conflicted. Now they write two files and git merges them.
#[test]
fn two_notes_may_share_a_slug_because_the_id_separates_them() {
    let (_root, paths) = initialized();

    let first = cmd::add(&paths, Some("Notes"), Some("one\n"), &[]).unwrap();
    let second = cmd::add(&paths, Some("Notes"), Some("two\n"), &[]).unwrap();

    let (first_id, first_slug) = parts(&first);
    let (second_id, second_slug) = parts(&second);
    assert_eq!(first_slug, "notes");
    assert_eq!(second_slug, "notes", "no `-2` invented");
    assert_ne!(first_id, second_id);

    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);
    assert!(notebook.join(note_file(&first)).is_file());
    assert!(notebook.join(note_file(&second)).is_file());
    assert_eq!(
        cmd::ls(&paths, &cmd::List::default())
            .unwrap()
            .lines()
            .count(),
        2
    );

    // The slug alone no longer says which one, so noda asks rather than guesses.
    let err = cmd::show(&paths, "notes").unwrap_err().to_string();
    assert!(err.contains("matches 2 notes"), "{err}");
    assert!(err.contains(first_id), "{err}");
    // Either id still resolves outright.
    assert!(cmd::show(&paths, first_id).unwrap().contains("one"));
    assert!(cmd::show(&paths, second_id).unwrap().contains("two"));
}

/// An id prefix resolves the way git lets an abbreviated object id resolve.
#[test]
fn a_note_resolves_from_a_prefix_of_its_id() {
    let (_root, paths) = initialized();
    let out = cmd::add(&paths, Some("Meeting Notes"), Some("agenda\n"), &[]).unwrap();
    let (id, _) = parts(&out);

    let whole = cmd::show(&paths, id).unwrap();
    assert_eq!(
        cmd::show(&paths, &id[..4]).unwrap(),
        whole,
        "four characters"
    );
    assert_eq!(cmd::show(&paths, &id[..1]).unwrap(), whole, "even one");
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
        "a slug is matched whole; only ids take a prefix"
    );
    assert!(cmd::show(&paths, "../../etc/passwd").is_err());
}

#[test]
fn ls_lists_notes_and_filters_by_tag() {
    let (_root, paths) = initialized();
    cmd::add(&paths, Some("Alpha"), Some("a\n"), &["work".to_string()]).unwrap();
    cmd::add(&paths, Some("Beta"), Some("b\n"), &[]).unwrap();

    let all = cmd::ls(&paths, &cmd::List::default()).unwrap();
    assert_eq!(all.lines().count(), 2);
    assert!(all.lines().next().unwrap().contains("Alpha"), "{all}");
    assert!(all.contains("[work]"), "{all}");

    let tagged = cmd::ls(
        &paths,
        &cmd::List {
            tag: Some("work"),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(tagged.lines().count(), 1);
    assert!(tagged.contains("Alpha"), "{tagged}");

    assert!(
        cmd::ls(
            &paths,
            &cmd::List {
                tag: Some("nope"),
                ..Default::default()
            }
        )
        .unwrap()
        .is_empty()
    );
}

#[test]
fn ls_can_target_another_notebook() {
    let (_root, paths) = initialized();
    cmd::add(&paths, Some("Alpha"), Some("a\n"), &[]).unwrap();
    noda::notebook::Notebook::create(&paths, "work").unwrap();

    assert!(
        cmd::ls(
            &paths,
            &cmd::List {
                notebook: Some("work"),
                ..Default::default()
            }
        )
        .unwrap()
        .is_empty()
    );
    assert!(
        cmd::ls(
            &paths,
            &cmd::List {
                notebook: Some("missing"),
                ..Default::default()
            }
        )
        .is_err()
    );
}

#[test]
fn tag_adds_and_removes_and_commits_once() {
    let (_root, paths) = initialized();
    cmd::add(&paths, Some("Alpha"), Some("a\n"), &["work".to_string()]).unwrap();
    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);
    let before = commit_count(&notebook);

    let out = cmd::tag(
        &paths,
        "alpha",
        &["+q3".to_string(), "-work".to_string()],
        cmd::Touch::Stamp,
    )
    .unwrap();
    assert!(out.ends_with("  [q3]"), "{out}");
    assert_eq!(commit_count(&notebook), before + 1);

    let text = cmd::show(&paths, "alpha").unwrap();
    assert!(text.contains("tags: [q3]"), "{text}");
    assert!(
        text.ends_with("a\n"),
        "the body survives a tag change: {text}"
    );
}

/// The note as it sits on disk. `show` dims the frontmatter, so it is the wrong
/// side of the colour handling to read a field back from.
fn note_text(paths: &Paths, key: &str) -> String {
    let path = cmd::path(paths, Some(key)).unwrap();
    std::fs::read_to_string(path.trim_end()).unwrap()
}

/// The two times a note carries.
fn times(paths: &Paths, key: &str) -> (Option<String>, Option<String>) {
    let note = note::Note::parse(&note_text(paths, key)).unwrap();
    (note.created, note.updated)
}

#[test]
fn a_new_note_is_created_and_updated_at_the_same_moment() {
    let (_root, paths) = initialized();
    cmd::add(&paths, Some("Alpha"), Some("a\n"), &[]).unwrap();

    let (created, updated) = times(&paths, "alpha");
    let created = created.expect("a new note records when it was made");
    assert_eq!(
        Some(&created),
        updated.as_ref(),
        "a note nobody has changed was last changed when it was made"
    );
    assert!(created.ends_with('Z'), "{created}");
}

/// `created` is a fact about the note and never moves again. `updated` is the
/// one that follows what noda does to it.
#[test]
fn changing_a_note_moves_updated_and_leaves_created_alone() {
    let (_root, paths) = initialized();
    let added = cmd::add(&paths, Some("Alpha"), Some("a\n"), &[]).unwrap();
    let created = times(&paths, "alpha").0;

    // Backdated, so the change is visible however fast the test runs.
    let path = paths
        .notebook_dir(cmd::DEFAULT_NOTEBOOK)
        .join(note_file(&added));
    let backdated = note::set_field(
        &std::fs::read_to_string(&path).unwrap(),
        "updated",
        "2000-01-01T00:00:00Z",
    )
    .unwrap();
    std::fs::write(&path, backdated).unwrap();

    cmd::tag(&paths, "alpha", &["+work".to_string()], cmd::Touch::Stamp).unwrap();
    let (after_created, after_updated) = times(&paths, "alpha");
    assert_eq!(after_created, created, "created does not move");
    assert_ne!(
        after_updated.as_deref(),
        Some("2000-01-01T00:00:00Z"),
        "a tag change is a change"
    );

    cmd::mv(&paths, "alpha", "Beta", false, cmd::Touch::Stamp).unwrap();
    assert_eq!(
        times(&paths, "beta").0,
        created,
        "a retitle is not a rebirth"
    );
}

/// Backdates `updated` on a note already on disk, so a change to it is visible
/// however fast the test runs.
fn backdate(paths: &Paths, summary: &str) -> PathBuf {
    let path = paths
        .notebook_dir(cmd::DEFAULT_NOTEBOOK)
        .join(note_file(summary));
    let text = note::set_field(
        &std::fs::read_to_string(&path).unwrap(),
        "updated",
        "2000-01-01T00:00:00Z",
    )
    .unwrap();
    std::fs::write(&path, text).unwrap();
    path
}

/// `--no-touch` is for the changes that are not the note being rewritten: a
/// typo, a tag, a title that was wrong from the start. The commit still records
/// that something happened — it is `updated`, the note's own claim about itself,
/// that is left alone.
#[test]
fn no_touch_leaves_updated_where_it_stands() {
    let (_root, paths) = initialized();
    let added = cmd::add(&paths, Some("Alpha"), Some("a\n"), &[]).unwrap();
    backdate(&paths, &added);
    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);
    let before = commit_count(&notebook);

    cmd::tag(&paths, "alpha", &["+work".to_string()], cmd::Touch::Keep).unwrap();
    assert_eq!(
        times(&paths, "alpha").1.as_deref(),
        Some("2000-01-01T00:00:00Z"),
        "the tag went on without redating the note"
    );

    cmd::mv(&paths, "alpha", "Beta", false, cmd::Touch::Keep).unwrap();
    let (created, updated) = times(&paths, "beta");
    assert_eq!(
        updated.as_deref(),
        Some("2000-01-01T00:00:00Z"),
        "and so did the new title"
    );
    assert!(created.is_some(), "created was never the field in question");

    assert_eq!(
        commit_count(&notebook),
        before + 2,
        "both changes are still commits: the file did change"
    );
    assert!(
        cmd::show(&paths, "beta").unwrap().contains("work"),
        "and the change itself landed"
    );
}

/// `tag` takes hyphen values, so a flag written after the tags arrives as one
/// more tag. `--no-touch` would strip to `-no-touch` and remove a tag nobody
/// has: a command that reports success and did nothing it was asked to.
#[test]
fn tag_says_where_a_flag_goes_rather_than_swallowing_it() {
    let (_root, paths) = initialized();
    cmd::add(&paths, Some("Alpha"), Some("a\n"), &[]).unwrap();
    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);
    let before = commit_count(&notebook);

    let err = cmd::tag(
        &paths,
        "alpha",
        &["+work".to_string(), "--no-touch".to_string()],
        cmd::Touch::Stamp,
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("--no-touch"), "{err}");
    assert!(err.contains("before"), "it says where the flag goes: {err}");
    assert_eq!(
        commit_count(&notebook),
        before,
        "and nothing was committed on the way to finding out"
    );
}

/// The case the flag exists for: a note that arrived with the dates its old
/// system gave it. Editing it must not overwrite them with today.
#[cfg(unix)]
#[test]
fn no_touch_keeps_an_imported_notes_own_dates_through_an_edit() {
    let (root, paths) = initialized();
    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);
    std::fs::write(
        notebook.join("k3f9m2p1-imported.md"),
        "---\ntitle: Imported\ncreated: 2019-03-14T08:21:00Z\nupdated: 2019-03-14T16:21:00+08:00\n---\n\nbody\n",
    )
    .unwrap();
    commit_working_tree(&paths, cmd::DEFAULT_NOTEBOOK, "add: imported");

    let editor = editor_script(
        &root,
        "append",
        r#"printf -- 'and one more line\n' >> "$1""#,
    );
    cmd::edit_with(&paths, "imported", &editor, cmd::Touch::Keep).unwrap();

    let text = note_text(&paths, "imported");
    assert!(
        text.contains("updated: 2019-03-14T16:21:00+08:00"),
        "the offset it was written with is still the offset it carries: {text}"
    );
    assert!(text.contains("and one more line"), "{text}");
}

/// Notes that predate the fields, or arrived without them, are left as they are.
/// The only honest value would come from git, and inventing one from the
/// filesystem would be inventing it after a clone.
#[test]
fn a_note_without_times_does_not_get_them_invented() {
    let (_root, paths) = initialized();
    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);
    std::fs::write(
        notebook.join("k3f9m2p1-imported.md"),
        "---\ntitle: Imported\n---\n\nbody\n",
    )
    .unwrap();

    assert_eq!(times(&paths, "imported"), (None, None));

    // A change noda makes is a change noda can date. It still does not backfill
    // the one it was never told.
    cmd::tag(
        &paths,
        "imported",
        &["+work".to_string()],
        cmd::Touch::Stamp,
    )
    .unwrap();
    let (created, updated) = times(&paths, "imported");
    assert_eq!(created, None, "noda does not know when this was written");
    assert!(updated.is_some(), "it does know when it just touched it");
}

/// A note written somewhere else brings fields noda has never heard of. `tag`
/// rewrites the note through `render`, so that is where they would be lost —
/// and for an imported note the file is the only copy.
#[test]
fn a_write_back_keeps_the_fields_noda_does_not_understand() {
    let (_root, paths) = initialized();
    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);
    std::fs::write(
        notebook.join("k3f9m2p1-imported.md"),
        "---\ntitle: Imported\nsource_id: 4821\nstarred: true\n---\n\nbody\n",
    )
    .unwrap();

    cmd::tag(
        &paths,
        "imported",
        &["+work".to_string()],
        cmd::Touch::Stamp,
    )
    .unwrap();

    let text = cmd::show(&paths, "imported").unwrap();
    assert!(text.contains("source_id: 4821"), "{text}");
    assert!(text.contains("starred: true"), "{text}");
    assert!(text.contains("tags: [work]"), "{text}");
    assert!(text.ends_with("body\n"), "{text}");
}

#[test]
fn tag_drops_the_tags_line_when_the_last_tag_goes() {
    let (_root, paths) = initialized();
    cmd::add(&paths, Some("Alpha"), Some("a\n"), &["work".to_string()]).unwrap();

    cmd::tag(&paths, "alpha", &["-work".to_string()], cmd::Touch::Stamp).unwrap();
    let text = cmd::show(&paths, "alpha").unwrap();
    assert!(!text.contains("tags:"), "{text}");
    assert!(
        cmd::ls(
            &paths,
            &cmd::List {
                tag: Some("work"),
                ..Default::default()
            }
        )
        .unwrap()
        .is_empty()
    );
}

#[test]
fn tag_requires_a_sign_and_commits_nothing_when_there_is_no_change() {
    let (_root, paths) = initialized();
    cmd::add(&paths, Some("Alpha"), Some("a\n"), &["work".to_string()]).unwrap();
    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);
    let before = commit_count(&notebook);

    let err = cmd::tag(&paths, "alpha", &["work".to_string()], cmd::Touch::Stamp).unwrap_err();
    assert!(err.to_string().contains("+work"), "{err}");

    // Re-adding a tag it already has, and dropping one it never had.
    let out = cmd::tag(
        &paths,
        "alpha",
        &["+work".to_string(), "-q3".to_string()],
        cmd::Touch::Stamp,
    )
    .unwrap();
    assert!(out.contains("no change"), "{out}");
    assert_eq!(commit_count(&notebook), before, "nothing to commit");
}

#[test]
fn tag_resolves_by_id_too() {
    let (_root, paths) = initialized();
    let out = cmd::add(&paths, Some("Alpha"), Some("a\n"), &[]).unwrap();
    let id = out.split_once("  ").unwrap().0.to_string();

    cmd::tag(&paths, &id, &["+work".to_string()], cmd::Touch::Stamp).unwrap();
    assert!(cmd::show(&paths, "alpha").unwrap().contains("tags: [work]"));
}

#[test]
fn mv_renames_the_slug_and_keeps_the_id() {
    let (_root, paths) = initialized();
    let added = cmd::add(&paths, Some("Alpha"), Some("a\n"), &["work".to_string()]).unwrap();
    let (id, _) = parts(&added);
    let id = id.to_string();
    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);

    let out = cmd::mv(&paths, "alpha", "Beta Notes", false, cmd::Touch::Stamp).unwrap();
    assert_eq!(out, format!("{id}  beta-notes  [work]"));

    assert!(
        !notebook.join(format!("{id}-alpha.md")).exists(),
        "old file is gone"
    );
    assert!(notebook.join(format!("{id}-beta-notes.md")).is_file());

    // Only the slug half of the filename moved, so the id still resolves; the
    // old slug no longer does. Nothing had to be told about the rename.
    assert!(
        cmd::show(&paths, &id)
            .unwrap()
            .contains("title: Beta Notes")
    );
    assert!(cmd::show(&paths, "beta-notes").is_ok());
    assert!(cmd::show(&paths, "alpha").is_err());

    let repo = git2::Repository::open(&notebook).unwrap();
    assert!(
        repo.statuses(None).unwrap().is_empty(),
        "the rename is fully committed, with no leftover"
    );
}

#[test]
fn mv_retitles_without_moving_when_the_slug_is_unchanged() {
    let (_root, paths) = initialized();
    cmd::add(&paths, Some("Alpha"), Some("a\n"), &[]).unwrap();

    cmd::mv(&paths, "alpha", "  ALPHA  ", false, cmd::Touch::Stamp).unwrap();
    let text = cmd::show(&paths, "alpha").unwrap();
    assert!(text.contains("title: ALPHA"), "{text}");
}

/// The default: retitle, then say which notes still name the filename it left.
/// Their links are stale rather than dead — the id in them still resolves — but
/// every Markdown reader outside noda sees only the path that is gone.
#[test]
fn mv_says_which_notes_linked_to_the_name_it_left() {
    let (_root, paths) = initialized();
    let ((target_id, _), (_, source_slug)) = linked_pair(&paths);

    let out = plain(&cmd::mv(&paths, &target_id, "Weekly sync", false, cmd::Touch::Stamp).unwrap());
    assert!(
        out.contains(&format!("1 note links to {target_id} by an older name")),
        "{out}"
    );
    assert!(
        out.contains(&format!("{source_slug}.md")),
        "and which: {out}"
    );
    assert!(
        note_text(&paths, &source_slug).contains(&format!("{target_id}-meeting-notes.md")),
        "reported, never rewritten"
    );
}

/// The opt-in half. The rename and the rewrites land in one commit, so the
/// notebook is never left in a state where half the links moved.
#[test]
fn mv_update_links_rewrites_the_notes_that_pointed_at_the_old_name() {
    let (_root, paths) = initialized();
    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);
    let ((target_id, _), (_, source_slug)) = linked_pair(&paths);
    let commits = commit_count(&notebook);
    let before = times(&paths, &source_slug);

    let out = plain(&cmd::mv(&paths, &target_id, "Weekly sync", true, cmd::Touch::Stamp).unwrap());
    assert!(out.contains("updated  1 note"), "{out}");
    assert!(
        !out.contains("links to"),
        "nothing was left stranded: {out}"
    );

    let text = note_text(&paths, &source_slug);
    assert!(
        text.contains(&format!("{target_id}-weekly-sync.md")),
        "{text}"
    );
    assert!(
        !text.contains("meeting-notes.md"),
        "the old name is gone: {text}"
    );
    assert_eq!(
        times(&paths, &source_slug),
        before,
        "a mechanical fixup is not somebody editing their note"
    );

    let audit = plain(&cmd::doctor(&paths, false, true, false).unwrap());
    assert!(audit.contains("in order"), "nothing stale is left: {audit}");
    assert_eq!(commit_count(&notebook), commits + 1, "one commit, not two");
    let repo = git2::Repository::open(&notebook).unwrap();
    assert!(
        repo.statuses(None).unwrap().is_empty(),
        "and nothing left in the worktree"
    );
}

/// The reason the match is on the id and not on the filename the rename just
/// left. After two retitles a link is two names behind, and an exact-name match
/// would walk straight past it — leaving it stale with nothing having said so.
#[test]
fn mv_update_links_catches_a_link_two_renames_behind() {
    let (_root, paths) = initialized();
    let ((target_id, _), (_, source_slug)) = linked_pair(&paths);

    cmd::mv(&paths, &target_id, "Weekly sync", false, cmd::Touch::Stamp).unwrap();
    let out = plain(&cmd::mv(&paths, &target_id, "Team sync", true, cmd::Touch::Stamp).unwrap());

    assert!(out.contains("updated  1 note"), "{out}");
    let text = note_text(&paths, &source_slug);
    assert!(
        text.contains(&format!("{target_id}-team-sync.md")),
        "{text}"
    );
    let audit = plain(&cmd::doctor(&paths, false, true, false).unwrap());
    assert!(audit.contains("in order"), "{audit}");
}

/// Which is also the repair for damage already done: the flag means "make the
/// links to this note say the name it has", so asking for it on a retitle that
/// renames nothing fixes what an earlier rename left behind.
#[test]
fn mv_update_links_repairs_without_having_to_retitle_again() {
    let (_root, paths) = initialized();
    let ((target_id, _), (_, source_slug)) = linked_pair(&paths);
    cmd::mv(&paths, &target_id, "Weekly sync", false, cmd::Touch::Stamp).unwrap();

    let out = plain(&cmd::mv(&paths, &target_id, "Weekly sync", true, cmd::Touch::Stamp).unwrap());
    assert!(out.contains("updated  1 note"), "{out}");
    assert!(
        note_text(&paths, &source_slug).contains(&format!("{target_id}-weekly-sync.md")),
        "the link names the note as it stands"
    );
}

/// The note being retitled is a note like any other, so a link it makes to
/// itself is rewritten too — read back from the file the rename just wrote,
/// never from the copy that was there a moment ago.
#[test]
fn mv_update_links_reaches_a_note_that_links_to_itself() {
    let (_root, paths) = initialized();
    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);
    let added = cmd::add(&paths, Some("Alpha"), Some("a\n"), &[]).unwrap();
    let (id, _) = parts(&added);
    let id = id.to_string();

    let path = notebook.join(format!("{id}-alpha.md"));
    let text = std::fs::read_to_string(&path).unwrap();
    std::fs::write(&path, format!("{text}\nand [me]({id}-alpha.md)\n")).unwrap();

    cmd::mv(&paths, &id, "Beta", true, cmd::Touch::Stamp).unwrap();
    let text = note_text(&paths, "beta");
    assert!(text.contains(&format!("[me]({id}-beta.md)")), "{text}");
}

/// A retitle that leaves the filename alone breaks nothing, so it says nothing —
/// and skips the walk that would have found out, which is `doctor --links`' cost
/// and has no business being paid on a rename that renamed nothing.
#[test]
fn a_retitle_that_keeps_the_slug_says_nothing_about_links() {
    let (_root, paths) = initialized();
    let ((target_id, _), _) = linked_pair(&paths);

    let out = plain(
        &cmd::mv(
            &paths,
            &target_id,
            "  MEETING NOTES  ",
            false,
            cmd::Touch::Stamp,
        )
        .unwrap(),
    );
    assert_eq!(out, format!("{target_id}  meeting-notes"), "{out}");
}

/// Retitling onto a slug another note already uses is no longer a collision to
/// step around: the ids differ, so the filenames do.
#[test]
fn mv_may_land_on_a_slug_another_note_already_uses() {
    let (_root, paths) = initialized();
    let alpha = cmd::add(&paths, Some("Alpha"), Some("a\n"), &[]).unwrap();
    let beta = cmd::add(&paths, Some("Beta"), Some("b\n"), &[]).unwrap();
    let (alpha_id, _) = parts(&alpha);
    let (beta_id, _) = parts(&beta);

    assert!(cmd::mv(&paths, "alpha", "   ", false, cmd::Touch::Stamp).is_err());

    let out = cmd::mv(&paths, alpha_id, "Beta", false, cmd::Touch::Stamp).unwrap();
    assert!(out.ends_with("  beta"), "no `-2` invented: {out}");
    assert!(
        cmd::show(&paths, beta_id).unwrap().ends_with("b\n"),
        "the other beta is untouched"
    );
    assert_eq!(
        cmd::ls(&paths, &cmd::List::default())
            .unwrap()
            .lines()
            .count(),
        2
    );
}

#[test]
fn mv_refuses_a_title_the_frontmatter_cannot_carry() {
    let (_root, paths) = initialized();
    cmd::add(&paths, Some("Alpha"), Some("a\n"), &[]).unwrap();

    let err = cmd::mv(
        &paths,
        "alpha",
        "Renamed\ntitle: hijacked",
        false,
        cmd::Touch::Stamp,
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("one line"), "{err}");
    assert!(
        cmd::show(&paths, "alpha").unwrap().contains("title: Alpha"),
        "the note keeps the title it had"
    );
}

/// Writes an executable stand-in for `$EDITOR`. The note path arrives as `$1`.
/// A path is used rather than an inline `sh -c '…'` because the editor string is
/// split on whitespace, exactly as a real `$EDITOR` would be.
#[cfg(unix)]
fn editor_script(root: &TempRoot, name: &str, script: &str) -> String {
    use std::os::unix::fs::PermissionsExt;

    let path = root.0.join(format!("{name}.sh"));
    std::fs::write(&path, format!("#!/bin/sh\n{script}\n")).expect("write editor script");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
        .expect("make editor script executable");
    path.to_str().expect("utf-8 path").to_string()
}

#[cfg(unix)]
#[test]
fn edit_commits_what_was_saved() {
    let (root, paths) = initialized();
    let added = cmd::add(&paths, Some("Alpha"), Some("a\n"), &[]).unwrap();
    let id = parts(&added).0.to_string();
    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);
    let before = commit_count(&notebook);

    let editor = editor_script(&root, "append", r#"printf 'appended\n' >> "$1""#);
    let out = cmd::edit_with(&paths, "alpha", &editor, cmd::Touch::Stamp).unwrap();
    assert_eq!(out, format!("{id}  alpha"));
    assert_eq!(commit_count(&notebook), before + 1);
    assert!(cmd::show(&paths, "alpha").unwrap().contains("appended"));

    let repo = git2::Repository::open(&notebook).unwrap();
    assert!(repo.statuses(None).unwrap().is_empty());
}

/// `edit` is how a note is usually changed, so it has to record that — but it
/// records it in place. Rearranging the block somebody just arranged in their
/// editor, as the price of noting that they did, is not a trade worth making.
#[cfg(unix)]
#[test]
fn edit_records_the_change_without_rearranging_the_block() {
    let (root, paths) = initialized();
    cmd::add(&paths, Some("Alpha"), Some("a\n"), &[]).unwrap();

    let editor = editor_script(
        &root,
        "rewrite",
        r#"printf -- '---\nzebra: 1\ntitle: Alpha\nupdated: 2000-01-01T00:00:00Z\n---\n\nsaved\n' > "$1""#,
    );
    cmd::edit_with(&paths, "alpha", &editor, cmd::Touch::Stamp).unwrap();

    let text = note_text(&paths, "alpha");
    let (block, _) = text.split_once("\n---\n").unwrap();
    let keys: Vec<&str> = block
        .trim_start_matches("---\n")
        .lines()
        .filter_map(|l| l.split_once(':').map(|(k, _)| k))
        .collect();
    assert_eq!(
        keys,
        ["zebra", "title", "updated"],
        "every line stayed where it was put: {text}"
    );
    assert!(
        !text.contains("2000-01-01"),
        "but the note was changed just now: {text}"
    );
    assert!(text.ends_with("saved\n"), "{text}");
}

#[cfg(unix)]
#[test]
fn edit_commits_nothing_when_the_file_is_untouched() {
    let (root, paths) = initialized();
    cmd::add(&paths, Some("Alpha"), Some("a\n"), &[]).unwrap();
    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);
    let before = commit_count(&notebook);

    let editor = editor_script(&root, "noop", "true");
    let out = cmd::edit_with(&paths, "alpha", &editor, cmd::Touch::Stamp).unwrap();
    assert!(out.contains("unchanged"), "{out}");
    assert_eq!(commit_count(&notebook), before);
}

#[cfg(unix)]
#[test]
fn edit_refuses_to_commit_a_broken_note() {
    let (root, paths) = initialized();
    let added = cmd::add(&paths, Some("Alpha"), Some("a\n"), &[]).unwrap();
    let file = note_file(&added);
    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);
    let before = commit_count(&notebook);

    let wiped = editor_script(&root, "wipe", r#"printf 'no frontmatter\n' > "$1""#);
    let err = cmd::edit_with(&paths, "alpha", &wiped, cmd::Touch::Stamp).unwrap_err();
    assert!(err.to_string().contains("not committed"), "{err}");
    assert_eq!(commit_count(&notebook), before);
    assert!(
        std::fs::read_to_string(notebook.join(&file))
            .unwrap()
            .contains("no frontmatter"),
        "the edit is left on disk to be fixed or discarded, never dropped"
    );
}

/// An editor cannot change which note it is editing. The id is in the filename,
/// and an editor is handed the file — so the guard `edit` used to need against a
/// rewritten `id:` field has nothing left to guard against.
#[cfg(unix)]
#[test]
fn an_edit_cannot_change_a_notes_identity() {
    let (root, paths) = initialized();
    let added = cmd::add(&paths, Some("Alpha"), Some("a\n"), &[]).unwrap();
    let id = parts(&added).0.to_string();

    // Writing an `id:` line into the frontmatter is now just another field.
    let reid = editor_script(
        &root,
        "reid",
        r#"printf -- '---\ntitle: Alpha\nid: zzzz\n---\n\nbody\n' > "$1""#,
    );
    let out = cmd::edit_with(&paths, "alpha", &reid, cmd::Touch::Stamp).unwrap();
    assert!(out.starts_with(&id), "the id is unmoved: {out}");
    assert!(cmd::show(&paths, &id).unwrap().contains("body"));
}

#[cfg(unix)]
#[test]
fn edit_reports_an_aborted_editor() {
    let (root, paths) = initialized();
    cmd::add(&paths, Some("Alpha"), Some("a\n"), &[]).unwrap();

    let editor = editor_script(&root, "abort", "exit 1");
    let err = cmd::edit_with(&paths, "alpha", &editor, cmd::Touch::Stamp).unwrap_err();
    assert!(err.to_string().contains("exited with"), "{err}");
}

#[test]
fn rm_deletes_the_note_and_leaves_a_revertible_commit() {
    let (_root, paths) = initialized();
    let added = cmd::add(&paths, Some("Alpha"), Some("a\n"), &["work".to_string()]).unwrap();
    let id = parts(&added).0.to_string();
    let file = note_file(&added);
    cmd::add(&paths, Some("Beta"), Some("b\n"), &[]).unwrap();
    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);
    let before = commit_count(&notebook);

    let out = cmd::rm(&paths, "alpha").unwrap();
    assert!(out.contains(&id) && out.contains("alpha"), "{out}");

    assert!(!notebook.join(&file).exists());
    assert!(cmd::show(&paths, "alpha").is_err());
    assert!(cmd::show(&paths, &id).is_err());
    assert!(
        cmd::show(&paths, "beta").is_ok(),
        "other notes are untouched"
    );

    assert_eq!(commit_count(&notebook), before + 1);
    let repo = git2::Repository::open(&notebook).unwrap();
    assert!(repo.statuses(None).unwrap().is_empty());

    // The note is still in history: the commit before HEAD still carries the file.
    let parent = repo
        .head()
        .unwrap()
        .peel_to_commit()
        .unwrap()
        .parent(0)
        .unwrap();
    assert!(
        parent.tree().unwrap().get_name(&file).is_some(),
        "the removal is revertible"
    );
}

/// The commands that only need to know *which* note they were pointed at used
/// to parse the file anyway, and so refused to run on one whose frontmatter had
/// gone — leaving the tools for clearing that up unusable exactly when they were
/// wanted.
#[test]
fn the_commands_that_do_not_read_a_note_work_on_one_that_cannot_be_read() {
    let (_root, paths) = initialized();
    let added = cmd::add(&paths, Some("Alpha"), Some("the original body\n"), &[]).unwrap();
    let id = parts(&added).0.to_string();
    let file = note_file(&added);
    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);
    std::fs::write(notebook.join(&file), "frontmatter is gone\n").unwrap();

    // The filename still says which note this is, so `resolve` never needed to
    // read it. History is about the file, and seeing what changed is how you
    // find out why it will not parse.
    assert!(cmd::log(&paths, Some(&id), None).unwrap().contains("add:"));
    assert!(cmd::diff(&paths, Some(&id)).unwrap().contains(&file));

    // And the one that undoes the damage. It writes over the file, so reading it
    // first was never necessary.
    cmd::restore(&paths, &id, "HEAD", cmd::Touch::Stamp).unwrap();
    let back = cmd::show(&paths, &id).unwrap();
    assert!(back.contains("the original body"), "{back}");
    assert_eq!(
        status_row(&plain(&cmd::status(&paths).unwrap()), "problems"),
        None,
        "the notebook is whole again"
    );
}

#[test]
fn rm_removes_a_note_whose_frontmatter_is_gone() {
    let (_root, paths) = initialized();
    let added = cmd::add(&paths, Some("Alpha"), Some("a\n"), &[]).unwrap();
    let id = parts(&added).0.to_string();
    let file = note_file(&added);
    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);

    // Deleting a file does not require understanding it.
    std::fs::write(notebook.join(&file), "broken\n").unwrap();
    let out = cmd::rm(&paths, &id).unwrap();
    assert!(
        out.contains(&id),
        "the filename still said what it was: {out}"
    );
    assert!(!notebook.join(&file).exists());
}

#[test]
fn the_commands_that_read_a_note_still_refuse_one_that_cannot_be_read() {
    let (_root, paths) = initialized();
    let added = cmd::add(&paths, Some("Alpha"), Some("a\n"), &[]).unwrap();
    let id = parts(&added).0.to_string();
    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);
    std::fs::write(notebook.join(note_file(&added)), "frontmatter is gone\n").unwrap();

    // The line is drawn at whether the command uses the note's contents. These
    // rewrite the frontmatter, so they have to be able to read it first.
    for err in [
        cmd::mv(&paths, &id, "Renamed", false, cmd::Touch::Stamp).unwrap_err(),
        cmd::tag(&paths, &id, &["+work".to_string()], cmd::Touch::Stamp).unwrap_err(),
    ] {
        assert!(err.to_string().contains("frontmatter"), "{err}");
    }
}

/// A file with neither an id in its name nor frontmatter is not a note, so it
/// does not resolve — it is a file the notebook happens to hold.
#[test]
fn a_file_that_is_not_a_note_does_not_resolve() {
    let (_root, paths) = initialized();
    cmd::add(&paths, Some("Alpha"), Some("a\n"), &[]).unwrap();
    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);

    std::fs::write(notebook.join("orphan.md"), "junk\n").unwrap();
    let err = cmd::log(&paths, Some("orphan"), None)
        .unwrap_err()
        .to_string();
    assert!(err.contains("not found"), "{err}");
}

#[test]
fn rm_resolves_by_id_and_reports_an_unknown_note() {
    let (_root, paths) = initialized();
    let added = cmd::add(&paths, Some("Alpha"), Some("a\n"), &[]).unwrap();
    let id = parts(&added).0.to_string();

    assert!(cmd::rm(&paths, "nope").is_err());
    cmd::rm(&paths, &id).unwrap();
    assert!(cmd::ls(&paths, &cmd::List::default()).unwrap().is_empty());
}

#[test]
fn notebook_add_creates_a_repo_and_records_its_remote() {
    let (_root, paths) = initialized();

    cmd::notebook_add(&paths, "work", Some("git@github.com:me/work-notes.git")).unwrap();
    assert!(paths.notebook_dir("work").join(".git").is_dir());

    let listed = cmd::notebook_ls(&paths).unwrap();
    assert!(
        listed.contains("git@github.com:me/work-notes.git"),
        "{listed}"
    );

    // A second notebook of the same name, and a name that escapes the data dir.
    assert!(cmd::notebook_add(&paths, "work", None).is_err());
    assert!(cmd::notebook_add(&paths, "../escape", None).is_err());
}

#[test]
fn notebook_ls_marks_the_active_notebook() {
    let (_root, paths) = initialized();
    cmd::notebook_add(&paths, "work", None).unwrap();

    let listed = cmd::notebook_ls(&paths).unwrap();
    let lines: Vec<&str> = listed.lines().collect();
    assert_eq!(lines.len(), 2);
    assert!(lines[0].starts_with("* default"), "{listed}");
    assert!(lines[1].starts_with("  work"), "{listed}");

    cmd::use_notebook(&paths, "work").unwrap();
    let listed = cmd::notebook_ls(&paths).unwrap();
    assert!(
        listed.lines().nth(1).unwrap().starts_with("* work"),
        "{listed}"
    );
}

#[test]
fn use_switches_which_notebook_the_note_commands_see() {
    let (_root, paths) = initialized();
    cmd::add(&paths, Some("Alpha"), Some("a\n"), &[]).unwrap();
    cmd::notebook_add(&paths, "work", None).unwrap();

    cmd::use_notebook(&paths, "work").unwrap();
    assert_eq!(cmd::notebook_current(&paths).unwrap(), "work");
    assert!(cmd::ls(&paths, &cmd::List::default()).unwrap().is_empty());
    assert!(
        cmd::show(&paths, "alpha").is_err(),
        "notebooks are separate"
    );

    cmd::add(&paths, Some("Work Item"), Some("w\n"), &[]).unwrap();
    assert!(
        cmd::ls(&paths, &cmd::List::default())
            .unwrap()
            .contains("Work Item")
    );
    assert!(
        cmd::ls(
            &paths,
            &cmd::List {
                notebook: Some("default"),
                ..Default::default()
            }
        )
        .unwrap()
        .contains("Alpha")
    );

    assert!(cmd::use_notebook(&paths, "missing").is_err());
}

#[test]
fn notebook_rm_refuses_the_active_one() {
    let (_root, paths) = initialized();
    cmd::notebook_add(&paths, "work", None).unwrap();

    let err = cmd::notebook_rm(&paths, cmd::DEFAULT_NOTEBOOK, true).unwrap_err();
    assert!(err.to_string().contains("noda use"), "{err}");
    assert!(paths.notebook_dir(cmd::DEFAULT_NOTEBOOK).exists());

    cmd::notebook_rm(&paths, "work", true).unwrap();
    assert!(!paths.notebook_dir("work").exists());
    assert!(
        cmd::notebook_rm(&paths, "work", true).is_err(),
        "already gone"
    );
}

#[test]
fn notebook_rm_asks_before_deleting_and_takes_no_for_an_answer() {
    let (_root, paths) = initialized();
    cmd::notebook_add(&paths, "work", None).unwrap();
    cmd::use_notebook(&paths, "work").unwrap();
    cmd::add(&paths, Some("Alpha"), Some("a\n"), &[]).unwrap();
    cmd::use_notebook(&paths, cmd::DEFAULT_NOTEBOOK).unwrap();

    let out = cmd::notebook_rm_confirmed(&paths, "work", false, |question| {
        assert!(question.contains("cannot be undone"), "{question}");
        assert!(question.contains("1 note "), "{question}");
        Ok(false)
    })
    .unwrap();
    assert!(out.contains("kept"), "{out}");
    assert!(paths.notebook_dir("work").exists(), "no still means no");

    // `--force` is the answer, so nothing is asked.
    cmd::notebook_rm_confirmed(&paths, "work", true, |_| panic!("--force must not ask")).unwrap();
    assert!(!paths.notebook_dir("work").exists());
}

#[test]
fn notebook_rm_refuses_when_there_is_nobody_to_ask() {
    let (_root, paths) = initialized();
    cmd::notebook_add(&paths, "work", None).unwrap();

    // The test harness has no terminal, which is exactly the case being checked:
    // piped or scripted, an irreversible delete must not be assumed.
    let err = cmd::notebook_rm(&paths, "work", false)
        .unwrap_err()
        .to_string();
    assert!(err.contains("--force"), "{err}");
    assert!(paths.notebook_dir("work").exists());
}

#[test]
fn notebook_rename_carries_the_active_pointer() {
    let (_root, paths) = initialized();
    cmd::add(&paths, Some("Alpha"), Some("a\n"), &[]).unwrap();
    cmd::notebook_add(&paths, "work", None).unwrap();

    cmd::notebook_rename(&paths, cmd::DEFAULT_NOTEBOOK, "personal").unwrap();
    assert_eq!(cmd::notebook_current(&paths).unwrap(), "personal");
    assert!(cmd::show(&paths, "alpha").is_ok(), "the notes came along");
    assert!(!paths.notebook_dir(cmd::DEFAULT_NOTEBOOK).exists());

    // Renaming a notebook that is not active leaves the pointer where it is.
    cmd::notebook_rename(&paths, "work", "archive").unwrap();
    assert_eq!(cmd::notebook_current(&paths).unwrap(), "personal");

    assert!(cmd::notebook_rename(&paths, "missing", "x").is_err());
    assert!(cmd::notebook_rename(&paths, "archive", "personal").is_err());
}

/// A bare repository standing in for GitHub. libgit2's local transport is the
/// same push/fetch machinery HTTPS and SSH use, so these tests exercise the real
/// sync code without a network or credentials.
fn bare_remote(root: &TempRoot, name: &str, branch: &str) -> String {
    let path = root.0.join(name);
    let repo = git2::Repository::init_bare(&path).expect("init bare remote");
    repo.set_head(&format!("refs/heads/{branch}"))
        .expect("point the remote at the branch under test");
    path.to_str().expect("utf-8 path").to_string()
}

/// The branch a notebook is on — `main` or `master`, depending on the machine's
/// `init.defaultBranch`, so no test may assume either.
fn branch_of(paths: &Paths, name: &str) -> String {
    noda::notebook::Notebook::open(paths, name)
        .expect("open notebook")
        .branch()
        .expect("branch")
}

/// A notebook wired to `url`, cloned so it has its own history to diverge.
fn mirror(paths: &Paths, url: &str, name: &str) {
    cmd::clone(paths, url, Some(name)).expect("clone mirror");
}

fn merge_commits(notebook: &Path) -> usize {
    let repo = git2::Repository::open(notebook).expect("open repo");
    let mut walk = repo.revwalk().expect("revwalk");
    walk.push_head().expect("push head");
    walk.filter_map(|oid| repo.find_commit(oid.ok()?).ok())
        .filter(|commit| commit.parent_count() > 1)
        .count()
}

#[test]
fn push_and_clone_round_trip_a_notebook() {
    let (root, paths) = initialized();
    let branch = branch_of(&paths, cmd::DEFAULT_NOTEBOOK);
    let url = bare_remote(&root, "origin.git", &branch);

    cmd::remote_set(&paths, &url).unwrap();
    assert_eq!(cmd::remote_show(&paths).unwrap(), url);
    cmd::add(&paths, Some("Meeting Notes"), Some("agenda\n"), &[]).unwrap();
    cmd::push(&paths).unwrap();

    // No name given: it comes from the URL, `origin.git` -> `origin`.
    cmd::clone(&paths, &url, None).unwrap();
    cmd::use_notebook(&paths, "origin").unwrap();
    assert!(
        cmd::ls(&paths, &cmd::List::default())
            .unwrap()
            .contains("Meeting Notes")
    );
    assert!(
        cmd::show(&paths, "meeting-notes")
            .unwrap()
            .contains("agenda")
    );

    // Cloning over an existing notebook is refused rather than merged into it.
    assert!(cmd::clone(&paths, &url, Some("origin")).is_err());
}

#[test]
fn clone_adopts_the_only_branch_when_the_remote_head_points_elsewhere() {
    let (root, paths) = initialized();
    // The remote's HEAD names a branch nothing was ever pushed to — what two
    // machines that disagree about `init.defaultBranch` produce between them.
    let url = bare_remote(&root, "origin.git", "trunk");
    cmd::remote_set(&paths, &url).unwrap();
    cmd::add(&paths, Some("Alpha"), Some("a\n"), &[]).unwrap();
    cmd::push(&paths).unwrap();

    cmd::clone(&paths, &url, Some("mirror")).unwrap();
    cmd::use_notebook(&paths, "mirror").unwrap();
    assert!(
        cmd::ls(&paths, &cmd::List::default())
            .unwrap()
            .contains("Alpha"),
        "a clone that checks out nothing reads as an empty notebook, not a broken one"
    );
    assert_eq!(
        branch_of(&paths, "mirror"),
        branch_of(&paths, cmd::DEFAULT_NOTEBOOK)
    );
}

#[test]
fn cloning_an_empty_remote_leaves_nothing_behind() {
    let (root, paths) = initialized();
    let url = bare_remote(&root, "empty.git", "main");

    let err = cmd::clone(&paths, &url, Some("mirror"))
        .unwrap_err()
        .to_string();
    assert!(err.contains("no commits"), "{err}");
    assert!(
        !paths.notebook_dir("mirror").exists(),
        "no half-clone for the next attempt to trip over"
    );
}

#[test]
fn sync_commits_pending_changes_before_pushing() {
    let (root, paths) = initialized();
    let branch = branch_of(&paths, cmd::DEFAULT_NOTEBOOK);
    let url = bare_remote(&root, "origin.git", &branch);
    cmd::remote_set(&paths, &url).unwrap();
    let added = cmd::add(&paths, Some("Alpha"), Some("a\n"), &[]).unwrap();

    // Edited outside noda — a `$EDITOR` left open, a file synced by another tool.
    let note = paths
        .notebook_dir(cmd::DEFAULT_NOTEBOOK)
        .join(note_file(&added));
    let text = std::fs::read_to_string(&note).unwrap();
    std::fs::write(&note, format!("{text}edited elsewhere\n")).unwrap();

    let out = cmd::sync(&paths).unwrap();
    assert!(out.contains("commit: local changes"), "{out}");
    assert!(out.contains("push:"), "{out}");
    assert_eq!(commit_count(&paths.notebook_dir(cmd::DEFAULT_NOTEBOOK)), 3);

    mirror(&paths, &url, "mirror");
    cmd::use_notebook(&paths, "mirror").unwrap();
    assert!(
        cmd::show(&paths, "alpha")
            .unwrap()
            .contains("edited elsewhere"),
        "the out-of-band edit reached the remote"
    );

    // A second sync has nothing to commit and nothing to send.
    let out = cmd::sync(&paths).unwrap();
    assert!(!out.contains("commit:"), "{out}");
    assert!(out.contains("already up to date"), "{out}");
}

/// `edit` refuses to commit an id change and leaves the file on disk. `sync`
/// stages the whole working tree, so without a guard of its own it picks that
/// file up and makes the disagreement permanent — and remote.
#[cfg(unix)]
#[test]
fn sync_fast_forwards_a_notebook_that_only_received() {
    let (root, paths) = initialized();
    let branch = branch_of(&paths, cmd::DEFAULT_NOTEBOOK);
    let url = bare_remote(&root, "origin.git", &branch);
    cmd::remote_set(&paths, &url).unwrap();
    cmd::add(&paths, Some("Alpha"), Some("a\n"), &[]).unwrap();
    cmd::sync(&paths).unwrap();

    mirror(&paths, &url, "mirror");
    cmd::add(&paths, Some("Beta"), Some("b\n"), &[]).unwrap();
    cmd::sync(&paths).unwrap();

    cmd::use_notebook(&paths, "mirror").unwrap();
    let out = cmd::sync(&paths).unwrap();
    assert!(out.contains("fast-forwarded"), "{out}");
    assert!(
        cmd::ls(&paths, &cmd::List::default())
            .unwrap()
            .contains("Beta")
    );
    assert_eq!(
        merge_commits(&paths.notebook_dir("mirror")),
        0,
        "a one-sided sync needs no merge commit"
    );
}

#[test]
fn sync_merges_notebooks_that_both_moved() {
    let (root, paths) = initialized();
    let branch = branch_of(&paths, cmd::DEFAULT_NOTEBOOK);
    let url = bare_remote(&root, "origin.git", &branch);
    cmd::remote_set(&paths, &url).unwrap();
    cmd::add(&paths, Some("Alpha"), Some("a\n"), &[]).unwrap();
    cmd::sync(&paths).unwrap();
    mirror(&paths, &url, "mirror");

    // Two machines, each writing a different note before either syncs.
    cmd::add(&paths, Some("Laptop"), Some("l\n"), &[]).unwrap();
    cmd::sync(&paths).unwrap();

    cmd::use_notebook(&paths, "mirror").unwrap();
    cmd::add(&paths, Some("Desktop"), Some("d\n"), &[]).unwrap();
    let out = cmd::sync(&paths).unwrap();
    assert!(out.contains("merged"), "{out}");
    assert_eq!(merge_commits(&paths.notebook_dir("mirror")), 1);

    let listed = cmd::ls(&paths, &cmd::List::default()).unwrap();
    assert!(listed.contains("Laptop"), "{listed}");
    assert!(listed.contains("Desktop"), "{listed}");

    // Each side wrote its own filename, so there was nothing to conflict over:
    // the merge is clean without noda rebuilding anything.
    let repo = git2::Repository::open(paths.notebook_dir("mirror")).unwrap();
    assert!(repo.statuses(None).unwrap().is_empty(), "nothing left over");
    assert_eq!(repo.state(), git2::RepositoryState::Clean);

    // And the merge comes back to the notebook that pushed first.
    cmd::use_notebook(&paths, cmd::DEFAULT_NOTEBOOK).unwrap();
    cmd::sync(&paths).unwrap();
    assert!(
        cmd::ls(&paths, &cmd::List::default())
            .unwrap()
            .contains("Desktop")
    );
}

#[test]
fn a_conflicting_pull_is_rolled_back() {
    let (root, paths) = initialized();
    let branch = branch_of(&paths, cmd::DEFAULT_NOTEBOOK);
    let url = bare_remote(&root, "origin.git", &branch);
    cmd::remote_set(&paths, &url).unwrap();
    cmd::add(&paths, Some("Shared"), Some("original\n"), &[]).unwrap();
    cmd::sync(&paths).unwrap();
    mirror(&paths, &url, "mirror");

    let rewrite = |notebook: &str, body: &str| {
        let dir = paths.notebook_dir(notebook);
        let path = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(std::result::Result::ok)
            .map(|e| e.path())
            .find(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.ends_with("-shared.md"))
            })
            .expect("the shared note");
        let text = std::fs::read_to_string(&path).unwrap();
        let head = text
            .split("---\n")
            .take(3)
            .collect::<Vec<_>>()
            .join("---\n");
        std::fs::write(&path, format!("{head}{body}")).unwrap();
    };

    rewrite(cmd::DEFAULT_NOTEBOOK, "from the laptop\n");
    cmd::sync(&paths).unwrap();

    cmd::use_notebook(&paths, "mirror").unwrap();
    rewrite("mirror", "from the desktop\n");
    let err = cmd::sync(&paths).unwrap_err().to_string();
    assert!(err.contains("shared.md"), "{err}");
    assert!(err.contains("rolled back"), "{err}");

    // The rollback has to leave a notebook that still works: no conflict
    // markers on disk, no half-finished merge, the local commit still there.
    let repo = git2::Repository::open(paths.notebook_dir("mirror")).unwrap();
    assert!(repo.statuses(None).unwrap().is_empty(), "worktree is clean");
    assert_eq!(repo.state(), git2::RepositoryState::Clean);
    assert!(
        cmd::show(&paths, "shared")
            .unwrap()
            .contains("from the desktop"),
        "the local edit survived"
    );
}

#[test]
fn push_is_rejected_when_the_remote_moved_ahead() {
    let (root, paths) = initialized();
    let branch = branch_of(&paths, cmd::DEFAULT_NOTEBOOK);
    let url = bare_remote(&root, "origin.git", &branch);
    cmd::remote_set(&paths, &url).unwrap();
    cmd::add(&paths, Some("Alpha"), Some("a\n"), &[]).unwrap();
    cmd::push(&paths).unwrap();
    mirror(&paths, &url, "mirror");

    cmd::add(&paths, Some("Laptop"), Some("l\n"), &[]).unwrap();
    cmd::push(&paths).unwrap();

    cmd::use_notebook(&paths, "mirror").unwrap();
    cmd::add(&paths, Some("Desktop"), Some("d\n"), &[]).unwrap();
    let err = cmd::push(&paths).unwrap_err().to_string();
    assert!(err.contains("noda pull"), "{err}");

    // Which is exactly what unblocks it.
    cmd::pull(&paths).unwrap();
    cmd::push(&paths).unwrap();
}

#[test]
fn pull_refuses_to_run_over_uncommitted_changes() {
    let (root, paths) = initialized();
    let branch = branch_of(&paths, cmd::DEFAULT_NOTEBOOK);
    let url = bare_remote(&root, "origin.git", &branch);
    cmd::remote_set(&paths, &url).unwrap();
    cmd::add(&paths, Some("Alpha"), Some("a\n"), &[]).unwrap();
    let note = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK).join("alpha.md");
    std::fs::write(&note, "half-finished\n").unwrap();

    let err = cmd::pull(&paths).unwrap_err().to_string();
    assert!(err.contains("noda sync"), "{err}");
    assert_eq!(
        std::fs::read_to_string(&note).unwrap(),
        "half-finished\n",
        "the refusal touches nothing"
    );
}

#[test]
fn pulling_an_empty_remote_is_not_an_error() {
    let (root, paths) = initialized();
    let branch = branch_of(&paths, cmd::DEFAULT_NOTEBOOK);
    let url = bare_remote(&root, "origin.git", &branch);
    cmd::remote_set(&paths, &url).unwrap();

    let out = cmd::pull(&paths).unwrap();
    assert!(out.contains("no `"), "{out}");
    assert!(out.contains(&branch), "{out}");
}

#[test]
fn the_network_commands_say_when_no_remote_is_set() {
    let (_root, paths) = initialized();
    for err in [
        cmd::remote_show(&paths).unwrap_err(),
        cmd::push(&paths).unwrap_err(),
        cmd::pull(&paths).unwrap_err(),
        cmd::sync(&paths).unwrap_err(),
    ] {
        assert!(err.to_string().contains("noda remote set"), "{err}");
    }
    assert!(cmd::remote_set(&paths, "  ").is_err(), "a URL is required");
}

/// Command output carries colour unconditionally — `anstream` strips it on the
/// way out when nobody is looking. Tests look at the text underneath.
fn plain(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' {
            for escaped in chars.by_ref() {
                if escaped == 'm' {
                    break;
                }
            }
        } else {
            out.push(ch);
        }
    }
    out
}

/// Commits the working tree the way an edit made outside noda would arrive.
fn commit_working_tree(paths: &Paths, notebook: &str, message: &str) {
    noda::notebook::Notebook::open(paths, notebook)
        .expect("open notebook")
        .commit_all(message)
        .expect("commit");
}

/// The value beside a label in `noda status` output.
fn status_row<'a>(status: &'a str, key: &str) -> Option<&'a str> {
    status
        .lines()
        .find(|line| line.starts_with(key))
        .map(|line| line[key.len()..].trim())
}

#[test]
fn status_reports_a_notebook_with_nowhere_to_sync() {
    let (_root, paths) = initialized();

    let out = plain(&cmd::status(&paths).unwrap());
    assert_eq!(
        status_row(&out, "notebook").unwrap().split("  ").next(),
        Some("default")
    );
    assert_eq!(status_row(&out, "notes"), Some("0"));
    assert_eq!(status_row(&out, "changes"), Some("clean"));
    assert!(
        status_row(&out, "remote").unwrap().contains("none"),
        "{out}"
    );
    assert!(
        status_row(&out, "sync").is_none(),
        "with no remote there is nothing to be in sync with: {out}"
    );

    cmd::add(&paths, Some("Alpha"), Some("a\n"), &[]).unwrap();
    cmd::add(&paths, Some("Beta"), Some("b\n"), &[]).unwrap();
    let out = plain(&cmd::status(&paths).unwrap());
    assert_eq!(status_row(&out, "notes"), Some("2"));
}

#[test]
fn status_counts_the_distance_from_the_remote_without_touching_it() {
    let (root, paths) = initialized();
    let branch = branch_of(&paths, cmd::DEFAULT_NOTEBOOK);
    let url = bare_remote(&root, "origin.git", &branch);
    cmd::remote_set(&paths, &url).unwrap();
    cmd::add(&paths, Some("Alpha"), Some("a\n"), &[]).unwrap();

    let out = plain(&cmd::status(&paths).unwrap());
    assert_eq!(status_row(&out, "sync"), Some("never synced"), "{out}");

    cmd::sync(&paths).unwrap();
    let out = plain(&cmd::status(&paths).unwrap());
    assert!(
        status_row(&out, "sync").unwrap().starts_with("in sync"),
        "{out}"
    );

    cmd::add(&paths, Some("Beta"), Some("b\n"), &[]).unwrap();
    let out = plain(&cmd::status(&paths).unwrap());
    assert!(
        status_row(&out, "sync").unwrap().starts_with("1 to push"),
        "{out}"
    );

    // The other side of the drift: a second notebook pushes, and this one is
    // behind it — but only once it has fetched, because status never does.
    mirror(&paths, &url, "mirror");
    cmd::sync(&paths).unwrap();
    cmd::use_notebook(&paths, "mirror").unwrap();
    let out = plain(&cmd::status(&paths).unwrap());
    assert!(
        status_row(&out, "sync").unwrap().starts_with("in sync"),
        "stale until it fetches, which is the point: {out}"
    );
    cmd::pull(&paths).unwrap();
    let out = plain(&cmd::status(&paths).unwrap());
    assert!(
        status_row(&out, "sync").unwrap().starts_with("in sync"),
        "{out}"
    );
}

/// A file with neither an id in its name nor a frontmatter block is not a note
/// and not a mistake — it is a file. It is listed as one and counted as one, and
/// it is never a problem, because a notebook is allowed to hold it.
#[test]
fn a_file_that_declares_nothing_is_listed_as_a_file() {
    let (_root, paths) = initialized();
    cmd::add(&paths, Some("Alpha"), Some("a\n"), &[]).unwrap();
    std::fs::write(
        paths.notebook_dir(cmd::DEFAULT_NOTEBOOK).join("stray.md"),
        "not a note at all\n",
    )
    .unwrap();

    let listed = plain(&cmd::ls(&paths, &cmd::List::default()).unwrap());
    assert!(
        listed.contains("Alpha"),
        "the note is still a note: {listed}"
    );
    assert!(
        listed.contains("files\n  stray.md"),
        "and the file is under its own heading: {listed}"
    );

    let out = plain(&cmd::status(&paths).unwrap());
    assert_eq!(status_row(&out, "notes"), Some("1"), "{out}");
    assert_eq!(status_row(&out, "files"), Some("1"), "{out}");
    assert_eq!(status_row(&out, "problems"), None, "{out}");
    assert_eq!(status_row(&out, "changes"), Some("1 file uncommitted"));
}

/// The frontmatter is the declaration "I am a note". A file that makes it but
/// carries no id in its name is one waiting to be adopted.
#[test]
fn status_reports_a_note_with_no_id_in_its_name() {
    let (_root, paths) = initialized();
    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);
    cmd::add(&paths, Some("Alpha"), Some("a\n"), &[]).unwrap();

    let out = plain(&cmd::status(&paths).unwrap());
    assert_eq!(
        status_row(&out, "problems"),
        None,
        "a healthy notebook says nothing about it: {out}"
    );

    plant_unnamed(&notebook, "hand-written");
    let out = plain(&cmd::status(&paths).unwrap());
    assert_eq!(
        status_row(&out, "problems"),
        Some("1 note has no id in its filename  (hand-written.md)"),
        "{out}"
    );
}

/// The other half of the pair: a name that claims an id over a file that never
/// declared itself. `abcdefgh` is a perfectly legal id, so the shape alone
/// cannot settle whether this is a broken note or somebody's file.
#[test]
fn status_reports_a_file_that_claims_an_id_without_frontmatter() {
    let (_root, paths) = initialized();
    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);
    cmd::add(&paths, Some("Alpha"), Some("a\n"), &[]).unwrap();
    std::fs::write(notebook.join("abcdefgh-hello.md"), "no frontmatter\n").unwrap();

    let out = plain(&cmd::status(&paths).unwrap());
    assert_eq!(
        status_row(&out, "problems"),
        Some("1 file is named like a note but has no frontmatter  (abcdefgh-hello.md)"),
        "{out}"
    );
}

/// Two machines can mint one id without ever meeting. The filenames differ, so
/// git merges them without a word and this is the only place it shows up.
#[test]
fn status_reports_one_id_carried_by_two_notes() {
    let (_root, paths) = initialized();
    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);
    plant(&notebook, "k3f9m2p1", "alpha");
    // Folded, the way every other comparison folds them: `K3F9M2P1` is not a
    // second id.
    plant(&notebook, "K3F9M2P1", "beta");

    let out = plain(&cmd::status(&paths).unwrap());
    assert_eq!(
        status_row(&out, "problems"),
        Some("1 id is carried by more than one note  (k3f9m2p1)"),
        "{out}"
    );
}

/// A note the way a merge or another machine delivers one: already adopted, with
/// an id noda never minted here. Minting has to see it, or it could hand the
/// same id out twice — and there is no undoing that.
#[test]
fn a_new_note_avoids_an_id_that_arrived_from_outside() {
    let (_root, paths) = initialized();
    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);
    plant(&notebook, "zzzzyyyy", "merged");

    let taken = noda::notebook::Notebook::open_active(&paths)
        .unwrap()
        .taken_ids()
        .unwrap();
    assert!(taken.contains("zzzzyyyy"), "{taken:?}");

    cmd::add(&paths, Some("Alpha"), Some("a\n"), &[]).unwrap();
    assert_eq!(
        status_row(&plain(&cmd::status(&paths).unwrap()), "problems"),
        None,
        "a note that arrived adopted is simply a note"
    );
    assert_eq!(
        cmd::ls(&paths, &cmd::List::default())
            .unwrap()
            .lines()
            .count(),
        2
    );
}

#[test]
fn a_wholesale_problem_is_counted_rather_than_listed() {
    let (_root, paths) = initialized();
    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);
    cmd::add(&paths, Some("Anchor"), Some("body\n"), &[]).unwrap();

    // A directory of hand-written notes copied in at once makes every one of
    // them a problem together. `status` has to stay one screen through that.
    for n in 0..12 {
        plant_unnamed(&notebook, &format!("note-{n:02}"));
    }

    let out = plain(&cmd::status(&paths).unwrap());
    let row = status_row(&out, "problems").unwrap();
    assert!(
        row.starts_with("12 notes have no id in their filenames"),
        "{row}"
    );
    assert_eq!(
        row.matches(".md").count(),
        3,
        "three named, not twelve: {row}"
    );
    assert!(row.ends_with("…)"), "and the rest elided: {row}");
    // Six lines: five rows plus the pointer to `noda doctor`. What matters is
    // that the count does not follow the number of notes, so a notebook four
    // times the size prints the same screen.
    assert_eq!(out.lines().count(), 6, "{out}");
    for n in 12..48 {
        plant_unnamed(&notebook, &format!("note-{n:02}"));
    }
    let bigger = plain(&cmd::status(&paths).unwrap());
    assert_eq!(bigger.lines().count(), 6, "{bigger}");
}

#[test]
fn several_kinds_are_totalled_before_they_are_broken_down() {
    let (_root, paths) = initialized();
    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);
    cmd::add(&paths, Some("Alpha"), Some("a\n"), &[]).unwrap();

    plant_unnamed(&notebook, "merged");
    plant_unnamed(&notebook, "dropped-in");
    std::fs::write(notebook.join("abcdefgh-hello.md"), "no frontmatter\n").unwrap();

    let out = plain(&cmd::status(&paths).unwrap());
    assert_eq!(
        status_row(&out, "problems"),
        Some("3 problems"),
        "the size of it comes first: {out}"
    );
    assert!(
        out.contains("2 notes have no id in their filenames  (dropped-in.md; merged.md)"),
        "{out}"
    );
    assert!(
        out.contains("1 file is named like a note but has no frontmatter  (abcdefgh-hello.md)"),
        "{out}"
    );
}

/// Writes an adopted note directly, the way a merge or another machine would.
fn plant(notebook: &Path, id: &str, slug: &str) {
    std::fs::write(
        notebook.join(format!("{id}-{slug}.md")),
        format!("---\ntitle: {slug}\n---\n\nbody\n"),
    )
    .unwrap();
}

/// Writes a note that declares itself but has no id in its name — a file written
/// by hand, or brought in from somewhere that never heard of noda.
fn plant_unnamed(notebook: &Path, name: &str) {
    std::fs::write(
        notebook.join(format!("{name}.md")),
        format!("---\ntitle: {name}\n---\n\nbody\n"),
    )
    .unwrap();
}

/// The one repair that cannot lose anything: the file has already said it is a
/// note, and all it lacks is a name.
#[test]
fn doctor_adopts_a_note_that_only_lacks_an_id() {
    let (_root, paths) = initialized();
    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);
    plant_unnamed(&notebook, "hand-written");
    let commits = commit_count(&notebook);

    let out = plain(&cmd::doctor(&paths, false, false, false).unwrap());
    assert!(out.contains("adopted 1 note"), "{out}");
    assert!(
        !notebook.join("hand-written.md").exists(),
        "the file moved to its adopted name"
    );

    let listed = cmd::ls(&paths, &cmd::List::default()).unwrap();
    assert_eq!(listed.lines().count(), 1, "{listed}");
    assert!(
        listed.contains("hand-written"),
        "the slug survives: {listed}"
    );
    assert_eq!(
        status_row(&plain(&cmd::status(&paths).unwrap()), "problems"),
        None,
        "and the notebook is in order again"
    );
    assert_eq!(
        commit_count(&notebook),
        commits + 1,
        "the repair is a commit, so it can be reverted like any other change"
    );
}

#[test]
fn doctor_names_every_file_where_status_elides() {
    let (_root, paths) = initialized();
    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);
    for n in 0..12 {
        plant_unnamed(&notebook, &format!("note-{n:02}"));
    }

    // `status` shows three and a `…`; this is where the rest can be seen.
    let out = plain(&cmd::doctor(&paths, true, false, false).unwrap());
    assert_eq!(out.matches(".md").count(), 12, "{out}");
    assert!(!out.contains('…'), "nothing elided here: {out}");
}

/// The break `updated` cannot avoid: a note edited outside noda changes without
/// noda getting to record that it did. git is the only witness, which is why
/// this check costs a walk of history and is asked for rather than assumed.
#[test]
fn doctor_times_reports_a_note_changed_outside_noda() {
    let (_root, paths) = initialized();
    let added = cmd::add(&paths, Some("Alpha"), Some("a\n"), &[]).unwrap();
    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);
    let path = notebook.join(note_file(&added));

    // A note whose contents moved on while its own record of when did not.
    let text = std::fs::read_to_string(&path).unwrap();
    let edited = note::set_field(
        &text.replace("a\n", "edited elsewhere\n"),
        "updated",
        "2000-01-01T00:00:00Z",
    )
    .unwrap();
    std::fs::write(&path, edited).unwrap();
    commit_working_tree(&paths, cmd::DEFAULT_NOTEBOOK, "edit: by hand");

    let quiet = plain(&cmd::doctor(&paths, false, false, false).unwrap());
    assert!(
        quiet.contains("in order"),
        "not looked for unless asked for"
    );

    let commits = commit_count(&notebook);
    let out = plain(&cmd::doctor(&paths, false, false, true).unwrap());
    assert!(out.contains("1 note was changed outside noda"), "{out}");
    assert!(out.contains(&note_file(&added)), "{out}");
    assert_eq!(
        commit_count(&notebook),
        commits,
        "the check reports and repairs nothing"
    );
}

/// The cheap half of the same flag: what the two fields say about each other,
/// without asking git anything.
#[test]
fn doctor_times_reports_what_cannot_be_read_and_what_runs_backwards() {
    let (_root, paths) = initialized();
    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);
    std::fs::write(
        notebook.join("k3f9m2p1-unreadable.md"),
        "---\ntitle: Unreadable\ncreated: last tuesday\n---\n\nbody\n",
    )
    .unwrap();
    std::fs::write(
        notebook.join("k3f9m2p2-backwards.md"),
        "---\ntitle: Backwards\ncreated: 2024-01-01T00:00:00Z\nupdated: 2020-01-01T00:00:00Z\n---\n\nbody\n",
    )
    .unwrap();

    let out = plain(&cmd::doctor(&paths, false, false, true).unwrap());
    assert!(out.contains("1 time cannot be read"), "{out}");
    assert!(
        out.contains("k3f9m2p1-unreadable.md created: last tuesday"),
        "{out}"
    );
    assert!(out.contains("1 note changed before being created"), "{out}");
    assert!(out.contains("k3f9m2p2-backwards.md"), "{out}");

    // An unreadable value is reported, not refused: it must not come between
    // somebody and their own prose.
    assert!(cmd::show(&paths, "unreadable").unwrap().contains("body"));
}

/// A notebook noda wrote by itself has nothing to report, which is what makes
/// the check worth running at all.
#[test]
fn doctor_times_is_quiet_about_notes_noda_wrote() {
    let (_root, paths) = initialized();
    cmd::add(&paths, Some("Alpha"), Some("a\n"), &[]).unwrap();
    cmd::tag(&paths, "alpha", &["+work".to_string()], cmd::Touch::Stamp).unwrap();
    cmd::mv(&paths, "alpha", "Renamed", false, cmd::Touch::Stamp).unwrap();

    let out = plain(&cmd::doctor(&paths, false, false, true).unwrap());
    assert!(out.contains("in order"), "{out}");
}

#[test]
fn doctor_writes_nothing_on_a_dry_run() {
    let (_root, paths) = initialized();
    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);
    plant_unnamed(&notebook, "hand-written");
    let commits = commit_count(&notebook);

    let out = plain(&cmd::doctor(&paths, true, false, false).unwrap());
    assert!(out.contains("nothing was changed"), "{out}");
    assert!(
        notebook.join("hand-written.md").exists(),
        "it renames a file, so a look first is free"
    );
    assert_eq!(commit_count(&notebook), commits);
}

#[test]
fn doctor_says_so_when_there_is_nothing_to_do() {
    let (_root, paths) = initialized();
    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);
    cmd::add(&paths, Some("Alpha"), Some("a\n"), &[]).unwrap();
    let commits = commit_count(&notebook);

    let out = plain(&cmd::doctor(&paths, false, false, false).unwrap());
    assert!(out.contains("in order"), "{out}");
    assert_eq!(
        commit_count(&notebook),
        commits,
        "and makes no empty commit"
    );
}

/// Both files are real notes. Keeping either one's identity means discarding the
/// other's, so this is reported and left alone.
#[test]
fn doctor_reports_but_does_not_settle_a_shared_id() {
    let (_root, paths) = initialized();
    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);
    plant(&notebook, "k3f9m2p1", "alpha");
    plant(&notebook, "K3F9M2P1", "beta");
    let commits = commit_count(&notebook);

    let out = plain(&cmd::doctor(&paths, false, false, false).unwrap());
    assert!(out.contains("carried by more than one note"), "{out}");
    assert!(out.contains("rename one of the files"), "{out}");
    assert!(notebook.join("k3f9m2p1-alpha.md").exists());
    assert!(notebook.join("K3F9M2P1-beta.md").exists());
    assert_eq!(commit_count(&notebook), commits, "nothing was decided");
}

/// The `abcdefgh-hello.md` case: a name that claims an id over a file that never
/// declared itself. It might be a note that lost its frontmatter, or a file that
/// was never one. Only its author knows.
#[test]
fn doctor_reports_but_does_not_settle_a_file_that_claims_an_id() {
    let (_root, paths) = initialized();
    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);
    std::fs::write(notebook.join("abcdefgh-hello.md"), "no frontmatter\n").unwrap();
    let commits = commit_count(&notebook);

    let out = plain(&cmd::doctor(&paths, false, false, false).unwrap());
    assert!(out.contains("abcdefgh-hello.md"), "{out}");
    assert!(out.contains("add a `---` block back"), "{out}");
    assert_eq!(
        std::fs::read_to_string(notebook.join("abcdefgh-hello.md")).unwrap(),
        "no frontmatter\n",
        "untouched"
    );
    assert_eq!(commit_count(&notebook), commits);
}

#[test]
fn doctor_ignores_a_file_that_was_never_a_note() {
    let (_root, paths) = initialized();
    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);
    cmd::add(&paths, Some("Alpha"), Some("a\n"), &[]).unwrap();

    // No id in the name, no frontmatter inside: not noda's business, and it must
    // not stand between an adoptable note and its repair.
    std::fs::write(notebook.join("scratch.md"), "just some markdown\n").unwrap();
    plant_unnamed(&notebook, "hand-written");

    let out = plain(&cmd::doctor(&paths, false, false, false).unwrap());
    assert!(out.contains("adopted 1 note"), "{out}");
    assert!(!out.contains("scratch.md"), "{out}");
    assert!(notebook.join("scratch.md").exists(), "left where it was");
}

/// Drops `name` into the notebook as a file that is not a note.
fn plant_file(notebook: &Path, name: &str) {
    std::fs::write(notebook.join(name), "contents\n").unwrap();
}

/// The expensive checks are the ones nobody asked for until they ask, so the
/// default run must not perform them — nor mention what they would have found.
#[test]
fn doctor_says_nothing_about_links_until_it_is_asked_to() {
    let (_root, paths) = initialized();
    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);
    cmd::add(&paths, Some("Alpha"), Some("see ![d](missing.png)\n"), &[]).unwrap();
    plant_file(&notebook, "unreferenced.png");

    let out = plain(&cmd::doctor(&paths, false, false, false).unwrap());
    assert!(out.contains("in order"), "{out}");
    assert!(!out.contains("unreferenced.png"), "{out}");
    assert!(!out.contains("missing.png"), "{out}");
}

#[test]
fn doctor_links_reports_a_file_no_note_links_to() {
    let (_root, paths) = initialized();
    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);
    cmd::add(&paths, Some("Alpha"), Some("![d](used.png)\n"), &[]).unwrap();
    plant_file(&notebook, "used.png");
    plant_file(&notebook, "receipt.pdf");
    let commits = commit_count(&notebook);

    let out = plain(&cmd::doctor(&paths, false, true, false).unwrap());
    assert!(out.contains("1 file no note links to"), "{out}");
    assert!(out.contains("receipt.pdf"), "{out}");
    assert!(
        !out.contains("used.png"),
        "a file a note links to is not an orphan: {out}"
    );
    assert!(
        notebook.join("receipt.pdf").exists(),
        "reported, never removed"
    );
    assert_eq!(commit_count(&notebook), commits, "and nothing was decided");
}

#[test]
fn doctor_links_reports_a_link_that_names_nothing() {
    let (_root, paths) = initialized();
    cmd::add(&paths, Some("Alpha"), Some("see ![d](missing.png)\n"), &[]).unwrap();

    let out = plain(&cmd::doctor(&paths, false, true, false).unwrap());
    assert!(out.contains("1 broken link"), "{out}");
    assert!(out.contains("missing.png"), "{out}");
    assert!(
        out.contains("alpha.md"),
        "the note holding it is named: {out}"
    );
}

/// The split the one "broken" category used to hide: a retitled note leaves the
/// destination naming a path that is gone and an id that is not. noda knows what
/// the link should say, and saying so is the whole point of separating them.
#[test]
fn doctor_links_tells_a_stale_link_from_a_broken_one() {
    let (_root, paths) = initialized();
    let ((target_id, _), (_, source_slug)) = linked_pair(&paths);
    cmd::mv(&paths, &target_id, "Weekly sync", false, cmd::Touch::Stamp).unwrap();

    let out = plain(&cmd::doctor(&paths, false, true, false).unwrap());
    assert!(out.contains("1 stale link"), "{out}");
    assert!(
        out.contains(&format!("{source_slug}.md")),
        "the note holding it is named: {out}"
    );
    assert!(
        out.contains(&format!("{target_id}-meeting-notes.md")),
        "the destination as written: {out}"
    );
    assert!(
        out.contains(&format!("now {target_id}-weekly-sync.md")),
        "and the name it should carry: {out}"
    );
    assert!(
        !out.contains("broken link"),
        "a link noda can resolve is not broken: {out}"
    );
}

/// The other half of the split. A destination shaped like a note's filename but
/// naming an id the notebook does not hold resolves to nothing, so it stays in
/// the category only its author can settle.
#[test]
fn doctor_links_calls_a_link_to_no_note_at_all_broken() {
    let (_root, paths) = initialized();
    cmd::add(
        &paths,
        Some("Alpha"),
        Some("see [it](zzzzzzzz-never-here.md)\n"),
        &[],
    )
    .unwrap();

    let out = plain(&cmd::doctor(&paths, false, true, false).unwrap());
    assert!(out.contains("1 broken link"), "{out}");
    assert!(out.contains("zzzzzzzz-never-here.md"), "{out}");
    assert!(!out.contains("stale"), "{out}");
}

/// Writes a file into the notebook's `.git/hooks`, executable or not.
#[cfg(unix)]
fn plant_hook(notebook: &Path, name: &str, executable: bool) {
    use std::os::unix::fs::PermissionsExt;

    let dir = notebook.join(".git/hooks");
    std::fs::create_dir_all(&dir).expect("create hooks dir");
    let path = dir.join(name);
    std::fs::write(&path, "#!/bin/sh\nexit 0\n").expect("write hook");
    let mode = if executable { 0o755 } else { 0o644 };
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).expect("set mode");
}

/// The gap this closes: the same hook fires under `git commit` and is dead under
/// `noda add`, and nothing said so.
#[cfg(unix)]
#[test]
fn doctor_reports_the_hooks_that_will_never_run() {
    let (_root, paths) = initialized();
    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);
    plant_hook(&notebook, "pre-commit", true);
    plant_hook(&notebook, "post-commit", true);

    let out = plain(&cmd::doctor(&paths, false, false, false).unwrap());
    assert!(out.contains("2 git hooks will never run"), "{out}");
    assert!(out.contains("pre-commit"), "{out}");
    assert!(out.contains("post-commit"), "{out}");
    assert!(
        out.contains("never calls git"),
        "the reason is the remedy: {out}"
    );
}

/// Neither is a hook git would have run either, so neither is noda's doing.
#[cfg(unix)]
#[test]
fn doctor_ignores_hooks_git_would_not_run() {
    let (_root, paths) = initialized();
    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);
    plant_hook(&notebook, "pre-commit.sample", true);
    plant_hook(&notebook, "post-commit", false);

    let out = plain(&cmd::doctor(&paths, false, false, false).unwrap());
    assert!(out.contains("in order"), "{out}");
}

/// Costs one `read_dir`, so it is not behind a flag — but a notebook with no
/// hooks must not gain a line saying so.
#[test]
fn doctor_says_nothing_about_hooks_when_there_are_none() {
    let (_root, paths) = initialized();
    cmd::add(&paths, Some("Alpha"), Some("a\n"), &[]).unwrap();

    let out = plain(&cmd::doctor(&paths, false, false, false).unwrap());
    assert!(out.contains("in order"), "{out}");
    assert!(!out.contains("hook"), "{out}");
}

/// A hook is not a problem with the notes, so it must not reach the summary that
/// counts them.
#[cfg(unix)]
#[test]
fn status_says_nothing_about_hooks() {
    let (_root, paths) = initialized();
    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);
    cmd::add(&paths, Some("Alpha"), Some("a\n"), &[]).unwrap();
    plant_hook(&notebook, "pre-commit", true);

    let out = plain(&cmd::status(&paths).unwrap());
    assert!(!out.contains("hook"), "{out}");
    assert!(!out.contains("problems"), "{out}");
}

/// `core.hooksPath` is where git looks, so it is where noda has to look: hooks
/// left behind in `.git/hooks` are dead under git too, and blaming noda for them
/// would be a false report.
#[cfg(unix)]
#[test]
fn doctor_follows_core_hookspath() {
    let (_root, paths) = initialized();
    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);
    plant_hook(&notebook, "pre-commit", true);

    let elsewhere = notebook.join("my-hooks");
    std::fs::create_dir_all(&elsewhere).unwrap();
    let path = elsewhere.join("post-commit");
    std::fs::write(&path, "#!/bin/sh\nexit 0\n").unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let repo = git2::Repository::open(&notebook).unwrap();
    repo.config()
        .unwrap()
        .set_str("core.hooksPath", "my-hooks")
        .unwrap();

    let out = plain(&cmd::doctor(&paths, false, false, false).unwrap());
    assert!(out.contains("1 git hook will never run"), "{out}");
    assert!(out.contains("post-commit"), "{out}");
    assert!(
        !out.contains("pre-commit"),
        "git would not run it either: {out}"
    );
}

/// The reason this reads Markdown with a parser instead of searching for the
/// filename: both of these are how a correct answer differs from a plausible
/// one, and both would otherwise be wrong.
#[test]
fn only_a_real_link_counts_as_a_reference() {
    let (_root, paths) = initialized();
    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);
    // Named in a fenced block, which is prose about a link, not a link.
    cmd::add(
        &paths,
        Some("Alpha"),
        Some("```\n![d](quoted.png)\n```\n"),
        &[],
    )
    .unwrap();
    // Named only at the bottom, which a search of the paragraph would miss.
    cmd::add(
        &paths,
        Some("Beta"),
        Some("see ![the diagram][d]\n\n[d]: referenced.png\n"),
        &[],
    )
    .unwrap();
    plant_file(&notebook, "quoted.png");
    plant_file(&notebook, "referenced.png");

    let out = plain(&cmd::doctor(&paths, false, true, false).unwrap());
    assert!(
        out.contains("quoted.png"),
        "a link inside a fence references nothing: {out}"
    );
    assert!(
        !out.contains("referenced.png"),
        "a reference-style link is still a link: {out}"
    );
}

/// A destination that reaches outside the notebook, or names somebody else's
/// server, is not a file this notebook can be missing.
#[test]
fn a_destination_the_notebook_does_not_own_is_never_broken() {
    let (_root, paths) = initialized();
    cmd::add(
        &paths,
        Some("Alpha"),
        Some("[a](https://example.com/x.png) [b](#section) [c](mailto:me@example.com)\n"),
        &[],
    )
    .unwrap();

    let out = plain(&cmd::doctor(&paths, false, true, false).unwrap());
    assert!(out.contains("in order"), "{out}");
}

/// Asking for a tag is asking about notes.
#[test]
fn listing_by_tag_does_not_list_the_notebooks_files() {
    let (_root, paths) = initialized();
    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);
    cmd::add(&paths, Some("Alpha"), Some("a\n"), &["work".to_string()]).unwrap();
    plant_file(&notebook, "receipt.pdf");

    assert!(
        cmd::ls(&paths, &cmd::List::default())
            .unwrap()
            .contains("receipt.pdf")
    );
    let tagged = cmd::ls(
        &paths,
        &cmd::List {
            tag: Some("work"),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(!tagged.contains("receipt.pdf"), "{tagged}");
    assert!(tagged.contains("Alpha"), "{tagged}");
}

/// A notebook holding one tagged note and one file, for the listing tests.
fn listable() -> (TempRoot, Paths, String) {
    let (root, paths) = initialized();
    let summary = cmd::add(
        &paths,
        Some("Meeting Notes"),
        Some("body\n"),
        &["work".to_string()],
    )
    .unwrap();
    cmd::file_add(
        &paths,
        std::slice::from_ref(&source_file(&root, "my diagram.png")),
        None,
    )
    .unwrap();
    (root, paths, summary)
}

#[test]
fn ls_json_carries_the_filename_as_well_as_the_id() {
    let (_root, paths, summary) = listable();
    let (id, slug) = parts(&summary);

    let out = cmd::ls(
        &paths,
        &cmd::List {
            format: cmd::Format::Json,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(out.ends_with("}\n"), "one object, one line: {out}");
    assert!(out.contains(&format!("\"id\":\"{id}\"")), "{out}");
    assert!(out.contains(&format!("\"slug\":\"{slug}\"")), "{out}");
    assert!(
        out.contains(&format!("\"file\":\"{id}-{slug}.md\"")),
        "the name a script needs next, not one it has to derive: {out}"
    );
    assert!(out.contains("\"title\":\"Meeting Notes\""), "{out}");
    assert!(out.contains("\"tags\":[\"work\"]"), "{out}");
    assert!(out.contains("\"files\":[\"my diagram.png\"]"), "{out}");
    assert!(out.contains("\"notebook\":\"default\""), "{out}");
}

/// The times are in `--json` whether or not `--time` was passed: `--time` is
/// about what fits on a terminal, and a program is not reading a terminal.
#[test]
fn ls_json_always_carries_the_times() {
    let (_root, paths, _) = listable();
    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);
    std::fs::write(
        notebook.join("k3f9m2p1-undated.md"),
        "---\ntitle: Undated\n---\n\nbody\n",
    )
    .unwrap();

    let out = cmd::ls(
        &paths,
        &cmd::List {
            format: cmd::Format::Json,
            ..Default::default()
        },
    )
    .unwrap();

    let created = out.match_indices("\"created\":").count();
    assert_eq!(created, 2, "one per note, present or not: {out}");
    assert!(
        out.contains("\"created\":null,\"updated\":null"),
        "a note with no times says so rather than dropping the keys: {out}"
    );
    assert!(out.contains("\"updated\":\"20"), "{out}");
}

/// The default listing answers "which note is this", and the title is the
/// answer. The slug says the same words with the spaces taken out, and two
/// RFC 3339 columns are forty characters of a question nobody asked — none of
/// which costs anything to read, which is why this is about width and not cost.
#[test]
fn ls_long_adds_the_slug_and_the_times_and_says_when_there_are_none() {
    let (_root, paths) = initialized();
    let added = cmd::add(&paths, Some("Alpha"), Some("a\n"), &[]).unwrap();
    let (id, slug) = parts(&added);
    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);
    std::fs::write(
        notebook.join("k3f9m2p1-undated.md"),
        "---\ntitle: Undated\n---\n\nbody\n",
    )
    .unwrap();

    let plain_out = cmd::ls(&paths, &cmd::List::default()).unwrap();
    assert!(!plain_out.contains('Z'), "no times by default: {plain_out}");
    let alpha = plain_out.lines().find(|l| l.contains("Alpha")).unwrap();
    assert_eq!(
        alpha.split_whitespace().collect::<Vec<_>>(),
        [id, "Alpha"],
        "the id and the title, and nothing that repeats the title: {alpha}"
    );

    let out = cmd::ls(
        &paths,
        &cmd::List {
            long: true,
            ..Default::default()
        },
    )
    .unwrap();
    let alpha = out.lines().find(|l| l.contains("Alpha")).unwrap();
    let columns: Vec<&str> = alpha.split_whitespace().collect();
    assert_eq!(columns[2], slug, "the slug comes back: {alpha}");
    assert_eq!(
        alpha.matches('Z').count(),
        2,
        "created and updated: {alpha}"
    );
    let undated = out.lines().find(|l| l.contains("Undated")).unwrap();
    let columns: Vec<&str> = undated.split_whitespace().collect();
    assert_eq!(
        columns,
        ["k3f9m2p1", "Undated", "undated", "-", "-"],
        "a hole the eye would have to measure is filled in: {undated}"
    );
}

/// `-l` extends the default row rather than rearranging it. A script that cuts
/// the first two fields off the front reads the same thing either way, and the
/// one field a note may not have stays at the end of both, where its absence
/// moves nothing.
#[test]
fn ls_long_keeps_the_columns_the_default_listing_starts_with() {
    let (_root, paths) = initialized();
    cmd::add(&paths, Some("Alpha"), Some("a\n"), &["work".to_string()]).unwrap();
    cmd::add(&paths, Some("Bravo"), Some("b\n"), &[]).unwrap();

    let head = |long| {
        cmd::ls(
            &paths,
            &cmd::List {
                long,
                ..Default::default()
            },
        )
        .unwrap()
        .lines()
        .map(|l| l.split_whitespace().take(2).collect::<Vec<_>>().join(" "))
        .collect::<Vec<_>>()
    };
    assert_eq!(head(false), head(true), "the id and the title, either way");

    let long = cmd::ls(
        &paths,
        &cmd::List {
            long: true,
            ..Default::default()
        },
    )
    .unwrap();
    let alpha = long.lines().find(|l| l.contains("Alpha")).unwrap();
    assert!(
        alpha.trim_end().ends_with("[work]"),
        "tags stay at the end: {alpha}"
    );
    let bravo = long.lines().find(|l| l.contains("Bravo")).unwrap();
    assert_eq!(
        bravo.split_whitespace().count(),
        5,
        "and a note without them ends one column earlier, not one column over: {bravo}"
    );
}

/// Ordering parses the stamps rather than comparing them as text. noda's own
/// are fixed-width UTC and would sort either way, but an imported note carries
/// the offset its own system used — and that is exactly the case this exists
/// for.
#[test]
fn ls_sorts_by_time_across_the_offsets_an_import_brings() {
    let (_root, paths) = initialized();
    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);
    // 08:21Z, written down as 16:21+08:00 — text order would put it last.
    for (name, title, created) in [
        ("k3f9m2p1-middle.md", "Middle", "2019-03-14T16:21:00+08:00"),
        ("k3f9m2p2-oldest.md", "Oldest", "2019-03-14T07:00:00Z"),
        ("k3f9m2p3-newest.md", "Newest", "2019-03-14T09:00:00Z"),
    ] {
        std::fs::write(
            notebook.join(name),
            format!("---\ntitle: {title}\ncreated: {created}\n---\n\nbody\n"),
        )
        .unwrap();
    }
    std::fs::write(
        notebook.join("k3f9m2p4-undated.md"),
        "---\ntitle: Undated\n---\n\nbody\n",
    )
    .unwrap();

    let titles = |sort| {
        cmd::ls(
            &paths,
            &cmd::List {
                sort,
                ..Default::default()
            },
        )
        .unwrap()
        .lines()
        .filter_map(|l| l.split_whitespace().nth(1).map(str::to_string))
        .collect::<Vec<_>>()
    };

    assert_eq!(
        titles(cmd::Sort::Created),
        ["Newest", "Middle", "Oldest", "Undated"],
        "newest first, and a note with no time to sort by sorts last"
    );
    assert_eq!(
        titles(cmd::Sort::Title),
        ["Middle", "Newest", "Oldest", "Undated"]
    );
    assert_eq!(
        titles(cmd::Sort::Slug),
        ["Middle", "Newest", "Oldest", "Undated"],
        "by slug, which is what the walk already produced"
    );
}

/// `--reverse` is applied after the sort, so it turns whichever order was asked
/// for — including the walk's own, which is why it does not require `--sort`.
#[test]
fn ls_reverse_turns_whichever_order_was_asked_for() {
    let (_root, paths) = initialized();
    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);
    for (name, title, created) in [
        ("k3f9m2p1-alpha.md", "Alpha", "2019-03-14T07:00:00Z"),
        ("k3f9m2p2-bravo.md", "Bravo", "2019-03-14T09:00:00Z"),
    ] {
        std::fs::write(
            notebook.join(name),
            format!("---\ntitle: {title}\ncreated: {created}\n---\n\nbody\n"),
        )
        .unwrap();
    }
    std::fs::write(
        notebook.join("k3f9m2p3-undated.md"),
        "---\ntitle: Undated\n---\n\nbody\n",
    )
    .unwrap();

    let titles = |sort, reverse| {
        cmd::ls(
            &paths,
            &cmd::List {
                sort,
                reverse,
                ..Default::default()
            },
        )
        .unwrap()
        .lines()
        .filter_map(|l| l.split_whitespace().nth(1).map(str::to_string))
        .collect::<Vec<_>>()
    };

    assert_eq!(
        titles(cmd::Sort::Created, true),
        ["Undated", "Alpha", "Bravo"],
        "oldest first, and the note with no time to sort by now leads"
    );
    assert_eq!(
        titles(cmd::Sort::Title, true),
        ["Undated", "Bravo", "Alpha"],
        "Z to A"
    );
    assert_eq!(
        titles(cmd::Sort::Slug, true),
        ["Undated", "Bravo", "Alpha"],
        "the default order turns too: that is what asking for it alone means"
    );
    assert_eq!(
        titles(cmd::Sort::Created, false),
        ["Bravo", "Alpha", "Undated"],
        "and without the flag nothing moved"
    );
}

/// One listing, one order. A table whose notes run Z-to-A while its files run
/// A-to-Z is not an order anyone asked for.
#[test]
fn ls_reverse_turns_the_files_with_the_notes() {
    let (root, paths) = initialized();
    cmd::add(&paths, Some("Alpha"), Some("a\n"), &[]).unwrap();
    for name in ["one.txt", "two.txt", "three.txt"] {
        let path = root.0.join(name);
        std::fs::write(&path, "x").unwrap();
        cmd::file_add(&paths, &[path], None).unwrap();
    }

    let files = |reverse| {
        let out = cmd::ls(
            &paths,
            &cmd::List {
                only: cmd::Only::Files,
                reverse,
                ..Default::default()
            },
        )
        .unwrap();
        plain(&out)
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty() && l != "files")
            .collect::<Vec<_>>()
    };

    assert_eq!(files(false), ["one.txt", "three.txt", "two.txt"]);
    assert_eq!(files(true), ["two.txt", "three.txt", "one.txt"]);
}

#[test]
fn ls_json_escapes_what_would_otherwise_break_the_document() {
    let (_root, paths) = initialized();
    cmd::add(&paths, Some(r#"He said "hi" \ bye"#), Some("x\n"), &[]).unwrap();

    let out = cmd::ls(
        &paths,
        &cmd::List {
            format: cmd::Format::Json,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(out.contains(r#""title":"He said \"hi\" \\ bye""#), "{out}");
}

#[test]
fn ls_json_says_so_when_the_notebook_is_empty() {
    let (_root, paths) = initialized();
    let out = cmd::ls(
        &paths,
        &cmd::List {
            format: cmd::Format::Json,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        out.trim_end(),
        r#"{"notebook":"default","notes":[],"files":[]}"#,
        "an empty listing is still a document a program can parse"
    );
}

/// A note is addressed by its id and a file by its name, so that is what each
/// one prints — whatever takes the output next expects exactly those.
#[test]
fn ls_quiet_prints_one_identifier_per_record() {
    let (_root, paths, summary) = listable();
    let (id, _) = parts(&summary);

    let out = cmd::ls(
        &paths,
        &cmd::List {
            format: cmd::Format::Quiet,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(out, format!("{id}\nmy diagram.png\n"));
}

/// The reason `-0` exists: `noda file add` allows a space in a name, so a
/// newline-separated list is not safe to hand to `xargs`.
#[test]
fn ls_quiet_can_separate_with_nul() {
    let (_root, paths, summary) = listable();
    let (id, _) = parts(&summary);

    let out = cmd::ls(
        &paths,
        &cmd::List {
            format: cmd::Format::Quiet,
            null: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(out, format!("{id}\0my diagram.png\0"));
}

#[test]
fn ls_can_leave_out_either_half() {
    let (_root, paths, summary) = listable();
    let (id, _) = parts(&summary);

    let notes = cmd::ls(
        &paths,
        &cmd::List {
            format: cmd::Format::Quiet,
            only: cmd::Only::Notes,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(notes, format!("{id}\n"));

    let files = cmd::ls(
        &paths,
        &cmd::List {
            format: cmd::Format::Quiet,
            only: cmd::Only::Files,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(files, "my diagram.png\n");

    // And the same subsetting reaches the other two formats.
    let table = plain(
        &cmd::ls(
            &paths,
            &cmd::List {
                only: cmd::Only::Files,
                ..Default::default()
            },
        )
        .unwrap(),
    );
    assert!(table.contains("my diagram.png"), "{table}");
    assert!(!table.contains("meeting-notes"), "{table}");
}

/// The one test that runs the real binary.
///
/// Everything else here calls the command functions, which is where the
/// behaviour lives — but `-0` is a promise about the *bytes that leave the
/// process*, and the layer between the two ate them: colour handling strips NUL
/// along with the escape sequences it exists to remove, and the newline every
/// other command wants would have arrived after the last terminator.
#[test]
fn ls_null_separators_survive_the_way_out_of_the_process() {
    let (root, _paths, _) = listable();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_noda"))
        .args(["ls", "-q0", "--files-only"])
        .env("XDG_CONFIG_HOME", root.0.join("config"))
        .env("XDG_DATA_HOME", root.0.join("data"))
        .env("XDG_STATE_HOME", root.0.join("state"))
        .env("XDG_CACHE_HOME", root.0.join("cache"))
        .output()
        .expect("run noda");

    assert_eq!(
        output.stdout,
        b"my diagram.png\0",
        "stdout was {:?}",
        String::from_utf8_lossy(&output.stdout)
    );
}

/// A file somewhere else on disk, to copy into a notebook from.
fn source_file(root: &TempRoot, name: &str) -> PathBuf {
    let dir = root.0.join("elsewhere");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, "contents\n").unwrap();
    path
}

#[test]
fn file_add_copies_it_in_and_commits_it() {
    let (root, paths) = initialized();
    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);
    let source = source_file(&root, "diagram.png");
    let commits = commit_count(&notebook);

    let out = plain(&cmd::file_add(&paths, std::slice::from_ref(&source), None).unwrap());
    assert_eq!(out.trim_end(), "added  diagram.png");
    assert!(notebook.join("diagram.png").is_file());
    assert!(source.is_file(), "a copy, so the original stays put");
    assert_eq!(
        commit_count(&notebook),
        commits + 1,
        "one commit, revertible like every other change"
    );
    assert!(
        plain(&cmd::ls(&paths, &cmd::List::default()).unwrap()).contains("files\n  diagram.png"),
        "and it is listed"
    );
}

#[test]
fn file_add_takes_several_at_once_in_one_commit() {
    let (root, paths) = initialized();
    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);
    let sources = vec![source_file(&root, "a.png"), source_file(&root, "b.pdf")];
    let commits = commit_count(&notebook);

    let out = plain(&cmd::file_add(&paths, &sources, None).unwrap());
    assert!(out.contains("added  a.png"), "{out}");
    assert!(out.contains("added  b.pdf"), "{out}");
    assert_eq!(commit_count(&notebook), commits + 1, "one commit, not two");
}

#[test]
fn file_add_will_not_overwrite_what_the_notebook_already_holds() {
    let (root, paths) = initialized();
    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);
    let source = source_file(&root, "diagram.png");
    cmd::file_add(&paths, std::slice::from_ref(&source), None).unwrap();
    std::fs::write(notebook.join("diagram.png"), "the one already here\n").unwrap();

    let err = cmd::file_add(&paths, std::slice::from_ref(&source), None)
        .unwrap_err()
        .to_string();
    assert!(err.contains("already holds diagram.png"), "{err}");
    assert!(err.contains("--as"), "and says how to get past it: {err}");
    assert_eq!(
        std::fs::read_to_string(notebook.join("diagram.png")).unwrap(),
        "the one already here\n",
        "untouched"
    );

    cmd::file_add(&paths, &[source], Some("diagram-2.png")).unwrap();
    assert!(notebook.join("diagram-2.png").is_file());
}

/// Nothing is copied until every source has been checked, so a command that
/// fails leaves the notebook exactly as it found it.
#[test]
fn file_add_copies_nothing_when_one_of_them_cannot_be_added() {
    let (root, paths) = initialized();
    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);
    let good = source_file(&root, "good.png");
    let missing = root.0.join("elsewhere").join("not-here.png");
    let commits = commit_count(&notebook);

    assert!(cmd::file_add(&paths, &[good, missing], None).is_err());
    assert!(
        !notebook.join("good.png").exists(),
        "the one that could have been copied was not"
    );
    assert_eq!(commit_count(&notebook), commits);
}

#[test]
fn file_add_refuses_the_names_it_could_not_then_account_for() {
    let (root, paths) = initialized();
    let source = source_file(&root, "diagram.png");

    for (rename, expected) in [
        (".hidden.png", "dotfiles"),
        ("sub/x.png", "cannot be a path"),
        // A `*.md` whose name splits into an id and a slug reads as a note that
        // has lost its frontmatter, and `doctor` would report it as one.
        ("abcdefgh-hello.md", "claims a note's id"),
    ] {
        let err = cmd::file_add(&paths, std::slice::from_ref(&source), Some(rename))
            .unwrap_err()
            .to_string();
        assert!(err.contains(expected), "{rename}: {err}");
    }

    let err = cmd::file_add(&paths, &[source.clone(), source], Some("x.png"))
        .unwrap_err()
        .to_string();
    assert!(err.contains("cannot be given with several"), "{err}");
}

#[test]
fn file_add_refuses_a_directory() {
    let (root, paths) = initialized();
    source_file(&root, "inside.png");
    let dir = root.0.join("elsewhere");

    let err = cmd::file_add(&paths, &[dir], None).unwrap_err().to_string();
    assert!(err.contains("not a file"), "{err}");
}

#[test]
fn file_rm_removes_it_as_a_commit() {
    let (root, paths) = initialized();
    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);
    cmd::file_add(&paths, &[source_file(&root, "diagram.png")], None).unwrap();
    let commits = commit_count(&notebook);

    let out = plain(&cmd::file_rm(&paths, "diagram.png").unwrap());
    assert_eq!(out.trim_end(), "removed  diagram.png");
    assert!(!notebook.join("diagram.png").exists());
    assert_eq!(commit_count(&notebook), commits + 1);
}

/// The two are not interchangeable: a note has an identity to lose, so the
/// command that removes one is not the command that removes a file.
#[test]
fn file_rm_refuses_a_note_and_says_which_command_wants_it() {
    let (_root, paths) = initialized();
    let summary = cmd::add(&paths, Some("Alpha"), Some("a\n"), &[]).unwrap();
    let file = note_file(&summary);

    let err = cmd::file_rm(&paths, &file).unwrap_err().to_string();
    assert!(err.contains("is a note"), "{err}");
    assert!(err.contains("`noda rm`"), "{err}");
    assert!(
        paths
            .notebook_dir(cmd::DEFAULT_NOTEBOOK)
            .join(&file)
            .exists(),
        "and the note is still there"
    );
}

#[test]
fn file_rm_says_so_when_there_is_no_such_file() {
    let (_root, paths) = initialized();
    let err = cmd::file_rm(&paths, "nope.txt").unwrap_err().to_string();
    assert!(err.contains("no file called nope.txt"), "{err}");
}

/// The default: rename it, then say what that just broke. Silence here would
/// leave links pointing at nothing with nothing having said so.
#[test]
fn file_mv_renames_and_reports_the_links_it_stranded() {
    let (root, paths) = initialized();
    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);
    cmd::file_add(
        &paths,
        std::slice::from_ref(&source_file(&root, "old.png")),
        None,
    )
    .unwrap();
    cmd::add(&paths, Some("Alpha"), Some("![a](old.png)\n"), &[]).unwrap();
    let commits = commit_count(&notebook);

    let out = plain(&cmd::file_mv(&paths, "old.png", "new.png", false).unwrap());
    assert!(out.contains("renamed  old.png -> new.png"), "{out}");
    assert!(out.contains("1 note links to old.png"), "{out}");
    assert!(out.contains("alpha.md"), "and says which: {out}");
    assert!(notebook.join("new.png").is_file());
    assert!(!notebook.join("old.png").exists());
    assert_eq!(commit_count(&notebook), commits + 1);
    assert!(
        !out.contains("updated"),
        "the notes were not touched: {out}"
    );
}

/// Renaming an attachment rewrites links in notes the command was not pointed
/// at. That is a mechanical fixup, not somebody editing their notes, and dating
/// every one of them today would flatten the order they are read in — which is
/// most of what `updated` is for.
#[test]
fn renaming_a_file_does_not_date_the_notes_that_linked_to_it() {
    let (root, paths) = initialized();
    cmd::file_add(
        &paths,
        std::slice::from_ref(&source_file(&root, "old.png")),
        None,
    )
    .unwrap();
    cmd::add(&paths, Some("Alpha"), Some("![a](old.png)\n"), &[]).unwrap();
    let before = times(&paths, "alpha");

    cmd::file_mv(&paths, "old.png", "new.png", true).unwrap();

    assert!(
        note_text(&paths, "alpha").contains("![a](new.png)"),
        "the link was rewritten"
    );
    assert_eq!(times(&paths, "alpha"), before, "the note was not edited");
}

/// Opt-in, because it edits the prose of notes the command was not pointed at.
#[test]
fn file_mv_update_links_rewrites_both_spellings_and_leaves_it_in_order() {
    let (root, paths) = initialized();
    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);
    cmd::file_add(
        &paths,
        std::slice::from_ref(&source_file(&root, "old.png")),
        None,
    )
    .unwrap();
    let first = cmd::add(
        &paths,
        Some("Alpha"),
        Some("Inline ![a](old.png) and a reference ![b][r].\n\n[r]: old.png\n"),
        &[],
    )
    .unwrap();
    let second = cmd::add(&paths, Some("Beta"), Some("![c](old.png#page=2)\n"), &[]).unwrap();
    let commits = commit_count(&notebook);

    let out = plain(&cmd::file_mv(&paths, "old.png", "new.png", true).unwrap());
    assert!(out.contains("updated  2 notes"), "{out}");
    assert!(!out.contains("still link"), "nothing was missed: {out}");

    let alpha = std::fs::read_to_string(notebook.join(note_file(&first))).unwrap();
    assert!(alpha.contains("![a](new.png)"), "{alpha}");
    assert!(alpha.contains("[r]: new.png"), "{alpha}");
    assert!(alpha.starts_with("---\ntitle: Alpha\n"), "frontmatter kept");
    let beta = std::fs::read_to_string(notebook.join(note_file(&second))).unwrap();
    assert!(
        beta.contains("![c](new.png#page=2)"),
        "the fragment says how to open it, not which file: {beta}"
    );

    assert!(
        plain(&cmd::doctor(&paths, false, true, false).unwrap()).contains("in order"),
        "no orphan and no broken link is left behind"
    );
    assert_eq!(
        commit_count(&notebook),
        commits + 1,
        "the rename and the rewrites are one commit"
    );
}

/// A destination written with backslash escapes is not in the source literally,
/// so it cannot be rewritten. It is reported rather than assumed fixed.
#[test]
fn file_mv_says_which_notes_it_could_not_rewrite() {
    let (root, paths) = initialized();
    cmd::file_add(
        &paths,
        std::slice::from_ref(&source_file(&root, "my(file).png")),
        None,
    )
    .unwrap();
    cmd::add(&paths, Some("Alpha"), Some("[a](my\\(file\\).png)\n"), &[]).unwrap();

    let out = plain(&cmd::file_mv(&paths, "my(file).png", "new.png", true).unwrap());
    assert!(out.contains("renamed"), "{out}");
    assert!(
        out.contains("1 note still links to my(file).png"),
        "reported, not silently left: {out}"
    );
}

#[test]
fn file_mv_refuses_what_it_should_not_rename() {
    let (root, paths) = initialized();
    let source = source_file(&root, "a.png");
    cmd::file_add(&paths, std::slice::from_ref(&source), None).unwrap();
    cmd::file_add(&paths, std::slice::from_ref(&source), Some("b.png")).unwrap();
    let summary = cmd::add(&paths, Some("Alpha"), Some("x\n"), &[]).unwrap();

    for (old, new, expected) in [
        ("a.png", "b.png", "already holds b.png"),
        ("a.png", "a.png", "already is its name"),
        ("a.png", "abcdefgh-hello.md", "claims a note's id"),
        ("nope.png", "x.png", "no file called nope.png"),
    ] {
        let err = cmd::file_mv(&paths, old, new, false)
            .unwrap_err()
            .to_string();
        assert!(err.contains(expected), "{old} -> {new}: {err}");
    }

    let err = cmd::file_mv(&paths, &note_file(&summary), "x.png", false)
        .unwrap_err()
        .to_string();
    assert!(err.contains("is a note"), "{err}");
    assert!(err.contains("`noda mv`"), "{err}");
}

#[test]
fn path_prints_the_notebook_a_note_and_a_file() {
    let (root, paths) = initialized();
    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);
    let summary = cmd::add(&paths, Some("Meeting Notes"), Some("x\n"), &[]).unwrap();
    let (id, _) = parts(&summary);
    cmd::file_add(
        &paths,
        std::slice::from_ref(&source_file(&root, "diagram.png")),
        None,
    )
    .unwrap();

    assert_eq!(
        cmd::path(&paths, None).unwrap().trim_end(),
        notebook.display().to_string(),
        "no argument is the notebook itself"
    );
    let note = notebook.join(note_file(&summary));
    assert_eq!(
        cmd::path(&paths, Some("meeting-notes")).unwrap().trim_end(),
        note.display().to_string()
    );
    assert_eq!(
        cmd::path(&paths, Some(&id[..4])).unwrap().trim_end(),
        note.display().to_string(),
        "an id prefix addresses a note here like everywhere else"
    );
    assert_eq!(
        cmd::path(&paths, Some("diagram.png")).unwrap().trim_end(),
        notebook.join("diagram.png").display().to_string()
    );
}

#[test]
fn path_says_so_when_nothing_answers_to_the_key() {
    let (_root, paths) = initialized();
    let err = cmd::path(&paths, Some("nope")).unwrap_err().to_string();
    assert!(err.contains("no note and no file"), "{err}");
}

/// A file may be named exactly like a note's slug. noda never guesses which one
/// was meant.
#[test]
fn path_refuses_a_key_that_names_both_a_note_and_a_file() {
    let (root, paths) = initialized();
    cmd::add(&paths, Some("Diagram"), Some("x\n"), &[]).unwrap();
    cmd::file_add(
        &paths,
        std::slice::from_ref(&source_file(&root, "diagram")),
        None,
    )
    .unwrap();

    let err = cmd::path(&paths, Some("diagram")).unwrap_err().to_string();
    assert!(err.contains("names both a note and a file"), "{err}");
    assert!(err.contains("diagram.md"), "and lists them: {err}");
}

/// A query the way a shell hands one over: one token per word.
fn search(paths: &Paths, query: &str) -> noda::Result<String> {
    let tokens: Vec<String> = query
        .split(' ')
        .filter(|token| !token.is_empty())
        .map(str::to_string)
        .collect();
    cmd::search(paths, &tokens)
}

#[test]
fn search_matches_the_body_the_title_and_the_tags() {
    let (_root, paths) = initialized();
    cmd::add(
        &paths,
        Some("Meeting Notes"),
        Some("discuss the Q3 budget\nand the hiring plan\n"),
        &["work".to_string()],
    )
    .unwrap();
    cmd::add(
        &paths,
        Some("Reading Log"),
        Some("a book about budgets\n"),
        &[],
    )
    .unwrap();

    // A body hit quotes the line it was found on.
    let out = plain(&search(&paths, "Q3 BUDGET").unwrap());
    assert_eq!(out.lines().count(), 2, "one result and its excerpt: {out}");
    assert!(
        out.lines().next().unwrap().contains("Meeting Notes"),
        "{out}"
    );
    assert!(
        out.lines()
            .nth(1)
            .unwrap()
            .contains("discuss the Q3 budget"),
        "{out}"
    );

    // A title or tag hit needs no excerpt — it is already on the first line.
    let out = plain(&search(&paths, "work").unwrap());
    assert_eq!(out.lines().count(), 1, "{out}");
    assert!(out.contains("[work]"), "{out}");
    assert_eq!(
        plain(&search(&paths, "reading").unwrap()).lines().count(),
        1
    );

    // Substring, not whole word: "budget" finds "budgets" too. Both notes match
    // in the body, so both bring an excerpt with them.
    let out = plain(&search(&paths, "budget").unwrap());
    assert_eq!(out.lines().count(), 4, "{out}");
    assert!(out.contains("a book about budgets"), "{out}");
    assert!(search(&paths, "absent").unwrap().is_empty());
}

#[test]
fn search_requires_every_term_but_not_their_order() {
    let (_root, paths) = initialized();
    cmd::add(&paths, Some("Alpha"), Some("budget for the offsite\n"), &[]).unwrap();
    cmd::add(&paths, Some("Beta"), Some("budget only\n"), &[]).unwrap();

    let out = plain(&search(&paths, "offsite budget").unwrap());
    assert!(out.contains("Alpha"), "{out}");
    assert!(!out.contains("beta"), "both terms are required: {out}");

    assert!(search(&paths, "   ").is_err(), "a query is required");
}

#[test]
fn search_works_on_a_language_without_spaces() {
    let (_root, paths) = initialized();
    cmd::add(
        &paths,
        Some("會議記錄"),
        Some("討論第三季預算與人力計畫\n"),
        &[],
    )
    .unwrap();
    cmd::add(&paths, Some("Reading Log"), Some("unrelated\n"), &[]).unwrap();

    // No word boundaries to tokenise on: substring matching is the whole point.
    let out = plain(&search(&paths, "第三季預算").unwrap());
    assert_eq!(out.lines().count(), 2, "{out}");
    assert!(out.contains("討論第三季預算與人力計畫"), "{out}");
    assert!(plain(&search(&paths, "會議").unwrap()).contains("會議記錄"));
}

#[test]
fn search_only_looks_at_the_note_not_the_file_around_it() {
    let (_root, paths) = initialized();
    let added = cmd::add(&paths, Some("Alpha"), Some("body\n"), &[]).unwrap();
    let id = added.split_once("  ").unwrap().0;

    // The frontmatter is the container, not searchable text. Spelled `text:---`
    // because a leading `-` is a negation, and the field prefix is the way to
    // say a term starts with one.
    assert!(search(&paths, "text:---").unwrap().is_empty());
    assert!(search(&paths, "title:").is_err(), "a field needs a value");
    // A bare word is text, and the id is not written anywhere in the note — it
    // is the filename. Asking for it as text finds nothing; `id:` is how you
    // ask for it as an identity.
    assert!(search(&paths, id).unwrap().is_empty());
    assert!(
        search(&paths, &format!("id:{}", &id[..4]))
            .unwrap()
            .contains("Alpha")
    );
}

#[test]
fn log_reports_the_notebook_history_newest_first() {
    let (_root, paths) = initialized();
    cmd::add(&paths, Some("Alpha"), Some("a\n"), &[]).unwrap();
    cmd::add(&paths, Some("Beta"), Some("b\n"), &[]).unwrap();

    let out = plain(&cmd::log(&paths, None, None).unwrap());
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 3, "{out}");
    assert!(lines[0].ends_with("add: beta"), "{out}");
    assert!(lines[2].ends_with("chore: initialize notebook"), "{out}");

    let fields: Vec<&str> = lines[0].split("  ").collect();
    assert_eq!(fields[0].len(), 7, "abbreviated commit id: {out}");
    assert_eq!(fields[1].len(), 16, "YYYY-MM-DD HH:MM: {out}");

    let limited = cmd::log(&paths, None, Some(1)).unwrap();
    assert_eq!(limited.lines().count(), 1);
}

#[test]
fn log_for_a_note_follows_it_across_a_rename() {
    let (_root, paths) = initialized();
    cmd::add(&paths, Some("Alpha"), Some("a\n"), &[]).unwrap();
    cmd::tag(&paths, "alpha", &["+work".to_string()], cmd::Touch::Stamp).unwrap();
    cmd::mv(&paths, "alpha", "Renamed", false, cmd::Touch::Stamp).unwrap();
    // A second note's history must not leak into the first note's.
    cmd::add(&paths, Some("Beta"), Some("b\n"), &[]).unwrap();

    let out = plain(&cmd::log(&paths, Some("renamed"), None).unwrap());
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 3, "{out}");
    assert!(lines[0].ends_with("mv: alpha -> renamed"), "{out}");
    assert!(lines[1].ends_with("tag: alpha"), "{out}");
    assert!(lines[2].ends_with("add: alpha"), "{out}");
    assert!(!out.contains("beta"), "{out}");

    // The id addresses the same history as the current slug does.
    let id = cmd::ls(&paths, &cmd::List::default())
        .unwrap()
        .lines()
        .find(|line| line.contains("Renamed"))
        .and_then(|line| line.split_whitespace().next())
        .expect("id")
        .to_string();
    assert_eq!(
        cmd::log(&paths, Some(&id), None).unwrap().lines().count(),
        3
    );
}

#[test]
fn diff_shows_the_last_commit_when_nothing_is_pending() {
    let (_root, paths) = initialized();
    let added = cmd::add(&paths, Some("Alpha"), Some("a\n"), &[]).unwrap();

    let out = plain(&cmd::diff(&paths, None).unwrap());
    assert!(
        out.contains(&format!("+++ b/{}", note_file(&added))),
        "{out}"
    );
    assert!(out.contains("+a"), "{out}");
}

#[test]
fn diff_shows_uncommitted_changes_when_there_are_some() {
    let (_root, paths) = initialized();
    let added = cmd::add(&paths, Some("Alpha"), Some("a\n"), &[]).unwrap();
    cmd::add(&paths, Some("Beta"), Some("b\n"), &[]).unwrap();

    let note = paths
        .notebook_dir(cmd::DEFAULT_NOTEBOOK)
        .join(note_file(&added));
    let text = std::fs::read_to_string(&note).unwrap();
    std::fs::write(&note, text.replace("a\n", "changed by hand\n")).unwrap();

    let out = plain(&cmd::diff(&paths, None).unwrap());
    assert!(out.contains("+changed by hand"), "{out}");
    assert!(out.contains("-a"), "{out}");
    assert!(!out.contains("beta"), "only what changed: {out}");

    // And it can be narrowed to one note.
    let scoped = plain(&cmd::diff(&paths, Some("beta")).unwrap());
    assert!(scoped.is_empty(), "beta is untouched: {scoped}");
}

#[test]
fn restore_returns_a_note_to_an_earlier_version_as_a_new_commit() {
    let (_root, paths) = initialized();
    let added = cmd::add(&paths, Some("Alpha"), Some("first\n"), &[]).unwrap();

    let note = paths
        .notebook_dir(cmd::DEFAULT_NOTEBOOK)
        .join(note_file(&added));
    let original = std::fs::read_to_string(&note).unwrap();
    std::fs::write(&note, original.replace("first\n", "second\n")).unwrap();
    commit_working_tree(&paths, cmd::DEFAULT_NOTEBOOK, "edit: alpha");
    let before = commit_count(&paths.notebook_dir(cmd::DEFAULT_NOTEBOOK));

    cmd::restore(&paths, "alpha", "HEAD~1", cmd::Touch::Stamp).unwrap();
    // Everything but `updated` comes back exactly: that one records when the
    // file changed, and it changed just now. It has its own test below.
    let held_aside = |text: &str| note::set_field(text, "updated", "-").unwrap();
    assert_eq!(
        held_aside(&std::fs::read_to_string(&note).unwrap()),
        held_aside(&original)
    );
    assert_eq!(
        commit_count(&paths.notebook_dir(cmd::DEFAULT_NOTEBOOK)),
        before + 1,
        "a restore moves history forward, it does not rewrite it"
    );
    assert!(
        cmd::log(&paths, Some("alpha"), None)
            .unwrap()
            .contains("restore: alpha")
    );

    // Restoring what is already there is not a commit.
    let out = cmd::restore(&paths, "alpha", "HEAD", cmd::Touch::Stamp).unwrap();
    assert!(out.contains("(no change)"), "{out}");
    assert_eq!(
        commit_count(&paths.notebook_dir(cmd::DEFAULT_NOTEBOOK)),
        before + 1
    );
}

/// The contents travel back; the record of when they landed does not. A version
/// from last year is being written to disk right now, and `updated` is noda's
/// answer to "when did this file last change", not to "which version is this".
#[test]
fn restore_dates_the_note_now_rather_than_then() {
    let (_root, paths) = initialized();
    let added = cmd::add(&paths, Some("Alpha"), Some("first\n"), &[]).unwrap();
    let path = paths
        .notebook_dir(cmd::DEFAULT_NOTEBOOK)
        .join(note_file(&added));

    let long_ago = note::set_field(
        &std::fs::read_to_string(&path).unwrap(),
        "updated",
        "2000-01-01T00:00:00Z",
    )
    .unwrap();
    std::fs::write(&path, &long_ago).unwrap();
    commit_working_tree(&paths, cmd::DEFAULT_NOTEBOOK, "edit: alpha");
    std::fs::write(&path, long_ago.replace("first\n", "second\n")).unwrap();
    commit_working_tree(&paths, cmd::DEFAULT_NOTEBOOK, "edit: alpha again");

    cmd::restore(&paths, "alpha", "HEAD~1", cmd::Touch::Stamp).unwrap();
    let after = std::fs::read_to_string(&path).unwrap();
    assert!(after.contains("first\n"), "the contents came back: {after}");
    assert!(
        !after.contains("2000-01-01"),
        "the date they were written did not: {after}"
    );

    // That same revision is now two back, the restore having moved history
    // forward. Asking for it again is not a change: its contents are already on
    // disk, and the only thing that differs is the timestamp the restore itself
    // wrote — which is not what is being compared.
    let out = cmd::restore(&paths, "alpha", "HEAD~2", cmd::Touch::Stamp).unwrap();
    assert!(out.contains("(no change)"), "{out}");
}

/// `--no-touch` turns a restore into the whole version coming back, `updated`
/// included — and with nothing written over the copy in history, "no change"
/// becomes an exact answer rather than one with a field held aside.
#[test]
fn restore_no_touch_brings_the_old_date_back_with_the_contents() {
    let (_root, paths) = initialized();
    let added = cmd::add(&paths, Some("Alpha"), Some("first\n"), &[]).unwrap();
    let path = backdate(&paths, &added);
    let long_ago = std::fs::read_to_string(&path).unwrap();
    commit_working_tree(&paths, cmd::DEFAULT_NOTEBOOK, "edit: alpha");

    let later = note::set_field(
        &long_ago.replace("first\n", "second\n"),
        "updated",
        "2024-01-01T00:00:00Z",
    )
    .unwrap();
    std::fs::write(&path, later).unwrap();
    commit_working_tree(&paths, cmd::DEFAULT_NOTEBOOK, "edit: alpha again");

    cmd::restore(&paths, "alpha", "HEAD~1", cmd::Touch::Keep).unwrap();
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        long_ago,
        "byte for byte the version that was asked for"
    );

    // Asking for that same revision again compares in full rather than around
    // `updated`, and still says nothing changed.
    let out = cmd::restore(&paths, "alpha", "HEAD~2", cmd::Touch::Keep).unwrap();
    assert!(out.contains("(no change)"), "{out}");
}

#[test]
fn restore_brings_back_a_deleted_note_with_its_id() {
    let (_root, paths) = initialized();
    let added = cmd::add(&paths, Some("Alpha"), Some("a\n"), &["work".to_string()]).unwrap();
    let id = parts(&added).0.to_string();
    let file = note_file(&added);
    cmd::rm(&paths, "alpha").unwrap();
    assert!(cmd::show(&paths, "alpha").is_err(), "gone");

    // Addressed by an id nothing on disk carries any more: the answer is in
    // history, where every commit records the filenames.
    let out = cmd::restore(&paths, &id, "HEAD~1", cmd::Touch::Stamp).unwrap();
    assert!(out.starts_with(&id), "the id comes back unchanged: {out}");
    assert!(cmd::show(&paths, &id).unwrap().contains("a\n"));
    assert!(
        cmd::ls(
            &paths,
            &cmd::List {
                tag: Some("work"),
                ..Default::default()
            }
        )
        .unwrap()
        .contains("Alpha")
    );
    assert!(
        paths
            .notebook_dir(cmd::DEFAULT_NOTEBOOK)
            .join(&file)
            .is_file(),
        "under the name it had"
    );
}

/// Deleted with `rm(1)` rather than `noda rm`, so nothing recorded that it went.
/// The filename is the record, and it comes back with it.
#[test]
fn restore_brings_back_a_note_deleted_outside_noda() {
    let (_root, paths) = initialized();
    let added = cmd::add(&paths, Some("Alpha"), Some("a\n"), &[]).unwrap();
    let id = parts(&added).0.to_string();
    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);

    std::fs::remove_file(notebook.join(note_file(&added))).unwrap();

    cmd::restore(&paths, &id, "HEAD", cmd::Touch::Stamp).unwrap();
    assert!(cmd::show(&paths, &id).unwrap().ends_with("a\n"));
    assert_eq!(
        status_row(&plain(&cmd::status(&paths).unwrap()), "problems"),
        None,
        "and nothing is left to report"
    );
}

/// The commit `deleted` prints is the one *before* the deletion, because that
/// is what `restore` has to be given — reporting the deletion and leaving the
/// `~1` to be worked out would be a remedy the reader has to finish.
#[test]
fn deleted_names_the_commit_that_brings_a_note_back() {
    let (_root, paths) = initialized();
    let added = cmd::add(&paths, Some("Gamma"), Some("g\n"), &[]).unwrap();
    let (id, slug) = parts(&added);
    let (id, slug) = (id.to_string(), slug.to_string());
    cmd::rm(&paths, "gamma").unwrap();

    let out = plain(&cmd::deleted(&paths, None, false).unwrap());
    let row = out.lines().next().unwrap();
    assert!(row.starts_with(&id), "{out}");
    assert!(row.contains(&slug), "{out}");
    assert!(
        row.ends_with("Gamma"),
        "the title it had when it went: {out}"
    );

    // The revision in the row is enough on its own.
    let commit = row.split_whitespace().nth(4).unwrap().to_string();
    cmd::restore(&paths, &slug, &commit, cmd::Touch::Stamp).unwrap();
    assert!(cmd::show(&paths, &id).unwrap().ends_with("g\n"));

    assert_eq!(
        cmd::deleted(&paths, None, false).unwrap(),
        "",
        "and it is not deleted any more — the check is against what is on disk, \
         not against what history did"
    );
}

/// `mv` changes a filename without changing an identity. Nothing was deleted,
/// and the tree comparison has to agree — it compares ids, not names.
#[test]
fn deleted_does_not_count_a_rename() {
    let (_root, paths) = initialized();
    cmd::add(&paths, Some("Beta"), Some("b\n"), &[]).unwrap();
    cmd::mv(&paths, "beta", "Beta Renamed", false, cmd::Touch::Stamp).unwrap();

    assert_eq!(cmd::deleted(&paths, None, false).unwrap(), "");
}

/// Nothing here reads a commit message, so a deletion made with plain git is
/// found exactly like one made with `noda rm`.
#[test]
fn deleted_finds_what_was_removed_outside_noda() {
    let (_root, paths) = initialized();
    let added = cmd::add(&paths, Some("Alpha"), Some("a\n"), &[]).unwrap();
    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);

    std::fs::remove_file(notebook.join(note_file(&added))).unwrap();
    commit_working_tree(&paths, cmd::DEFAULT_NOTEBOOK, "whatever i felt like typing");

    let out = plain(&cmd::deleted(&paths, None, false).unwrap());
    assert!(out.contains(parts(&added).0), "{out}");
    assert!(out.contains("Alpha"), "{out}");
}

/// Deleted twice with a restore in between: the disappearance that counts is
/// the last one, so the commit offered is the one that undoes *that*.
#[test]
fn deleted_reports_the_most_recent_disappearance() {
    let (_root, paths) = initialized();
    let added = cmd::add(&paths, Some("Alpha"), Some("first\n"), &[]).unwrap();
    let slug = parts(&added).1.to_string();

    cmd::rm(&paths, &slug).unwrap();
    let first = plain(&cmd::deleted(&paths, None, false).unwrap());
    let commit = first
        .lines()
        .next()
        .unwrap()
        .split_whitespace()
        .nth(4)
        .unwrap()
        .to_string();
    cmd::restore(&paths, &slug, &commit, cmd::Touch::Stamp).unwrap();

    // Change it, then lose it again. The older commit would bring back the
    // wrong contents.
    cmd::tag(&paths, &slug, &["+work".to_string()], cmd::Touch::Stamp).unwrap();
    cmd::rm(&paths, &slug).unwrap();

    let out = plain(&cmd::deleted(&paths, None, false).unwrap());
    assert_eq!(out.lines().count(), 2, "one note, one hint: {out}");
    let latest = out
        .lines()
        .next()
        .unwrap()
        .split_whitespace()
        .nth(4)
        .unwrap()
        .to_string();
    assert_ne!(latest, commit, "not the first deletion");

    cmd::restore(&paths, &slug, &latest, cmd::Touch::Stamp).unwrap();
    assert!(
        cmd::show(&paths, &slug).unwrap().contains("tags: [work]"),
        "the version that was actually lost came back"
    );
}

/// `--json` carries the full object ids, not the abbreviations the table shows:
/// `restore` takes either, and an abbreviation is a thing that can stop being
/// unique later.
#[test]
fn deleted_json_carries_what_a_script_needs_to_restore() {
    let (_root, paths) = initialized();
    let added = cmd::add(&paths, Some("Gamma"), Some("g\n"), &[]).unwrap();
    let (id, slug) = parts(&added);
    let (id, slug) = (id.to_string(), slug.to_string());
    cmd::rm(&paths, &slug).unwrap();

    let out = cmd::deleted(&paths, None, true).unwrap();
    assert!(out.ends_with("}\n"), "one object, one line: {out}");
    assert!(out.contains("\"notebook\":\"default\""), "{out}");
    assert!(out.contains(&format!("\"id\":\"{id}\"")), "{out}");
    assert!(out.contains(&format!("\"slug\":\"{slug}\"")), "{out}");
    assert!(
        out.contains(&format!("\"file\":\"{id}-{slug}.md\"")),
        "the name it had when it went: {out}"
    );
    assert!(out.contains("\"title\":\"Gamma\""), "{out}");
    // RFC 3339 UTC, the same spelling a note's own times use.
    assert!(out.contains("\"removed_at\":\"20"), "{out}");
    assert!(out.contains("Z\""), "{out}");

    // The revision is usable straight out of the document, at full length.
    let restore_from = out
        .split("\"restore_from\":\"")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .unwrap()
        .to_string();
    assert_eq!(restore_from.len(), 40, "a full object id: {restore_from}");
    cmd::restore(&paths, &slug, &restore_from, cmd::Touch::Stamp).unwrap();
    assert!(cmd::show(&paths, &id).unwrap().ends_with("g\n"));
}

/// A program asking for JSON gets a document either way — an empty list is an
/// answer, where the table prints nothing at all.
#[test]
fn deleted_json_is_a_document_even_when_nothing_is_gone() {
    let (_root, paths) = initialized();
    cmd::add(&paths, Some("Alpha"), Some("a\n"), &[]).unwrap();

    assert_eq!(cmd::deleted(&paths, None, false).unwrap(), "");
    assert_eq!(
        cmd::deleted(&paths, None, true).unwrap(),
        "{\"notebook\":\"default\",\"deleted\":[]}\n"
    );
}

#[test]
fn deleted_can_target_another_notebook() {
    let (_root, paths) = initialized();
    cmd::add(&paths, Some("Alpha"), Some("a\n"), &[]).unwrap();
    cmd::rm(&paths, "alpha").unwrap();
    noda::notebook::Notebook::create(&paths, "work").unwrap();

    assert!(
        cmd::deleted(&paths, Some("work"), false)
            .unwrap()
            .is_empty(),
        "the other notebook has lost nothing"
    );
    assert!(
        cmd::deleted(&paths, Some("default"), false)
            .unwrap()
            .contains("alpha")
    );
    assert!(cmd::deleted(&paths, Some("missing"), false).is_err());
}

#[test]
fn deleted_says_nothing_when_nothing_is_gone() {
    let (_root, paths) = initialized();
    cmd::add(&paths, Some("Alpha"), Some("a\n"), &[]).unwrap();

    assert_eq!(cmd::deleted(&paths, None, false).unwrap(), "");
}

#[test]
fn restore_reports_what_it_cannot_find() {
    let (_root, paths) = initialized();
    cmd::add(&paths, Some("Alpha"), Some("a\n"), &[]).unwrap();

    let err = cmd::restore(&paths, "alpha", "nonsense", cmd::Touch::Stamp)
        .unwrap_err()
        .to_string();
    assert!(err.contains("unknown revision"), "{err}");

    // The note exists now, but not that far back.
    let err = cmd::restore(&paths, "alpha", "HEAD~1", cmd::Touch::Stamp)
        .unwrap_err()
        .to_string();
    assert!(err.contains("did not exist"), "{err}");

    assert!(cmd::restore(&paths, "missing", "HEAD", cmd::Touch::Stamp).is_err());
}

fn config_file(paths: &Paths) -> PathBuf {
    paths.config_dir().join("config.toml")
}

#[test]
fn init_leaves_a_starter_config_that_changes_nothing() {
    let (_root, paths) = initialized();

    let text = std::fs::read_to_string(config_file(&paths)).expect("config written");
    assert!(text.contains("# editor ="), "{text}");
    assert!(text.contains("# author ="), "{text}");
    assert!(text.contains("# notebook ="), "{text}");
    assert!(
        text.lines()
            .all(|line| line.trim().is_empty() || line.trim_start().starts_with('#')),
        "everything is commented out, so the defaults still apply: {text}"
    );

    // Which means every setting still reports itself as a default.
    let shown = plain(&cmd::config_show(&paths).unwrap());
    assert_eq!(shown.lines().count(), 3, "{shown}");
    assert!(shown.contains("notebook  default"), "{shown}");

    // And a second init does not overwrite what the user has since written.
    std::fs::write(config_file(&paths), "editor = \"nvim\"\n").unwrap();
    cmd::init(&paths).unwrap();
    assert_eq!(
        std::fs::read_to_string(config_file(&paths)).unwrap(),
        "editor = \"nvim\"\n"
    );
}

#[test]
fn config_set_and_get_round_trip_and_report_their_source() {
    let (_root, paths) = initialized();

    cmd::config_set(&paths, "editor", "nvim").unwrap();
    assert_eq!(cmd::config_get(&paths, "editor").unwrap(), "nvim");

    let shown = plain(&cmd::config_show(&paths).unwrap());
    assert!(shown.contains("editor    nvim"), "{shown}");
    assert!(shown.contains("(config.toml)"), "{shown}");

    // Unsetting drops back to the environment or the built-in. What that value
    // is depends on the machine running the tests, so the check is on where the
    // value now comes from, not on what it is.
    let out = cmd::config_unset(&paths, "editor").unwrap();
    assert!(out.contains("now from"), "{out}");
    let shown = plain(&cmd::config_show(&paths).unwrap());
    let row = shown
        .lines()
        .find(|line| line.starts_with("editor"))
        .unwrap();
    assert!(!row.contains("(config.toml)"), "{shown}");
    assert!(
        cmd::config_unset(&paths, "editor")
            .unwrap()
            .contains("was not set")
    );
}

#[test]
fn the_first_setting_written_lands_under_the_header_not_above_it() {
    let (_root, paths) = initialized();
    cmd::config_set(&paths, "editor", "helix").unwrap();

    let text = std::fs::read_to_string(config_file(&paths)).unwrap();
    let header = text.find("# noda configuration").expect("header kept");
    let setting = text.find("editor = \"helix\"").expect("setting written");
    assert!(
        header < setting,
        "a config that reads back to front is worse than no comments: {text}"
    );
}

#[test]
fn config_set_keeps_the_comments_around_it() {
    let (_root, paths) = initialized();
    std::fs::write(
        config_file(&paths),
        "# my notes identity, not my work one\nauthor = \"Someone <s@example.com>\"\n\n# the editor I like\neditor = \"helix\"\n",
    )
    .unwrap();

    cmd::config_set(&paths, "editor", "nvim").unwrap();

    let text = std::fs::read_to_string(config_file(&paths)).unwrap();
    assert!(
        text.contains("# my notes identity, not my work one"),
        "{text}"
    );
    assert!(text.contains("# the editor I like"), "{text}");
    assert!(text.contains("editor = \"nvim\""), "{text}");
    assert!(
        text.contains("author = \"Someone <s@example.com>\""),
        "{text}"
    );
}

#[test]
fn the_configured_author_is_who_commits() {
    let (_root, paths) = initialized();
    cmd::config_set(&paths, "author", "Note Taker <notes@example.com>").unwrap();
    cmd::add(&paths, Some("Alpha"), Some("a\n"), &[]).unwrap();

    let repo = git2::Repository::open(paths.notebook_dir(cmd::DEFAULT_NOTEBOOK)).unwrap();
    let head = repo.head().unwrap().peel_to_commit().unwrap();
    assert_eq!(head.author().name(), Ok("Note Taker"));
    assert_eq!(head.author().email(), Ok("notes@example.com"));

    assert!(plain(&cmd::config_show(&paths).unwrap()).contains("Note Taker <notes@example.com>"));
}

#[test]
fn config_refuses_what_it_cannot_act_on() {
    let (_root, paths) = initialized();

    let err = cmd::config_set(&paths, "editr", "nvim")
        .unwrap_err()
        .to_string();
    assert!(err.contains("editor, author, notebook"), "{err}");
    assert!(cmd::config_get(&paths, "editr").is_err());

    // Half an identity is not an identity: it would end up in every commit.
    let err = cmd::config_set(&paths, "author", "just-a-name")
        .unwrap_err()
        .to_string();
    assert!(err.contains("Name <email>"), "{err}");

    // A file that is not TOML is reported against its path, not swallowed.
    std::fs::write(config_file(&paths), "editor = = nvim\n").unwrap();
    let err = cmd::config_show(&paths).unwrap_err().to_string();
    assert!(err.contains("config.toml"), "{err}");
}

#[test]
fn the_configured_notebook_is_what_init_creates_and_what_stands_in() {
    let root = TempRoot::new();
    let paths = root.paths();
    std::fs::create_dir_all(paths.config_dir()).unwrap();
    std::fs::write(config_file(&paths), "notebook = \"work\"\n").unwrap();

    cmd::init(&paths).unwrap();
    assert!(paths.notebook_dir("work").join(".git").is_dir());
    assert!(!paths.notebook_dir(cmd::DEFAULT_NOTEBOOK).exists());
    assert_eq!(cmd::notebook_current(&paths).unwrap(), "work");

    cmd::add(&paths, Some("Alpha"), Some("a\n"), &[]).unwrap();

    // State is "where am I now" and can be thrown away; config is "where I
    // belong", so losing the pointer must not lose the notebook.
    std::fs::remove_file(paths.active_file()).unwrap();
    assert!(
        cmd::ls(&paths, &cmd::List::default())
            .unwrap()
            .contains("Alpha")
    );
}

#[test]
fn commands_refuse_to_run_before_init() {
    let root = TempRoot::new();
    let paths = root.paths();
    let err = cmd::ls(&paths, &cmd::List::default()).unwrap_err();
    assert!(err.to_string().contains("noda init"), "{err}");
}

/// The commit the active notebook is on, in full.
fn head_commit(paths: &Paths) -> String {
    let name = noda::notebook::active_name(paths).expect("active notebook");
    git2::Repository::open(paths.notebook_dir(&name))
        .expect("open repo")
        .head()
        .expect("head")
        .peel_to_commit()
        .expect("commit")
        .id()
        .to_string()
}

/// The `commit` column of every line of a `noda blame`.
fn blamed(paths: &Paths, key: &str) -> Vec<(String, String)> {
    plain(&cmd::blame(paths, key).unwrap())
        .lines()
        .map(|line| {
            let (commit, rest) = line.split_once("  ").expect("commit and the rest");
            let text = rest.split_at(18).1;
            (commit.to_string(), text.to_string())
        })
        .collect()
}

#[test]
fn blame_credits_each_line_to_the_commit_that_wrote_it() {
    let (_root, paths) = initialized();
    let added = cmd::add(&paths, Some("Alpha"), Some("first\n"), &[]).unwrap();
    let note = paths
        .notebook_dir(cmd::DEFAULT_NOTEBOOK)
        .join(note_file(&added));

    let created = head_commit(&paths);
    let text = std::fs::read_to_string(&note).unwrap();
    std::fs::write(&note, text.replace("first\n", "first\nsecond\n")).unwrap();
    commit_working_tree(&paths, cmd::DEFAULT_NOTEBOOK, "edit: alpha");
    let edited = head_commit(&paths);

    let lines = blamed(&paths, "alpha");
    assert_eq!(
        lines,
        vec![
            (created[..7].to_string(), "first".to_string()),
            (edited[..7].to_string(), "second".to_string()),
        ]
    );
}

/// The reason this is not libgit2's blame. Every one of its `TRACK_COPIES`
/// options is documented as not implemented, so it stops at a rename — and
/// `noda mv` renames a note whenever its title changes, which would make the
/// wrong answer the normal one. Picking the note out of each commit by id
/// instead means a rename never comes up.
#[test]
fn blame_reaches_past_a_rename() {
    let (_root, paths) = initialized();
    let added = cmd::add(&paths, Some("Alpha"), Some("written before\n"), &[]).unwrap();
    let created = head_commit(&paths);

    cmd::mv(&paths, "alpha", "Renamed", false, cmd::Touch::Stamp).unwrap();
    let renamed = head_commit(&paths);
    assert_ne!(created, renamed, "the rename is its own commit");

    let note = paths
        .notebook_dir(cmd::DEFAULT_NOTEBOOK)
        .join(format!("{}-renamed.md", parts(&added).0));
    let text = std::fs::read_to_string(&note).unwrap();
    std::fs::write(
        &note,
        text.replace("written before\n", "written before\nafter\n"),
    )
    .unwrap();
    commit_working_tree(&paths, cmd::DEFAULT_NOTEBOOK, "edit: renamed");
    let after = head_commit(&paths);

    let lines = blamed(&paths, "renamed");
    assert_eq!(
        lines,
        vec![
            (created[..7].to_string(), "written before".to_string()),
            (after[..7].to_string(), "after".to_string()),
        ],
        "the line predates the rename and must not be credited to it"
    );
}

/// `updated` is rewritten on every edit, so blaming the frontmatter would put a
/// block of same-coloured noise above the prose and make every note look like it
/// was written all at once.
#[test]
fn blame_reports_the_body_and_not_the_frontmatter() {
    let (_root, paths) = initialized();
    cmd::add(&paths, Some("Alpha"), Some("body\n"), &[]).unwrap();
    cmd::tag(&paths, "alpha", &["+work".to_string()], cmd::Touch::Stamp).unwrap();

    let out = plain(&cmd::blame(&paths, "alpha").unwrap());
    assert!(!out.contains("---"), "{out}");
    assert!(!out.contains("title:"), "{out}");
    assert!(!out.contains("updated:"), "{out}");
    assert!(out.contains("body"), "{out}");
}

/// A note edited outside noda has lines nobody has committed. They belong to no
/// commit, and saying so is the honest answer.
#[test]
fn blame_marks_the_lines_that_are_not_committed() {
    let (_root, paths) = initialized();
    let added = cmd::add(&paths, Some("Alpha"), Some("committed\n"), &[]).unwrap();
    let created = head_commit(&paths);

    let note = paths
        .notebook_dir(cmd::DEFAULT_NOTEBOOK)
        .join(note_file(&added));
    let text = std::fs::read_to_string(&note).unwrap();
    std::fs::write(&note, text.replace("committed\n", "committed\nfresh\n")).unwrap();

    let lines = blamed(&paths, "alpha");
    assert_eq!(
        lines,
        vec![
            (created[..7].to_string(), "committed".to_string()),
            ("0000000".to_string(), "fresh".to_string()),
        ]
    );
}

/// A note the notebook holds and no commit does. There is nothing to walk, and
/// crediting a line to a commit that never saw it would be worse than saying so.
#[test]
fn blame_says_nothing_is_committed_when_no_commit_holds_the_note() {
    let (_root, paths) = initialized();
    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);
    plant(&notebook, "k3f9m2p1", "planted");

    let lines = blamed(&paths, "planted");
    assert_eq!(lines, vec![("0000000".to_string(), "body".to_string())]);
}

/// Two notes, the second linking to the first. Returns `(id, slug)` of each.
fn linked_pair(paths: &Paths) -> ((String, String), (String, String)) {
    let target = cmd::add(paths, Some("Meeting notes"), Some("agenda\n"), &[]).unwrap();
    let (target_id, target_slug) = parts(&target);
    let source = cmd::add(
        paths,
        Some("Q3 budget"),
        Some(&format!(
            "see [the meeting]({target_id}-{target_slug}.md)\n"
        )),
        &[],
    )
    .unwrap();
    let (source_id, source_slug) = parts(&source);
    (
        (target_id.to_string(), target_slug.to_string()),
        (source_id.to_string(), source_slug.to_string()),
    )
}

#[test]
fn backlinks_name_the_notes_that_point_at_one() {
    let (_root, paths) = initialized();
    let ((_, target), (source_id, source_slug)) = linked_pair(&paths);

    let out = plain(&cmd::backlinks(&paths, &target, cmd::Format::Table).unwrap());
    assert!(out.contains(&source_id), "{out}");
    assert!(
        !out.contains(&source_slug),
        "the slug would say the title twice: {out}"
    );
    assert!(out.contains("Q3 budget"), "the title comes with it: {out}");

    // The other direction is not this command's question: the note that does
    // the linking has nothing pointing at it.
    let out = plain(&cmd::backlinks(&paths, &source_slug, cmd::Format::Table).unwrap());
    assert!(out.contains("nothing links to"), "{out}");
}

/// The reason the match is on the id and not on the filename. `noda mv` moves
/// the slug half and says nothing to the notes that linked to the note, so the
/// destination is left naming a path that no longer exists — and still naming
/// exactly one note.
#[test]
fn backlinks_survive_a_retitle() {
    let (_root, paths) = initialized();
    let ((_, target), (source_id, _)) = linked_pair(&paths);
    cmd::mv(&paths, &target, "Weekly sync", false, cmd::Touch::Stamp).unwrap();

    // The link is now broken as far as any Markdown reader is concerned, and
    // stale as far as noda is: the id in it still names exactly one note.
    let audit = plain(&cmd::doctor(&paths, false, true, false).unwrap());
    assert!(audit.contains("stale link"), "{audit}");

    let out = plain(&cmd::backlinks(&paths, "weekly-sync", cmd::Format::Table).unwrap());
    assert!(
        out.contains(&source_id),
        "the id in the destination still names the note: {out}"
    );
}

/// An attachment has no id to fall back on — its name is the whole of its
/// identity — but the question is the same one.
#[test]
fn backlinks_answer_for_a_file_too() {
    let (_root, paths) = initialized();
    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);
    plant_file(&notebook, "diagram.png");
    let added = cmd::add(
        &paths,
        Some("Alpha"),
        Some("![the shape](diagram.png)\n"),
        &[],
    )
    .unwrap();
    cmd::add(&paths, Some("Beta"), Some("no links here\n"), &[]).unwrap();

    let out = plain(&cmd::backlinks(&paths, "diagram.png", cmd::Format::Table).unwrap());
    assert!(out.contains(parts(&added).0), "{out}");
    assert!(!out.contains("beta"), "{out}");
}

/// Settled deliberately: it is what the file says, and leaving it out would be
/// noda deciding the author did not mean it.
#[test]
fn a_note_that_links_to_itself_is_its_own_backlink() {
    let (_root, paths) = initialized();
    let added = cmd::add(&paths, Some("Alpha"), Some("placeholder\n"), &[]).unwrap();
    let (id, slug) = parts(&added);
    let path = paths
        .notebook_dir(cmd::DEFAULT_NOTEBOOK)
        .join(note_file(&added));
    let text = std::fs::read_to_string(&path).unwrap();
    std::fs::write(
        &path,
        text.replace("placeholder", &format!("see [me]({id}-{slug}.md)")),
    )
    .unwrap();
    commit_working_tree(&paths, cmd::DEFAULT_NOTEBOOK, "edit: alpha");

    let out = plain(&cmd::backlinks(&paths, slug, cmd::Format::Table).unwrap());
    assert!(out.contains(id), "{out}");
}

/// Three links to one place is one backlink: `link::targets` is a set, and the
/// question is which notes point here, not how many times.
#[test]
fn a_note_linking_three_times_is_one_backlink() {
    let (_root, paths) = initialized();
    let target = cmd::add(&paths, Some("Meeting notes"), Some("agenda\n"), &[]).unwrap();
    let (id, slug) = parts(&target);
    cmd::add(
        &paths,
        Some("Q3 budget"),
        Some(&format!(
            "[a]({id}-{slug}.md) and [b]({id}-{slug}.md) and [c]({id}-{slug}.md#x)\n"
        )),
        &[],
    )
    .unwrap();

    let out = plain(&cmd::backlinks(&paths, slug, cmd::Format::Table).unwrap());
    assert_eq!(out.lines().count(), 1, "{out}");
}

/// Only a real link counts — the same rule `doctor --links` follows, and the
/// reason both read Markdown with a parser instead of searching for the name.
#[test]
fn a_mention_is_not_a_backlink() {
    let (_root, paths) = initialized();
    let target = cmd::add(&paths, Some("Meeting notes"), Some("agenda\n"), &[]).unwrap();
    let (id, slug) = parts(&target);
    cmd::add(
        &paths,
        Some("Q3 budget"),
        Some(&format!(
            "the file is {id}-{slug}.md, and [[{slug}]] is not a link\n\n```\n[q]({id}-{slug}.md)\n```\n"
        )),
        &[],
    )
    .unwrap();

    let out = plain(&cmd::backlinks(&paths, slug, cmd::Format::Table).unwrap());
    assert!(out.contains("nothing links to"), "{out}");
}

#[test]
fn backlinks_print_json_and_ids_on_request() {
    let (_root, paths) = initialized();
    let ((target_id, target_slug), (source_id, source_slug)) = linked_pair(&paths);

    let json = cmd::backlinks(&paths, &target_slug, cmd::Format::Json).unwrap();
    assert!(
        json.contains(&format!("\"target\":\"{target_id}-{target_slug}.md\"")),
        "it names what was resolved: {json}"
    );
    assert!(json.contains(&format!("\"id\":\"{source_id}\"")), "{json}");
    assert!(
        json.contains(&format!("\"file\":\"{source_id}-{source_slug}.md\"")),
        "{json}"
    );

    let quiet = cmd::backlinks(&paths, &target_slug, cmd::Format::Quiet).unwrap();
    assert_eq!(quiet, format!("{source_id}\n"));

    // A document either way, like every other listing.
    let empty = cmd::backlinks(&paths, &source_slug, cmd::Format::Json).unwrap();
    assert!(empty.contains("\"backlinks\":[]"), "{empty}");
}

#[test]
fn backlinks_say_when_the_key_names_nothing() {
    let (_root, paths) = initialized();
    let err = cmd::backlinks(&paths, "ghost", cmd::Format::Table)
        .unwrap_err()
        .to_string();
    assert!(err.contains("nothing called `ghost`"), "{err}");
}

/// A fixed "today", so what counts as overdue is stated rather than read off
/// the clock the test is running on.
const TODAY: &str = "2026-08-02";

/// A note of nothing but action items opens with `- `, which clap reads as an
/// option unless told otherwise. Has to go through the real binary: every other
/// test here calls `cmd::` directly and never meets the parser.
#[test]
fn add_takes_a_body_that_opens_with_a_list() {
    let (root, paths) = initialized();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_noda"))
        .args(["add", "Alpha", "-c", "- [ ] send the contract\n"])
        .env("XDG_CONFIG_HOME", root.0.join("config"))
        .env("XDG_DATA_HOME", root.0.join("data"))
        .env("XDG_STATE_HOME", root.0.join("state"))
        .env("XDG_CACHE_HOME", root.0.join("cache"))
        .output()
        .expect("run noda");

    assert!(
        output.status.success(),
        "stderr was {:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    let out = plain(&cmd::todo_on(&paths, false, TODAY).unwrap());
    assert!(out.contains("send the contract"), "{out}");
}

#[test]
fn todo_lists_unticked_items_soonest_first() {
    let (_root, paths) = initialized();
    cmd::add(
        &paths,
        Some("Meeting notes"),
        Some(
            "- [ ] send the contract due:2026-08-10\n- [x] confirm legal\n- [ ] align with Alice\n",
        ),
        &[],
    )
    .unwrap();
    cmd::add(
        &paths,
        Some("Q3 planning"),
        Some("- [ ] check the terms due:2026-08-05\n"),
        &[],
    )
    .unwrap();

    let out = plain(&cmd::todo_on(&paths, false, TODAY).unwrap());
    let rows: Vec<&str> = out.lines().collect();
    assert_eq!(rows.len(), 3, "the ticked one is not listed: {out}");
    assert!(rows[0].contains("2026-08-05"), "{out}");
    assert!(rows[0].contains("check the terms"), "{out}");
    assert!(rows[1].contains("2026-08-10"), "{out}");
    assert!(
        rows[2].contains("align with Alice"),
        "an item with no due date sorts last: {out}"
    );
    assert!(
        rows[0].contains("q3-planning") && rows[1].contains("meeting-notes"),
        "each item names the note it is in: {out}"
    );
}

/// The whole reason the palette gained an exception. The escapes have to be
/// there before `plain` strips them, so this looks at the raw output.
#[test]
fn todo_marks_a_due_date_that_has_passed() {
    let (_root, paths) = initialized();
    cmd::add(
        &paths,
        Some("Alpha"),
        Some("- [ ] late one due:2026-07-01\n- [ ] later one due:2026-12-01\n"),
        &[],
    )
    .unwrap();

    let out = cmd::todo_on(&paths, false, TODAY).unwrap();
    let late = out.lines().next().unwrap();
    let later = out.lines().nth(1).unwrap();
    assert!(late.contains("2026-07-01"), "{out}");
    assert!(
        late.contains("\u{1b}[31m"),
        "a date in the past is coloured: {late:?}"
    );
    assert!(
        !later.contains("\u{1b}[31m"),
        "one still to come is not: {later:?}"
    );
}

/// Today is not late. The comparison is `<`, and a test is the only place that
/// distinction gets written down.
#[test]
fn todo_does_not_call_today_overdue() {
    let (_root, paths) = initialized();
    cmd::add(
        &paths,
        Some("Alpha"),
        Some("- [ ] due today due:2026-08-02\n"),
        &[],
    )
    .unwrap();

    let out = cmd::todo_on(&paths, false, TODAY).unwrap();
    assert!(!out.contains("\u{1b}[31m"), "{out:?}");
}

#[test]
fn todo_says_when_there_is_nothing_to_do() {
    let (_root, paths) = initialized();
    cmd::add(&paths, Some("Alpha"), Some("- [x] all done\n"), &[]).unwrap();

    let out = plain(&cmd::todo_on(&paths, false, TODAY).unwrap());
    assert!(out.contains("nothing to do"), "{out}");
}

/// Like `ls` and `deleted`: a program asking for JSON gets a document either
/// way, and an empty list is an answer.
#[test]
fn todo_json_carries_the_fields_and_prints_even_when_empty() {
    let (_root, paths) = initialized();
    let empty = cmd::todo_on(&paths, true, TODAY).unwrap();
    assert_eq!(empty.trim_end(), "{\"notebook\":\"default\",\"todo\":[]}");

    let added = cmd::add(
        &paths,
        Some("Alpha"),
        Some("- [ ] one due:2026-08-10\n- [ ] two\n"),
        &[],
    )
    .unwrap();
    let id = parts(&added).0;

    let out = cmd::todo_on(&paths, true, TODAY).unwrap();
    assert!(out.contains(&format!("\"id\":\"{id}\"")), "{out}");
    assert!(out.contains("\"slug\":\"alpha\""), "{out}");
    assert!(
        out.contains(&format!("\"file\":\"{id}-alpha.md\"")),
        "{out}"
    );
    assert!(
        out.contains("\"text\":\"one\",\"due\":\"2026-08-10\""),
        "{out}"
    );
    assert!(
        out.contains("\"text\":\"two\",\"due\":null"),
        "an item with no date says so rather than dropping the key: {out}"
    );
    assert!(
        !out.contains("overdue"),
        "a program has its own clock: {out}"
    );
}

/// Long items are printed whole. A list that cuts the sentence off is a list you
/// have to open the note to read.
#[test]
fn todo_does_not_truncate_an_item() {
    let (_root, paths) = initialized();
    let long = "chase the vendor about the revised statement of work and the indemnity clause";
    cmd::add(&paths, Some("Alpha"), Some(&format!("- [ ] {long}\n")), &[]).unwrap();

    let out = plain(&cmd::todo_on(&paths, false, TODAY).unwrap());
    assert!(out.contains(long), "{out}");
    assert!(!out.contains('…'), "{out}");
}

/// A `sync` merge carries a note across without changing it, and must not be
/// credited with writing it.
#[test]
fn blame_looks_past_a_merge_that_only_carried_the_note() {
    let (root, paths) = initialized();
    let branch = branch_of(&paths, cmd::DEFAULT_NOTEBOOK);
    let url = bare_remote(&root, "origin.git", &branch);
    cmd::remote_set(&paths, &url).unwrap();
    cmd::add(&paths, Some("Alpha"), Some("a\n"), &[]).unwrap();
    cmd::sync(&paths).unwrap();
    mirror(&paths, &url, "mirror");

    // Two machines, each writing its own note before either syncs.
    cmd::add(&paths, Some("Laptop"), Some("from the laptop\n"), &[]).unwrap();
    let wrote_it = head_commit(&paths);
    cmd::sync(&paths).unwrap();

    cmd::use_notebook(&paths, "mirror").unwrap();
    cmd::add(&paths, Some("Desktop"), Some("from the desktop\n"), &[]).unwrap();
    cmd::sync(&paths).unwrap();
    assert_eq!(merge_commits(&paths.notebook_dir("mirror")), 1);

    let lines = blamed(&paths, "laptop");
    assert_eq!(
        lines,
        vec![(wrote_it[..7].to_string(), "from the laptop".to_string())],
        "the merge carried the note, it did not write it"
    );
}

/// Annotated, not lightweight: a snapshot records that somebody closed a chapter
/// at a moment, and a bare pointer records neither the somebody nor the moment.
#[test]
fn snapshot_marks_the_current_commit_with_an_annotated_tag() {
    let (_root, paths) = initialized();
    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);
    cmd::add(&paths, Some("Alpha"), Some("a\n"), &[]).unwrap();

    let out = cmd::snapshot(&paths, "2026-q3", Some("end of quarter")).unwrap();
    assert!(out.contains("snapshot: 2026-q3 ->"), "{out}");

    let repo = git2::Repository::open(&notebook).unwrap();
    let reference = repo.find_reference("refs/tags/2026-q3").unwrap();
    let tag = reference.peel_to_tag().expect("annotated, not lightweight");
    assert_eq!(tag.message().unwrap(), Some("end of quarter"));
    assert_eq!(
        tag.target().unwrap().id(),
        repo.head().unwrap().peel_to_commit().unwrap().id()
    );

    let listed = plain(&cmd::snapshot_ls(&paths).unwrap());
    assert!(listed.contains("2026-q3"), "{listed}");
    assert!(listed.contains("end of quarter"), "{listed}");
}

/// The point of the whole feature: `restore` already promised to take a tag, and
/// until now nothing in noda could make one.
#[test]
fn a_note_restores_from_a_snapshot_by_name() {
    let (_root, paths) = initialized();
    let added = cmd::add(&paths, Some("Alpha"), Some("first\n"), &[]).unwrap();
    cmd::snapshot(&paths, "before", None).unwrap();

    let note = paths
        .notebook_dir(cmd::DEFAULT_NOTEBOOK)
        .join(note_file(&added));
    let original = std::fs::read_to_string(&note).unwrap();
    std::fs::write(&note, original.replace("first\n", "second\n")).unwrap();
    commit_working_tree(&paths, cmd::DEFAULT_NOTEBOOK, "edit: alpha");

    cmd::restore(&paths, "alpha", "before", cmd::Touch::Stamp).unwrap();
    assert!(std::fs::read_to_string(&note).unwrap().contains("first"));
}

/// `sync` already commits the whole working tree without a guard, and for the
/// same reason: a snapshot that quietly left out what is on disk would be a
/// snapshot of something nobody has.
#[test]
fn snapshot_commits_what_is_on_disk_first() {
    let (_root, paths) = initialized();
    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);
    cmd::add(&paths, Some("Alpha"), Some("a\n"), &[]).unwrap();
    std::fs::write(notebook.join("receipt.txt"), "uncommitted\n").unwrap();
    let before = commit_count(&notebook);

    let out = cmd::snapshot(&paths, "now", None).unwrap();
    assert!(out.contains("commit: local changes"), "{out}");
    assert_eq!(commit_count(&notebook), before + 1);

    let repo = git2::Repository::open(&notebook).unwrap();
    assert!(
        repo.statuses(None).unwrap().is_empty(),
        "the snapshot marks a commit that holds everything"
    );

    // And a clean notebook gains no empty commit.
    let out = cmd::snapshot(&paths, "again", None).unwrap();
    assert!(!out.contains("commit:"), "{out}");
    assert_eq!(commit_count(&notebook), before + 1);
}

/// A name that can be reassigned cannot be cited, which is the whole use.
#[test]
fn snapshot_refuses_to_move_one_that_already_exists() {
    let (_root, paths) = initialized();
    cmd::add(&paths, Some("Alpha"), Some("a\n"), &[]).unwrap();
    cmd::snapshot(&paths, "q3", None).unwrap();
    cmd::add(&paths, Some("Beta"), Some("b\n"), &[]).unwrap();

    let err = cmd::snapshot(&paths, "q3", None).unwrap_err().to_string();
    assert!(err.contains("already exists"), "{err}");
    assert!(err.contains("git tag -d q3"), "the way out is named: {err}");
}

#[test]
fn snapshot_refuses_a_name_git_cannot_hold() {
    let (_root, paths) = initialized();
    cmd::add(&paths, Some("Alpha"), Some("a\n"), &[]).unwrap();

    let err = cmd::snapshot(&paths, "not a name", None)
        .unwrap_err()
        .to_string();
    assert!(err.contains("invalid snapshot name"), "{err}");
}

#[test]
fn snapshot_says_when_there_are_none() {
    let (_root, paths) = initialized();
    let out = plain(&cmd::snapshot_ls(&paths).unwrap());
    assert!(out.contains("no snapshots"), "{out}");
    assert!(out.contains("noda snapshot <name>"), "{out}");
}

/// A snapshot that stayed on the machine it was taken on could not be cited from
/// anywhere else, which is most of what a snapshot is for.
#[test]
fn snapshots_travel_with_the_notebook() {
    let (root, paths) = initialized();
    let branch = branch_of(&paths, cmd::DEFAULT_NOTEBOOK);
    let url = bare_remote(&root, "origin.git", &branch);
    cmd::remote_set(&paths, &url).unwrap();
    cmd::add(&paths, Some("Alpha"), Some("a\n"), &[]).unwrap();
    cmd::snapshot(&paths, "q3", Some("end of quarter")).unwrap();
    cmd::sync(&paths).unwrap();

    let remote = git2::Repository::open_bare(&url).unwrap();
    assert!(
        remote.find_reference("refs/tags/q3").is_ok(),
        "the snapshot reached the remote"
    );

    // And comes back down on the other side.
    mirror(&paths, &url, "mirror");
    cmd::use_notebook(&paths, "mirror").unwrap();
    let listed = plain(&cmd::snapshot_ls(&paths).unwrap());
    assert!(listed.contains("q3"), "{listed}");
    assert!(listed.contains("end of quarter"), "{listed}");
}

/// Two machines that each made a `q3` must not overwrite each other in silence —
/// and the clash must not take the notes down with it, which is what sending the
/// tag anyway would do.
#[test]
fn a_snapshot_name_taken_on_the_remote_is_not_overwritten() {
    let (root, paths) = initialized();
    let branch = branch_of(&paths, cmd::DEFAULT_NOTEBOOK);
    let url = bare_remote(&root, "origin.git", &branch);
    cmd::remote_set(&paths, &url).unwrap();
    cmd::add(&paths, Some("Alpha"), Some("a\n"), &[]).unwrap();
    cmd::snapshot(&paths, "q3", Some("theirs")).unwrap();
    cmd::sync(&paths).unwrap();

    // A second notebook off the same remote, with a different `q3`.
    mirror(&paths, &url, "mirror");
    cmd::use_notebook(&paths, "mirror").unwrap();
    let repo = git2::Repository::open(paths.notebook_dir("mirror")).unwrap();
    repo.tag_delete("q3").unwrap();
    cmd::add(&paths, Some("Beta"), Some("b\n"), &[]).unwrap();
    cmd::snapshot(&paths, "q3", Some("ours")).unwrap();

    let out = cmd::push(&paths).unwrap();
    assert!(out.contains("snapshot `q3` was not sent"), "{out}");
    assert!(out.contains("git tag -d q3"), "the way out is named: {out}");

    let remote = git2::Repository::open_bare(&url).unwrap();
    let tag = remote
        .find_reference("refs/tags/q3")
        .unwrap()
        .peel_to_tag()
        .unwrap();
    assert_eq!(
        tag.message().unwrap(),
        Some("theirs"),
        "the remote's snapshot still means what it meant"
    );
    // The notes went, which is the point: a name nobody can agree on must not
    // hold up the prose.
    let head = remote
        .find_reference(&format!("refs/heads/{branch}"))
        .unwrap()
        .peel_to_commit()
        .unwrap();
    assert!(
        head.tree()
            .unwrap()
            .iter()
            .filter_map(|entry| entry.name().ok().map(str::to_string))
            .any(|name| name.contains("beta")),
        "the branch reached the remote even though the snapshot did not"
    );
}

/// A small export, written to a file the way a browser's "export all" would.
fn export(root: &TempRoot, name: &str, tiddlers: &str) -> PathBuf {
    let path = root.0.join(name);
    std::fs::write(&path, tiddlers).unwrap();
    path
}

const TIDDLERS: &str = r#"[
  {"title":"Meeting Notes","text":"See [[Reading Log]] and ''this''.\n","tags":"work [[two words]]",
   "created":"20190314082100000","modified":"20241102164012123","creator":"henry",
   "type":"text/vnd.tiddlywiki"},
  {"title":"Reading Log","text":"A {{Transclusion}} nobody can translate.\n","tags":"",
   "created":"20200101000000000","modified":"20200101000000000"},
  {"title":"$:/config/Something","text":"not a note"},
  {"title":"A Picture","text":"aGk=","type":"image/png"}
]"#;

/// The whole promise of the two-commit import: whatever the conversion did, the
/// text the export actually held is still in history and one command away.
#[test]
fn import_writes_the_originals_first_and_the_conversion_second() {
    let (root, paths) = initialized();
    let file = export(&root, "wiki.json", TIDDLERS);
    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);
    let before = commit_count(&notebook);

    let out = plain(&cmd::import_tiddlywiki(&paths, std::slice::from_ref(&file), true).unwrap());
    assert!(out.contains("imported  2 notes from tiddlywiki"), "{out}");
    assert_eq!(
        commit_count(&notebook),
        before + 2,
        "one commit for the notes as written, one for the conversion"
    );

    let converted = note_text(&paths, "meeting-notes");
    assert!(converted.contains("**this**"), "converted: {converted}");
    let original = cmd::restore(&paths, "meeting-notes", "HEAD~1", cmd::Touch::Keep).unwrap();
    assert!(!original.contains("(no change)"), "{original}");
    assert!(
        note_text(&paths, "meeting-notes").contains("''this''"),
        "the WikiText the export held is one restore away"
    );
}

#[test]
fn import_carries_the_times_and_fields_the_wiki_had() {
    let (root, paths) = initialized();
    let file = export(&root, "wiki.json", TIDDLERS);
    cmd::import_tiddlywiki(&paths, std::slice::from_ref(&file), true).unwrap();

    let text = note_text(&paths, "meeting-notes");
    assert!(text.contains("created: 2019-03-14T08:21:00.000Z"), "{text}");
    assert!(text.contains("updated: 2024-11-02T16:40:12.123Z"), "{text}");
    assert!(text.contains("creator: henry"), "{text}");
    assert!(text.contains("source_key: Meeting Notes"), "{text}");
    assert_eq!(
        times(&paths, "meeting-notes").0.as_deref(),
        Some("2019-03-14T08:21:00.000Z"),
        "noda reads back what the wiki wrote"
    );
    // A title list keeps the spaces inside its double brackets.
    let out = cmd::ls(
        &paths,
        &cmd::List {
            tag: Some("two words"),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(out.contains("Meeting Notes"), "{out}");
}

/// The link is to a tiddler by title, and the file it becomes is not named
/// until the id is minted — so the rewrite cannot happen until every note
/// exists, which is what the second pass is for.
#[test]
fn import_points_links_at_the_files_the_notes_became() {
    let (root, paths) = initialized();
    let file = export(&root, "wiki.json", TIDDLERS);
    cmd::import_tiddlywiki(&paths, std::slice::from_ref(&file), true).unwrap();

    let reading = cmd::path(&paths, Some("reading-log")).unwrap();
    let name = Path::new(reading.trim())
        .file_name()
        .unwrap()
        .to_str()
        .unwrap();
    let meeting = note_text(&paths, "meeting-notes");
    assert!(
        meeting.contains(&format!("[Reading Log]({name})")),
        "the link names the file: {meeting}"
    );
    let out = plain(&cmd::backlinks(&paths, "reading-log", cmd::Format::Table).unwrap());
    assert!(out.contains("Meeting Notes"), "and noda can see it: {out}");
}

#[test]
fn what_is_not_a_note_is_reported_rather_than_imported() {
    let (root, paths) = initialized();
    let file = export(&root, "wiki.json", TIDDLERS);
    let out = plain(&cmd::import_tiddlywiki(&paths, std::slice::from_ref(&file), true).unwrap());
    assert!(out.contains("1 system tiddler"), "{out}");
    assert!(out.contains("1 not text (image/png)"), "{out}");
}

/// `--no-convert` is the whole first half and none of the second.
#[test]
fn import_can_leave_the_wikitext_as_it_stands() {
    let (root, paths) = initialized();
    let file = export(&root, "wiki.json", TIDDLERS);
    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);
    let before = commit_count(&notebook);

    cmd::import_tiddlywiki(&paths, std::slice::from_ref(&file), false).unwrap();
    assert_eq!(
        commit_count(&notebook),
        before + 1,
        "one commit, no conversion"
    );
    assert!(note_text(&paths, "meeting-notes").contains("''this''"));
}

/// Running the same import twice is not two notebooks.
#[test]
fn a_second_import_of_the_same_export_changes_nothing() {
    let (root, paths) = initialized();
    let file = export(&root, "wiki.json", TIDDLERS);
    cmd::import_tiddlywiki(&paths, std::slice::from_ref(&file), true).unwrap();
    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);
    let after_first = commit_count(&notebook);

    let out = plain(&cmd::import_tiddlywiki(&paths, std::slice::from_ref(&file), true).unwrap());
    assert!(out.contains("imported  0 notes"), "{out}");
    assert!(out.contains("already imported as"), "{out}");
    assert_eq!(
        commit_count(&notebook),
        after_first,
        "and nothing committed"
    );
}

/// The marker is a frontmatter field rather than a tag, because tags belong to
/// whoever writes the notes — and `doctor` is what makes a field findable.
#[test]
fn doctor_reports_the_notes_an_import_could_not_finish() {
    let (root, paths) = initialized();
    let file = export(&root, "wiki.json", TIDDLERS);
    cmd::import_tiddlywiki(&paths, std::slice::from_ref(&file), true).unwrap();

    assert!(
        note_text(&paths, "reading-log").contains("unconverted: transclusion"),
        "the record is in the note"
    );
    let out = plain(&cmd::doctor(&paths, false, false, false).unwrap());
    assert!(
        out.contains("1 note carries text an importer did not convert"),
        "and doctor is the handle: {out}"
    );
    assert!(out.contains("1 note transclusion"), "{out}");
}

#[test]
fn a_file_that_is_not_an_export_is_refused_by_name() {
    let (root, paths) = initialized();
    let file = export(&root, "notes.md", "# just some markdown\n");
    let err = cmd::import_tiddlywiki(&paths, std::slice::from_ref(&file), true)
        .unwrap_err()
        .to_string();
    assert!(err.contains("no tiddler store"), "{err}");
}

/// A wiki exported in pieces has links running between the pieces, so several
/// files are one import rather than several.
#[test]
fn several_exports_are_read_as_one_import() {
    let (root, paths) = initialized();
    let first = export(
        &root,
        "one.json",
        r#"[{"title":"Alpha","text":"points at [[Beta]]\n"}]"#,
    );
    let second = export(
        &root,
        "two.json",
        r#"[{"title":"Beta","text":"and back at [[Alpha]]\n"}]"#,
    );

    let out = plain(&cmd::import_tiddlywiki(&paths, &[first, second], true).unwrap());
    assert!(out.contains("imported  2 notes"), "{out}");
    let alpha = note_text(&paths, "alpha");
    assert!(
        alpha.contains("](") && !alpha.contains("[[Beta]]"),
        "a link across the two files resolves: {alpha}"
    );
    let out = plain(&cmd::backlinks(&paths, "alpha", cmd::Format::Table).unwrap());
    assert!(out.contains("Beta"), "and in both directions: {out}");
}

/// Exports taken in pieces overlap. The first copy is the one that lands, and
/// the second is reported rather than written twice.
#[test]
fn a_note_given_twice_in_one_import_arrives_once() {
    let (root, paths) = initialized();
    let first = export(&root, "one.json", r#"[{"title":"Alpha","text":"first\n"}]"#);
    let second = export(&root, "two.json", r#"[{"title":"Alpha","text":"again\n"}]"#);

    let out = plain(&cmd::import_tiddlywiki(&paths, &[first, second], true).unwrap());
    assert!(out.contains("imported  1 note"), "{out}");
    assert!(out.contains("given twice in this import"), "{out}");
    assert!(
        note_text(&paths, "alpha").contains("first"),
        "the first one"
    );
}

/// The gap this closes: a link written today pointing at a note that arrived
/// last week. The resolver starts from what the notebook already holds, so a
/// wiki can be brought in over several sittings.
#[test]
fn a_later_import_links_to_what_an_earlier_one_brought() {
    let (root, paths) = initialized();
    let first = export(&root, "one.json", r#"[{"title":"Alpha","text":"a\n"}]"#);
    cmd::import_tiddlywiki(&paths, std::slice::from_ref(&first), true).unwrap();

    let second = export(
        &root,
        "two.json",
        r#"[{"title":"Beta","text":"points at [[Alpha]]\n"}]"#,
    );
    cmd::import_tiddlywiki(&paths, std::slice::from_ref(&second), true).unwrap();

    let beta = note_text(&paths, "beta");
    assert!(
        !beta.contains("[[Alpha]]"),
        "the link to last week's note resolves: {beta}"
    );
    let out = plain(&cmd::backlinks(&paths, "alpha", cmd::Format::Table).unwrap());
    assert!(out.contains("Beta"), "{out}");
}

/// Every file is read before anything is written, so a bad one leaves no half
/// an import behind.
#[test]
fn a_file_that_cannot_be_read_stops_the_import_before_it_writes() {
    let (root, paths) = initialized();
    let good = export(&root, "one.json", r#"[{"title":"Alpha","text":"a\n"}]"#);
    let bad = export(&root, "two.json", "not an export at all\n");
    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);
    let before = commit_count(&notebook);

    let err = cmd::import_tiddlywiki(&paths, &[good, bad], true)
        .unwrap_err()
        .to_string();
    assert!(err.contains("two.json"), "it says which file: {err}");
    assert_eq!(commit_count(&notebook), before, "and wrote nothing");
    assert!(cmd::ls(&paths, &cmd::List::default()).unwrap().is_empty());
}
