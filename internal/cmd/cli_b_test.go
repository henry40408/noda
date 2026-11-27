package cmd

import (
	"context"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"testing"

	"github.com/henry40408/noda/internal/config"
	"github.com/henry40408/noda/internal/gitx"
	"github.com/henry40408/noda/internal/gitx/gittest"
	"github.com/henry40408/noda/internal/note"
	"github.com/henry40408/noda/internal/paths"
)

// bInit is an isolated home with the default notebook created by `init`.
// Signing is off in noda's own config, which outranks git's.
func bInit(t *testing.T) Env {
	t.Helper()
	e := Env{Paths: paths.Rooted(t.TempDir()), Git: gittest.Require(t)}
	if err := os.MkdirAll(e.Paths.ConfigDir(), 0o777); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(e.Paths.ConfigDir(), "config.toml"), []byte("sign = false\n"), 0o666); err != nil {
		t.Fatal(err)
	}
	if _, err := Init(e); err != nil {
		t.Fatal(err)
	}
	return e
}

// bMust makes `must(Push(e))` read as the Rust `.unwrap()`.
func bMust(t *testing.T) func(string, error) string {
	return func(out string, err error) string {
		t.Helper()
		if err != nil {
			t.Fatal(err)
		}
		return out
	}
}

// bErr is the message of a command that has to fail.
func bErr(t *testing.T, out string, err error) string {
	t.Helper()
	if err == nil {
		t.Fatalf("expected an error, got %q", out)
	}
	return err.Error()
}

func bAdd(t *testing.T, e Env, title, content string) string {
	t.Helper()
	out, err := Add(e, AddOptions{Title: &title, Content: &content})
	if err != nil {
		t.Fatal(err)
	}
	return out
}

func bGit(t *testing.T, e Env, dir string, args ...string) string {
	t.Helper()
	result, err := e.Git.Run(context.Background(), gitx.Cmd{Dir: dir, Args: args})
	if err != nil {
		t.Fatal(err)
	}
	return strings.TrimSpace(string(result.Stdout))
}

func bCommitCount(t *testing.T, e Env, notebook string) int {
	t.Helper()
	count, err := strconv.Atoi(bGit(t, e, notebook, "rev-list", "--count", "HEAD"))
	if err != nil {
		t.Fatal(err)
	}
	return count
}

func bMergeCommits(t *testing.T, e Env, notebook string) int {
	t.Helper()
	count, err := strconv.Atoi(bGit(t, e, notebook, "rev-list", "--count", "--merges", "HEAD"))
	if err != nil {
		t.Fatal(err)
	}
	return count
}

// bBareRemote is a bare repository standing in for GitHub, reached through a
// file:// URL so push and fetch run over a real transport without a network.
func bBareRemote(t *testing.T, e Env, name, branch string) string {
	t.Helper()
	path := filepath.Join(t.TempDir(), name)
	bGit(t, e, filepath.Dir(path), "init", "--bare", path)
	bGit(t, e, path, "symbolic-ref", "HEAD", "refs/heads/"+branch)
	return "file://" + path
}

// bBranchOf is `main` or `master` per `init.defaultBranch`, so no test may
// assume either.
func bBranchOf(t *testing.T, e Env, name string) string {
	t.Helper()
	opened, err := e.open(name)
	if err != nil {
		t.Fatal(err)
	}
	branch, err := opened.Branch()
	if err != nil {
		t.Fatal(err)
	}
	return branch
}

// bMirror is a notebook wired to url, cloned so it has its own history to diverge.
func bMirror(t *testing.T, e Env, url, name string) {
	t.Helper()
	if _, err := Clone(e, url, name); err != nil {
		t.Fatal(err)
	}
}

// bCommitWorkingTree commits the way an edit made outside noda would arrive.
func bCommitWorkingTree(t *testing.T, e Env, name, message string) {
	t.Helper()
	opened, err := e.open(name)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := opened.CommitAll(message); err != nil {
		t.Fatal(err)
	}
}

func bStatusRow(status, key string) (string, bool) {
	for _, line := range strings.Split(status, "\n") {
		if strings.HasPrefix(line, key) {
			return strings.TrimSpace(line[len(key):]), true
		}
	}
	return "", false
}

// bRow is a status row that has to be there.
func bRow(t *testing.T, status, key string) string {
	t.Helper()
	row, ok := bStatusRow(status, key)
	if !ok {
		t.Fatalf("no %q row in:\n%s", key, status)
	}
	return row
}

func bLineCount(text string) int {
	if text == "" {
		return 0
	}
	return len(strings.Split(strings.TrimSuffix(text, "\n"), "\n"))
}

func bLineWith(t *testing.T, listed string, match func(string) bool) string {
	t.Helper()
	for _, line := range strings.Split(listed, "\n") {
		if match(line) {
			return line
		}
	}
	t.Fatalf("no such line in:\n%s", listed)
	return ""
}

func bContains(t *testing.T, text string, wants ...string) {
	t.Helper()
	for _, want := range wants {
		if !strings.Contains(text, want) {
			t.Fatalf("missing %q in:\n%s", want, text)
		}
	}
}

func bLacks(t *testing.T, text string, unwanted ...string) {
	t.Helper()
	for _, unwant := range unwanted {
		if strings.Contains(text, unwant) {
			t.Fatalf("unexpected %q in:\n%s", unwant, text)
		}
	}
}

func bParts(summary string) (id, slug string) {
	id, rest, _ := strings.Cut(summary, "  ")
	slug, _, _ = strings.Cut(rest, "  ")
	return id, slug
}

func bNoteFile(summary string) string {
	id, slug := bParts(summary)
	return id + "-" + slug + ".md"
}

// bPlant writes an adopted note directly, the way a merge or another machine would.
func bPlant(t *testing.T, notebook, id, slug string) {
	t.Helper()
	text := "---\ntitle: " + slug + "\n---\n\nbody\n"
	if err := os.WriteFile(filepath.Join(notebook, id+"-"+slug+".md"), []byte(text), 0o666); err != nil {
		t.Fatal(err)
	}
}

// bPlantUnnamed writes a note with frontmatter but no id in its name.
func bPlantUnnamed(t *testing.T, notebook, name string) {
	t.Helper()
	text := "---\ntitle: " + name + "\n---\n\nbody\n"
	if err := os.WriteFile(filepath.Join(notebook, name+".md"), []byte(text), 0o666); err != nil {
		t.Fatal(err)
	}
}

func bPlantFile(t *testing.T, notebook, name string) {
	t.Helper()
	if err := os.WriteFile(filepath.Join(notebook, name), []byte("contents\n"), 0o666); err != nil {
		t.Fatal(err)
	}
}

func bWrite(t *testing.T, path, text string) {
	t.Helper()
	if err := os.WriteFile(path, []byte(text), 0o666); err != nil {
		t.Fatal(err)
	}
}

func bRead(t *testing.T, path string) string {
	t.Helper()
	text, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	return string(text)
}

// bPlantHook writes a file into the notebook's .git/hooks, executable or not.
func bPlantHook(t *testing.T, notebook, name string, executable bool) {
	t.Helper()
	dir := filepath.Join(notebook, ".git", "hooks")
	if err := os.MkdirAll(dir, 0o777); err != nil {
		t.Fatal(err)
	}
	mode := os.FileMode(0o644)
	if executable {
		mode = 0o755
	}
	path := filepath.Join(dir, name)
	bWrite(t, path, "#!/bin/sh\nexit 0\n")
	if err := os.Chmod(path, mode); err != nil {
		t.Fatal(err)
	}
}

func bLinkedPair(t *testing.T, e Env) (targetID, targetSlug, sourceSlug string) {
	t.Helper()
	targetID, targetSlug = bParts(bAdd(t, e, "Meeting notes", "agenda\n"))
	_, sourceSlug = bParts(bAdd(t, e, "Q3 budget", "see [the meeting]("+targetID+"-"+targetSlug+".md)\n"))
	return targetID, targetSlug, sourceSlug
}

func bShow(t *testing.T, e Env, key string) string {
	t.Helper()
	out, err := Show(e, key)
	if err != nil {
		t.Fatal(err)
	}
	return out
}

func bLs(t *testing.T, e Env) string {
	t.Helper()
	out, err := Ls(e, List{})
	if err != nil {
		t.Fatal(err)
	}
	return out
}

func bDefaultDir(e Env) string { return e.Paths.NotebookDir(config.DefaultNotebook) }

func TestNotebookAddCreatesARepoAndRecordsItsRemote(t *testing.T) {
	e := bInit(t)

	if _, err := NotebookAdd(e, "work", "git@github.com:me/work-notes.git"); err != nil {
		t.Fatal(err)
	}
	if info, err := os.Stat(filepath.Join(e.Paths.NotebookDir("work"), ".git")); err != nil || !info.IsDir() {
		t.Fatal("no .git directory")
	}

	listed, err := NotebookLs(e)
	if err != nil {
		t.Fatal(err)
	}
	bContains(t, listed, "git@github.com:me/work-notes.git")

	// A duplicate name, and a name that escapes the data dir.
	if _, err := NotebookAdd(e, "work", ""); err == nil {
		t.Fatal("a duplicate name was accepted")
	}
	if _, err := NotebookAdd(e, "../escape", ""); err == nil {
		t.Fatal("a name that escapes was accepted")
	}
}

func TestNotebookLsMarksTheActiveNotebook(t *testing.T) {
	e := bInit(t)
	must := bMust(t)
	must(NotebookAdd(e, "work", ""))

	listed := must(NotebookLs(e))
	lines := strings.Split(strings.TrimSuffix(listed, "\n"), "\n")
	if len(lines) != 2 {
		t.Fatalf("want 2 lines:\n%s", listed)
	}
	if !strings.HasPrefix(lines[0], "* default") || !strings.HasPrefix(lines[1], "  work") {
		t.Fatalf("%s", listed)
	}

	must(Use(e, "work"))
	listed = must(NotebookLs(e))
	if !strings.HasPrefix(strings.Split(listed, "\n")[1], "* work") {
		t.Fatalf("%s", listed)
	}
}

func TestUseSwitchesWhichNotebookTheNoteCommandsSee(t *testing.T) {
	e := bInit(t)
	must := bMust(t)
	bAdd(t, e, "Alpha", "a\n")
	must(NotebookAdd(e, "work", ""))

	must(Use(e, "work"))
	if current := must(NotebookCurrent(e)); current != "work" {
		t.Fatalf("current: %q", current)
	}
	if listed := bLs(t, e); listed != "" {
		t.Fatalf("the new notebook is not empty: %q", listed)
	}
	if _, err := Show(e, "alpha"); err == nil {
		t.Fatal("notebooks are separate")
	}

	bAdd(t, e, "Work Item", "w\n")
	bContains(t, bLs(t, e), "Work Item")
	bContains(t, must(Ls(e, List{Notebook: "default"})), "Alpha")

	if _, err := Use(e, "missing"); err == nil {
		t.Fatal("switched to a notebook that is not there")
	}
}

func TestNotebookRmRefusesTheActiveOne(t *testing.T) {
	e := bInit(t)
	bMust(t)(NotebookAdd(e, "work", ""))

	out, err := NotebookRm(e, config.DefaultNotebook, true)
	message := bErr(t, out, err)
	bContains(t, message, "noda use")
	if !fileExists(bDefaultDir(e)) {
		t.Fatal("the active notebook was removed")
	}

	bMust(t)(NotebookRm(e, "work", true))
	if fileExists(e.Paths.NotebookDir("work")) {
		t.Fatal("work is still there")
	}
	if _, err := NotebookRm(e, "work", true); err == nil {
		t.Fatal("already gone")
	}
}

func TestNotebookRmAsksBeforeDeletingAndTakesNoForAnAnswer(t *testing.T) {
	e := bInit(t)
	must := bMust(t)
	must(NotebookAdd(e, "work", ""))
	must(Use(e, "work"))
	bAdd(t, e, "Alpha", "a\n")
	must(Use(e, config.DefaultNotebook))

	e.Confirm = func(question string) (bool, error) {
		bContains(t, question, "cannot be undone", "1 note ")
		return false, nil
	}
	out := must(NotebookRm(e, "work", false))
	bContains(t, out, "kept")
	if !fileExists(e.Paths.NotebookDir("work")) {
		t.Fatal("no still means no")
	}

	e.Confirm = func(string) (bool, error) {
		t.Fatal("--force must not ask")
		return false, nil
	}
	must(NotebookRm(e, "work", true))
	if fileExists(e.Paths.NotebookDir("work")) {
		t.Fatal("work is still there")
	}
}

// `noda status` covers only the active notebook; this shows how far behind the
// others are, without going to the network.
func TestNotebookLsSaysWhereEachNotebookStands(t *testing.T) {
	e := bInit(t)
	must := bMust(t)
	url := bBareRemote(t, e, "origin.git", bBranchOf(t, e, config.DefaultNotebook))

	// No remote is not the same as never synced: it can never leave that state.
	must(NotebookAdd(e, "solo", ""))
	listed := plain(NotebookLs(e))
	solo := bLineWith(t, listed, func(l string) bool { return strings.Contains(l, "solo") })
	bContains(t, solo, "no remote")

	must(RemoteSet(e, url))
	listed = plain(NotebookLs(e))
	active := bLineWith(t, listed, func(l string) bool { return strings.HasPrefix(l, "*") })
	bContains(t, active, "never synced")

	bAdd(t, e, "Alpha", "a\n")
	must(Push(e))
	listed = plain(NotebookLs(e))
	active = bLineWith(t, listed, func(l string) bool { return strings.HasPrefix(l, "*") })
	bContains(t, active, "in sync")

	bAdd(t, e, "Beta", "b\n")
	listed = plain(NotebookLs(e))
	active = bLineWith(t, listed, func(l string) bool { return strings.HasPrefix(l, "*") })
	bContains(t, active, "1 to push")

	// Nothing above touches the network, so an unreachable remote costs nothing
	// to list. A path that was never created stands in for one.
	must(NotebookAdd(e, "gone", "file:///nowhere/at/all.git"))
	listed = plain(NotebookLs(e))
	gone := bLineWith(t, listed, func(l string) bool { return strings.Contains(l, "gone") })
	bContains(t, gone, "never synced")
}

func TestNotebookRmRefusesWhenThereIsNobodyToAsk(t *testing.T) {
	e := bInit(t)
	bMust(t)(NotebookAdd(e, "work", ""))

	// Nobody to ask is the case being checked: piped or scripted, an
	// irreversible delete is not assumed.
	e.Confirm = nil
	_, err := NotebookRm(e, "work", false)
	if err == nil {
		t.Fatal("deleted without asking")
	}
	bContains(t, err.Error(), "--force")
	if !fileExists(e.Paths.NotebookDir("work")) {
		t.Fatal("work was removed")
	}
}

func TestNotebookRenameCarriesTheActivePointer(t *testing.T) {
	e := bInit(t)
	must := bMust(t)
	bAdd(t, e, "Alpha", "a\n")
	must(NotebookAdd(e, "work", ""))

	must(NotebookRename(e, config.DefaultNotebook, "personal"))
	if current := must(NotebookCurrent(e)); current != "personal" {
		t.Fatalf("current: %q", current)
	}
	if _, err := Show(e, "alpha"); err != nil {
		t.Fatalf("the notes came along: %v", err)
	}
	if fileExists(bDefaultDir(e)) {
		t.Fatal("the old directory is still there")
	}

	must(NotebookRename(e, "work", "archive"))
	if current := must(NotebookCurrent(e)); current != "personal" {
		t.Fatalf("current: %q", current)
	}

	if _, err := NotebookRename(e, "missing", "x"); err == nil {
		t.Fatal("renamed a notebook that is not there")
	}
	if _, err := NotebookRename(e, "archive", "personal"); err == nil {
		t.Fatal("renamed onto an existing notebook")
	}
}

// The container image authenticates over HTTPS with a token in the URL (it has
// no shell for a credential helper), so a remote may carry a secret.
func TestATokenInTheRemoteIsNeverPrintedBack(t *testing.T) {
	const url = "https://x-access-token:ghp_secret@github.com/me/notes.git"
	e := bInit(t)
	must := bMust(t)

	// Four places a remote is shown, starting with `remote set`'s answer.
	screens := []string{must(RemoteSet(e, url)), must(RemoteShow(e)), must(Status(e)), must(NotebookLs(e))}
	for _, shown := range screens {
		bLacks(t, shown, "ghp_secret")
		bContains(t, shown, "***@github.com")
	}

	// Redaction is display only: push and fetch still use what was configured.
	if onDisk := bGit(t, e, bDefaultDir(e), "config", "remote.origin.url"); onDisk != url {
		t.Fatalf("the URL on disk was rewritten: %q", onDisk)
	}
}

func TestPushSaysHowMuchItSent(t *testing.T) {
	e := bInit(t)
	must := bMust(t)
	url := bBareRemote(t, e, "origin.git", bBranchOf(t, e, config.DefaultNotebook))
	must(RemoteSet(e, url))
	bAdd(t, e, "Alpha", "a\n")

	// Never synced: what the remote holds is unknown until fetched, so no
	// count is given rather than one guessed from local history.
	out := plain(Push(e))
	if !strings.HasPrefix(out, "push:") {
		t.Fatalf("%s", out)
	}
	if strings.Contains(out, "commit") {
		t.Fatalf("a first push counted what it could not know: %s", out)
	}

	// Nothing moved, and saying so distinguishes this from the line above.
	bContains(t, plain(Push(e)), "nothing to send")

	bAdd(t, e, "Beta", "b\n")
	bAdd(t, e, "Gamma", "c\n")
	bContains(t, plain(Push(e)), "(2 commits)")

	// Singular for one.
	bAdd(t, e, "Delta", "d\n")
	bContains(t, plain(Push(e)), "(1 commit)")
}

// A push carrying only a snapshot has sent something, so not `nothing to send`.
func TestASnapshotIsSomethingSentEvenWhenNoCommitIs(t *testing.T) {
	e := bInit(t)
	must := bMust(t)
	url := bBareRemote(t, e, "origin.git", bBranchOf(t, e, config.DefaultNotebook))
	must(RemoteSet(e, url))
	bAdd(t, e, "Alpha", "a\n")
	must(Push(e))

	// The notebook is clean, so this adds a tag and no commit.
	must(Snapshot(e, "q3", ""))
	out := plain(Push(e))
	bContains(t, out, "1 snapshot")
	bLacks(t, out, "nothing to send")
}

// Counted between the fetch and the merge, the one moment the tracking ref has
// moved and the branch has not.
func TestPullSaysHowMuchArrived(t *testing.T) {
	e := bInit(t)
	must := bMust(t)
	url := bBareRemote(t, e, "origin.git", bBranchOf(t, e, config.DefaultNotebook))
	must(RemoteSet(e, url))
	bAdd(t, e, "Alpha", "a\n")
	must(Sync(e))

	bMirror(t, e, url, "mirror")
	bAdd(t, e, "Beta", "b\n")
	bAdd(t, e, "Gamma", "c\n")
	must(Sync(e))

	// Only the remote moved: a fast-forward.
	must(Use(e, "mirror"))
	bContains(t, plain(Pull(e)), "fast-forwarded 2 commits")

	// Both sides move, so this makes a merge commit, and the count has to survive it.
	bAdd(t, e, "Local", "l\n")
	must(Use(e, config.DefaultNotebook))
	bAdd(t, e, "Remote", "r\n")
	must(Sync(e))
	must(Use(e, "mirror"))
	bContains(t, plain(Pull(e)), "merged 1 commit")
}

func TestPushAndCloneRoundTripANotebook(t *testing.T) {
	e := bInit(t)
	must := bMust(t)
	url := bBareRemote(t, e, "origin.git", bBranchOf(t, e, config.DefaultNotebook))

	must(RemoteSet(e, url))
	if shown := must(RemoteShow(e)); shown != url {
		t.Fatalf("remote show: %q", shown)
	}
	bAdd(t, e, "Meeting Notes", "agenda\n")
	must(Push(e))

	// No name given: `origin.git` -> `origin`.
	must(Clone(e, url, ""))
	must(Use(e, "origin"))
	bContains(t, bLs(t, e), "Meeting Notes")
	bContains(t, bShow(t, e, "meeting-notes"), "agenda")

	if _, err := Clone(e, url, "origin"); err == nil {
		t.Fatal("cloned over an existing notebook")
	}
}

func TestCloneAdoptsTheOnlyBranchWhenTheRemoteHeadPointsElsewhere(t *testing.T) {
	e := bInit(t)
	must := bMust(t)
	// The remote's HEAD names a branch nothing was pushed to: what two machines
	// with different `init.defaultBranch` produce.
	url := bBareRemote(t, e, "origin.git", "trunk")
	must(RemoteSet(e, url))
	bAdd(t, e, "Alpha", "a\n")
	must(Push(e))

	must(Clone(e, url, "mirror"))
	must(Use(e, "mirror"))
	if !strings.Contains(bLs(t, e), "Alpha") {
		t.Fatal("a clone that checks out nothing reads as an empty notebook, not a broken one")
	}
	if got, want := bBranchOf(t, e, "mirror"), bBranchOf(t, e, config.DefaultNotebook); got != want {
		t.Fatalf("branch %q, want %q", got, want)
	}
}

func TestCloningAnEmptyRemoteLeavesNothingBehind(t *testing.T) {
	e := bInit(t)
	url := bBareRemote(t, e, "empty.git", "main")

	_, err := Clone(e, url, "mirror")
	if err == nil {
		t.Fatal("cloned a remote with nothing in it")
	}
	bContains(t, err.Error(), "no commits")
	if fileExists(e.Paths.NotebookDir("mirror")) {
		t.Fatal("no half-clone for the next attempt to trip over")
	}
}

func TestSyncCommitsPendingChangesBeforePushing(t *testing.T) {
	e := bInit(t)
	must := bMust(t)
	url := bBareRemote(t, e, "origin.git", bBranchOf(t, e, config.DefaultNotebook))
	must(RemoteSet(e, url))
	added := bAdd(t, e, "Alpha", "a\n")

	// Edited outside noda.
	path := filepath.Join(bDefaultDir(e), bNoteFile(added))
	bWrite(t, path, bRead(t, path)+"edited elsewhere\n")

	out := must(Sync(e))
	bContains(t, out, "commit: local changes", "push:")
	if got := bCommitCount(t, e, bDefaultDir(e)); got != 3 {
		t.Fatalf("commits: %d", got)
	}

	bMirror(t, e, url, "mirror")
	must(Use(e, "mirror"))
	if !strings.Contains(bShow(t, e, "alpha"), "edited elsewhere") {
		t.Fatal("the out-of-band edit reached the remote")
	}

	out = must(Sync(e))
	bLacks(t, out, "commit:")
	bContains(t, out, "already up to date")
}

func TestSyncFastForwardsANotebookThatOnlyReceived(t *testing.T) {
	e := bInit(t)
	must := bMust(t)
	url := bBareRemote(t, e, "origin.git", bBranchOf(t, e, config.DefaultNotebook))
	must(RemoteSet(e, url))
	bAdd(t, e, "Alpha", "a\n")
	must(Sync(e))

	bMirror(t, e, url, "mirror")
	bAdd(t, e, "Beta", "b\n")
	must(Sync(e))

	must(Use(e, "mirror"))
	bContains(t, must(Sync(e)), "fast-forwarded")
	bContains(t, bLs(t, e), "Beta")
	if got := bMergeCommits(t, e, e.Paths.NotebookDir("mirror")); got != 0 {
		t.Fatalf("a one-sided sync needs no merge commit, got %d", got)
	}
}

func TestSyncMergesNotebooksThatBothMoved(t *testing.T) {
	e := bInit(t)
	must := bMust(t)
	url := bBareRemote(t, e, "origin.git", bBranchOf(t, e, config.DefaultNotebook))
	must(RemoteSet(e, url))
	bAdd(t, e, "Alpha", "a\n")
	must(Sync(e))
	bMirror(t, e, url, "mirror")

	// Two machines, each writing a different note before either syncs.
	bAdd(t, e, "Laptop", "l\n")
	must(Sync(e))

	must(Use(e, "mirror"))
	bAdd(t, e, "Desktop", "d\n")
	bContains(t, must(Sync(e)), "merged")
	mirrorDir := e.Paths.NotebookDir("mirror")
	if got := bMergeCommits(t, e, mirrorDir); got != 1 {
		t.Fatalf("merge commits: %d", got)
	}

	listed := bLs(t, e)
	bContains(t, listed, "Laptop", "Desktop")

	// Each side wrote its own filename, so the merge is clean.
	if left := bGit(t, e, mirrorDir, "status", "--porcelain"); left != "" {
		t.Fatalf("nothing left over: %q", left)
	}
	if fileExists(filepath.Join(mirrorDir, ".git", "MERGE_HEAD")) {
		t.Fatal("a merge is still in progress")
	}

	must(Use(e, config.DefaultNotebook))
	must(Sync(e))
	bContains(t, bLs(t, e), "Desktop")
}

func TestAConflictingPullIsRolledBack(t *testing.T) {
	e := bInit(t)
	must := bMust(t)
	url := bBareRemote(t, e, "origin.git", bBranchOf(t, e, config.DefaultNotebook))
	must(RemoteSet(e, url))
	bAdd(t, e, "Shared", "original\n")
	must(Sync(e))
	bMirror(t, e, url, "mirror")

	bRewriteShared(t, e, config.DefaultNotebook, "from the laptop\n")
	must(Sync(e))

	must(Use(e, "mirror"))
	bRewriteShared(t, e, "mirror", "from the desktop\n")
	out, err := Sync(e)
	message := bErr(t, out, err)
	bContains(t, message, "shared.md", "rolled back")

	// The rollback leaves a working notebook: no conflict markers, no
	// half-finished merge, the local commit still there.
	mirrorDir := e.Paths.NotebookDir("mirror")
	if left := bGit(t, e, mirrorDir, "status", "--porcelain"); left != "" {
		t.Fatalf("worktree is not clean: %q", left)
	}
	if fileExists(filepath.Join(mirrorDir, ".git", "MERGE_HEAD")) {
		t.Fatal("a merge is still in progress")
	}
	if !strings.Contains(bShow(t, e, "shared"), "from the desktop") {
		t.Fatal("the local edit survived")
	}
}

// bRewriteShared replaces what follows the frontmatter of the shared note.
func bRewriteShared(t *testing.T, e Env, notebook, body string) {
	t.Helper()
	dir := e.Paths.NotebookDir(notebook)
	entries, err := os.ReadDir(dir)
	if err != nil {
		t.Fatal(err)
	}
	for _, entry := range entries {
		if !strings.HasSuffix(entry.Name(), "-shared.md") {
			continue
		}
		path := filepath.Join(dir, entry.Name())
		head := strings.Join(strings.Split(bRead(t, path), "---\n")[:3], "---\n")
		bWrite(t, path, head+body)
		return
	}
	t.Fatal("the shared note is missing")
}

func TestPushIsRejectedWhenTheRemoteMovedAhead(t *testing.T) {
	e := bInit(t)
	must := bMust(t)
	url := bBareRemote(t, e, "origin.git", bBranchOf(t, e, config.DefaultNotebook))
	must(RemoteSet(e, url))
	bAdd(t, e, "Alpha", "a\n")
	must(Push(e))
	bMirror(t, e, url, "mirror")

	bAdd(t, e, "Laptop", "l\n")
	must(Push(e))

	must(Use(e, "mirror"))
	bAdd(t, e, "Desktop", "d\n")
	_, err := Push(e)
	if err == nil {
		t.Fatal("a push that does not fast-forward went through")
	}
	bContains(t, err.Error(), "noda pull")

	must(Pull(e))
	must(Push(e))
}

func TestPullRefusesToRunOverUncommittedChanges(t *testing.T) {
	e := bInit(t)
	must := bMust(t)
	url := bBareRemote(t, e, "origin.git", bBranchOf(t, e, config.DefaultNotebook))
	must(RemoteSet(e, url))
	bAdd(t, e, "Alpha", "a\n")
	path := filepath.Join(bDefaultDir(e), "alpha.md")
	bWrite(t, path, "half-finished\n")

	_, err := Pull(e)
	if err == nil {
		t.Fatal("pulled over uncommitted changes")
	}
	bContains(t, err.Error(), "noda sync")
	if got := bRead(t, path); got != "half-finished\n" {
		t.Fatalf("the refusal touched the file: %q", got)
	}
}

func TestPullingAnEmptyRemoteIsNotAnError(t *testing.T) {
	e := bInit(t)
	must := bMust(t)
	branch := bBranchOf(t, e, config.DefaultNotebook)
	must(RemoteSet(e, bBareRemote(t, e, "origin.git", branch)))

	bContains(t, must(Pull(e)), "no `", branch)
}

func TestTheNetworkCommandsSayWhenNoRemoteIsSet(t *testing.T) {
	e := bInit(t)
	for name, run := range map[string]func() error{
		"remote show": func() error { _, err := RemoteShow(e); return err },
		"push":        func() error { _, err := Push(e); return err },
		"pull":        func() error { _, err := Pull(e); return err },
		"sync":        func() error { _, err := Sync(e); return err },
	} {
		err := run()
		if err == nil || !strings.Contains(err.Error(), "noda remote set") {
			t.Fatalf("%s: %v", name, err)
		}
	}
	if _, err := RemoteSet(e, "  "); err == nil {
		t.Fatal("a URL is required")
	}
}

func TestStatusReportsANotebookWithNowhereToSync(t *testing.T) {
	e := bInit(t)

	out := plain(Status(e))
	if first := strings.Split(bRow(t, out, "notebook"), "  ")[0]; first != "default" {
		t.Fatalf("notebook row: %q", first)
	}
	if got := bRow(t, out, "notes"); got != "0" {
		t.Fatalf("notes: %q", got)
	}
	if got := bRow(t, out, "changes"); got != "clean" {
		t.Fatalf("changes: %q", got)
	}
	bContains(t, bRow(t, out, "remote"), "none")
	if _, ok := bStatusRow(out, "sync"); ok {
		t.Fatalf("with no remote there is nothing to be in sync with: %s", out)
	}

	bAdd(t, e, "Alpha", "a\n")
	bAdd(t, e, "Beta", "b\n")
	out = plain(Status(e))
	if got := bRow(t, out, "notes"); got != "2" {
		t.Fatalf("notes: %q", got)
	}
}

func TestStatusCountsTheDistanceFromTheRemoteWithoutTouchingIt(t *testing.T) {
	e := bInit(t)
	must := bMust(t)
	url := bBareRemote(t, e, "origin.git", bBranchOf(t, e, config.DefaultNotebook))
	must(RemoteSet(e, url))
	bAdd(t, e, "Alpha", "a\n")

	out := plain(Status(e))
	if got := bRow(t, out, "sync"); got != "never synced" {
		t.Fatalf("sync: %q\n%s", got, out)
	}

	must(Sync(e))
	out = plain(Status(e))
	if got := bRow(t, out, "sync"); !strings.HasPrefix(got, "in sync") {
		t.Fatalf("%s", out)
	}

	bAdd(t, e, "Beta", "b\n")
	out = plain(Status(e))
	if got := bRow(t, out, "sync"); !strings.HasPrefix(got, "1 to push") {
		t.Fatalf("%s", out)
	}

	// Behind a second notebook's push, but only once fetched, as status never fetches.
	bMirror(t, e, url, "mirror")
	must(Sync(e))
	must(Use(e, "mirror"))
	out = plain(Status(e))
	if got := bRow(t, out, "sync"); !strings.HasPrefix(got, "in sync") {
		t.Fatalf("stale until it fetches, which is the point: %s", out)
	}
	must(Pull(e))
	out = plain(Status(e))
	if got := bRow(t, out, "sync"); !strings.HasPrefix(got, "in sync") {
		t.Fatalf("%s", out)
	}
}

// A file with neither an id in its name nor frontmatter is listed and counted as
// a file, never as a problem.
func TestAFileThatDeclaresNothingIsListedAsAFile(t *testing.T) {
	e := bInit(t)
	bAdd(t, e, "Alpha", "a\n")
	bWrite(t, filepath.Join(bDefaultDir(e), "stray.md"), "not a note at all\n")

	listed := plain(Ls(e, List{}))
	bContains(t, listed, "Alpha", "files\n  stray.md")

	out := plain(Status(e))
	if got := bRow(t, out, "notes"); got != "1" {
		t.Fatalf("notes: %q\n%s", got, out)
	}
	if got := bRow(t, out, "files"); got != "1" {
		t.Fatalf("files: %q\n%s", got, out)
	}
	if row, ok := bStatusRow(out, "problems"); ok {
		t.Fatalf("problems: %q\n%s", row, out)
	}
	if got := bRow(t, out, "changes"); got != "1 file uncommitted" {
		t.Fatalf("changes: %q", got)
	}
}

// Frontmatter declares a note; one with no id in its name is waiting to be adopted.
func TestStatusReportsANoteWithNoIDInItsName(t *testing.T) {
	e := bInit(t)
	bAdd(t, e, "Alpha", "a\n")

	out := plain(Status(e))
	if row, ok := bStatusRow(out, "problems"); ok {
		t.Fatalf("a healthy notebook says nothing about it: %q", row)
	}

	bPlantUnnamed(t, bDefaultDir(e), "hand-written")
	out = plain(Status(e))
	if got := bRow(t, out, "problems"); got != "1 note has no id in its filename  (hand-written.md)" {
		t.Fatalf("%s", out)
	}
}

// `abcdefgh` is a legal id, so the shape alone cannot tell a broken note from
// somebody's file.
func TestStatusReportsAFileThatClaimsAnIDWithoutFrontmatter(t *testing.T) {
	e := bInit(t)
	bAdd(t, e, "Alpha", "a\n")
	bWrite(t, filepath.Join(bDefaultDir(e), "abcdefgh-hello.md"), "no frontmatter\n")

	out := plain(Status(e))
	if got := bRow(t, out, "problems"); got != "1 file is named like a note but has no frontmatter  (abcdefgh-hello.md)" {
		t.Fatalf("%s", out)
	}
}

// Two machines can mint one id; the filenames differ, so git merges them
// silently and this is the only place it shows.
func TestStatusReportsOneIDCarriedByTwoNotes(t *testing.T) {
	e := bInit(t)
	bPlant(t, bDefaultDir(e), "k3f9m2p1", "alpha")
	// Ids fold case, so `K3F9M2P1` is not a second id.
	bPlant(t, bDefaultDir(e), "K3F9M2P1", "beta")

	out := plain(Status(e))
	if got := bRow(t, out, "problems"); got != "1 id is carried by more than one note  (k3f9m2p1)" {
		t.Fatalf("%s", out)
	}
}

// Minting must see ids that arrived from elsewhere, or it could hand one out
// twice, which cannot be undone.
func TestANewNoteAvoidsAnIDThatArrivedFromOutside(t *testing.T) {
	e := bInit(t)
	bPlant(t, bDefaultDir(e), "zzzzyyyy", "merged")

	opened, err := e.openActive()
	if err != nil {
		t.Fatal(err)
	}
	taken, err := opened.TakenIDs()
	if err != nil {
		t.Fatal(err)
	}
	if _, ok := taken["zzzzyyyy"]; !ok {
		t.Fatalf("%v", taken)
	}

	bAdd(t, e, "Alpha", "a\n")
	if row, ok := bStatusRow(plain(Status(e)), "problems"); ok {
		t.Fatalf("a note that arrived adopted is simply a note: %q", row)
	}
	if got := bLineCount(bLs(t, e)); got != 2 {
		t.Fatalf("lines: %d", got)
	}
}

func TestAWholesaleProblemIsCountedRatherThanListed(t *testing.T) {
	e := bInit(t)
	notebook := bDefaultDir(e)
	bAdd(t, e, "Anchor", "body\n")

	// A directory of hand-written notes copied in at once; `status` must stay one screen.
	for n := 0; n < 12; n++ {
		bPlantUnnamed(t, notebook, "note-"+bPad(n))
	}

	out := plain(Status(e))
	row := bRow(t, out, "problems")
	if !strings.HasPrefix(row, "12 notes have no id in their filenames") {
		t.Fatalf("%s", row)
	}
	if got := strings.Count(row, ".md"); got != 3 {
		t.Fatalf("three named, not twelve: %s", row)
	}
	if !strings.HasSuffix(row, "…)") {
		t.Fatalf("and the rest elided: %s", row)
	}
	// The line count does not grow with the number of notes.
	if got := bLineCount(out); got != 6 {
		t.Fatalf("lines: %d\n%s", got, out)
	}
	for n := 12; n < 48; n++ {
		bPlantUnnamed(t, notebook, "note-"+bPad(n))
	}
	bigger := plain(Status(e))
	if got := bLineCount(bigger); got != 6 {
		t.Fatalf("lines: %d\n%s", got, bigger)
	}
}

func bPad(n int) string {
	if n < 10 {
		return "0" + strconv.Itoa(n)
	}
	return strconv.Itoa(n)
}

func TestSeveralKindsAreTotalledBeforeTheyAreBrokenDown(t *testing.T) {
	e := bInit(t)
	notebook := bDefaultDir(e)
	bAdd(t, e, "Alpha", "a\n")

	bPlantUnnamed(t, notebook, "merged")
	bPlantUnnamed(t, notebook, "dropped-in")
	bWrite(t, filepath.Join(notebook, "abcdefgh-hello.md"), "no frontmatter\n")

	out := plain(Status(e))
	if got := bRow(t, out, "problems"); got != "3 problems" {
		t.Fatalf("the size of it comes first: %s", out)
	}
	bContains(t, out,
		"2 notes have no id in their filenames  (dropped-in.md; merged.md)",
		"1 file is named like a note but has no frontmatter  (abcdefgh-hello.md)")
}

// The one repair that cannot lose anything: the file declares itself a note and
// only lacks a name.
func TestDoctorAdoptsANoteThatOnlyLacksAnID(t *testing.T) {
	e := bInit(t)
	notebook := bDefaultDir(e)
	bPlantUnnamed(t, notebook, "hand-written")
	commits := bCommitCount(t, e, notebook)

	out := plain(Doctor(e, DoctorOptions{}))
	bContains(t, out, "adopted 1 note")
	if fileExists(filepath.Join(notebook, "hand-written.md")) {
		t.Fatal("the file moved to its adopted name")
	}

	listed := bLs(t, e)
	if got := bLineCount(listed); got != 1 {
		t.Fatalf("lines: %d\n%s", got, listed)
	}
	if !strings.Contains(listed, "hand-written") {
		t.Fatalf("the slug survives: %s", listed)
	}
	if row, ok := bStatusRow(plain(Status(e)), "problems"); ok {
		t.Fatalf("the notebook is not in order again: %q", row)
	}
	if got := bCommitCount(t, e, notebook); got != commits+1 {
		t.Fatalf("the repair is a commit, so it can be reverted like any other change: %d, was %d", got, commits)
	}
}

func TestDoctorNamesEveryFileWhereStatusElides(t *testing.T) {
	e := bInit(t)
	for n := 0; n < 12; n++ {
		bPlantUnnamed(t, bDefaultDir(e), "note-"+bPad(n))
	}

	// `status` shows three and a `…`; this shows the rest.
	out := plain(Doctor(e, DoctorOptions{DryRun: true}))
	if got := strings.Count(out, ".md"); got != 12 {
		t.Fatalf("named %d:\n%s", got, out)
	}
	bLacks(t, out, "…")
}

// An edit outside noda leaves `updated` behind; only git saw it, which is why
// this check walks history and is opt-in.
func TestDoctorTimesReportsANoteChangedOutsideNoda(t *testing.T) {
	e := bInit(t)
	added := bAdd(t, e, "Alpha", "a\n")
	notebook := bDefaultDir(e)
	path := filepath.Join(notebook, bNoteFile(added))

	edited, ok := note.SetField(strings.ReplaceAll(bRead(t, path), "a\n", "edited elsewhere\n"), "updated", "2000-01-01T00:00:00Z")
	if !ok {
		t.Fatal("no frontmatter to edit")
	}
	bWrite(t, path, edited)
	bCommitWorkingTree(t, e, config.DefaultNotebook, "edit: by hand")

	bContains(t, plain(Doctor(e, DoctorOptions{})), "in order")

	commits := bCommitCount(t, e, notebook)
	out := plain(Doctor(e, DoctorOptions{Times: true}))
	bContains(t, out, "1 note was changed outside noda", bNoteFile(added))
	if got := bCommitCount(t, e, notebook); got != commits {
		t.Fatalf("the check reports and repairs nothing: %d, was %d", got, commits)
	}
}

// The cheap half of the flag: the two fields checked against each other, without git.
func TestDoctorTimesReportsWhatCannotBeReadAndWhatRunsBackwards(t *testing.T) {
	e := bInit(t)
	notebook := bDefaultDir(e)
	bWrite(t, filepath.Join(notebook, "k3f9m2p1-unreadable.md"),
		"---\ntitle: Unreadable\ncreated: last tuesday\n---\n\nbody\n")
	bWrite(t, filepath.Join(notebook, "k3f9m2p2-backwards.md"),
		"---\ntitle: Backwards\ncreated: 2024-01-01T00:00:00Z\nupdated: 2020-01-01T00:00:00Z\n---\n\nbody\n")

	out := plain(Doctor(e, DoctorOptions{Times: true}))
	bContains(t, out,
		"1 time cannot be read",
		"k3f9m2p1-unreadable.md created: last tuesday",
		"1 note changed before being created",
		"k3f9m2p2-backwards.md")

	// Reported, not refused: it must not come between somebody and their prose.
	bContains(t, bShow(t, e, "unreadable"), "body")
}

func TestDoctorTimesIsQuietAboutNotesNodaWrote(t *testing.T) {
	e := bInit(t)
	must := bMust(t)
	bAdd(t, e, "Alpha", "a\n")
	must(Tag(e, "alpha", []string{"+work"}, TouchStamp))
	must(Mv(e, Rename{Key: "alpha", NewTitle: "Renamed", Touch: TouchStamp}))

	bContains(t, plain(Doctor(e, DoctorOptions{Times: true})), "in order")
}

func TestDoctorWritesNothingOnADryRun(t *testing.T) {
	e := bInit(t)
	notebook := bDefaultDir(e)
	bPlantUnnamed(t, notebook, "hand-written")
	commits := bCommitCount(t, e, notebook)

	bContains(t, plain(Doctor(e, DoctorOptions{DryRun: true})), "nothing was changed")
	if !fileExists(filepath.Join(notebook, "hand-written.md")) {
		t.Fatal("it renames a file, so a look first is free")
	}
	if got := bCommitCount(t, e, notebook); got != commits {
		t.Fatalf("commits: %d, was %d", got, commits)
	}
}

func TestDoctorSaysSoWhenThereIsNothingToDo(t *testing.T) {
	e := bInit(t)
	notebook := bDefaultDir(e)
	bAdd(t, e, "Alpha", "a\n")
	commits := bCommitCount(t, e, notebook)

	bContains(t, plain(Doctor(e, DoctorOptions{})), "in order")
	if got := bCommitCount(t, e, notebook); got != commits {
		t.Fatalf("an empty commit was made: %d, was %d", got, commits)
	}
}

// Keeping either note's identity discards the other's, so it is only reported.
func TestDoctorReportsButDoesNotSettleASharedID(t *testing.T) {
	e := bInit(t)
	notebook := bDefaultDir(e)
	bPlant(t, notebook, "k3f9m2p1", "alpha")
	bPlant(t, notebook, "K3F9M2P1", "beta")
	commits := bCommitCount(t, e, notebook)

	out := plain(Doctor(e, DoctorOptions{}))
	bContains(t, out, "carried by more than one note", "rename one of the files")
	if !fileExists(filepath.Join(notebook, "k3f9m2p1-alpha.md")) || !fileExists(filepath.Join(notebook, "K3F9M2P1-beta.md")) {
		t.Fatal("a file was moved")
	}
	if got := bCommitCount(t, e, notebook); got != commits {
		t.Fatalf("nothing was decided: %d, was %d", got, commits)
	}
}

// It might be a note that lost its frontmatter, or never a note; only its author knows.
func TestDoctorReportsButDoesNotSettleAFileThatClaimsAnID(t *testing.T) {
	e := bInit(t)
	notebook := bDefaultDir(e)
	path := filepath.Join(notebook, "abcdefgh-hello.md")
	bWrite(t, path, "no frontmatter\n")
	commits := bCommitCount(t, e, notebook)

	out := plain(Doctor(e, DoctorOptions{}))
	bContains(t, out, "abcdefgh-hello.md", "add a `---` block back")
	if got := bRead(t, path); got != "no frontmatter\n" {
		t.Fatalf("not untouched: %q", got)
	}
	if got := bCommitCount(t, e, notebook); got != commits {
		t.Fatalf("commits: %d, was %d", got, commits)
	}
}

func TestDoctorIgnoresAFileThatWasNeverANote(t *testing.T) {
	e := bInit(t)
	notebook := bDefaultDir(e)
	bAdd(t, e, "Alpha", "a\n")

	// Not noda's business, and it must not block the adoptable note's repair.
	bWrite(t, filepath.Join(notebook, "scratch.md"), "just some markdown\n")
	bPlantUnnamed(t, notebook, "hand-written")

	out := plain(Doctor(e, DoctorOptions{}))
	bContains(t, out, "adopted 1 note")
	bLacks(t, out, "scratch.md")
	if !fileExists(filepath.Join(notebook, "scratch.md")) {
		t.Fatal("left where it was")
	}
}

// The expensive checks are opt-in, so the default run neither performs nor mentions them.
func TestDoctorSaysNothingAboutLinksUntilItIsAskedTo(t *testing.T) {
	e := bInit(t)
	bAdd(t, e, "Alpha", "see ![d](missing.png)\n")
	bPlantFile(t, bDefaultDir(e), "unreferenced.png")

	out := plain(Doctor(e, DoctorOptions{}))
	bContains(t, out, "in order")
	bLacks(t, out, "unreferenced.png", "missing.png")
}

func TestDoctorLinksReportsAFileNoNoteLinksTo(t *testing.T) {
	e := bInit(t)
	notebook := bDefaultDir(e)
	bAdd(t, e, "Alpha", "![d](used.png)\n")
	bPlantFile(t, notebook, "used.png")
	bPlantFile(t, notebook, "receipt.pdf")
	commits := bCommitCount(t, e, notebook)

	out := plain(Doctor(e, DoctorOptions{Links: true}))
	bContains(t, out, "1 file no note links to", "receipt.pdf")
	if strings.Contains(out, "used.png") {
		t.Fatalf("a file a note links to is not an orphan: %s", out)
	}
	if !fileExists(filepath.Join(notebook, "receipt.pdf")) {
		t.Fatal("reported, never removed")
	}
	if got := bCommitCount(t, e, notebook); got != commits {
		t.Fatalf("and nothing was decided: %d, was %d", got, commits)
	}
}

func TestDoctorLinksReportsALinkThatNamesNothing(t *testing.T) {
	e := bInit(t)
	bAdd(t, e, "Alpha", "see ![d](missing.png)\n")

	out := plain(Doctor(e, DoctorOptions{Links: true}))
	bContains(t, out, "1 broken link", "missing.png", "alpha.md")
}

// A retitle leaves a link naming a path that is gone and an id that is not;
// noda knows what the link should say.
func TestDoctorLinksTellsAStaleLinkFromABrokenOne(t *testing.T) {
	e := bInit(t)
	targetID, _, sourceSlug := bLinkedPair(t, e)
	bMust(t)(Mv(e, Rename{Key: targetID, NewTitle: "Weekly sync", Touch: TouchStamp}))

	out := plain(Doctor(e, DoctorOptions{Links: true}))
	bContains(t, out,
		"1 stale link",
		sourceSlug+".md",
		targetID+"-meeting-notes.md",
		"now "+targetID+"-weekly-sync.md")
	if strings.Contains(out, "broken link") {
		t.Fatalf("a link noda can resolve is not broken: %s", out)
	}
}

// A note-shaped destination whose id the notebook lacks resolves to nothing,
// so only its author can settle it.
func TestDoctorLinksCallsALinkToNoNoteAtAllBroken(t *testing.T) {
	e := bInit(t)
	bAdd(t, e, "Alpha", "see [it](zzzzzzzz-never-here.md)\n")

	out := plain(Doctor(e, DoctorOptions{Links: true}))
	bContains(t, out, "1 broken link", "zzzzzzzz-never-here.md")
	bLacks(t, out, "stale")
}

// A hook fires under `git commit` but not under `noda add`; this says so.
func TestDoctorReportsTheHooksThatWillNeverRun(t *testing.T) {
	e := bInit(t)
	notebook := bDefaultDir(e)
	bPlantHook(t, notebook, "pre-commit", true)
	bPlantHook(t, notebook, "post-commit", true)

	out := plain(Doctor(e, DoctorOptions{}))
	bContains(t, out, "2 git hooks will never run", "pre-commit", "post-commit", "never calls git")
}

// Neither would run under git either.
func TestDoctorIgnoresHooksGitWouldNotRun(t *testing.T) {
	e := bInit(t)
	notebook := bDefaultDir(e)
	bPlantHook(t, notebook, "pre-commit.sample", true)
	bPlantHook(t, notebook, "post-commit", false)

	bContains(t, plain(Doctor(e, DoctorOptions{})), "in order")
}

// One directory read, so not behind a flag, but no hooks means no line.
func TestDoctorSaysNothingAboutHooksWhenThereAreNone(t *testing.T) {
	e := bInit(t)
	bAdd(t, e, "Alpha", "a\n")

	out := plain(Doctor(e, DoctorOptions{}))
	bContains(t, out, "in order")
	bLacks(t, out, "hook")
}

func TestDoctorIgnoresASymlinkToADirectoryAmongTheHooks(t *testing.T) {
	e := bInit(t)
	notebook := bDefaultDir(e)
	bPlantHook(t, notebook, "post-commit", false)
	if err := os.Symlink(notebook, filepath.Join(notebook, ".git", "hooks", "pre-commit.d")); err != nil {
		t.Fatal(err)
	}

	bContains(t, plain(Doctor(e, DoctorOptions{})), "in order")
}

// A hook is not a problem with the notes, so it stays out of the summary.
func TestStatusSaysNothingAboutHooks(t *testing.T) {
	e := bInit(t)
	bAdd(t, e, "Alpha", "a\n")
	bPlantHook(t, bDefaultDir(e), "pre-commit", true)

	bLacks(t, plain(Status(e)), "hook", "problems")
}

// Git looks in `core.hooksPath`, so noda does: hooks left in `.git/hooks` are
// dead under git too.
func TestDoctorFollowsCoreHooksPath(t *testing.T) {
	e := bInit(t)
	notebook := bDefaultDir(e)
	bPlantHook(t, notebook, "pre-commit", true)

	elsewhere := filepath.Join(notebook, "my-hooks")
	if err := os.MkdirAll(elsewhere, 0o777); err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(elsewhere, "post-commit")
	bWrite(t, path, "#!/bin/sh\nexit 0\n")
	if err := os.Chmod(path, 0o755); err != nil {
		t.Fatal(err)
	}
	bGit(t, e, notebook, "config", "core.hooksPath", "my-hooks")

	out := plain(Doctor(e, DoctorOptions{}))
	bContains(t, out, "1 git hook will never run", "post-commit")
	if strings.Contains(out, "pre-commit") {
		t.Fatalf("git would not run it either: %s", out)
	}
}

// `todo` leaves these out without a word, so `doctor` says it without a flag.
func TestDoctorNamesTheActionItemsRawHTMLHides(t *testing.T) {
	e := bInit(t)
	bAdd(t, e, "Alpha", "<details>\n<summary>s</summary>\n- [ ] call the bank\n</details>\n")

	out := plain(Doctor(e, DoctorOptions{}))
	bContains(t, out, "1 action item is hidden from `noda todo`", "-alpha.md: call the bank", "next blank line")

	bAdd(t, e, "Beta", "<div>\n- [ ] file the form\n")
	out = plain(Doctor(e, DoctorOptions{}))
	bContains(t, out, "2 action items are hidden", "-alpha.md: call the bank", "-beta.md: file the form")
}

func TestDoctorSaysNothingAboutASpacedSection(t *testing.T) {
	e := bInit(t)
	bAdd(t, e, "Alpha", "<details>\n<summary>s</summary>\n\n- [ ] a\n\n</details>\n")

	out := plain(Doctor(e, DoctorOptions{}))
	bContains(t, out, "in order")
	bLacks(t, out, "hidden")
}
