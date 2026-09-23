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
    unsigned(&paths);
    cmd::init(&paths).expect("init");
    (root, paths)
}

/// Turns signing off before anything commits. libgit2 reads the developer's real
/// `~/.config/git/config`, so `commit.gpgsign = true` would send every test to
/// gpg; noda's setting outranks git's. Tests that write `config.toml` wholesale
/// set `sign = false` themselves.
fn unsigned(paths: &Paths) {
    std::fs::create_dir_all(paths.config_dir()).expect("config dir");
    std::fs::write(paths.config_dir().join("config.toml"), "sign = false\n").expect("config");
}

fn commit_count(notebook: &Path) -> usize {
    let repo = git2::Repository::open(notebook).expect("open repo");
    let mut walk = repo.revwalk().expect("revwalk");
    walk.push_head().expect("push head");
    walk.count()
}

/// The id and slug from the `id  slug  [tags]` line mutating commands print.
fn parts(summary: &str) -> (&str, &str) {
    let (id, rest) = summary.split_once("  ").expect("id and slug");
    (id, rest.split("  ").next().expect("slug"))
}

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
    // The identity is the filename; nothing in the file can disagree with it.
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

    // A second line would become a field of its own, so `render` and `parse`
    // would stop being inverses.
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

    // Trimmed on the way in because it is trimmed on the way out; otherwise the
    // tag shown is not the tag `ls --tag` matches.
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
    // Removal stays permissive so a tag that predates the check can still go.
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

/// `<id>-<slug>.md` spends 12 bytes before the slug, and a long title used to
/// push the name past the 255-byte component limit (`File name too long`).
#[test]
fn a_title_too_long_for_a_filename_still_becomes_a_note() {
    let (_root, paths) = initialized();
    let title = "GitHub - coding-horror/basic-computer-games: An updated version of the \
classic \"Basic Computer Games\" book, with well-written examples in a variety of \
common MEMORY SAFE, SCRIPTING programming languages. See \
https://coding-horror.github.io/basic-computer-games/";

    let out = cmd::add(&paths, Some(title), Some("body\n"), &[]).unwrap();

    let file = note_file(&out);
    assert!(
        file.len() <= 255,
        "the filename has to fit a path component: {} bytes, {file}",
        file.len()
    );
    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);
    assert!(notebook.join(&file).is_file(), "{file}");

    // The slug is cut; the title is kept whole.
    let (_, slug) = parts(&out);
    assert!(
        note_text(&paths, slug).contains(&format!("title: {title}")),
        "the whole title survives in the frontmatter"
    );
}

/// The id keeps the filenames apart. The old `-2` suffix was only local: two
/// machines adding "Notes" both wrote `notes.md` and the sync conflicted.
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

    // The slug is ambiguous, so noda asks rather than guesses.
    let err = cmd::show(&paths, "notes").unwrap_err().to_string();
    assert!(err.contains("matches 2 notes"), "{err}");
    assert!(err.contains(first_id), "{err}");
    assert!(cmd::show(&paths, first_id).unwrap().contains("one"));
    assert!(cmd::show(&paths, second_id).unwrap().contains("two"));
}

/// As git resolves an abbreviated object id.
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

    // Crockford folds case and the I/L/O confusables.
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
    // The brackets and the tag are coloured separately, so the raw output does
    // not contain `[work]` as one run.
    assert!(plain(&all).contains("[work]"), "{all}");

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

#[test]
fn pin_and_unpin_are_one_commit_each_and_the_second_press_is_neither() {
    let (_root, paths) = initialized();
    cmd::add(&paths, Some("Alpha"), Some("a\n"), &[]).unwrap();
    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);
    let before = commit_count(&notebook);

    let out = cmd::pin(&paths, "alpha", true, cmd::Touch::Stamp).unwrap();
    assert!(out.ends_with("  pinned"), "{out}");
    assert_eq!(commit_count(&notebook), before + 1);
    assert!(note_text(&paths, "alpha").contains("pinned: true"));

    // Already pinned: nothing written, nothing committed.
    let again = cmd::pin(&paths, "alpha", true, cmd::Touch::Stamp).unwrap();
    assert!(again.ends_with("(no change)"), "{again}");
    assert_eq!(commit_count(&notebook), before + 1);

    let off = cmd::pin(&paths, "alpha", false, cmd::Touch::Stamp).unwrap();
    assert!(off.ends_with("  unpinned"), "{off}");
    assert_eq!(commit_count(&notebook), before + 2);
    // The line is removed rather than set to false, so the file is as it was.
    assert!(!note_text(&paths, "alpha").contains("pinned"));
}

#[test]
fn a_pin_is_above_every_listing_and_below_every_reversed_one() {
    let (_root, paths) = initialized();
    cmd::add(&paths, Some("Alpha"), Some("a\n"), &[]).unwrap();
    cmd::add(&paths, Some("Beta"), Some("b\n"), &[]).unwrap();
    cmd::add(&paths, Some("Gamma"), Some("g\n"), &[]).unwrap();
    cmd::pin(&paths, "beta", true, cmd::Touch::Stamp).unwrap();

    let listed = |reverse: bool| {
        cmd::ls(
            &paths,
            &cmd::List {
                format: cmd::Format::Quiet,
                reverse,
                ..Default::default()
            },
        )
        .unwrap()
    };
    let first = |out: &str| out.lines().next().unwrap().to_string();
    let beta = note_id(&paths, "beta");

    assert_eq!(first(&listed(false)), beta, "a pin comes first");
    // `-r` reverses the whole listing, pins included.
    assert_eq!(
        listed(true).lines().last().unwrap(),
        beta,
        "-r left the pin on top"
    );

    let table = plain(&cmd::ls(&paths, &cmd::List::default()).unwrap());
    assert!(
        table.lines().next().unwrap().ends_with("  pinned"),
        "{table}"
    );
    assert_eq!(
        table.lines().filter(|line| line.contains("pinned")).count(),
        1,
        "{table}"
    );

    let json = cmd::ls(
        &paths,
        &cmd::List {
            format: cmd::Format::Json,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(json.contains("\"pinned\":true"), "{json}");
    assert!(json.contains("\"pinned\":false"), "{json}");
}

#[test]
fn search_narrows_to_the_pinned_notes_and_away_from_them() {
    let (_root, paths) = initialized();
    cmd::add(&paths, Some("Alpha"), Some("a\n"), &[]).unwrap();
    cmd::add(&paths, Some("Beta"), Some("b\n"), &[]).unwrap();
    cmd::pin(&paths, "beta", true, cmd::Touch::Stamp).unwrap();

    let found = |query: &str| cmd::search(&paths, &[query.to_string()]).unwrap();
    assert!(found("pinned:true").contains("Beta"));
    assert!(!found("pinned:true").contains("Alpha"));
    assert!(found("pinned:false").contains("Alpha"));
    assert!(!found("pinned:false").contains("Beta"));
    // A typo is refused rather than matching everything.
    assert!(cmd::search(&paths, &["pinned:ture".to_string()]).is_err());
}

fn note_id(paths: &Paths, key: &str) -> String {
    let path = cmd::path(paths, Some(key)).unwrap();
    let stem = Path::new(path.trim_end()).file_stem().unwrap();
    note::split_stem(stem.to_str().unwrap())
        .unwrap()
        .0
        .to_string()
}

/// The note as it sits on disk; `show` dims the frontmatter.
fn note_text(paths: &Paths, key: &str) -> String {
    let path = cmd::path(paths, Some(key)).unwrap();
    std::fs::read_to_string(path.trim_end()).unwrap()
}

/// `(created, updated)`.
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

#[test]
fn changing_a_note_moves_updated_and_leaves_created_alone() {
    let (_root, paths) = initialized();
    let added = cmd::add(&paths, Some("Alpha"), Some("a\n"), &[]).unwrap();
    let created = times(&paths, "alpha").0;

    // Backdated so the change is visible however fast the test runs.
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

/// Backdates `updated` so a change to it is visible however fast the test runs.
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

/// The commit still records the change; only `updated` is left alone.
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

/// `tag` takes hyphen values, so a flag after the tags would arrive as a tag:
/// `--no-touch` would become removing `-no-touch`, reporting success and doing nothing.
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

/// The case the flag exists for: an imported note's dates must survive an edit.
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

/// The only honest value would come from git; one taken from the filesystem
/// would be invented after a clone.
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

    // A change noda makes is dated, but the missing `created` is not backfilled.
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

/// `tag` rewrites through `render`, which is where unknown fields would be lost —
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

    // Only the slug moved, so the id still resolves and the old slug does not.
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

/// The links are stale rather than dead (their id still resolves), but any
/// Markdown reader outside noda sees only the path that is gone.
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

/// The rename and the rewrites are one commit, so no state has half the links moved.
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

/// Why the match is on the id: after two retitles a link is two names behind,
/// and an exact-name match would miss it.
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

/// The flag means "make links say this note's current name", so it also repairs
/// what an earlier rename left behind.
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

/// Read back from the file the rename just wrote, not the copy from before.
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

/// And skips the link walk, which is `doctor --links`' cost.
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

/// The ids differ, so the filenames do.
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

/// Writes an executable stand-in for `$EDITOR`; the note path arrives as `$1`. A
/// script rather than `sh -c '…'` because the editor string is split on whitespace.
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

/// `updated` is stamped in place: noda does not rearrange a block somebody just
/// arranged in their editor.
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

/// The id is in the filename, which an editor cannot change.
#[cfg(unix)]
#[test]
fn an_edit_cannot_change_a_notes_identity() {
    let (root, paths) = initialized();
    let added = cmd::add(&paths, Some("Alpha"), Some("a\n"), &[]).unwrap();
    let id = parts(&added).0.to_string();

    // An `id:` line in the frontmatter is just another field.
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

    // Still in history: the commit before HEAD carries the file.
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

/// Commands that only need to know which note must not refuse a broken one;
/// they are the tools for clearing it up.
#[test]
fn the_commands_that_do_not_read_a_note_work_on_one_that_cannot_be_read() {
    let (_root, paths) = initialized();
    let added = cmd::add(&paths, Some("Alpha"), Some("the original body\n"), &[]).unwrap();
    let id = parts(&added).0.to_string();
    let file = note_file(&added);
    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);
    std::fs::write(notebook.join(&file), "frontmatter is gone\n").unwrap();

    // `resolve` reads only the filename, and history is how you find out why it
    // will not parse.
    assert!(cmd::log(&paths, Some(&id), None).unwrap().contains("add:"));
    assert!(cmd::diff(&paths, Some(&id), false).unwrap().contains(&file));

    // It writes over the file, so it never needs to read it.
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

    // These rewrite the frontmatter, so they must read it first.
    for err in [
        cmd::mv(&paths, &id, "Renamed", false, cmd::Touch::Stamp).unwrap_err(),
        cmd::tag(&paths, &id, &["+work".to_string()], cmd::Touch::Stamp).unwrap_err(),
    ] {
        assert!(err.to_string().contains("frontmatter"), "{err}");
    }
}

/// Neither an id in its name nor frontmatter: a file, not a note.
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

    // A duplicate name, and a name that escapes the data dir.
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

    cmd::notebook_rm_confirmed(&paths, "work", true, |_| panic!("--force must not ask")).unwrap();
    assert!(!paths.notebook_dir("work").exists());
}

/// `noda status` covers only the active notebook; this shows how far behind the
/// others are, without going to the network.
#[test]
fn notebook_ls_says_where_each_notebook_stands() {
    let (root, paths) = initialized();
    let branch = branch_of(&paths, cmd::DEFAULT_NOTEBOOK);
    let url = bare_remote(&root, "origin.git", &branch);

    // No remote is not the same as never synced: it can never leave that state.
    cmd::notebook_add(&paths, "solo", None).unwrap();
    let listed = plain(&cmd::notebook_ls(&paths).unwrap());
    let solo = listed.lines().find(|l| l.contains("solo")).unwrap();
    assert!(solo.contains("no remote"), "{listed}");

    cmd::remote_set(&paths, &url).unwrap();
    let listed = plain(&cmd::notebook_ls(&paths).unwrap());
    let active = listed.lines().find(|l| l.starts_with('*')).unwrap();
    assert!(active.contains("never synced"), "{listed}");

    cmd::add(&paths, Some("Alpha"), Some("a\n"), &[]).unwrap();
    cmd::push(&paths).unwrap();
    let listed = plain(&cmd::notebook_ls(&paths).unwrap());
    let active = listed.lines().find(|l| l.starts_with('*')).unwrap();
    assert!(active.contains("in sync"), "{listed}");

    cmd::add(&paths, Some("Beta"), Some("b\n"), &[]).unwrap();
    let listed = plain(&cmd::notebook_ls(&paths).unwrap());
    let active = listed.lines().find(|l| l.starts_with('*')).unwrap();
    assert!(active.contains("1 to push"), "{listed}");

    // Nothing above touches the network, so an unreachable remote costs nothing
    // to list. A path that was never created stands in for one.
    cmd::notebook_add(&paths, "gone", Some("file:///nowhere/at/all.git")).unwrap();
    let listed = plain(&cmd::notebook_ls(&paths).unwrap());
    let gone = listed.lines().find(|l| l.contains("gone")).unwrap();
    assert!(gone.contains("never synced"), "{listed}");
}

#[test]
fn notebook_rm_refuses_when_there_is_nobody_to_ask() {
    let (_root, paths) = initialized();
    cmd::notebook_add(&paths, "work", None).unwrap();

    // The harness has no terminal, which is the case being checked: piped or
    // scripted, an irreversible delete is not assumed.
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

    cmd::notebook_rename(&paths, "work", "archive").unwrap();
    assert_eq!(cmd::notebook_current(&paths).unwrap(), "personal");

    assert!(cmd::notebook_rename(&paths, "missing", "x").is_err());
    assert!(cmd::notebook_rename(&paths, "archive", "personal").is_err());
}

/// A bare repository standing in for GitHub. libgit2's local transport is the
/// same push/fetch machinery as HTTPS and SSH, with no network or credentials.
fn bare_remote(root: &TempRoot, name: &str, branch: &str) -> String {
    let path = root.0.join(name);
    let repo = git2::Repository::init_bare(&path).expect("init bare remote");
    repo.set_head(&format!("refs/heads/{branch}"))
        .expect("point the remote at the branch under test");
    path.to_str().expect("utf-8 path").to_string()
}

/// `main` or `master` per `init.defaultBranch`, so no test may assume either.
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

/// The container image authenticates over HTTPS with a token in the URL (it has
/// no shell for a credential helper), so a remote may carry a secret.
#[test]
fn a_token_in_the_remote_is_never_printed_back() {
    const URL: &str = "https://x-access-token:ghp_secret@github.com/me/notes.git";

    let (_root, paths) = initialized();

    // Four places a remote is shown, starting with `remote set`'s answer.
    let screens = [
        cmd::remote_set(&paths, URL).unwrap(),
        cmd::remote_show(&paths).unwrap(),
        cmd::status(&paths).unwrap(),
        cmd::notebook_ls(&paths).unwrap(),
    ];
    for shown in &screens {
        assert!(!shown.contains("ghp_secret"), "{shown}");
        assert!(shown.contains("***@github.com"), "{shown}");
    }

    // Redaction is display only: push and fetch still use what was configured.
    let repo = git2::Repository::open(paths.notebook_dir(cmd::DEFAULT_NOTEBOOK)).expect("open");
    let origin = repo.find_remote("origin").expect("remote");
    assert_eq!(
        origin.url().ok(),
        Some(URL),
        "the URL on disk was rewritten"
    );
}

#[test]
fn push_says_how_much_it_sent() {
    let (root, paths) = initialized();
    let branch = branch_of(&paths, cmd::DEFAULT_NOTEBOOK);
    let url = bare_remote(&root, "origin.git", &branch);
    cmd::remote_set(&paths, &url).unwrap();
    cmd::add(&paths, Some("Alpha"), Some("a\n"), &[]).unwrap();

    // Never synced: what the remote holds is unknown until fetched, so no
    // count is given rather than one guessed from local history.
    let out = plain(&cmd::push(&paths).unwrap());
    assert!(out.starts_with("push:"), "{out}");
    assert!(
        !out.contains("commit"),
        "a first push counted what it could not know: {out}"
    );

    // Nothing moved, and saying so distinguishes this from the line above.
    let out = plain(&cmd::push(&paths).unwrap());
    assert!(out.contains("nothing to send"), "{out}");

    cmd::add(&paths, Some("Beta"), Some("b\n"), &[]).unwrap();
    cmd::add(&paths, Some("Gamma"), Some("c\n"), &[]).unwrap();
    let out = plain(&cmd::push(&paths).unwrap());
    assert!(out.contains("(2 commits)"), "{out}");

    // Singular for one.
    cmd::add(&paths, Some("Delta"), Some("d\n"), &[]).unwrap();
    let out = plain(&cmd::push(&paths).unwrap());
    assert!(out.contains("(1 commit)"), "{out}");
}

/// A push carrying only a snapshot has sent something, so not `nothing to send`.
#[test]
fn a_snapshot_is_something_sent_even_when_no_commit_is() {
    let (root, paths) = initialized();
    let branch = branch_of(&paths, cmd::DEFAULT_NOTEBOOK);
    let url = bare_remote(&root, "origin.git", &branch);
    cmd::remote_set(&paths, &url).unwrap();
    cmd::add(&paths, Some("Alpha"), Some("a\n"), &[]).unwrap();
    cmd::push(&paths).unwrap();

    // The notebook is clean, so this adds a tag and no commit.
    cmd::snapshot(&paths, "q3", None).unwrap();
    let out = plain(&cmd::push(&paths).unwrap());
    assert!(out.contains("1 snapshot"), "{out}");
    assert!(!out.contains("nothing to send"), "{out}");
}

/// Counted between the fetch and the merge, the one moment the tracking ref has
/// moved and the branch has not.
#[test]
fn pull_says_how_much_arrived() {
    let (root, paths) = initialized();
    let branch = branch_of(&paths, cmd::DEFAULT_NOTEBOOK);
    let url = bare_remote(&root, "origin.git", &branch);
    cmd::remote_set(&paths, &url).unwrap();
    cmd::add(&paths, Some("Alpha"), Some("a\n"), &[]).unwrap();
    cmd::sync(&paths).unwrap();

    mirror(&paths, &url, "mirror");
    cmd::add(&paths, Some("Beta"), Some("b\n"), &[]).unwrap();
    cmd::add(&paths, Some("Gamma"), Some("c\n"), &[]).unwrap();
    cmd::sync(&paths).unwrap();

    // Only the remote moved: a fast-forward.
    cmd::use_notebook(&paths, "mirror").unwrap();
    let out = plain(&cmd::pull(&paths).unwrap());
    assert!(out.contains("fast-forwarded 2 commits"), "{out}");

    // Both sides move, so this makes a merge commit, and the count has to survive it.
    cmd::add(&paths, Some("Local"), Some("l\n"), &[]).unwrap();
    cmd::use_notebook(&paths, cmd::DEFAULT_NOTEBOOK).unwrap();
    cmd::add(&paths, Some("Remote"), Some("r\n"), &[]).unwrap();
    cmd::sync(&paths).unwrap();
    cmd::use_notebook(&paths, "mirror").unwrap();
    let out = plain(&cmd::pull(&paths).unwrap());
    assert!(out.contains("merged 1 commit"), "{out}");
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

    // No name given: `origin.git` -> `origin`.
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

    assert!(cmd::clone(&paths, &url, Some("origin")).is_err());
}

#[test]
fn clone_adopts_the_only_branch_when_the_remote_head_points_elsewhere() {
    let (root, paths) = initialized();
    // The remote's HEAD names a branch nothing was pushed to — what two machines
    // with different `init.defaultBranch` produce.
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

    // Edited outside noda.
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

    let out = cmd::sync(&paths).unwrap();
    assert!(!out.contains("commit:"), "{out}");
    assert!(out.contains("already up to date"), "{out}");
}

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

    // Each side wrote its own filename, so the merge is clean.
    let repo = git2::Repository::open(paths.notebook_dir("mirror")).unwrap();
    assert!(repo.statuses(None).unwrap().is_empty(), "nothing left over");
    assert_eq!(repo.state(), git2::RepositoryState::Clean);

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

    // The rollback leaves a working notebook: no conflict markers, no
    // half-finished merge, the local commit still there.
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

/// Output carries colour unconditionally (`anstream` strips it); tests read the text.
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

    // Behind a second notebook's push — but only once fetched, as status never fetches.
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

/// A file with neither an id in its name nor frontmatter is listed and counted as
/// a file, never as a problem.
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

/// Frontmatter declares a note; one with no id in its name is waiting to be adopted.
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

/// `abcdefgh` is a legal id, so the shape alone cannot tell a broken note from
/// somebody's file.
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

/// Two machines can mint one id; the filenames differ, so git merges them
/// silently and this is the only place it shows.
#[test]
fn status_reports_one_id_carried_by_two_notes() {
    let (_root, paths) = initialized();
    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);
    plant(&notebook, "k3f9m2p1", "alpha");
    // Ids fold case, so `K3F9M2P1` is not a second id.
    plant(&notebook, "K3F9M2P1", "beta");

    let out = plain(&cmd::status(&paths).unwrap());
    assert_eq!(
        status_row(&out, "problems"),
        Some("1 id is carried by more than one note  (k3f9m2p1)"),
        "{out}"
    );
}

/// Minting must see ids that arrived from elsewhere, or it could hand one out
/// twice — which cannot be undone.
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

    // A directory of hand-written notes copied in at once; `status` must stay one screen.
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
    // The line count does not grow with the number of notes.
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

/// Writes a note with frontmatter but no id in its name — hand-written or from elsewhere.
fn plant_unnamed(notebook: &Path, name: &str) {
    std::fs::write(
        notebook.join(format!("{name}.md")),
        format!("---\ntitle: {name}\n---\n\nbody\n"),
    )
    .unwrap();
}

/// The one repair that cannot lose anything: the file declares itself a note and
/// only lacks a name.
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

    // `status` shows three and a `…`; this shows the rest.
    let out = plain(&cmd::doctor(&paths, true, false, false).unwrap());
    assert_eq!(out.matches(".md").count(), 12, "{out}");
    assert!(!out.contains('…'), "nothing elided here: {out}");
}

/// An edit outside noda leaves `updated` behind; only git saw it, which is why
/// this check walks history and is opt-in.
#[test]
fn doctor_times_reports_a_note_changed_outside_noda() {
    let (_root, paths) = initialized();
    let added = cmd::add(&paths, Some("Alpha"), Some("a\n"), &[]).unwrap();
    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);
    let path = notebook.join(note_file(&added));

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

/// The cheap half of the flag: the two fields checked against each other, without git.
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

    // Reported, not refused: it must not come between somebody and their prose.
    assert!(cmd::show(&paths, "unreadable").unwrap().contains("body"));
}

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

/// Keeping either note's identity discards the other's, so it is only reported.
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

/// It might be a note that lost its frontmatter, or never a note; only its author knows.
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

    // Not noda's business, and it must not block the adoptable note's repair.
    std::fs::write(notebook.join("scratch.md"), "just some markdown\n").unwrap();
    plant_unnamed(&notebook, "hand-written");

    let out = plain(&cmd::doctor(&paths, false, false, false).unwrap());
    assert!(out.contains("adopted 1 note"), "{out}");
    assert!(!out.contains("scratch.md"), "{out}");
    assert!(notebook.join("scratch.md").exists(), "left where it was");
}

fn plant_file(notebook: &Path, name: &str) {
    std::fs::write(notebook.join(name), "contents\n").unwrap();
}

/// The expensive checks are opt-in, so the default run neither performs nor mentions them.
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

/// A retitle leaves a link naming a path that is gone and an id that is not;
/// noda knows what the link should say.
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

/// A note-shaped destination whose id the notebook lacks resolves to nothing,
/// so only its author can settle it.
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

/// A hook fires under `git commit` but not under `noda add`; this says so.
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

/// Neither would run under git either.
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

/// One `read_dir`, so not behind a flag — but no hooks means no line.
#[test]
fn doctor_says_nothing_about_hooks_when_there_are_none() {
    let (_root, paths) = initialized();
    cmd::add(&paths, Some("Alpha"), Some("a\n"), &[]).unwrap();

    let out = plain(&cmd::doctor(&paths, false, false, false).unwrap());
    assert!(out.contains("in order"), "{out}");
    assert!(!out.contains("hook"), "{out}");
}

#[cfg(unix)]
#[test]
fn doctor_ignores_a_symlink_to_a_directory_among_the_hooks() {
    let (_root, paths) = initialized();
    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);
    plant_hook(&notebook, "post-commit", false);
    std::os::unix::fs::symlink(&notebook, notebook.join(".git/hooks/pre-commit.d")).unwrap();

    let out = plain(&cmd::doctor(&paths, false, false, false).unwrap());
    assert!(out.contains("in order"), "{out}");
}

/// A hook is not a problem with the notes, so it stays out of the summary.
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

/// Git looks in `core.hooksPath`, so noda does: hooks left in `.git/hooks` are
/// dead under git too.
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

/// Why Markdown is parsed rather than searched for the filename.
#[test]
fn only_a_real_link_counts_as_a_reference() {
    let (_root, paths) = initialized();
    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);
    // In a fenced block: prose about a link, not a link.
    cmd::add(
        &paths,
        Some("Alpha"),
        Some("```\n![d](quoted.png)\n```\n"),
        &[],
    )
    .unwrap();
    // Only at the bottom (a reference definition), which a search of the paragraph would miss.
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

/// Outside the notebook or on another server: not a file this notebook can be missing.
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

/// One tagged note and one file.
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

/// `--time` is about terminal width, and a program is not reading a terminal.
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

/// The default row is narrow: the slug repeats the title, and two RFC 3339
/// columns are forty characters.
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

    let plain_out = plain(&cmd::ls(&paths, &cmd::List::default()).unwrap());
    assert!(!plain_out.contains('Z'), "no times by default: {plain_out}");
    let alpha = plain_out.lines().find(|l| l.contains("Alpha")).unwrap();
    assert_eq!(
        alpha.split_whitespace().collect::<Vec<_>>(),
        [id, "Alpha"],
        "the id and the title, and nothing that repeats the title: {alpha}"
    );

    let out = plain(
        &cmd::ls(
            &paths,
            &cmd::List {
                long: true,
                ..Default::default()
            },
        )
        .unwrap(),
    );
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

/// Padding goes outside the escapes: a cell measured with its escapes misaligns
/// every column after it, and spaces before a reset escape `trim_end`.
#[test]
fn ls_colours_the_columns_without_moving_them() {
    let (_root, paths) = initialized();
    let added = cmd::add(&paths, Some("Alpha"), Some("a\n"), &["work".to_string()]).unwrap();
    let short = parts(&added).1.to_string();
    let added = cmd::add(&paths, Some("A Much Longer Title"), Some("b\n"), &[]).unwrap();
    let long_slug = parts(&added).1.to_string();
    // No times, so the last column is short on this row and full on the others —
    // the only way to get padding right of the rightmost cell.
    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);
    std::fs::write(
        notebook.join("k3f9m2p1-undated.md"),
        "---\ntitle: Undated\n---\n\nbody\n",
    )
    .unwrap();

    let out = cmd::ls(
        &paths,
        &cmd::List {
            long: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(out.contains('\u{1b}'), "the listing is coloured: {out:?}");

    for line in out.lines() {
        assert!(
            !plain(line).ends_with(' '),
            "a row ends where its content ends: {line:?}"
        );
    }

    let stripped = plain(&out);
    let slug_at = |slug: &str| {
        stripped
            .lines()
            .find(|line| line.contains(slug))
            .and_then(|line| line.find(slug))
            .expect("the slug column")
    };
    assert_eq!(
        slug_at(&short),
        slug_at(&long_slug),
        "the slug column holds its place: {stripped}"
    );
    assert_eq!(slug_at(&short), slug_at("undated"), "{stripped}");
}

/// Grey rather than `dim`, which a terminal may ignore. Read before `plain` strips it.
#[test]
fn ls_greys_the_punctuation_a_tag_list_is_written_with() {
    let (_root, paths) = initialized();
    cmd::add(
        &paths,
        Some("Alpha"),
        Some("a\n"),
        &["work".to_string(), "q3".to_string()],
    )
    .unwrap();

    let out = cmd::ls(&paths, &cmd::List::default()).unwrap();
    let row = out.lines().next().unwrap();
    let grey = "\u{1b}[90m";
    let cyan = "\u{1b}[36m";
    for punctuation in ["[", ", ", "]"] {
        assert!(
            row.contains(&format!("{grey}{punctuation}")),
            "{punctuation:?} is grey: {row:?}"
        );
    }
    for tag in ["work", "q3"] {
        assert!(
            row.contains(&format!("{cyan}{tag}")),
            "{tag:?} keeps the tag colour: {row:?}"
        );
    }
    assert!(
        !row.contains("\u{1b}[2m"),
        "nothing in the column leans on dim: {row:?}"
    );
    assert!(plain(&out).contains("[work, q3]"), "{out}");
}

/// A script cutting the first two fields reads the same thing either way, and
/// the one field a note may lack stays last in both.
#[test]
fn ls_long_keeps_the_columns_the_default_listing_starts_with() {
    let (_root, paths) = initialized();
    cmd::add(&paths, Some("Alpha"), Some("a\n"), &["work".to_string()]).unwrap();
    cmd::add(&paths, Some("Bravo"), Some("b\n"), &[]).unwrap();

    let head = |long| {
        plain(
            &cmd::ls(
                &paths,
                &cmd::List {
                    long,
                    ..Default::default()
                },
            )
            .unwrap(),
        )
        .lines()
        .map(|l| l.split_whitespace().take(2).collect::<Vec<_>>().join(" "))
        .collect::<Vec<_>>()
    };
    assert_eq!(head(false), head(true), "the id and the title, either way");

    let long = plain(
        &cmd::ls(
            &paths,
            &cmd::List {
                long: true,
                ..Default::default()
            },
        )
        .unwrap(),
    );
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

/// Parsed rather than compared as text: an imported note keeps its old offset.
#[test]
fn ls_sorts_by_time_across_the_offsets_an_import_brings() {
    let (_root, paths) = initialized();
    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);
    // 08:21Z, written as 16:21+08:00 — text order would put it last.
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

/// Applied after the sort, so it needs no `--sort`.
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

/// One listing, one order: notes and files reverse together.
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

/// A note by its id and a file by its name, as the next command expects.
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

/// `noda file add` allows spaces in names, so newline-separated output is not
/// safe for `xargs`.
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

    // The same subsetting in the other two formats.
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

/// Runs the real binary: `-0` is a promise about the bytes that leave the
/// process, and colour handling strips NUL along with the escapes.
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

/// A file elsewhere on disk to copy into a notebook.
fn source_file(root: &TempRoot, name: &str) -> PathBuf {
    let dir = root.0.join("elsewhere");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, "contents\n").unwrap();
    path
}

#[test]
fn readme_writes_the_front_page_and_commits_it() {
    let (_root, paths) = initialized();
    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);
    let commits = commit_count(&notebook);

    let out = plain(&cmd::readme(&paths, false).unwrap());
    assert!(out.contains("wrote README.md"), "{out}");
    assert!(out.contains(cmd::DEFAULT_NOTEBOOK), "and says where: {out}");

    let written = std::fs::read_to_string(notebook.join("README.md")).unwrap();
    assert!(
        written.starts_with(&format!("# {}\n", cmd::DEFAULT_NOTEBOOK)),
        "the notebook names itself: {written}"
    );
    assert!(
        written.contains("<id>-<slug>.md"),
        "and explains the filenames, which is what a stranger asks first: {written}"
    );
    assert!(
        written.contains(&format!("noda use {}", cmd::DEFAULT_NOTEBOOK)),
        "and how to work on it: {written}"
    );
    assert_eq!(
        commit_count(&notebook),
        commits + 1,
        "one commit, revertible like every other change"
    );
}

#[test]
fn readme_says_nothing_that_the_next_note_would_falsify() {
    let (_root, paths) = initialized();
    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);
    cmd::add(&paths, Some("Alpha"), Some("body\n"), &[]).unwrap();
    cmd::readme(&paths, false).unwrap();

    let written = std::fs::read_to_string(notebook.join("README.md")).unwrap();
    assert!(
        !written.contains("Alpha"),
        "no index of the notes: it would be stale from the next `noda add` onward, \
         and `noda ls` is that list: {written}"
    );
}

#[test]
fn readme_will_not_overwrite_prose_someone_wrote() {
    let (_root, paths) = initialized();
    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);
    cmd::readme(&paths, false).unwrap();
    std::fs::write(notebook.join("README.md"), "# hand-written\n").unwrap();
    let commits = commit_count(&notebook);

    let err = cmd::readme(&paths, false).unwrap_err().to_string();
    assert!(err.contains("already exists"), "{err}");
    assert!(
        err.contains("--force"),
        "and says how to get past it: {err}"
    );
    assert_eq!(
        std::fs::read_to_string(notebook.join("README.md")).unwrap(),
        "# hand-written\n",
        "the one already there is untouched"
    );
    assert_eq!(
        commit_count(&notebook),
        commits,
        "and nothing was committed"
    );
}

#[test]
fn readme_force_replaces_it_in_a_commit() {
    let (_root, paths) = initialized();
    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);
    std::fs::write(notebook.join("README.md"), "# hand-written\n").unwrap();
    let commits = commit_count(&notebook);

    let out = plain(&cmd::readme(&paths, true).unwrap());
    assert!(out.contains("rewrote README.md"), "{out}");
    assert!(
        std::fs::read_to_string(notebook.join("README.md"))
            .unwrap()
            .contains("A [noda]"),
        "the template replaced it"
    );
    assert_eq!(
        commit_count(&notebook),
        commits + 1,
        "so `git revert` brings the old one back"
    );
}

#[test]
fn readme_is_a_file_rather_than_a_note_and_never_an_orphan() {
    let (_root, paths) = initialized();
    cmd::add(&paths, Some("Alpha"), Some("body\n"), &[]).unwrap();
    cmd::readme(&paths, false).unwrap();

    let listed = plain(&cmd::ls(&paths, &cmd::List::default()).unwrap());
    assert!(
        listed.contains("files\n  README.md"),
        "it is one of the notebook's files, not one of its notes: {listed}"
    );

    let out = plain(&cmd::doctor(&paths, false, true, false).unwrap());
    assert!(
        !out.contains("README.md"),
        "and never reported as unlinked — the only way to clear that would be to \
         link the front page from a note, which reads backwards: {out}"
    );
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

/// Every source is checked before anything is copied.
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
        // An id-and-slug `*.md` name reads as a note that lost its frontmatter.
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

/// Links point at an attachment's name, so it is refused rather than cut like a
/// slug — in noda's words, not an errno from the copy.
#[test]
fn file_add_refuses_a_name_too_long_to_be_a_filename() {
    let (root, paths) = initialized();
    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);
    let source = source_file(&root, "diagram.png");
    cmd::file_add(&paths, std::slice::from_ref(&source), None).unwrap();
    let commits = commit_count(&notebook);
    let too_long = format!("{}.png", "a".repeat(252));
    assert_eq!(too_long.len(), 256);

    let err = cmd::file_add(&paths, std::slice::from_ref(&source), Some(&too_long))
        .unwrap_err()
        .to_string();
    assert!(err.contains("has to fit in 255 bytes"), "{err}");
    assert!(err.contains("is 256"), "it says how far over: {err}");

    // The other command that writes a name.
    let err = cmd::file_mv(&paths, "diagram.png", &too_long, false)
        .unwrap_err()
        .to_string();
    assert!(err.contains("has to fit in 255 bytes"), "{err}");

    assert!(notebook.join("diagram.png").is_file(), "left where it was");
    assert_eq!(commit_count(&notebook), commits, "and nothing committed");
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

/// A note has an identity to lose, so removing one is a different command.
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

/// The default: rename, then report the links that now point at nothing.
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

/// A mechanical fixup; dating every note today would flatten their order.
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

/// Opt-in, because it edits notes the command was not pointed at.
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

/// A backslash-escaped destination is not in the source literally, so it is
/// reported rather than assumed fixed.
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

/// noda never guesses between a note's slug and a file of the same name.
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

/// A query as a shell hands it over: one token per word.
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

    // A body hit quotes its line.
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

    // A title or tag hit needs no excerpt.
    let out = plain(&search(&paths, "work").unwrap());
    assert_eq!(out.lines().count(), 1, "{out}");
    assert!(out.contains("[work]"), "{out}");
    assert_eq!(
        plain(&search(&paths, "reading").unwrap()).lines().count(),
        1
    );

    // Substring, not whole word: "budget" finds "budgets".
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

    // No spaces to tokenise on, so substring matching is the point.
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

    // The frontmatter is not searchable. `text:---` because a leading `-` negates.
    assert!(search(&paths, "text:---").unwrap().is_empty());
    assert!(search(&paths, "title:").is_err(), "a field needs a value");
    // The id is the filename, not text in the note; `id:` asks for it.
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

    // Past the unpushed-mark margin, a space on every row since there is no remote.
    let fields: Vec<&str> = lines[0].trim_start().split("  ").collect();
    assert_eq!(fields[0].len(), 7, "abbreviated commit id: {out}");
    assert_eq!(fields[1].len(), 16, "YYYY-MM-DD HH:MM: {out}");

    let limited = cmd::log(&paths, None, Some(1)).unwrap();
    assert_eq!(limited.lines().count(), 1);
}

/// `status` says how many to push; this says which.
#[test]
fn log_marks_the_commits_the_remote_has_not_seen() {
    let (root, paths) = initialized();
    let branch = branch_of(&paths, cmd::DEFAULT_NOTEBOOK);
    let url = bare_remote(&root, "origin.git", &branch);

    // No remote: every commit is technically unpushed, so marking any would say nothing.
    cmd::add(&paths, Some("Alpha"), Some("a\n"), &[]).unwrap();
    let out = plain(&cmd::log(&paths, None, None).unwrap());
    assert!(!out.contains('↑'), "nothing to compare against yet: {out}");

    cmd::remote_set(&paths, &url).unwrap();
    cmd::push(&paths).unwrap();
    cmd::add(&paths, Some("Beta"), Some("b\n"), &[]).unwrap();
    cmd::add(&paths, Some("Gamma"), Some("c\n"), &[]).unwrap();

    let out = plain(&cmd::log(&paths, None, None).unwrap());
    let lines: Vec<&str> = out.lines().collect();
    assert!(lines[0].starts_with('↑'), "{out}");
    assert!(lines[1].starts_with('↑'), "{out}");
    assert!(lines[2].starts_with(' '), "already pushed: {out}");

    // The count and the marks are one judgement.
    let status = plain(&cmd::status(&paths).unwrap());
    assert_eq!(
        status_row(&status, "sync"),
        Some("2 to push (as of the last sync)")
    );
    assert_eq!(out.matches('↑').count(), 2, "{out}");

    // The margin is one character on every row, so the ids stay in one column.
    for line in &lines {
        let id: String = line.chars().skip(2).take(7).collect();
        assert!(
            id.len() == 7 && id.chars().all(|c| c.is_ascii_hexdigit()),
            "the id column moved on `{line}`: {out}"
        );
    }
}

/// After a merging `pull` the unpushed commits are not a run along the top of
/// the log, so walking down from `HEAD` would mark one the remote has.
#[test]
fn a_merge_leaves_the_unpushed_commits_scattered_and_they_are_still_right() {
    let (root, paths) = initialized();
    let branch = branch_of(&paths, cmd::DEFAULT_NOTEBOOK);
    let url = bare_remote(&root, "origin.git", &branch);
    cmd::remote_set(&paths, &url).unwrap();
    cmd::add(&paths, Some("Alpha"), Some("a\n"), &[]).unwrap();
    cmd::sync(&paths).unwrap();

    // The other machine adds one and sends it.
    mirror(&paths, &url, "mirror");
    cmd::use_notebook(&paths, "mirror").unwrap();
    cmd::add(&paths, Some("Theirs"), Some("t\n"), &[]).unwrap();
    cmd::sync(&paths).unwrap();

    // This one adds its own without having seen that, so the pull merges.
    cmd::use_notebook(&paths, cmd::DEFAULT_NOTEBOOK).unwrap();
    cmd::add(&paths, Some("Mine"), Some("m\n"), &[]).unwrap();
    cmd::pull(&paths).unwrap();

    let out = plain(&cmd::log(&paths, None, None).unwrap());
    let marked: Vec<&str> = out.lines().filter(|line| line.starts_with('↑')).collect();

    // The merge and the local commit; `theirs` sits between them and is what a
    // linear scan would get wrong.
    assert_eq!(marked.len(), 2, "{out}");
    assert!(marked.iter().any(|line| line.contains("merge:")), "{out}");
    assert!(marked.iter().any(|line| line.contains("mine")), "{out}");
    assert!(
        out.lines()
            .any(|line| line.contains("theirs") && line.starts_with(' ')),
        "a commit the remote already has was marked: {out}"
    );

    // The marks are `graph_ahead_behind`'s answer enumerated, so they match `status`.
    let status = plain(&cmd::status(&paths).unwrap());
    assert_eq!(
        status_row(&status, "sync"),
        Some("2 to push (as of the last sync)")
    );
}

/// `-n` can cut above the oldest unpushed commit; a subset of marks must not
/// pass for the whole.
#[test]
fn a_cut_listing_says_how_many_marks_are_below_it() {
    let (root, paths) = initialized();
    let branch = branch_of(&paths, cmd::DEFAULT_NOTEBOOK);
    let url = bare_remote(&root, "origin.git", &branch);
    cmd::remote_set(&paths, &url).unwrap();
    cmd::add(&paths, Some("Alpha"), Some("a\n"), &[]).unwrap();
    cmd::push(&paths).unwrap();
    cmd::add(&paths, Some("Beta"), Some("b\n"), &[]).unwrap();
    cmd::add(&paths, Some("Gamma"), Some("c\n"), &[]).unwrap();

    let cut = plain(&cmd::log(&paths, None, Some(1)).unwrap());
    assert!(cut.contains("1 more to push"), "{cut}");

    let whole = plain(&cmd::log(&paths, None, None).unwrap());
    assert!(!whole.contains("more to push"), "{whole}");

    // Nor for one note's log: `unpushed` counts branch commits, so subtracting
    // one note's rows from it means nothing.
    let note = plain(&cmd::log(&paths, Some("alpha"), Some(1)).unwrap());
    assert!(!note.contains("more to push"), "{note}");
}

#[test]
fn log_for_a_note_follows_it_across_a_rename() {
    let (_root, paths) = initialized();
    cmd::add(&paths, Some("Alpha"), Some("a\n"), &[]).unwrap();
    cmd::tag(&paths, "alpha", &["+work".to_string()], cmd::Touch::Stamp).unwrap();
    cmd::mv(&paths, "alpha", "Renamed", false, cmd::Touch::Stamp).unwrap();
    cmd::add(&paths, Some("Beta"), Some("b\n"), &[]).unwrap();

    let out = plain(&cmd::log(&paths, Some("renamed"), None).unwrap());
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 3, "{out}");
    assert!(lines[0].ends_with("mv: alpha -> renamed"), "{out}");
    assert!(lines[1].ends_with("tag: alpha"), "{out}");
    assert!(lines[2].ends_with("add: alpha"), "{out}");
    assert!(!out.contains("beta"), "{out}");

    // The id addresses the same history as the current slug.
    let id = plain(&cmd::ls(&paths, &cmd::List::default()).unwrap())
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

    let out = plain(&cmd::diff(&paths, None, false).unwrap());
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

    let out = plain(&cmd::diff(&paths, None, false).unwrap());
    assert!(out.contains("+changed by hand"), "{out}");
    assert!(out.contains("-a"), "{out}");
    assert!(!out.contains("beta"), "only what changed: {out}");

    let scoped = plain(&cmd::diff(&paths, Some("beta"), false).unwrap());
    assert!(scoped.is_empty(), "beta is untouched: {scoped}");
}

/// `status` counts, `log` marks which, and this shows what is in them.
#[test]
fn diff_against_the_remote_shows_what_a_push_would_carry() {
    let (root, paths) = initialized();
    let branch = branch_of(&paths, cmd::DEFAULT_NOTEBOOK);
    let url = bare_remote(&root, "origin.git", &branch);
    cmd::remote_set(&paths, &url).unwrap();
    cmd::add(&paths, Some("Alpha"), Some("a\n"), &[]).unwrap();

    // Never synced: the notebook differs from the remote by everything, and
    // "no changes" would be the wrong answer that looks right.
    let err = cmd::diff(&paths, None, true).unwrap_err().to_string();
    assert!(err.contains("never synced"), "{err}");
    assert!(
        err.contains("noda sync"),
        "a way out, not just a refusal: {err}"
    );

    cmd::push(&paths).unwrap();
    let level = plain(&cmd::diff(&paths, None, true).unwrap());
    assert!(level.is_empty(), "nothing to send: {level}");

    cmd::add(&paths, Some("Beta"), Some("b\n"), &[]).unwrap();
    let out = plain(&cmd::diff(&paths, None, true).unwrap());
    assert!(out.contains("+b"), "{out}");
    assert!(!out.contains("alpha"), "alpha is already there: {out}");

    let scoped = plain(&cmd::diff(&paths, Some("alpha"), true).unwrap());
    assert!(
        scoped.is_empty(),
        "alpha is already on the remote: {scoped}"
    );
}

/// Why `origin/main...HEAD` and not `origin/main HEAD`: with unpulled remote
/// commits, the two-dot form's rename detection (needed because `noda mv`
/// renames) pairs their file with yours and reports a rename that never happened:
///
/// ```text
/// c7pjk17v-theirnote.md => pt1a8xar-beta.md | 4 ++--
/// ```
#[test]
fn diffing_against_the_remote_ignores_what_has_not_been_pulled() {
    let (root, paths) = initialized();
    let branch = branch_of(&paths, cmd::DEFAULT_NOTEBOOK);
    let url = bare_remote(&root, "origin.git", &branch);
    cmd::remote_set(&paths, &url).unwrap();
    cmd::add(&paths, Some("Alpha"), Some("a\n"), &[]).unwrap();
    cmd::sync(&paths).unwrap();

    // The other machine writes one and sends it.
    mirror(&paths, &url, "mirror");
    cmd::use_notebook(&paths, "mirror").unwrap();
    cmd::add(&paths, Some("Theirs"), Some("t\n"), &[]).unwrap();
    cmd::sync(&paths).unwrap();

    cmd::use_notebook(&paths, cmd::DEFAULT_NOTEBOOK).unwrap();
    cmd::add(&paths, Some("Mine"), Some("m\n"), &[]).unwrap();

    // Fetch without merging — what a rolled-back pull leaves.
    let repo = git2::Repository::open(paths.notebook_dir(cmd::DEFAULT_NOTEBOOK)).unwrap();
    repo.find_remote("origin")
        .unwrap()
        .fetch(
            &[format!("+refs/heads/{branch}:refs/remotes/origin/{branch}")],
            None,
            None,
        )
        .unwrap();

    let status = plain(&cmd::status(&paths).unwrap());
    assert_eq!(
        status_row(&status, "sync"),
        Some("1 to push, 1 to pull (as of the last sync)")
    );

    let out = plain(&cmd::diff(&paths, None, true).unwrap());
    assert!(out.contains("mine"), "what a push would carry: {out}");
    assert!(
        !out.contains("theirs"),
        "a commit that was never pulled turned up in what this end would send: {out}"
    );
    assert!(!out.contains("=>"), "a rename nobody performed: {out}");
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
    // All but `updated`, which records the write just made (tested below).
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

/// `updated` answers "when did this file last change", not "which version is this".
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

    // Two back now. Asking again is no change: only `updated` differs, and it is
    // not compared.
    let out = cmd::restore(&paths, "alpha", "HEAD~2", cmd::Touch::Stamp).unwrap();
    assert!(out.contains("(no change)"), "{out}");
}

/// With nothing overwritten, "no change" compares in full rather than holding
/// `updated` aside.
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

    // The same revision again compares in full and still says nothing changed.
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

    // By an id no file carries any more: commits record the filenames.
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

/// Deleted with `rm(1)`, so nothing recorded it; the filename comes back with the file.
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

/// The commit before the deletion, which is what `restore` needs — not a `~1`
/// left for the reader to work out.
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

/// `mv` changes a filename, not an identity; the tree comparison compares ids.
#[test]
fn deleted_does_not_count_a_rename() {
    let (_root, paths) = initialized();
    cmd::add(&paths, Some("Beta"), Some("b\n"), &[]).unwrap();
    cmd::mv(&paths, "beta", "Beta Renamed", false, cmd::Touch::Stamp).unwrap();

    assert_eq!(cmd::deleted(&paths, None, false).unwrap(), "");
}

/// No commit message is read, so a plain-git deletion is found like `noda rm`'s.
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

/// The last disappearance counts, so the commit offered undoes that one.
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

    // Changed, then lost again: the older commit would bring back the wrong contents.
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

/// Full object ids, not the table's abbreviations, which can stop being unique later.
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
    // RFC 3339 UTC, like a note's own times.
    assert!(out.contains("\"removed_at\":\"20"), "{out}");
    assert!(out.contains("Z\""), "{out}");

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

/// An empty list is an answer; the table prints nothing.
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

    // The note exists, but not that far back.
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
    let root = TempRoot::new();
    let paths = root.paths();
    // The template rather than `init`, because `initialized()` writes its own
    // config; this is the call `init` makes when there is none.
    assert!(noda::config::Config::write_template(&paths).expect("template written"));

    let text = std::fs::read_to_string(config_file(&paths)).expect("config written");
    assert!(text.contains("# editor ="), "{text}");
    assert!(text.contains("# author ="), "{text}");
    assert!(text.contains("# notebook ="), "{text}");
    assert!(text.contains("# sign ="), "{text}");
    assert!(
        text.lines()
            .all(|line| line.trim().is_empty() || line.trim_start().starts_with('#')),
        "everything is commented out, so the defaults still apply: {text}"
    );

    // So nothing reports the file as its source. The values are the machine's:
    // `sign` follows git's `commit.gpgsign`.
    let shown = plain(&cmd::config_show(&paths).unwrap());
    assert_eq!(shown.lines().count(), 4, "{shown}");
    assert!(shown.contains("notebook  default"), "{shown}");
    assert!(!shown.contains("(config.toml)"), "{shown}");

    // A second init does not overwrite what the user wrote.
    let (_root, paths) = initialized();
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

    // Falls back to the environment or the built-in, which varies by machine,
    // so only the source is checked.
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
    // The starter config, since `initialized()`'s has no header.
    let root = TempRoot::new();
    let paths = root.paths();
    noda::config::Config::write_template(&paths).expect("template");
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

    // A name without an email would end up in every commit.
    let err = cmd::config_set(&paths, "author", "just-a-name")
        .unwrap_err()
        .to_string();
    assert!(err.contains("Name <email>"), "{err}");

    // Invalid TOML is reported against its path.
    std::fs::write(config_file(&paths), "editor = = nvim\n").unwrap();
    let err = cmd::config_show(&paths).unwrap_err().to_string();
    assert!(err.contains("config.toml"), "{err}");
}

#[test]
fn the_configured_notebook_is_what_init_creates_and_what_stands_in() {
    let root = TempRoot::new();
    let paths = root.paths();
    std::fs::create_dir_all(paths.config_dir()).unwrap();
    std::fs::write(config_file(&paths), "notebook = \"work\"\nsign = false\n").unwrap();

    cmd::init(&paths).unwrap();
    assert!(paths.notebook_dir("work").join(".git").is_dir());
    assert!(!paths.notebook_dir(cmd::DEFAULT_NOTEBOOK).exists());
    assert_eq!(cmd::notebook_current(&paths).unwrap(), "work");

    cmd::add(&paths, Some("Alpha"), Some("a\n"), &[]).unwrap();

    // State is disposable; config is not, so losing the pointer must not lose the notebook.
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

/// The `commit` column of each `noda blame` line.
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

/// libgit2's blame stops at a rename (every `TRACK_COPIES` option is
/// unimplemented) and `noda mv` renames on retitle; picking the note by id avoids it.
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

/// `updated` changes on every edit, so blaming frontmatter would credit it all
/// to the latest commit.
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

/// Lines edited outside noda belong to no commit, and are shown that way.
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

/// Crediting a line to a commit that never saw it would be worse than saying so.
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

    // The linking note has nothing pointing at it.
    let out = plain(&cmd::backlinks(&paths, &source_slug, cmd::Format::Table).unwrap());
    assert!(out.contains("nothing links to"), "{out}");
}

/// `noda mv` leaves the link naming a path that is gone and an id that is not.
#[test]
fn backlinks_survive_a_retitle() {
    let (_root, paths) = initialized();
    let ((_, target), (source_id, _)) = linked_pair(&paths);
    cmd::mv(&paths, &target, "Weekly sync", false, cmd::Touch::Stamp).unwrap();

    // Broken for any Markdown reader, stale for noda: the id still names one note.
    let audit = plain(&cmd::doctor(&paths, false, true, false).unwrap());
    assert!(audit.contains("stale link"), "{audit}");

    let out = plain(&cmd::backlinks(&paths, "weekly-sync", cmd::Format::Table).unwrap());
    assert!(
        out.contains(&source_id),
        "the id in the destination still names the note: {out}"
    );
}

/// An attachment's name is its whole identity, but the question is the same.
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

/// It is what the file says; leaving it out would be noda overruling the author.
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

/// `link::targets` is a set: the question is which notes, not how many times.
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

/// The same rule `doctor --links` follows: Markdown is parsed, not searched.
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

/// A fixed "today", so overdue does not depend on the clock.
const TODAY: &str = "2026-08-02";

/// A body of action items opens with `- `, which clap reads as an option unless
/// told otherwise. Runs the real binary, since `cmd::` calls never meet the parser.
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

/// Why the palette has an exception. Reads the raw output, before `plain`.
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

/// Today is not late: the comparison is `<`.
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

/// Like `ls` and `deleted`: an empty list is still a JSON document.
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

/// A cut-off item would have to be read in the note.
#[test]
fn todo_does_not_truncate_an_item() {
    let (_root, paths) = initialized();
    let long = "chase the vendor about the revised statement of work and the indemnity clause";
    cmd::add(&paths, Some("Alpha"), Some(&format!("- [ ] {long}\n")), &[]).unwrap();

    let out = plain(&cmd::todo_on(&paths, false, TODAY).unwrap());
    assert!(out.contains(long), "{out}");
    assert!(!out.contains('…'), "{out}");
}

/// A `sync` merge carries a note across unchanged and must not be credited with it.
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

/// Annotated, not lightweight: a snapshot records who closed a chapter and when.
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

/// `restore` already accepted a tag; this is what makes one.
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

/// Like `sync`: a snapshot that left out what is on disk would be of something
/// nobody has.
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

    // A clean notebook gains no empty commit.
    let out = cmd::snapshot(&paths, "again", None).unwrap();
    assert!(!out.contains("commit:"), "{out}");
    assert_eq!(commit_count(&notebook), before + 1);
}

/// A name that can be reassigned cannot be cited.
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
fn a_tag_on_a_blob_does_not_break_the_snapshot_listing() {
    let (_root, paths) = initialized();
    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);
    cmd::add(&paths, Some("Alpha"), Some("a\n"), &[]).unwrap();
    cmd::snapshot(&paths, "kept", None).unwrap();

    let repo = git2::Repository::open(&notebook).unwrap();
    let blob = repo.blob(b"not a commit").unwrap();
    repo.reference("refs/tags/on-a-blob", blob, false, "test")
        .unwrap();

    let listed = plain(&cmd::snapshot_ls(&paths).unwrap());
    assert!(listed.contains("kept"), "{listed}");
    assert!(!listed.contains("on-a-blob"), "{listed}");
}

#[test]
fn snapshot_says_when_there_are_none() {
    let (_root, paths) = initialized();
    let out = plain(&cmd::snapshot_ls(&paths).unwrap());
    assert!(out.contains("no snapshots"), "{out}");
    assert!(out.contains("noda snapshot <name>"), "{out}");
}

/// A snapshot that stays on one machine cannot be cited from another.
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

    mirror(&paths, &url, "mirror");
    cmd::use_notebook(&paths, "mirror").unwrap();
    let listed = plain(&cmd::snapshot_ls(&paths).unwrap());
    assert!(listed.contains("q3"), "{listed}");
    assert!(listed.contains("end of quarter"), "{listed}");
}

/// Two machines' `q3` must not silently overwrite each other, and the clash must
/// not block the notes, which sending the tag anyway would.
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
    // The notes still went: a disputed name must not hold up the prose.
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

/// A small export written to a file, as a browser's "export all" would.
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

/// Whatever the conversion did, the export's text is in history, one command away.
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

#[test]
fn import_keeps_a_multi_line_field_without_it_reading_as_another() {
    let (root, paths) = initialized();
    let file = export(
        &root,
        "wiki.json",
        r#"[{"title":"Kept","text":"body","caption":"one\ntitle: two"}]"#,
    );
    cmd::import_tiddlywiki(&paths, std::slice::from_ref(&file), true).unwrap();

    let text = note_text(&paths, "kept");
    assert!(text.contains(r#"caption: "one\ntitle: two""#), "{text}");
    let out = plain(&cmd::ls(&paths, &cmd::List::default()).unwrap());
    assert!(out.contains("Kept"), "{out}");
    assert!(
        !out.contains("two"),
        "the value's second line is not a title: {out}"
    );
}

/// A title too long for a filename used to abort the whole import, leaving
/// pass one's notes uncommitted.
#[test]
fn import_takes_a_tiddler_whose_title_is_too_long_for_a_filename() {
    let (root, paths) = initialized();
    let title = "GitHub - coding-horror/basic-computer-games: An updated version of the \
classic Basic Computer Games book, with well-written examples in a variety of \
common MEMORY SAFE, SCRIPTING programming languages. See \
https://coding-horror.github.io/basic-computer-games/";
    let file = export(
        &root,
        "wiki.json",
        &format!(r#"[{{"title":"{title}","text":"body\n"}}]"#),
    );

    let out = plain(&cmd::import_tiddlywiki(&paths, std::slice::from_ref(&file), true).unwrap());

    assert!(out.contains("imported  1 note from tiddlywiki"), "{out}");
    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);
    let names: Vec<String> = std::fs::read_dir(&notebook)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| note::names_a_note(name))
        .collect();
    assert_eq!(names.len(), 1, "{names:?}");
    let name = &names[0];
    assert!(name.len() <= 255, "{} bytes, {name}", name.len());
    assert!(
        std::fs::read_to_string(notebook.join(name))
            .unwrap()
            .contains(&format!("title: {title}")),
        "the tiddler's own title is not cut"
    );
}

/// A refused write used to end the run with `?`, leaving everything so far
/// uncommitted. It is now a per-note reason, like a title `check` rejected.
#[test]
fn import_reports_a_note_it_could_not_write_instead_of_ending_the_run() {
    let (root, paths) = initialized();
    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);
    let file = export(&root, "wiki.json", TIDDLERS);
    let commits = commit_count(&notebook);

    // A single write cannot be made to fail (the filename has a minted id, so
    // no collision can be planted), so every write fails: the directory is read-only.
    let was = std::fs::metadata(&notebook).unwrap().permissions();
    let mut readonly = was.clone();
    readonly.set_readonly(true);
    std::fs::set_permissions(&notebook, readonly).unwrap();
    if std::fs::write(notebook.join("probe"), "x").is_ok() {
        // Running as root, or on a filesystem that ignores the mode.
        std::fs::remove_file(notebook.join("probe")).unwrap();
        std::fs::set_permissions(&notebook, was).unwrap();
        return;
    }

    let out = cmd::import_tiddlywiki(&paths, std::slice::from_ref(&file), true);

    std::fs::set_permissions(&notebook, was).unwrap();
    let out = plain(&out.expect("a refused write is reported, not returned as an error"));
    assert!(out.contains("imported  0 notes from tiddlywiki"), "{out}");
    assert!(out.contains("not imported:"), "{out}");
    assert!(
        out.contains("2 Permission denied"),
        "the OS's own words for why: {out}"
    );
    assert_eq!(commit_count(&notebook), commits, "nothing committed");
    let left = std::fs::read_dir(&notebook)
        .unwrap()
        .filter(|entry| note::names_a_note(&entry.as_ref().unwrap().file_name().to_string_lossy()))
        .count();
    assert_eq!(left, 0, "and nothing left in the working tree");
}

/// The target file is not named until its id is minted, so links are rewritten
/// once every note exists — the second pass.
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

/// `--no-convert` is the first pass only.
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

/// A frontmatter field rather than a tag, because tags belong to whoever writes
/// the notes; `doctor` makes the field findable.
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

/// Links run between the pieces of a split export, so several files are one import.
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

/// Overlapping pieces: the first copy lands and the second is reported.
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

/// The resolver starts from what the notebook already holds, so a wiki can be
/// imported over several sittings.
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

/// Every file is read before anything is written.
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

/// Reads the commit on stdin and prints a fixed armored block. A real key would
/// test gpg instead, on a keyring that may be locked.
fn stub_gpg(root: &TempRoot, name: &str, script: &str) -> String {
    let path = root.0.join(name);
    std::fs::write(&path, format!("#!/bin/sh\n{script}")).expect("write stub");
    let mut perms = std::fs::metadata(&path).expect("stat stub").permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut perms, 0o755);
    std::fs::set_permissions(&path, perms).expect("chmod stub");
    path.to_str().expect("utf-8 path").to_string()
}

/// A stub that signs; `FAILS` refuses.
const SIGNS: &str = "cat > /dev/null\n\
     printf -- '-----BEGIN PGP SIGNATURE-----\\n\\nc3R1Yg==\\n-----END PGP SIGNATURE-----\\n'\n";
const FAILS: &str = "cat > /dev/null\nexit 1\n";

/// `gpg.openpgp.program` and `gpg.format` are consulted first; without pinning
/// them a developer's own setting would sign with their real key, and the test
/// would pass without testing the stub.
fn sign_with(paths: &Paths, notebook: &str, program: &str) {
    let repo = git2::Repository::open(paths.notebook_dir(notebook)).expect("open repo");
    let mut config = repo.config().expect("config");
    config.set_str("gpg.format", "openpgp").expect("set format");
    config
        .set_str("gpg.openpgp.program", program)
        .expect("set gpg.openpgp.program");
    cmd::config_set(paths, "sign", "true").expect("sign on");
}

fn head_signature(notebook: &Path) -> Option<String> {
    let repo = git2::Repository::open(notebook).expect("open repo");
    let head = repo.head().expect("head").peel_to_commit().expect("commit");
    repo.extract_signature(&head.id(), None)
        .ok()
        .map(|(sig, _)| sig.as_str().expect("utf-8 signature").to_string())
}

#[test]
fn signing_attaches_the_signature_and_still_moves_the_branch() {
    let (root, paths) = initialized();
    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);
    assert_eq!(head_signature(&notebook), None, "unsigned by default");

    let before = commit_count(&notebook);
    sign_with(
        &paths,
        cmd::DEFAULT_NOTEBOOK,
        &stub_gpg(&root, "gpg-ok", SIGNS),
    );
    let out = cmd::add(&paths, Some("Alpha"), Some("a\n"), &[]).unwrap();

    let signature = head_signature(&notebook).expect("signed");
    assert!(signature.contains("BEGIN PGP SIGNATURE"), "{signature}");

    // `commit_signed` only writes an object; the branch has to move with it and
    // leave a clean working tree.
    assert_eq!(commit_count(&notebook), before + 1);
    assert!(
        cmd::ls(&paths, &cmd::List::default())
            .unwrap()
            .contains("Alpha")
    );
    assert!(notebook.join(note_file(&out)).exists());
    let repo = git2::Repository::open(&notebook).unwrap();
    assert!(
        repo.statuses(None).unwrap().is_empty(),
        "nothing uncommitted"
    );

    // The next commit builds on it.
    cmd::add(&paths, Some("Beta"), Some("b\n"), &[]).unwrap();
    let head = repo.head().unwrap().peel_to_commit().unwrap();
    assert_eq!(head.parent_count(), 1);
    assert_eq!(commit_count(&notebook), before + 2);
}

#[test]
fn a_gpg_that_fails_takes_the_commit_with_it() {
    let (root, paths) = initialized();
    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);
    sign_with(
        &paths,
        cmd::DEFAULT_NOTEBOOK,
        &stub_gpg(&root, "gpg-no", FAILS),
    );
    let before = commit_count(&notebook);

    let err = cmd::add(&paths, Some("Alpha"), Some("a\n"), &[])
        .unwrap_err()
        .to_string();
    assert!(err.contains("could not sign"), "{err}");
    assert_eq!(
        commit_count(&notebook),
        before,
        "an unsigned commit is not the fallback"
    );
}

#[test]
fn a_gpg_that_is_not_gpg_is_not_taken_at_its_word() {
    let (root, paths) = initialized();
    let notebook = paths.notebook_dir(cmd::DEFAULT_NOTEBOOK);
    // Exits 0 and prints nothing, as a wrong `gpg.program` would.
    let quiet = stub_gpg(&root, "gpg-quiet", "cat > /dev/null\n");
    sign_with(&paths, cmd::DEFAULT_NOTEBOOK, &quiet);
    let before = commit_count(&notebook);

    let err = cmd::add(&paths, Some("Alpha"), Some("a\n"), &[])
        .unwrap_err()
        .to_string();
    assert!(err.contains("no OpenPGP signature"), "{err}");
    assert_eq!(commit_count(&notebook), before);
}

#[test]
fn a_format_noda_cannot_sign_is_refused_at_the_commit() {
    let (root, paths) = initialized();
    sign_with(
        &paths,
        cmd::DEFAULT_NOTEBOOK,
        &stub_gpg(&root, "gpg-ok2", SIGNS),
    );
    let repo = git2::Repository::open(paths.notebook_dir(cmd::DEFAULT_NOTEBOOK)).unwrap();
    repo.config().unwrap().set_str("gpg.format", "ssh").unwrap();

    let err = cmd::add(&paths, Some("Alpha"), Some("a\n"), &[])
        .unwrap_err()
        .to_string();
    assert!(err.contains("OpenPGP only"), "{err}");

    // Reading still works: the notebook is unsignable, not broken.
    assert!(cmd::ls(&paths, &cmd::List::default()).is_ok());
}

#[test]
fn the_merge_a_pull_makes_is_signed_too() {
    let (root, paths) = initialized();
    let branch = branch_of(&paths, cmd::DEFAULT_NOTEBOOK);
    let url = bare_remote(&root, "origin.git", &branch);
    cmd::remote_set(&paths, &url).unwrap();
    cmd::add(&paths, Some("Alpha"), Some("a\n"), &[]).unwrap();
    cmd::sync(&paths).unwrap();
    mirror(&paths, &url, "mirror");

    cmd::add(&paths, Some("Laptop"), Some("l\n"), &[]).unwrap();
    cmd::sync(&paths).unwrap();

    // The merge is a commit like any other, so it is signed like any other.
    cmd::use_notebook(&paths, "mirror").unwrap();
    sign_with(&paths, "mirror", &stub_gpg(&root, "gpg-merge", SIGNS));
    cmd::add(&paths, Some("Desktop"), Some("d\n"), &[]).unwrap();
    let out = cmd::sync(&paths).unwrap();
    assert!(out.contains("merged"), "{out}");

    let notebook = paths.notebook_dir("mirror");
    let repo = git2::Repository::open(&notebook).unwrap();
    let head = repo.head().unwrap().peel_to_commit().unwrap();
    assert_eq!(head.parent_count(), 2, "the merge is what HEAD points at");
    assert!(
        head_signature(&notebook)
            .expect("signed merge")
            .contains("BEGIN PGP SIGNATURE")
    );
    assert_eq!(repo.state(), git2::RepositoryState::Clean);
}

#[test]
fn sign_is_a_setting_like_any_other() {
    let (_root, paths) = initialized();

    assert_eq!(cmd::config_get(&paths, "sign").unwrap(), "false");
    cmd::config_set(&paths, "sign", "true").unwrap();
    assert_eq!(cmd::config_get(&paths, "sign").unwrap(), "true");

    // Written as a boolean, not a string.
    let text = std::fs::read_to_string(config_file(&paths)).unwrap();
    assert!(text.contains("sign = true"), "{text}");

    let shown = plain(&cmd::config_show(&paths).unwrap());
    assert!(shown.contains("sign      true"), "{shown}");
    assert!(shown.contains("(config.toml)"), "{shown}");

    // git's spellings (`yes`) are refused rather than read as false.
    let err = cmd::config_set(&paths, "sign", "yes")
        .unwrap_err()
        .to_string();
    assert!(err.contains("`true` or `false`"), "{err}");
    assert_eq!(cmd::config_get(&paths, "sign").unwrap(), "true");

    // Unset hands the decision back to git.
    let out = cmd::config_unset(&paths, "sign").unwrap();
    assert!(out.contains("now from"), "{out}");
}
