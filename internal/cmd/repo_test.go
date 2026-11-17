package cmd

import (
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/henry40408/noda/internal/note"
	"github.com/henry40408/noda/internal/style"
)

// plain strips the escapes from a command's output. A command that failed
// panics: the test cannot go on, and a panic names the line.
func plain(text string, err error) string {
	if err != nil {
		panic(err)
	}
	return style.Strip(text)
}

func TestStatusNamesTheNotebookAndSaysWhatIsLeftOut(t *testing.T) {
	e, _ := aNotebook(t)
	addNote(t, e, "Plan", "x")

	out := plain(Status(e))
	for _, want := range []string{"notebook  main  (", "notes     1", "changes   clean", "remote    none — set one with `noda remote set <url>`"} {
		if !strings.Contains(out, want) {
			t.Fatalf("missing %q in:\n%s", want, out)
		}
	}
	if strings.Contains(out, "files") {
		t.Fatalf("a zero row is left out:\n%s", out)
	}
}

func TestDoctorAdoptsAnUnnamedNoteInOneCommit(t *testing.T) {
	e, opened := aNotebook(t)
	text := "---\ntitle: Loose\n---\n\nbody\n"
	if err := os.WriteFile(filepath.Join(opened.Path, "loose.md"), []byte(text), 0o666); err != nil {
		t.Fatal(err)
	}

	out := plain(Doctor(e, DoctorOptions{DryRun: true}))
	if !strings.Contains(out, "1 note has no id in its filename") || !strings.Contains(out, "would adopt 1 note — nothing was changed") {
		t.Fatalf("dry run:\n%s", out)
	}
	if _, err := os.Stat(filepath.Join(opened.Path, "loose.md")); err != nil {
		t.Fatal("a dry run moved the file")
	}

	out = plain(Doctor(e, DoctorOptions{}))
	if !strings.HasSuffix(out, "adopted 1 note") {
		t.Fatalf("adopt:\n%s", out)
	}
	if out := plain(Doctor(e, DoctorOptions{})); out != "the notebook is in order" {
		t.Fatalf("after the repair: %q", out)
	}
}

func TestLogBlameAndBacklinks(t *testing.T) {
	e, _ := aNotebook(t)
	target := addNote(t, e, "Target", "first line")
	linker := addNote(t, e, "Linker", "see ["+"it]("+note.FileName(target, "target")+")")

	log := plain(Log(e, target, 0))
	if !strings.Contains(log, "add: target") {
		t.Fatalf("log:\n%s", log)
	}
	blame := plain(Blame(e, target))
	if !strings.Contains(blame, "first line") {
		t.Fatalf("blame:\n%s", blame)
	}

	links := plain(Backlinks(e, target, FormatTable))
	if !strings.HasPrefix(links, linker+"  Linker") {
		t.Fatalf("backlinks:\n%s", links)
	}
	quiet, err := Backlinks(e, linker, FormatQuiet)
	if err != nil || quiet != "" {
		t.Fatalf("nothing links to the linker: %q, %v", quiet, err)
	}
	if out := plain(Backlinks(e, linker, FormatTable)); !strings.HasPrefix(out, "nothing links to ") {
		t.Fatalf("got %q", out)
	}
}

func TestTodoSortsByDueAndMarksOverdue(t *testing.T) {
	e, _ := aNotebook(t)
	addNote(t, e, "Chores", "- [ ] later due:2030-01-01\n- [ ] sooner due:2020-01-01\n- [ ] whenever\n- [x] done\n")

	out := plain(TodoOn(e, false, "2026-05-05"))
	lines := strings.Split(strings.TrimSpace(out), "\n")
	if len(lines) != 3 || !strings.HasSuffix(lines[0], "2020-01-01  sooner") || !strings.HasSuffix(lines[2], "whenever") {
		t.Fatalf("got:\n%s", out)
	}
	if !strings.Contains(painted(TodoOn(e, false, "2026-05-05")), style.Overdue.Paint("2020-01-01")) {
		t.Fatal("the past due date is not painted overdue")
	}
	asJSON := plain(TodoOn(e, true, "2026-05-05"))
	if !strings.Contains(asJSON, `"due":"2020-01-01"`) || !strings.Contains(asJSON, `"due":null`) {
		t.Fatalf("json: %s", asJSON)
	}
}

func painted(text string, err error) string {
	if err != nil {
		panic(err)
	}
	return text
}

func TestDeletedAndRestoreUndoARemoval(t *testing.T) {
	e, _ := aNotebook(t)
	id := addNote(t, e, "Doomed", "keep me")
	if _, err := Rm(e, id); err != nil {
		t.Fatal(err)
	}

	listed := plain(Deleted(e, "", false))
	if !strings.Contains(listed, id+"  doomed") || !strings.Contains(listed, "brings one back") {
		t.Fatalf("deleted:\n%s", listed)
	}
	fields := strings.Fields(listed)
	revision := fields[4]

	out := plain(Restore(e, id, revision, TouchStamp))
	if !strings.HasPrefix(out, id+"  doomed") {
		t.Fatalf("restore: %q", out)
	}
	text, err := os.ReadFile(notePath(t, e, id))
	if err != nil || !strings.Contains(string(text), "keep me") {
		t.Fatalf("restored text: %q, %v", text, err)
	}
	again := plain(Restore(e, id, revision, TouchStamp))
	if !strings.HasSuffix(again, "(no change)") {
		t.Fatalf("a second restore changes nothing: %q", again)
	}
}

func TestDiffShowsAnUncommittedEdit(t *testing.T) {
	e, _ := aNotebook(t)
	id := addNote(t, e, "Draft", "one")
	path := notePath(t, e, id)
	text, _ := os.ReadFile(path)
	if err := os.WriteFile(path, []byte(strings.Replace(string(text), "one", "two", 1)), 0o666); err != nil {
		t.Fatal(err)
	}
	out := plain(Diff(e, id, false))
	if !strings.Contains(out, "-one") || !strings.Contains(out, "+two") {
		t.Fatalf("diff:\n%s", out)
	}
}

func TestFilesAreAddedRenamedAndRemoved(t *testing.T) {
	e, opened := aNotebook(t)
	source := filepath.Join(t.TempDir(), "report.pdf")
	if err := os.WriteFile(source, []byte("pdf"), 0o666); err != nil {
		t.Fatal(err)
	}
	if out, err := FileAdd(e, []string{source}, ""); err != nil || out != "added  report.pdf\n" {
		t.Fatalf("add: %q, %v", out, err)
	}
	if _, err := FileAdd(e, []string{source}, ""); err == nil || !strings.Contains(err.Error(), "already holds") {
		t.Fatalf("a duplicate is refused: %v", err)
	}
	if _, err := FileAdd(e, []string{source, source}, "x"); err == nil {
		t.Fatal("--as with several is refused")
	}

	id := addNote(t, e, "Cites", "read [it](report.pdf)")
	out, err := FileMv(e, "report.pdf", "q3.pdf", false)
	if err != nil || !strings.Contains(out, "1 note links to report.pdf") || !strings.Contains(out, "cites") {
		t.Fatalf("mv reports the stranded link: %q, %v", out, err)
	}
	if _, err = FileMv(e, "q3.pdf", "final.pdf", true); err != nil {
		t.Fatal(err)
	}
	if _, err := FileMv(e, "final.pdf", "report.pdf", true); err != nil {
		t.Fatal(err)
	}
	body, _ := os.ReadFile(notePath(t, e, id))
	if !strings.Contains(string(body), "(report.pdf)") {
		t.Fatalf("link:\n%s", body)
	}
	if _, err := FileRm(e, note.FileName(id, "cites")); err == nil || !strings.Contains(err.Error(), "is a note") {
		t.Fatalf("a note is not a file: %v", err)
	}
	if out, err := FileRm(e, "report.pdf"); err != nil || out != "removed  report.pdf" {
		t.Fatalf("rm: %q, %v", out, err)
	}
	if _, err := os.Stat(filepath.Join(opened.Path, "report.pdf")); !os.IsNotExist(err) {
		t.Fatal("the file is still there")
	}
}

func TestNotebooksAreAddedListedRenamedAndRemoved(t *testing.T) {
	e, _ := aNotebook(t)
	if _, err := NotebookAdd(e, "scratch", ""); err != nil {
		t.Fatal(err)
	}
	listed := plain(NotebookLs(e))
	if !strings.Contains(listed, "* main") || !strings.Contains(listed, "  scratch") || !strings.Contains(listed, "no remote") {
		t.Fatalf("ls:\n%s", listed)
	}

	if _, err := NotebookRm(e, "main", true); err == nil || !strings.Contains(err.Error(), "is the active notebook") {
		t.Fatalf("the active notebook stays: %v", err)
	}
	if _, err := NotebookRm(e, "scratch", false); err == nil || !strings.Contains(err.Error(), "no terminal") {
		t.Fatalf("no terminal, no confirmation: %v", err)
	}
	e.Confirm = func(string) (bool, error) { return false, nil }
	if out, _ := NotebookRm(e, "scratch", false); out != "kept notebook `scratch`" {
		t.Fatalf("declined: %q", out)
	}

	if _, err := NotebookRename(e, "main", "home"); err != nil {
		t.Fatal(err)
	}
	if current, _ := NotebookCurrent(e); current != "home" {
		t.Fatalf("the active pointer follows the rename: %q", current)
	}
	if _, err := NotebookRm(e, "scratch", true); err != nil {
		t.Fatal(err)
	}
}

func TestRemoteSetRedactsAndShowRefusesWhenUnset(t *testing.T) {
	e, _ := aNotebook(t)
	if _, err := RemoteShow(e); err == nil || !strings.Contains(err.Error(), "has no remote") {
		t.Fatalf("got %v", err)
	}
	if _, err := RemoteSet(e, "  "); err == nil {
		t.Fatal("a blank URL is refused")
	}
	out, err := RemoteSet(e, "https://user:secret@example.com/repo.git")
	if err != nil || strings.Contains(out, "secret") {
		t.Fatalf("the password leaked: %q, %v", out, err)
	}
}

func TestSnapshotThenCloneFromTheNotebook(t *testing.T) {
	e, opened := aNotebook(t)
	addNote(t, e, "Kept", "x")
	if out, err := Snapshot(e, "v1", ""); err != nil || !strings.HasPrefix(out, "snapshot: v1 -> ") {
		t.Fatalf("snapshot: %q, %v", out, err)
	}
	if listed := plain(SnapshotLs(e)); !strings.HasPrefix(listed, "v1  ") {
		t.Fatalf("ls: %q", listed)
	}

	out, err := Clone(e, "file://"+opened.Path, "copy")
	if err != nil || !strings.HasPrefix(out, "cloned `copy` (1 notes) to ") {
		t.Fatalf("clone: %q, %v", out, err)
	}
	if _, err := Clone(e, "", ""); err == nil {
		t.Fatal("no name to take from an empty URL")
	}
}
