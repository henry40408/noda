package cmd

import (
	"context"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strconv"
	"strings"
	"sync"
	"testing"

	"github.com/henry40408/noda/internal/config"
	"github.com/henry40408/noda/internal/gitx"
	"github.com/henry40408/noda/internal/gitx/gittest"
	"github.com/henry40408/noda/internal/note"
	"github.com/henry40408/noda/internal/notebook"
	"github.com/henry40408/noda/internal/paths"
)

// dToday is a fixed "today", so overdue does not depend on the clock.
const dToday = "2026-08-02"

// dInitialized is an isolated home with sign = false and the default notebook
// made by Init, as the Rust `initialized()` does.
func dInitialized(t *testing.T) Env {
	t.Helper()
	e := dBare(t)
	dWriteConfig(t, e, "sign = false\n")
	if _, err := Init(e); err != nil {
		t.Fatal(err)
	}
	return e
}

// dBare is an isolated home with nothing in it.
func dBare(t *testing.T) Env {
	t.Helper()
	return Env{Paths: paths.Rooted(t.TempDir()), Git: gittest.Require(t)}
}

func dConfigFile(e Env) string { return filepath.Join(e.Paths.ConfigDir(), "config.toml") }

func dWriteConfig(t *testing.T, e Env, text string) {
	t.Helper()
	if err := os.MkdirAll(e.Paths.ConfigDir(), 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(dConfigFile(e), []byte(text), 0o644); err != nil {
		t.Fatal(err)
	}
}

func dNotebookDir(e Env) string { return e.Paths.NotebookDir(config.DefaultNotebook) }

// dGit runs the system git in dir and returns its trimmed standard output.
func dGit(t *testing.T, e Env, dir string, args ...string) string {
	t.Helper()
	result, err := e.Git.Run(context.Background(), gitx.Cmd{Dir: dir, Args: args})
	if err != nil {
		t.Fatalf("git %v: %v", args, err)
	}
	return strings.TrimSpace(string(result.Stdout))
}

func dCommitCount(t *testing.T, e Env, dir string) int {
	t.Helper()
	count, err := strconv.Atoi(dGit(t, e, dir, "rev-list", "--count", "HEAD"))
	if err != nil {
		t.Fatal(err)
	}
	return count
}

func dMergeCommits(t *testing.T, e Env, dir string) int {
	t.Helper()
	count, err := strconv.Atoi(dGit(t, e, dir, "rev-list", "--merges", "--count", "HEAD"))
	if err != nil {
		t.Fatal(err)
	}
	return count
}

// dParts is the id and slug from the `id  slug  [tags]` line mutating commands print.
func dParts(summary string) (id, slug string) {
	id, rest, _ := strings.Cut(summary, "  ")
	slug, _, _ = strings.Cut(rest, "  ")
	return id, slug
}

func dNoteFile(summary string) string {
	id, slug := dParts(summary)
	return id + "-" + slug + ".md"
}

func dAdd(t *testing.T, e Env, title, content string, tags ...string) string {
	t.Helper()
	out, err := Add(e, AddOptions{Title: &title, Content: &content, Tags: tags})
	if err != nil {
		t.Fatal(err)
	}
	return out
}

// dDone drops the text of a command; a failure panics, as `plain` does.
func dDone(_ string, err error) {
	if err != nil {
		panic(err)
	}
}

func dMust(t *testing.T, out string, err error) string {
	t.Helper()
	if err != nil {
		t.Fatal(err)
	}
	return out
}

func dRename(t *testing.T, e Env, key, title string) {
	t.Helper()
	if _, err := Mv(e, Rename{Key: key, NewTitle: title, Touch: TouchStamp}); err != nil {
		t.Fatal(err)
	}
}

func dHas(t *testing.T, text, want string) {
	t.Helper()
	if !strings.Contains(text, want) {
		t.Fatalf("missing %q in:\n%s", want, text)
	}
}

func dLacks(t *testing.T, text, unwanted string) {
	t.Helper()
	if strings.Contains(text, unwanted) {
		t.Fatalf("unexpected %q in:\n%s", unwanted, text)
	}
}

func dEqual(t *testing.T, got, want any) {
	t.Helper()
	if got != want {
		t.Fatalf("got %#v, want %#v", got, want)
	}
}

func dFailsWith(t *testing.T, err error, want string) {
	t.Helper()
	if err == nil {
		t.Fatalf("expected an error containing %q", want)
	}
	dHas(t, err.Error(), want)
}

func dRead(t *testing.T, path string) string {
	t.Helper()
	text, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	return string(text)
}

func dWrite(t *testing.T, path, text string) {
	t.Helper()
	if err := os.WriteFile(path, []byte(text), 0o666); err != nil {
		t.Fatal(err)
	}
}

// dEditNote replaces every old with replacement in a note on disk, uncommitted, as Rust `str::replace` does.
func dEditNote(t *testing.T, path, old, replacement string) {
	t.Helper()
	dWrite(t, path, strings.ReplaceAll(dRead(t, path), old, replacement))
}

// dCommitWorkingTree commits the working tree the way an edit made outside noda would arrive.
func dCommitWorkingTree(t *testing.T, e Env, name, message string) {
	t.Helper()
	opened, err := e.open(name)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := opened.CommitAll(message); err != nil {
		t.Fatal(err)
	}
}

func dHeadCommit(t *testing.T, e Env) string {
	t.Helper()
	name, err := notebook.ActiveName(e.Paths)
	if err != nil {
		t.Fatal(err)
	}
	return dGit(t, e, e.Paths.NotebookDir(name), "rev-parse", "HEAD")
}

func dBranchOf(t *testing.T, e Env, name string) string {
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

// dBareRemote is a bare repository standing in for GitHub, its HEAD on branch.
func dBareRemote(t *testing.T, e Env, branch string) string {
	t.Helper()
	root := t.TempDir()
	url := filepath.Join(root, "origin.git")
	dGit(t, e, root, "init", "--bare", url)
	dGit(t, e, url, "symbolic-ref", "HEAD", "refs/heads/"+branch)
	return url
}

// dMirror is a notebook wired to url, cloned so it has its own history to diverge.
func dMirror(e Env, url, name string) { dDone(Clone(e, url, name)) }

func dUse(e Env, name string) { dDone(Use(e, name)) }

// dStatusRow is the value on a `status` row, or ok false when there is none.
func dStatusRow(status, key string) (string, bool) {
	for _, line := range strings.Split(status, "\n") {
		if strings.HasPrefix(line, key) {
			return strings.TrimSpace(line[len(key):]), true
		}
	}
	return "", false
}

func dSyncRow(t *testing.T, e Env) string {
	t.Helper()
	row, _ := dStatusRow(plain(Status(e)), "sync")
	return row
}

// dBackdate sets `updated` far in the past so a change to it is visible however
// fast the test runs.
func dBackdate(t *testing.T, e Env, summary string) string {
	t.Helper()
	path := filepath.Join(dNotebookDir(e), dNoteFile(summary))
	dSetUpdated(t, path, "2000-01-01T00:00:00Z")
	return path
}

func dSetUpdated(t *testing.T, path, stamp string) {
	t.Helper()
	text, ok := note.SetField(dRead(t, path), "updated", stamp)
	if !ok {
		t.Fatal("no frontmatter")
	}
	dWrite(t, path, text)
}

func dPlant(t *testing.T, dir, id, slug string) {
	t.Helper()
	dWrite(t, filepath.Join(dir, id+"-"+slug+".md"), "---\ntitle: "+slug+"\n---\n\nbody\n")
}

// dBlamed is the commit column and the text of each `noda blame` line.
func dBlamed(t *testing.T, e Env, key string) [][2]string {
	t.Helper()
	var rows [][2]string
	for _, line := range strings.Split(strings.TrimSuffix(plain(Blame(e, key)), "\n"), "\n") {
		commit, rest, found := strings.Cut(line, "  ")
		if !found {
			t.Fatalf("no commit column in %q", line)
		}
		rows = append(rows, [2]string{commit, rest[18:]})
	}
	return rows
}

func dExpectBlame(t *testing.T, got [][2]string, want [][2]string, why string) {
	t.Helper()
	if len(got) != len(want) {
		t.Fatalf("%s: got %v, want %v", why, got, want)
	}
	for i := range want {
		if got[i] != want[i] {
			t.Fatalf("%s: got %v, want %v", why, got, want)
		}
	}
}

// dLinkedPair is two notes, the second linking to the first: (id, slug) of each.
func dLinkedPair(t *testing.T, e Env) (targetID, targetSlug, sourceID, sourceSlug string) {
	t.Helper()
	target := dAdd(t, e, "Meeting notes", "agenda\n")
	targetID, targetSlug = dParts(target)
	source := dAdd(t, e, "Q3 budget", "see [the meeting]("+targetID+"-"+targetSlug+".md)\n")
	sourceID, sourceSlug = dParts(source)
	return targetID, targetSlug, sourceID, sourceSlug
}

func dBacklinksTable(t *testing.T, e Env, key string) string {
	t.Helper()
	return plain(Backlinks(e, key, FormatTable))
}

func dRevisionOf(t *testing.T, deleted string) string {
	t.Helper()
	fields := strings.Fields(strings.Split(deleted, "\n")[0])
	if len(fields) < 5 {
		t.Fatalf("no revision column in %q", deleted)
	}
	return fields[4]
}

func TestLogReportsTheNotebookHistoryNewestFirst(t *testing.T) {
	e := dInitialized(t)
	dAdd(t, e, "Alpha", "a\n")
	dAdd(t, e, "Beta", "b\n")

	out := plain(Log(e, "", 0))
	lines := strings.Split(strings.TrimSuffix(out, "\n"), "\n")
	dEqual(t, len(lines), 3)
	if !strings.HasSuffix(lines[0], "add: beta") || !strings.HasSuffix(lines[2], "chore: initialize notebook") {
		t.Fatalf("got:\n%s", out)
	}

	// Past the unpushed-mark margin, a space on every row since there is no remote.
	fields := strings.Split(strings.TrimLeft(lines[0], " "), "  ")
	dEqual(t, len(fields[0]), 7)
	dEqual(t, len(fields[1]), 16)

	limited, err := Log(e, "", 1)
	if err != nil {
		t.Fatal(err)
	}
	dEqual(t, strings.Count(limited, "\n"), 1)
}

// `status` says how many to push; this says which.
func TestLogMarksTheCommitsTheRemoteHasNotSeen(t *testing.T) {
	e := dInitialized(t)
	url := dBareRemote(t, e, dBranchOf(t, e, config.DefaultNotebook))

	// No remote: every commit is technically unpushed, so marking any would say nothing.
	dAdd(t, e, "Alpha", "a\n")
	dLacks(t, plain(Log(e, "", 0)), "↑")

	dDone(RemoteSet(e, url))
	dDone(Push(e))
	dAdd(t, e, "Beta", "b\n")
	dAdd(t, e, "Gamma", "c\n")

	out := plain(Log(e, "", 0))
	lines := strings.Split(strings.TrimSuffix(out, "\n"), "\n")
	if !strings.HasPrefix(lines[0], "↑") || !strings.HasPrefix(lines[1], "↑") || !strings.HasPrefix(lines[2], " ") {
		t.Fatalf("marks:\n%s", out)
	}

	// The count and the marks are one judgement.
	dEqual(t, dSyncRow(t, e), "2 to push (as of the last sync)")
	dEqual(t, strings.Count(out, "↑"), 2)

	// The margin is one character on every row, so the ids stay in one column.
	for _, line := range lines {
		id := string([]rune(line)[2:9])
		if len(id) != 7 || strings.Trim(id, "0123456789abcdefABCDEF") != "" {
			t.Fatalf("the id column moved on %q:\n%s", line, out)
		}
	}
}

// After a merging `pull` the unpushed commits are not a run along the top of
// the log, so walking down from `HEAD` would mark one the remote has.
func TestAMergeLeavesTheUnpushedCommitsScatteredAndTheyAreStillRight(t *testing.T) {
	e := dInitialized(t)
	url := dBareRemote(t, e, dBranchOf(t, e, config.DefaultNotebook))
	dDone(RemoteSet(e, url))
	dAdd(t, e, "Alpha", "a\n")
	dDone(Sync(e))

	// The other machine adds one and sends it.
	dMirror(e, url, "mirror")
	dUse(e, "mirror")
	dAdd(t, e, "Theirs", "t\n")
	dDone(Sync(e))

	// This one adds its own without having seen that, so the pull merges.
	dUse(e, config.DefaultNotebook)
	dAdd(t, e, "Mine", "m\n")
	dDone(Pull(e))

	out := plain(Log(e, "", 0))
	var marked []string
	theirsUnmarked := false
	for _, line := range strings.Split(out, "\n") {
		if strings.HasPrefix(line, "↑") {
			marked = append(marked, line)
		}
		if strings.Contains(line, "theirs") && strings.HasPrefix(line, " ") {
			theirsUnmarked = true
		}
	}

	// The merge and the local commit; `theirs` sits between them and is what a
	// linear scan would get wrong.
	dEqual(t, len(marked), 2)
	joined := strings.Join(marked, "\n")
	dHas(t, joined, "merge:")
	dHas(t, joined, "mine")
	if !theirsUnmarked {
		t.Fatalf("a commit the remote already has was marked:\n%s", out)
	}

	// The marks match `status`.
	dEqual(t, dSyncRow(t, e), "2 to push (as of the last sync)")
}

// `-n` can cut above the oldest unpushed commit; a subset of marks must not
// pass for the whole.
func TestACutListingSaysHowManyMarksAreBelowIt(t *testing.T) {
	e := dInitialized(t)
	url := dBareRemote(t, e, dBranchOf(t, e, config.DefaultNotebook))
	dDone(RemoteSet(e, url))
	dAdd(t, e, "Alpha", "a\n")
	dDone(Push(e))
	dAdd(t, e, "Beta", "b\n")
	dAdd(t, e, "Gamma", "c\n")

	dHas(t, plain(Log(e, "", 1)), "1 more to push")
	dLacks(t, plain(Log(e, "", 0)), "more to push")

	// Nor for one note's log: `unpushed` counts branch commits, so subtracting
	// one note's rows from it means nothing.
	dLacks(t, plain(Log(e, "alpha", 1)), "more to push")
}

func TestLogForANoteFollowsItAcrossARename(t *testing.T) {
	e := dInitialized(t)
	dAdd(t, e, "Alpha", "a\n")
	dDone(Tag(e, "alpha", []string{"+work"}, TouchStamp))
	dRename(t, e, "alpha", "Renamed")
	dAdd(t, e, "Beta", "b\n")

	out := plain(Log(e, "renamed", 0))
	lines := strings.Split(strings.TrimSuffix(out, "\n"), "\n")
	dEqual(t, len(lines), 3)
	if !strings.HasSuffix(lines[0], "mv: alpha -> renamed") || !strings.HasSuffix(lines[1], "tag: alpha") ||
		!strings.HasSuffix(lines[2], "add: alpha") {
		t.Fatalf("got:\n%s", out)
	}
	dLacks(t, out, "beta")

	// The id addresses the same history as the current slug.
	id := ""
	for _, line := range strings.Split(plain(Ls(e, List{})), "\n") {
		if strings.Contains(line, "Renamed") {
			id = strings.Fields(line)[0]
		}
	}
	if id == "" {
		t.Fatal("no id")
	}
	byID, err := Log(e, id, 0)
	if err != nil {
		t.Fatal(err)
	}
	dEqual(t, strings.Count(byID, "\n"), 3)
}

func TestDiffShowsTheLastCommitWhenNothingIsPending(t *testing.T) {
	e := dInitialized(t)
	added := dAdd(t, e, "Alpha", "a\n")

	out := plain(Diff(e, "", false))
	dHas(t, out, "+++ b/"+dNoteFile(added))
	dHas(t, out, "+a")
}

func TestDiffShowsUncommittedChangesWhenThereAreSome(t *testing.T) {
	e := dInitialized(t)
	added := dAdd(t, e, "Alpha", "a\n")
	dAdd(t, e, "Beta", "b\n")

	dEditNote(t, filepath.Join(dNotebookDir(e), dNoteFile(added)), "a\n", "changed by hand\n")

	out := plain(Diff(e, "", false))
	dHas(t, out, "+changed by hand")
	dHas(t, out, "-a")
	dLacks(t, out, "beta")

	if scoped := plain(Diff(e, "beta", false)); scoped != "" {
		t.Fatalf("beta is untouched: %q", scoped)
	}
}

// `status` counts, `log` marks which, and this shows what is in them.
func TestDiffAgainstTheRemoteShowsWhatAPushWouldCarry(t *testing.T) {
	e := dInitialized(t)
	url := dBareRemote(t, e, dBranchOf(t, e, config.DefaultNotebook))
	dDone(RemoteSet(e, url))
	dAdd(t, e, "Alpha", "a\n")

	// Never synced: the notebook differs from the remote by everything, and
	// "no changes" would be the wrong answer that looks right.
	_, err := Diff(e, "", true)
	dFailsWith(t, err, "never synced")
	dFailsWith(t, err, "noda sync")

	dDone(Push(e))
	if level := plain(Diff(e, "", true)); level != "" {
		t.Fatalf("nothing to send: %q", level)
	}

	dAdd(t, e, "Beta", "b\n")
	out := plain(Diff(e, "", true))
	dHas(t, out, "+b")
	dLacks(t, out, "alpha")

	if scoped := plain(Diff(e, "alpha", true)); scoped != "" {
		t.Fatalf("alpha is already on the remote: %q", scoped)
	}
}

// With unpulled remote commits, a two-dot diff's rename detection would pair
// their file with yours and report a rename that never happened.
func TestDiffingAgainstTheRemoteIgnoresWhatHasNotBeenPulled(t *testing.T) {
	e := dInitialized(t)
	branch := dBranchOf(t, e, config.DefaultNotebook)
	url := dBareRemote(t, e, branch)
	dDone(RemoteSet(e, url))
	dAdd(t, e, "Alpha", "a\n")
	dDone(Sync(e))

	// The other machine writes one and sends it.
	dMirror(e, url, "mirror")
	dUse(e, "mirror")
	dAdd(t, e, "Theirs", "t\n")
	dDone(Sync(e))

	dUse(e, config.DefaultNotebook)
	dAdd(t, e, "Mine", "m\n")

	// Fetch without merging: what a rolled-back pull leaves.
	dGit(t, e, dNotebookDir(e), "fetch", "origin", "+refs/heads/"+branch+":refs/remotes/origin/"+branch)

	dEqual(t, dSyncRow(t, e), "1 to push, 1 to pull (as of the last sync)")

	out := plain(Diff(e, "", true))
	dHas(t, out, "mine")
	dLacks(t, out, "theirs")
	dLacks(t, out, "=>")
}

func TestRestoreReturnsANoteToAnEarlierVersionAsANewCommit(t *testing.T) {
	e := dInitialized(t)
	added := dAdd(t, e, "Alpha", "first\n")
	path := filepath.Join(dNotebookDir(e), dNoteFile(added))
	original := dRead(t, path)
	dWrite(t, path, strings.Replace(original, "first\n", "second\n", 1))
	dCommitWorkingTree(t, e, config.DefaultNotebook, "edit: alpha")
	before := dCommitCount(t, e, dNotebookDir(e))

	dDone(Restore(e, "alpha", "HEAD~1", TouchStamp))
	// All but `updated`, which records the write just made (tested below).
	heldAside := func(text string) string {
		held, ok := note.SetField(text, "updated", "-")
		if !ok {
			t.Fatal("no frontmatter")
		}
		return held
	}
	dEqual(t, heldAside(dRead(t, path)), heldAside(original))
	if got := dCommitCount(t, e, dNotebookDir(e)); got != before+1 {
		t.Fatalf("a restore moves history forward, it does not rewrite it: %d, was %d", got, before)
	}
	dHas(t, plain(Log(e, "alpha", 0)), "restore: alpha")

	// Restoring what is already there is not a commit.
	out, err := Restore(e, "alpha", "HEAD", TouchStamp)
	if err != nil {
		t.Fatal(err)
	}
	dHas(t, out, "(no change)")
	dEqual(t, dCommitCount(t, e, dNotebookDir(e)), before+1)
}

// `updated` answers "when did this file last change", not "which version is this".
func TestRestoreDatesTheNoteNowRatherThanThen(t *testing.T) {
	e := dInitialized(t)
	added := dAdd(t, e, "Alpha", "first\n")
	path := filepath.Join(dNotebookDir(e), dNoteFile(added))

	dSetUpdated(t, path, "2000-01-01T00:00:00Z")
	longAgo := dRead(t, path)
	dCommitWorkingTree(t, e, config.DefaultNotebook, "edit: alpha")
	dWrite(t, path, strings.Replace(longAgo, "first\n", "second\n", 1))
	dCommitWorkingTree(t, e, config.DefaultNotebook, "edit: alpha again")

	dDone(Restore(e, "alpha", "HEAD~1", TouchStamp))
	after := dRead(t, path)
	dHas(t, after, "first\n")
	dLacks(t, after, "2000-01-01")

	// Two back now. Asking again is no change: only `updated` differs, and it is
	// not compared.
	out, err := Restore(e, "alpha", "HEAD~2", TouchStamp)
	if err != nil {
		t.Fatal(err)
	}
	dHas(t, out, "(no change)")
}

// With nothing overwritten, "no change" compares in full rather than holding
// `updated` aside.
func TestRestoreNoTouchBringsTheOldDateBackWithTheContents(t *testing.T) {
	e := dInitialized(t)
	added := dAdd(t, e, "Alpha", "first\n")
	path := dBackdate(t, e, added)
	longAgo := dRead(t, path)
	dCommitWorkingTree(t, e, config.DefaultNotebook, "edit: alpha")

	later, ok := note.SetField(strings.Replace(longAgo, "first\n", "second\n", 1), "updated", "2024-01-01T00:00:00Z")
	if !ok {
		t.Fatal("no frontmatter")
	}
	dWrite(t, path, later)
	dCommitWorkingTree(t, e, config.DefaultNotebook, "edit: alpha again")

	dDone(Restore(e, "alpha", "HEAD~1", TouchKeep))
	if got := dRead(t, path); got != longAgo {
		t.Fatalf("byte for byte the version that was asked for:\n%s\nwant:\n%s", got, longAgo)
	}

	// The same revision again compares in full and still says nothing changed.
	out, err := Restore(e, "alpha", "HEAD~2", TouchKeep)
	if err != nil {
		t.Fatal(err)
	}
	dHas(t, out, "(no change)")
}

func TestRestoreBringsBackADeletedNoteWithItsID(t *testing.T) {
	e := dInitialized(t)
	added := dAdd(t, e, "Alpha", "a\n", "work")
	id, _ := dParts(added)
	file := dNoteFile(added)
	dDone(Rm(e, "alpha"))
	if _, err := Show(e, "alpha"); err == nil {
		t.Fatal("gone")
	}

	// By an id no file carries any more: commits record the filenames.
	out, err := Restore(e, id, "HEAD~1", TouchStamp)
	if err != nil {
		t.Fatal(err)
	}
	if !strings.HasPrefix(out, id) {
		t.Fatalf("the id comes back unchanged: %q", out)
	}
	shown, err := Show(e, id)
	dHas(t, dMust(t, shown, err), "a\n")
	listed, err := Ls(e, List{Tag: "work"})
	dHas(t, dMust(t, listed, err), "Alpha")
	if info, err := os.Stat(filepath.Join(dNotebookDir(e), file)); err != nil || !info.Mode().IsRegular() {
		t.Fatalf("under the name it had: %v", err)
	}
}

// Deleted with `rm(1)`, so nothing recorded it; the filename comes back with the file.
func TestRestoreBringsBackANoteDeletedOutsideNoda(t *testing.T) {
	e := dInitialized(t)
	added := dAdd(t, e, "Alpha", "a\n")
	id, _ := dParts(added)

	if err := os.Remove(filepath.Join(dNotebookDir(e), dNoteFile(added))); err != nil {
		t.Fatal(err)
	}

	dDone(Restore(e, id, "HEAD", TouchStamp))
	shown, err := Show(e, id)
	if !strings.HasSuffix(dMust(t, shown, err), "a\n") {
		t.Fatalf("got %q", shown)
	}
	if row, found := dStatusRow(plain(Status(e)), "problems"); found {
		t.Fatalf("nothing is left to report: %q", row)
	}
}

// The commit before the deletion, which is what `restore` needs, not a `~1`
// left for the reader to work out.
func TestDeletedNamesTheCommitThatBringsANoteBack(t *testing.T) {
	e := dInitialized(t)
	added := dAdd(t, e, "Gamma", "g\n")
	id, slug := dParts(added)
	dDone(Rm(e, "gamma"))

	out := plain(Deleted(e, "", false))
	row := strings.Split(out, "\n")[0]
	if !strings.HasPrefix(row, id) || !strings.Contains(row, slug) || !strings.HasSuffix(row, "Gamma") {
		t.Fatalf("got:\n%s", out)
	}

	// The revision in the row is enough on its own.
	commit := strings.Fields(row)[4]
	dDone(Restore(e, slug, commit, TouchStamp))
	shown, err := Show(e, id)
	if !strings.HasSuffix(dMust(t, shown, err), "g\n") {
		t.Fatalf("got %q", shown)
	}

	// The check is against what is on disk, not against what history did.
	after, err := Deleted(e, "", false)
	if err != nil || after != "" {
		t.Fatalf("it is not deleted any more: %q, %v", after, err)
	}
}

// `mv` changes a filename, not an identity; the tree comparison compares ids.
func TestDeletedDoesNotCountARename(t *testing.T) {
	e := dInitialized(t)
	dAdd(t, e, "Beta", "b\n")
	dRename(t, e, "beta", "Beta Renamed")

	out, err := Deleted(e, "", false)
	if err != nil || out != "" {
		t.Fatalf("got %q, %v", out, err)
	}
}

// No commit message is read, so a plain-git deletion is found like `noda rm`'s.
func TestDeletedFindsWhatWasRemovedOutsideNoda(t *testing.T) {
	e := dInitialized(t)
	added := dAdd(t, e, "Alpha", "a\n")

	if err := os.Remove(filepath.Join(dNotebookDir(e), dNoteFile(added))); err != nil {
		t.Fatal(err)
	}
	dCommitWorkingTree(t, e, config.DefaultNotebook, "whatever i felt like typing")

	out := plain(Deleted(e, "", false))
	id, _ := dParts(added)
	dHas(t, out, id)
	dHas(t, out, "Alpha")
}

// The last disappearance counts, so the commit offered undoes that one.
func TestDeletedReportsTheMostRecentDisappearance(t *testing.T) {
	e := dInitialized(t)
	added := dAdd(t, e, "Alpha", "first\n")
	_, slug := dParts(added)

	dDone(Rm(e, slug))
	commit := dRevisionOf(t, plain(Deleted(e, "", false)))
	dDone(Restore(e, slug, commit, TouchStamp))

	// Changed, then lost again: the older commit would bring back the wrong contents.
	dDone(Tag(e, slug, []string{"+work"}, TouchStamp))
	dDone(Rm(e, slug))

	out := plain(Deleted(e, "", false))
	dEqual(t, strings.Count(strings.TrimSuffix(out, "\n"), "\n")+1, 2)
	latest := dRevisionOf(t, out)
	if latest == commit {
		t.Fatalf("not the first deletion: %s", latest)
	}

	dDone(Restore(e, slug, latest, TouchStamp))
	shown, err := Show(e, slug)
	dHas(t, dMust(t, shown, err), "tags: [work]")
}

// Full object ids, not the table's abbreviations, which can stop being unique later.
func TestDeletedJSONCarriesWhatAScriptNeedsToRestore(t *testing.T) {
	e := dInitialized(t)
	added := dAdd(t, e, "Gamma", "g\n")
	id, slug := dParts(added)
	dDone(Rm(e, slug))

	out, err := Deleted(e, "", true)
	if err != nil {
		t.Fatal(err)
	}
	if !strings.HasSuffix(out, "}\n") {
		t.Fatalf("one object, one line: %q", out)
	}
	dHas(t, out, `"notebook":"default"`)
	dHas(t, out, `"id":"`+id+`"`)
	dHas(t, out, `"slug":"`+slug+`"`)
	dHas(t, out, `"file":"`+id+"-"+slug+`.md"`)
	dHas(t, out, `"title":"Gamma"`)
	// RFC 3339 UTC, like a note's own times.
	dHas(t, out, `"removed_at":"20`)
	dHas(t, out, `Z"`)

	_, afterKey, _ := strings.Cut(out, `"restore_from":"`)
	restoreFrom, _, _ := strings.Cut(afterKey, `"`)
	if len(restoreFrom) != 40 {
		t.Fatalf("a full object id: %q", restoreFrom)
	}
	dDone(Restore(e, slug, restoreFrom, TouchStamp))
	shown, err := Show(e, id)
	if !strings.HasSuffix(dMust(t, shown, err), "g\n") {
		t.Fatalf("got %q", shown)
	}
}

// An empty list is an answer; the table prints nothing.
func TestDeletedJSONIsADocumentEvenWhenNothingIsGone(t *testing.T) {
	e := dInitialized(t)
	dAdd(t, e, "Alpha", "a\n")

	table, err := Deleted(e, "", false)
	if err != nil || table != "" {
		t.Fatalf("got %q, %v", table, err)
	}
	asJSON, err := Deleted(e, "", true)
	if err != nil || asJSON != "{\"notebook\":\"default\",\"deleted\":[]}\n" {
		t.Fatalf("got %q, %v", asJSON, err)
	}
}

func TestDeletedCanTargetAnotherNotebook(t *testing.T) {
	e := dInitialized(t)
	dAdd(t, e, "Alpha", "a\n")
	dDone(Rm(e, "alpha"))
	if _, err := notebook.Create(e.Git, e.Paths, "work"); err != nil {
		t.Fatal(err)
	}

	other, err := Deleted(e, "work", false)
	if err != nil || other != "" {
		t.Fatalf("the other notebook has lost nothing: %q, %v", other, err)
	}
	own, err := Deleted(e, "default", false)
	dHas(t, dMust(t, own, err), "alpha")
	if _, err := Deleted(e, "missing", false); err == nil {
		t.Fatal("a notebook that is not there is refused")
	}
}

func TestDeletedSaysNothingWhenNothingIsGone(t *testing.T) {
	e := dInitialized(t)
	dAdd(t, e, "Alpha", "a\n")

	out, err := Deleted(e, "", false)
	if err != nil || out != "" {
		t.Fatalf("got %q, %v", out, err)
	}
}

func TestRestoreReportsWhatItCannotFind(t *testing.T) {
	e := dInitialized(t)
	dAdd(t, e, "Alpha", "a\n")

	_, err := Restore(e, "alpha", "nonsense", TouchStamp)
	dFailsWith(t, err, "unknown revision")

	// The note exists, but not that far back.
	_, err = Restore(e, "alpha", "HEAD~1", TouchStamp)
	dFailsWith(t, err, "did not exist")

	if _, err := Restore(e, "missing", "HEAD", TouchStamp); err == nil {
		t.Fatal("a note that is not there is refused")
	}
}

func TestInitLeavesAStarterConfigThatChangesNothing(t *testing.T) {
	e := dBare(t)
	// The template rather than `Init`, because `dInitialized` writes its own
	// config; this is the call `Init` makes when there is none.
	wrote, err := config.WriteTemplate(e.Paths)
	if err != nil || !wrote {
		t.Fatalf("template written: %v, %v", wrote, err)
	}

	text := dRead(t, dConfigFile(e))
	for _, key := range []string{"# editor =", "# author =", "# notebook =", "# sign ="} {
		dHas(t, text, key)
	}
	for _, line := range strings.Split(text, "\n") {
		if strings.TrimSpace(line) != "" && !strings.HasPrefix(strings.TrimSpace(line), "#") {
			t.Fatalf("everything is commented out, so the defaults still apply:\n%s", text)
		}
	}

	// So nothing reports the file as its source. The values are the machine's:
	// `sign` follows git's `commit.gpgsign`.
	shown := plain(ConfigShow(e))
	dEqual(t, strings.Count(strings.TrimSuffix(shown, "\n"), "\n")+1, 4)
	dHas(t, shown, "notebook  default")
	dLacks(t, shown, "(config.toml)")

	// A second init does not overwrite what the user wrote.
	other := dInitialized(t)
	dWriteConfig(t, other, "editor = \"nvim\"\n")
	if _, err := Init(other); err != nil {
		t.Fatal(err)
	}
	dEqual(t, dRead(t, dConfigFile(other)), "editor = \"nvim\"\n")
}

func TestConfigSetAndGetRoundTripAndReportTheirSource(t *testing.T) {
	e := dInitialized(t)

	dDone(ConfigSet(e, "editor", "nvim"))
	got, err := ConfigGet(e, "editor")
	dEqual(t, dMust(t, got, err), "nvim")

	shown := plain(ConfigShow(e))
	dHas(t, shown, "editor    nvim")
	dHas(t, shown, "(config.toml)")

	// Falls back to the environment or the built-in, which varies by machine,
	// so only the source is checked.
	out, err := ConfigUnset(e, "editor")
	dHas(t, dMust(t, out, err), "now from")
	shown = plain(ConfigShow(e))
	row := ""
	for _, line := range strings.Split(shown, "\n") {
		if strings.HasPrefix(line, "editor") {
			row = line
			break
		}
	}
	dLacks(t, row, "(config.toml)")
	again, err := ConfigUnset(e, "editor")
	dHas(t, dMust(t, again, err), "was not set")
}

func TestTheFirstSettingWrittenLandsUnderTheHeaderNotAboveIt(t *testing.T) {
	// The starter config, since `dInitialized`'s has no header.
	e := dBare(t)
	if _, err := config.WriteTemplate(e.Paths); err != nil {
		t.Fatal(err)
	}
	dDone(ConfigSet(e, "editor", "helix"))

	text := dRead(t, dConfigFile(e))
	header := strings.Index(text, "# noda configuration")
	setting := strings.Index(text, `editor = "helix"`)
	if header < 0 || setting < 0 {
		t.Fatalf("header %d, setting %d:\n%s", header, setting, text)
	}
	if header >= setting {
		t.Fatalf("a config that reads back to front is worse than no comments:\n%s", text)
	}
}

func TestConfigSetKeepsTheCommentsAroundIt(t *testing.T) {
	e := dInitialized(t)
	dWriteConfig(t, e, "# my notes identity, not my work one\nauthor = \"Someone <s@example.com>\"\n\n# the editor I like\neditor = \"helix\"\n")

	dDone(ConfigSet(e, "editor", "nvim"))

	text := dRead(t, dConfigFile(e))
	dHas(t, text, "# my notes identity, not my work one")
	dHas(t, text, "# the editor I like")
	dHas(t, text, `editor = "nvim"`)
	dHas(t, text, `author = "Someone <s@example.com>"`)
}

func TestTheConfiguredAuthorIsWhoCommits(t *testing.T) {
	e := dInitialized(t)
	dDone(ConfigSet(e, "author", "Note Taker <notes@example.com>"))
	dAdd(t, e, "Alpha", "a\n")

	dEqual(t, dGit(t, e, dNotebookDir(e), "log", "-1", "--format=%an"), "Note Taker")
	dEqual(t, dGit(t, e, dNotebookDir(e), "log", "-1", "--format=%ae"), "notes@example.com")

	dHas(t, plain(ConfigShow(e)), "Note Taker <notes@example.com>")
}

func TestConfigRefusesWhatItCannotActOn(t *testing.T) {
	e := dInitialized(t)

	_, err := ConfigSet(e, "editr", "nvim")
	dFailsWith(t, err, "editor, author, notebook")
	if _, err := ConfigGet(e, "editr"); err == nil {
		t.Fatal("an unknown key is refused")
	}

	// A name without an email would end up in every commit.
	_, err = ConfigSet(e, "author", "just-a-name")
	dFailsWith(t, err, "Name <email>")

	// Invalid TOML is reported against its path.
	dWriteConfig(t, e, "editor = = nvim\n")
	_, err = ConfigShow(e)
	dFailsWith(t, err, "config.toml")
}

func TestTheConfiguredNotebookIsWhatInitCreatesAndWhatStandsIn(t *testing.T) {
	e := dBare(t)
	dWriteConfig(t, e, "notebook = \"work\"\nsign = false\n")

	if _, err := Init(e); err != nil {
		t.Fatal(err)
	}
	if info, err := os.Stat(filepath.Join(e.Paths.NotebookDir("work"), ".git")); err != nil || !info.IsDir() {
		t.Fatalf("the configured notebook is created: %v", err)
	}
	if _, err := os.Stat(dNotebookDir(e)); err == nil {
		t.Fatal("the default notebook is not")
	}
	current, err := NotebookCurrent(e)
	dEqual(t, dMust(t, current, err), "work")

	dAdd(t, e, "Alpha", "a\n")

	// State is disposable; config is not, so losing the pointer must not lose the notebook.
	if err := os.Remove(e.Paths.ActiveFile()); err != nil {
		t.Fatal(err)
	}
	listed, err := Ls(e, List{})
	dHas(t, dMust(t, listed, err), "Alpha")
}

func TestCommandsRefuseToRunBeforeInit(t *testing.T) {
	e := dBare(t)
	_, err := Ls(e, List{})
	dFailsWith(t, err, "noda init")
}

func TestBlameCreditsEachLineToTheCommitThatWroteIt(t *testing.T) {
	e := dInitialized(t)
	added := dAdd(t, e, "Alpha", "first\n")
	path := filepath.Join(dNotebookDir(e), dNoteFile(added))

	created := dHeadCommit(t, e)
	dEditNote(t, path, "first\n", "first\nsecond\n")
	dCommitWorkingTree(t, e, config.DefaultNotebook, "edit: alpha")
	edited := dHeadCommit(t, e)

	dExpectBlame(t, dBlamed(t, e, "alpha"), [][2]string{
		{created[:7], "first"},
		{edited[:7], "second"},
	}, "each line goes to the commit that wrote it")
}

// `noda mv` renames on retitle, and blame must still reach the lines written
// before the rename.
func TestBlameReachesPastARename(t *testing.T) {
	e := dInitialized(t)
	added := dAdd(t, e, "Alpha", "written before\n")
	created := dHeadCommit(t, e)

	dRename(t, e, "alpha", "Renamed")
	renamed := dHeadCommit(t, e)
	if created == renamed {
		t.Fatal("the rename is its own commit")
	}

	id, _ := dParts(added)
	path := filepath.Join(dNotebookDir(e), id+"-renamed.md")
	dEditNote(t, path, "written before\n", "written before\nafter\n")
	dCommitWorkingTree(t, e, config.DefaultNotebook, "edit: renamed")
	after := dHeadCommit(t, e)

	dExpectBlame(t, dBlamed(t, e, "renamed"), [][2]string{
		{created[:7], "written before"},
		{after[:7], "after"},
	}, "the line predates the rename and must not be credited to it")
}

// `updated` changes on every edit, so blaming frontmatter would credit it all
// to the latest commit.
func TestBlameReportsTheBodyAndNotTheFrontmatter(t *testing.T) {
	e := dInitialized(t)
	dAdd(t, e, "Alpha", "body\n")
	dDone(Tag(e, "alpha", []string{"+work"}, TouchStamp))

	out := plain(Blame(e, "alpha"))
	dLacks(t, out, "---")
	dLacks(t, out, "title:")
	dLacks(t, out, "updated:")
	dHas(t, out, "body")
}

// Lines edited outside noda belong to no commit, and are shown that way.
func TestBlameMarksTheLinesThatAreNotCommitted(t *testing.T) {
	e := dInitialized(t)
	added := dAdd(t, e, "Alpha", "committed\n")
	created := dHeadCommit(t, e)

	dEditNote(t, filepath.Join(dNotebookDir(e), dNoteFile(added)), "committed\n", "committed\nfresh\n")

	dExpectBlame(t, dBlamed(t, e, "alpha"), [][2]string{
		{created[:7], "committed"},
		{"0000000", "fresh"},
	}, "an uncommitted line has no commit")
}

// Crediting a line to a commit that never saw it would be worse than saying so.
func TestBlameSaysNothingIsCommittedWhenNoCommitHoldsTheNote(t *testing.T) {
	e := dInitialized(t)
	dPlant(t, dNotebookDir(e), "k3f9m2p1", "planted")

	dExpectBlame(t, dBlamed(t, e, "planted"), [][2]string{{"0000000", "body"}}, "no commit holds it")
}

func TestBacklinksNameTheNotesThatPointAtOne(t *testing.T) {
	e := dInitialized(t)
	_, target, sourceID, sourceSlug := dLinkedPair(t, e)

	out := dBacklinksTable(t, e, target)
	dHas(t, out, sourceID)
	dLacks(t, out, sourceSlug) // the slug would say the title twice
	dHas(t, out, "Q3 budget")

	// The linking note has nothing pointing at it.
	dHas(t, dBacklinksTable(t, e, sourceSlug), "nothing links to")
}

// `noda mv` leaves the link naming a path that is gone and an id that is not.
func TestBacklinksSurviveARetitle(t *testing.T) {
	e := dInitialized(t)
	_, target, sourceID, _ := dLinkedPair(t, e)
	dRename(t, e, target, "Weekly sync")

	// Broken for any Markdown reader, stale for noda: the id still names one note.
	audit := plain(Doctor(e, DoctorOptions{Links: true}))
	dHas(t, audit, "stale link")

	// The id in the destination still names the note.
	dHas(t, dBacklinksTable(t, e, "weekly-sync"), sourceID)
}

// An attachment's name is its whole identity, but the question is the same.
func TestBacklinksAnswerForAFileToo(t *testing.T) {
	e := dInitialized(t)
	dWrite(t, filepath.Join(dNotebookDir(e), "diagram.png"), "contents\n")
	added := dAdd(t, e, "Alpha", "![the shape](diagram.png)\n")
	dAdd(t, e, "Beta", "no links here\n")

	out := dBacklinksTable(t, e, "diagram.png")
	id, _ := dParts(added)
	dHas(t, out, id)
	dLacks(t, out, "beta")
}

// It is what the file says; leaving it out would be noda overruling the author.
func TestANoteThatLinksToItselfIsItsOwnBacklink(t *testing.T) {
	e := dInitialized(t)
	added := dAdd(t, e, "Alpha", "placeholder\n")
	id, slug := dParts(added)
	dEditNote(t, filepath.Join(dNotebookDir(e), dNoteFile(added)), "placeholder", "see [me]("+id+"-"+slug+".md)")
	dCommitWorkingTree(t, e, config.DefaultNotebook, "edit: alpha")

	dHas(t, dBacklinksTable(t, e, slug), id)
}

// A set of notes: the question is which notes, not how many times.
func TestANoteLinkingThreeTimesIsOneBacklink(t *testing.T) {
	e := dInitialized(t)
	id, slug := dParts(dAdd(t, e, "Meeting notes", "agenda\n"))
	link := id + "-" + slug + ".md"
	dAdd(t, e, "Q3 budget", "[a]("+link+") and [b]("+link+") and [c]("+link+"#x)\n")

	out := dBacklinksTable(t, e, slug)
	dEqual(t, strings.Count(strings.TrimSuffix(out, "\n"), "\n")+1, 1)
}

// The same rule `doctor --links` follows: Markdown is parsed, not searched.
func TestAMentionIsNotABacklink(t *testing.T) {
	e := dInitialized(t)
	id, slug := dParts(dAdd(t, e, "Meeting notes", "agenda\n"))
	dAdd(t, e, "Q3 budget", "the file is "+id+"-"+slug+".md, and [["+slug+"]] is not a link\n\n```\n[q]("+id+"-"+slug+".md)\n```\n")

	dHas(t, dBacklinksTable(t, e, slug), "nothing links to")
}

func TestBacklinksPrintJSONAndIDsOnRequest(t *testing.T) {
	e := dInitialized(t)
	targetID, targetSlug, sourceID, sourceSlug := dLinkedPair(t, e)

	asJSON, err := Backlinks(e, targetSlug, FormatJSON)
	dMust(t, asJSON, err)
	// It names what was resolved.
	dHas(t, asJSON, `"target":"`+targetID+"-"+targetSlug+`.md"`)
	dHas(t, asJSON, `"id":"`+sourceID+`"`)
	dHas(t, asJSON, `"file":"`+sourceID+"-"+sourceSlug+`.md"`)

	quiet, err := Backlinks(e, targetSlug, FormatQuiet)
	dEqual(t, dMust(t, quiet, err), sourceID+"\n")

	// A document either way, like every other listing.
	empty, err := Backlinks(e, sourceSlug, FormatJSON)
	dHas(t, dMust(t, empty, err), `"backlinks":[]`)
}

func TestBacklinksSayWhenTheKeyNamesNothing(t *testing.T) {
	e := dInitialized(t)
	_, err := Backlinks(e, "ghost", FormatTable)
	dFailsWith(t, err, "nothing called `ghost`")
}

var (
	dBuildOnce sync.Once
	dBinary    string
	dBuildErr  error
)

// dNodaBinary builds cmd/noda once: the argument parser lives in package main,
// and only the real binary meets it.
func dNodaBinary(t *testing.T) string {
	t.Helper()
	dBuildOnce.Do(func() {
		dir, err := os.MkdirTemp("", "noda-cli-d-")
		if err != nil {
			dBuildErr = err
			return
		}
		dBinary = filepath.Join(dir, "noda")
		build := exec.Command("go", "build", "-o", dBinary, "github.com/henry40408/noda/cmd/noda")
		if out, err := build.CombinedOutput(); err != nil {
			dBuildErr = fmt.Errorf("%w: %s", err, out)
		}
	})
	if dBuildErr != nil {
		t.Fatalf("building cmd/noda: %v", dBuildErr)
	}
	return dBinary
}

// A body of action items opens with `- `, which an option parser reads as a
// flag unless told otherwise. Runs the real binary, since command calls never
// meet the parser.
func TestAddTakesABodyThatOpensWithAList(t *testing.T) {
	binary := dNodaBinary(t)
	root := t.TempDir()
	env := gittest.Isolate(t)
	e := Env{Paths: paths.Rooted(root), Git: gittest.Require(t)}
	dWriteConfig(t, e, "sign = false\n")
	if _, err := Init(e); err != nil {
		t.Fatal(err)
	}

	command := exec.Command(binary, "add", "Alpha", "-c", "- [ ] send the contract\n")
	command.Env = nil
	for name, value := range env {
		command.Env = append(command.Env, name+"="+value)
	}
	command.Env = append(command.Env,
		"XDG_CONFIG_HOME="+filepath.Join(root, "config"),
		"XDG_DATA_HOME="+filepath.Join(root, "data"),
		"XDG_STATE_HOME="+filepath.Join(root, "state"),
		"XDG_CACHE_HOME="+filepath.Join(root, "cache"))
	if output, err := command.CombinedOutput(); err != nil {
		t.Fatalf("stderr was %s: %v", output, err)
	}
	dHas(t, plain(TodoOn(e, false, dToday)), "send the contract")
}

func TestTodoListsUntickedItemsSoonestFirst(t *testing.T) {
	e := dInitialized(t)
	dAdd(t, e, "Meeting notes", "- [ ] send the contract due:2026-08-10\n- [x] confirm legal\n- [ ] align with Alice\n")
	dAdd(t, e, "Q3 planning", "- [ ] check the terms due:2026-08-05\n")

	out := plain(TodoOn(e, false, dToday))
	rows := strings.Split(strings.TrimSuffix(out, "\n"), "\n")
	if len(rows) != 3 {
		t.Fatalf("the ticked one is not listed:\n%s", out)
	}
	dHas(t, rows[0], "2026-08-05")
	dHas(t, rows[0], "check the terms")
	dHas(t, rows[1], "2026-08-10")
	dHas(t, rows[2], "align with Alice") // an item with no due date sorts last
	// Each item names the note it is in.
	dHas(t, rows[0], "q3-planning")
	dHas(t, rows[1], "meeting-notes")
}

// Why the palette has an exception. Reads the raw output, before `plain`.
func TestTodoMarksADueDateThatHasPassed(t *testing.T) {
	e := dInitialized(t)
	dAdd(t, e, "Alpha", "- [ ] late one due:2026-07-01\n- [ ] later one due:2026-12-01\n")

	out := painted(TodoOn(e, false, dToday))
	lines := strings.Split(out, "\n")
	dHas(t, lines[0], "2026-07-01")
	dHas(t, lines[0], "\x1b[31m") // a date in the past is coloured
	dLacks(t, lines[1], "\x1b[31m")
}

// Today is not late: the comparison is `<`.
func TestTodoDoesNotCallTodayOverdue(t *testing.T) {
	e := dInitialized(t)
	dAdd(t, e, "Alpha", "- [ ] due today due:2026-08-02\n")

	dLacks(t, painted(TodoOn(e, false, dToday)), "\x1b[31m")
}

func TestTodoSaysWhenThereIsNothingToDo(t *testing.T) {
	e := dInitialized(t)
	dAdd(t, e, "Alpha", "- [x] all done\n")

	dHas(t, plain(TodoOn(e, false, dToday)), "nothing to do")
}

// Like `ls` and `deleted`: an empty list is still a JSON document.
func TestTodoJSONCarriesTheFieldsAndPrintsEvenWhenEmpty(t *testing.T) {
	e := dInitialized(t)
	empty, err := TodoOn(e, true, dToday)
	dEqual(t, strings.TrimRight(dMust(t, empty, err), "\n"), `{"notebook":"default","todo":[]}`)

	id, _ := dParts(dAdd(t, e, "Alpha", "- [ ] one due:2026-08-10\n- [ ] two\n"))

	out, err := TodoOn(e, true, dToday)
	dMust(t, out, err)
	dHas(t, out, `"id":"`+id+`"`)
	dHas(t, out, `"slug":"alpha"`)
	dHas(t, out, `"file":"`+id+`-alpha.md"`)
	dHas(t, out, `"text":"one","due":"2026-08-10"`)
	// An item with no date says so rather than dropping the key.
	dHas(t, out, `"text":"two","due":null`)
	// A program has its own clock.
	dLacks(t, out, "overdue")
}

// A cut-off item would have to be read in the note.
func TestTodoDoesNotTruncateAnItem(t *testing.T) {
	e := dInitialized(t)
	long := "chase the vendor about the revised statement of work and the indemnity clause"
	dAdd(t, e, "Alpha", "- [ ] "+long+"\n")

	out := plain(TodoOn(e, false, dToday))
	dHas(t, out, long)
	dLacks(t, out, "…")
}

// A `sync` merge carries a note across unchanged and must not be credited with it.
func TestBlameLooksPastAMergeThatOnlyCarriedTheNote(t *testing.T) {
	e := dInitialized(t)
	url := dBareRemote(t, e, dBranchOf(t, e, config.DefaultNotebook))
	dDone(RemoteSet(e, url))
	dAdd(t, e, "Alpha", "a\n")
	dDone(Sync(e))
	dMirror(e, url, "mirror")

	// Two machines, each writing its own note before either syncs.
	dAdd(t, e, "Laptop", "from the laptop\n")
	wroteIt := dHeadCommit(t, e)
	dDone(Sync(e))

	dUse(e, "mirror")
	dAdd(t, e, "Desktop", "from the desktop\n")
	dDone(Sync(e))
	dEqual(t, dMergeCommits(t, e, e.Paths.NotebookDir("mirror")), 1)

	dExpectBlame(t, dBlamed(t, e, "laptop"), [][2]string{{wroteIt[:7], "from the laptop"}},
		"the merge carried the note, it did not write it")
}

// Annotated, not lightweight: a snapshot records who closed a chapter and when.
func TestSnapshotMarksTheCurrentCommitWithAnAnnotatedTag(t *testing.T) {
	e := dInitialized(t)
	dir := dNotebookDir(e)
	dAdd(t, e, "Alpha", "a\n")

	out, err := Snapshot(e, "2026-q3", "end of quarter")
	dHas(t, dMust(t, out, err), "snapshot: 2026-q3 ->")

	dEqual(t, dGit(t, e, dir, "cat-file", "-t", "refs/tags/2026-q3"), "tag") // annotated, not lightweight
	dEqual(t, dGit(t, e, dir, "tag", "-l", "--format=%(contents:subject)", "2026-q3"), "end of quarter")
	dEqual(t, dGit(t, e, dir, "rev-parse", "refs/tags/2026-q3^{commit}"), dGit(t, e, dir, "rev-parse", "HEAD"))

	listed := plain(SnapshotLs(e))
	dHas(t, listed, "2026-q3")
	dHas(t, listed, "end of quarter")
}

// `restore` already accepted a tag; this is what makes one.
func TestANoteRestoresFromASnapshotByName(t *testing.T) {
	e := dInitialized(t)
	added := dAdd(t, e, "Alpha", "first\n")
	dDone(Snapshot(e, "before", ""))

	path := filepath.Join(dNotebookDir(e), dNoteFile(added))
	dEditNote(t, path, "first\n", "second\n")
	dCommitWorkingTree(t, e, config.DefaultNotebook, "edit: alpha")

	dDone(Restore(e, "alpha", "before", TouchStamp))
	dHas(t, dRead(t, path), "first")
}

// Like `sync`: a snapshot that left out what is on disk would be of something
// nobody has.
func TestSnapshotCommitsWhatIsOnDiskFirst(t *testing.T) {
	e := dInitialized(t)
	dir := dNotebookDir(e)
	dAdd(t, e, "Alpha", "a\n")
	dWrite(t, filepath.Join(dir, "receipt.txt"), "uncommitted\n")
	before := dCommitCount(t, e, dir)

	out, err := Snapshot(e, "now", "")
	dHas(t, dMust(t, out, err), "commit: local changes")
	dEqual(t, dCommitCount(t, e, dir), before+1)

	// The snapshot marks a commit that holds everything.
	dEqual(t, dGit(t, e, dir, "status", "--porcelain"), "")

	// A clean notebook gains no empty commit.
	out, err = Snapshot(e, "again", "")
	dLacks(t, dMust(t, out, err), "commit:")
	dEqual(t, dCommitCount(t, e, dir), before+1)
}

// A name that can be reassigned cannot be cited.
func TestSnapshotRefusesToMoveOneThatAlreadyExists(t *testing.T) {
	e := dInitialized(t)
	dAdd(t, e, "Alpha", "a\n")
	dDone(Snapshot(e, "q3", ""))
	dAdd(t, e, "Beta", "b\n")

	_, err := Snapshot(e, "q3", "")
	dFailsWith(t, err, "already exists")
	dFailsWith(t, err, "git tag -d q3") // the way out is named
}

func TestSnapshotRefusesANameGitCannotHold(t *testing.T) {
	e := dInitialized(t)
	dAdd(t, e, "Alpha", "a\n")

	_, err := Snapshot(e, "not a name", "")
	dFailsWith(t, err, "invalid snapshot name")
}

func TestATagOnABlobDoesNotBreakTheSnapshotListing(t *testing.T) {
	e := dInitialized(t)
	dir := dNotebookDir(e)
	dAdd(t, e, "Alpha", "a\n")
	dDone(Snapshot(e, "kept", ""))

	result, err := e.Git.Run(context.Background(), gitx.Cmd{
		Dir: dir, Args: []string{"hash-object", "-w", "--stdin"}, Stdin: []byte("not a commit"),
	})
	if err != nil {
		t.Fatal(err)
	}
	dGit(t, e, dir, "update-ref", "refs/tags/on-a-blob", strings.TrimSpace(string(result.Stdout)))

	listed := plain(SnapshotLs(e))
	dHas(t, listed, "kept")
	dLacks(t, listed, "on-a-blob")
}

func TestSnapshotSaysWhenThereAreNone(t *testing.T) {
	e := dInitialized(t)
	out := plain(SnapshotLs(e))
	dHas(t, out, "no snapshots")
	dHas(t, out, "noda snapshot <name>")
}

// A snapshot that stays on one machine cannot be cited from another.
func TestSnapshotsTravelWithTheNotebook(t *testing.T) {
	e := dInitialized(t)
	url := dBareRemote(t, e, dBranchOf(t, e, config.DefaultNotebook))
	dDone(RemoteSet(e, url))
	dAdd(t, e, "Alpha", "a\n")
	dDone(Snapshot(e, "q3", "end of quarter"))
	dDone(Sync(e))

	// The snapshot reached the remote.
	dGit(t, e, url, "rev-parse", "--verify", "refs/tags/q3")

	dMirror(e, url, "mirror")
	dUse(e, "mirror")
	listed := plain(SnapshotLs(e))
	dHas(t, listed, "q3")
	dHas(t, listed, "end of quarter")
}

// Two machines' `q3` must not silently overwrite each other, and the clash must
// not block the notes, which sending the tag anyway would.
func TestASnapshotNameTakenOnTheRemoteIsNotOverwritten(t *testing.T) {
	e := dInitialized(t)
	branch := dBranchOf(t, e, config.DefaultNotebook)
	url := dBareRemote(t, e, branch)
	dDone(RemoteSet(e, url))
	dAdd(t, e, "Alpha", "a\n")
	dDone(Snapshot(e, "q3", "theirs"))
	dDone(Sync(e))

	// A second notebook off the same remote, with a different `q3`.
	dMirror(e, url, "mirror")
	dUse(e, "mirror")
	dGit(t, e, e.Paths.NotebookDir("mirror"), "tag", "-d", "q3")
	dAdd(t, e, "Beta", "b\n")
	dDone(Snapshot(e, "q3", "ours"))

	pushed, err := Push(e)
	out := dMust(t, pushed, err)
	dHas(t, out, "snapshot `q3` was not sent")
	dHas(t, out, "git tag -d q3") // the way out is named

	// The remote's snapshot still means what it meant.
	dEqual(t, dGit(t, e, url, "tag", "-l", "--format=%(contents:subject)", "q3"), "theirs")
	// The notes still went: a disputed name must not hold up the prose.
	dHas(t, dGit(t, e, url, "ls-tree", "--name-only", "refs/heads/"+branch), "beta")
}
