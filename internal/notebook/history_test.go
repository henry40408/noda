package notebook

import (
	"os"
	"os/exec"
	"path/filepath"
	"regexp"
	"strconv"
	"strings"
	"testing"

	"github.com/henry40408/noda/internal/clock"
	"github.com/henry40408/noda/internal/gitx"
	"github.com/henry40408/noda/internal/gitx/gittest"
	"github.com/henry40408/noda/internal/paths"
)

// fixture builds one of testdata/repos' scenarios and opens its notebook.
func fixture(t *testing.T, scenario string) *Notebook {
	t.Helper()
	root := t.TempDir()
	script, err := filepath.Abs("../../testdata/repos/build.sh")
	if err != nil {
		t.Fatal(err)
	}
	if out, err := exec.Command("sh", script, root, scenario).CombinedOutput(); err != nil {
		t.Fatalf("build.sh %s: %v\n%s", scenario, err, out)
	}
	env := gittest.Isolate(t)
	env["TZ"] = "UTC"
	git, err := gitx.New(gitx.Options{Env: env})
	if err != nil {
		t.Fatal(err)
	}
	nb, err := Open(git, paths.Rooted(root), "default")
	if err != nil {
		t.Fatal(err)
	}
	return nb
}

// golden is the output of one command in testdata/golden/repos/<scenario>/<file>.
func golden(t *testing.T, scenario, file, command string) []string {
	t.Helper()
	raw, err := os.ReadFile(filepath.Join("../../testdata/golden/repos", scenario, file+".txt"))
	if err != nil {
		t.Fatal(err)
	}
	var out []string
	in := false
	for _, line := range strings.Split(string(raw), "\n") {
		switch {
		case line == "$ "+command:
			in = true
		case strings.HasPrefix(line, "$ "), line == "--- stderr":
			in = false
		case in:
			out = append(out, strings.TrimRight(line, " "))
		}
	}
	for len(out) > 0 && out[len(out)-1] == "" {
		out = out[:len(out)-1]
	}
	return out
}

// numbered replaces each distinct short id with <OID:n> in order of appearance,
// as the goldens do.
type numbered map[string]int

func (n numbered) label(oid string) string {
	short := Short(oid)
	if short == "0000000" {
		return short
	}
	if _, seen := n[short]; !seen {
		n[short] = len(n) + 1
	}
	return "<OID:" + itoa(n[short]) + ">"
}

func itoa(n int) string { return strconv.Itoa(n) }

func renderLog(entries []Entry) []string {
	ids := numbered{}
	var out []string
	for _, entry := range entries {
		out = append(out, "  "+ids.label(entry.ID)+"  "+clock.FormatTime(entry.Seconds, entry.OffsetMinutes)+"  "+entry.Summary)
	}
	return out
}

func renderBlame(lines []BlameLine) []string {
	ids := map[string]int{}
	var out []string
	for _, line := range lines {
		label := Short(line.Commit)
		if line.Commit != "" {
			if _, seen := ids[label]; !seen {
				ids[label] = len(ids) + 1
			}
			label = "<OID:" + itoa(ids[label]) + ">"
		}
		when := clock.FormatTime(line.Seconds, line.OffsetMinutes)
		if line.Commit == "" {
			when = "not committed   "
		}
		out = append(out, strings.TrimRight(label+"  "+when+"  "+line.Text, " "))
	}
	return out
}

var indexLine = regexp.MustCompile(`(?m)^index ([0-9a-f]{7})\.\.([0-9a-f]{7})`)

// renderDiff numbers the blob ids as the goldens do; the all-zero id of a file
// that did not exist is not an object and stays as it is.
func renderDiff(text string) []string {
	ids := map[string]int{}
	label := func(id string) string {
		if id == "0000000" {
			return id
		}
		if _, seen := ids[id]; !seen {
			ids[id] = len(ids) + 1
		}
		return "<OID:" + itoa(ids[id]) + ">"
	}
	text = indexLine.ReplaceAllStringFunc(text, func(match string) string {
		parts := indexLine.FindStringSubmatch(match)
		return "index " + label(parts[1]) + ".." + label(parts[2])
	})
	return strings.Split(strings.TrimRight(text, "\n"), "\n")
}

// trimmed ignores trailing blanks: the goldens keep none, while a diff's blank
// context line is one space.
func trimmed(lines []string) string {
	out := make([]string, len(lines))
	for i, line := range lines {
		out[i] = strings.TrimRight(line, " ")
	}
	return strings.Join(out, "\n")
}

func same(t *testing.T, what string, got, want []string) {
	t.Helper()
	if trimmed(got) != trimmed(want) {
		t.Errorf("%s\n--- got\n%s\n--- want\n%s", what, strings.Join(got, "\n"), strings.Join(want, "\n"))
	}
}

func TestLinearLogMatchesTheRustGolden(t *testing.T) {
	nb := fixture(t, "linear")
	all, err := nb.Log("", 0)
	if err != nil {
		t.Fatal(err)
	}
	same(t, "log", renderLog(all), golden(t, "linear", "log", "noda log"))

	one, err := nb.Log("01abc234", 0)
	if err != nil {
		t.Fatal(err)
	}
	same(t, "log by id", renderLog(one), golden(t, "linear", "log-note-id", "noda log 01abc234"))

	limited, err := nb.Log("", 2)
	if err != nil || len(limited) != 2 {
		t.Errorf("limit: %d %v", len(limited), err)
	}
	if _, err := nb.Log("a*", 0); err == nil {
		t.Error("a glob character reached the pathspec")
	}
}

func TestLinearDeletedAndLastChanged(t *testing.T) {
	nb := fixture(t, "linear")
	gone, err := nb.Deleted()
	if err != nil || len(gone) != 1 {
		t.Fatal(gone, err)
	}
	got := gone[0]
	line := got.ID + "  " + got.Slug + "  " + clock.FormatTime(got.RemovedAt, got.OffsetMinutes) + "  <OID:1>  " + got.Title
	same(t, "deleted", []string{line}, golden(t, "linear", "deleted", "noda deleted")[:1])
	if got.RemovedIn == got.RestoreFrom || got.RestoreFromShort() != Short(got.RestoreFrom) {
		t.Errorf("%+v", got)
	}

	last, err := nb.LastChanged()
	if err != nil {
		t.Fatal(err)
	}
	if last["01abc234"] != 1700032400 || last["02def567"] != 1700025200 {
		t.Errorf("%v", last)
	}
	if last["03ghj890"] != 1700028800 {
		t.Error("a deleted note counts at the commit that last changed it")
	}
}

func TestLinearBlameMatchesTheRustGolden(t *testing.T) {
	nb := fixture(t, "linear")
	lines, err := nb.Blame("02def567", "meeting-notes")
	if err != nil {
		t.Fatal(err)
	}
	same(t, "blame", renderBlame(lines), golden(t, "linear", "blame", "noda blame meeting-notes"))
}

func TestLinearDiffOfACleanNotebookIsTheLastCommit(t *testing.T) {
	nb := fixture(t, "linear")
	text, err := nb.Diff("")
	if err != nil {
		t.Fatal(err)
	}
	same(t, "diff", renderDiff(text), golden(t, "linear", "diff", "noda diff"))
}

func TestDirtyDiffLeavesUntrackedFilesOut(t *testing.T) {
	nb := fixture(t, "dirty")
	all, err := nb.Diff("")
	if err != nil {
		t.Fatal(err)
	}
	same(t, "diff", renderDiff(all), golden(t, "dirty", "diff", "noda diff"))
	one, err := nb.Diff("01abc234-budget-review.md")
	if err != nil {
		t.Fatal(err)
	}
	same(t, "diff note", renderDiff(one), golden(t, "dirty", "diff-note", "noda diff budget-review"))
	none, err := nb.Diff("04kmn123-fresh-idea.md")
	if err != nil || none != "" {
		t.Errorf("%q %v", none, err)
	}
}

func TestMergeLogBlameAndDiff(t *testing.T) {
	nb := fixture(t, "merge")
	all, err := nb.Log("", 0)
	if err != nil {
		t.Fatal(err)
	}
	same(t, "log", renderLog(all), golden(t, "merge", "log", "noda log"))
	two, err := nb.Log("", 3)
	if err != nil {
		t.Fatal(err)
	}
	same(t, "log -n 3", renderLog(two), golden(t, "merge", "log-2", "noda log -n 3"))

	lines, err := nb.Blame("01abc234", "budget-review")
	if err != nil {
		t.Fatal(err)
	}
	same(t, "blame", renderBlame(lines), golden(t, "merge", "blame", "noda blame budget-review"))
	text, err := nb.Diff("")
	if err != nil {
		t.Fatal(err)
	}
	same(t, "diff", renderDiff(text), golden(t, "merge", "diff", "noda diff"))
}

func TestRenameRewriteFollowsTheNoteById(t *testing.T) {
	nb := fixture(t, "rename-rewrite")
	entries, err := nb.Log("01abc234", 0)
	if err != nil {
		t.Fatal(err)
	}
	same(t, "log by id", renderLog(entries), golden(t, "rename-rewrite", "log-id", "noda log 01abc234"))
	text, err := nb.Diff("")
	if err != nil {
		t.Fatal(err)
	}
	same(t, "diff", renderDiff(text), golden(t, "rename-rewrite", "diff", "noda diff"))
}

// Rust follows a rename by id and so credits `golf` and `lima` to the note's
// first commit. git follows by content, and a rewrite this large is not one
// file to it. The user accepted the difference (CLAUDE.md, "Accepted differences
// from the Rust version"), so the golden is kept as the Rust record and this test
// pins what git does instead.
func TestRenameRewriteBlameDiffersFromRustAsKnown(t *testing.T) {
	nb := fixture(t, "rename-rewrite")
	lines, err := nb.Blame("01abc234", "q3-budget-review")
	if err != nil {
		t.Fatal(err)
	}
	got := renderBlame(lines)
	want := golden(t, "rename-rewrite", "blame", "noda blame 01abc234")
	if len(got) != len(want) {
		t.Fatalf("%d lines, want %d", len(got), len(want))
	}
	// git credits `golf` and `lima` to the commit that rewrote the file.
	if lines[6].Commit != lines[0].Commit || lines[11].Commit != lines[0].Commit {
		t.Errorf("the known difference moved:\n%s", strings.Join(got, "\n"))
	}
}

func TestSnapshotsReadFromTheTagsFixture(t *testing.T) {
	nb := fixture(t, "tags")
	snapshots, err := nb.Snapshots()
	if err != nil {
		t.Fatal(err)
	}
	ids := numbered{}
	var got []string
	for _, snapshot := range snapshots {
		got = append(got, strings.TrimRight(pad(snapshot.Name, 10)+"  "+clock.FormatTime(snapshot.Seconds, snapshot.OffsetMinutes)+"  "+ids.label(snapshot.Target)+"  "+snapshot.Message, " "))
	}
	same(t, "snapshots", got, golden(t, "tags", "snapshot-list", "noda snapshot"))
}

func pad(text string, width int) string {
	return text + strings.Repeat(" ", width-len(text))
}

func TestSnapshotMakesAnAnnotatedTagThatIsNeverMoved(t *testing.T) {
	nb := fixture(t, "linear")
	head, err := nb.Snapshot("2024-q1", "2024-q1")
	if err != nil {
		t.Fatal(err)
	}
	if kind := gitOut(t, nb, "cat-file", "-t", "refs/tags/2024-q1"); kind != "tag" {
		t.Errorf("a %s tag", kind)
	}
	snapshots, err := nb.Snapshots()
	if err != nil || len(snapshots) != 1 {
		t.Fatal(snapshots, err)
	}
	if snapshots[0].Target != head || snapshots[0].Message != "2024-q1" || snapshots[0].ShortTarget() != Short(head) {
		t.Errorf("%+v", snapshots[0])
	}
	if _, err := nb.Snapshot("2024-q1", "again"); err == nil || !strings.Contains(err.Error(), "already exists") {
		t.Errorf("%v", err)
	}
	for _, bad := range []string{"has space", "a..b", "x~1", "", "a\nb"} {
		if _, err := nb.Snapshot(bad, "m"); err == nil || !strings.Contains(err.Error(), "invalid snapshot name") {
			t.Errorf("%q: %v", bad, err)
		}
	}
	if _, err := nb.Snapshot("-dash", "m"); err != nil {
		t.Errorf("a name starting with a dash: %v", err)
	}
}

func TestDiffRemoteNeedsASyncedBranchAndShowsWhatAPushCarries(t *testing.T) {
	if _, err := fixture(t, "linear").DiffRemote("main", ""); err == nil || !strings.Contains(err.Error(), "never synced") {
		t.Errorf("%v", err)
	}
	nb := fixture(t, "ahead")
	text, err := nb.DiffRemote("main", "")
	if err != nil {
		t.Fatal(err)
	}
	want := golden(t, "ahead", "diff-remote", "noda diff --remote")
	same(t, "diff --remote", renderDiff(text), want)
}

func TestRevisionNoteAtAndIDAt(t *testing.T) {
	nb := fixture(t, "linear")
	head, err := nb.Revision("HEAD")
	if err != nil || len(head) != 40 {
		t.Fatal(head, err)
	}
	if _, err := nb.Revision("nope"); err == nil || !strings.Contains(err.Error(), "unknown revision: nope") {
		t.Errorf("%v", err)
	}
	if _, err := nb.Revision("HEAD^{tree}"); err == nil || !strings.Contains(err.Error(), "is not a commit") {
		t.Errorf("%v", err)
	}
	if _, err := nb.Revision("HEAD:01abc234-q3-budget-review.md"); err == nil || !strings.Contains(err.Error(), "is not a commit") {
		t.Errorf("%v", err)
	}

	slug, text, held, err := nb.NoteAt(head, "01abc234")
	if err != nil || !held || slug != "q3-budget-review" || !strings.HasPrefix(text, "---\n") {
		t.Errorf("%q %v %v", slug, held, err)
	}
	if _, _, held, _ := nb.NoteAt(head, "03ghj890"); held {
		t.Error("the deleted note is not at HEAD")
	}
	if id, ok, err := nb.IDAt(head, "meeting"); err != nil || ok {
		t.Errorf("a slug is matched whole: %q %v %v", id, ok, err)
	}
	if id, ok, err := nb.IDAt(head, "02DEF"); err != nil || !ok || id != "02def567" {
		t.Errorf("%q %v %v", id, ok, err)
	}
	if id, ok, _ := nb.IDAt(head, "meeting-notes"); !ok || id != "02def567" {
		t.Errorf("%q %v", id, ok)
	}

	oid := gitOut(t, nb, "rev-parse", "HEAD:02def567-meeting-notes.md")
	if body, ok, err := nb.BlobText(oid); err != nil || !ok || !strings.Contains(body, "Meeting notes") {
		t.Errorf("%v %v", ok, err)
	}
	if _, ok, err := nb.BlobText(strings.Repeat("0", 40)); err != nil || ok {
		t.Errorf("a missing blob is none: %v %v", ok, err)
	}
	if _, ok, _ := nb.BlobText("HEAD"); ok {
		t.Error("only an object id names a blob")
	}
}

func TestBlameOfUncommittedLinesMatchesTheRustGolden(t *testing.T) {
	nb := fixture(t, "dirty")
	lines, err := nb.Blame("01abc234", "budget-review")
	if err != nil {
		t.Fatal(err)
	}
	same(t, "blame", renderBlame(lines), golden(t, "dirty", "blame-uncommitted", "noda blame budget-review"))
}

func TestBlameOfAnUncommittedEditAndRename(t *testing.T) {
	nb := fixture(t, "linear")
	path := nb.NotePath("02def567", "meeting-notes")
	text, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	renamed := nb.NotePath("02def567", "retitled")
	if err := os.WriteFile(renamed, append(text, []byte("typed just now\n")...), 0o644); err != nil {
		t.Fatal(err)
	}
	_ = os.Remove(path)
	lines, err := nb.Blame("02def567", "retitled")
	if err != nil {
		t.Fatal(err)
	}
	last := lines[len(lines)-1]
	if last.Commit != "" || last.Seconds != 0 || last.ShortCommit() != "0000000" || last.Text != "typed just now" {
		t.Errorf("%+v", last)
	}
	if lines[0].Commit == "" || lines[0].Seconds == 0 {
		t.Errorf("history lost across the rename: %+v", lines[0])
	}

	fresh := nb.NotePath("0newnote", "fresh")
	if err := os.WriteFile(fresh, []byte("---\ntitle: x\n---\n\nonly\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	only, err := nb.Blame("0newnote", "fresh")
	if err != nil || len(only) != 1 || only[0].Commit != "" || only[0].Text != "only" {
		t.Errorf("%+v %v", only, err)
	}
}

func TestColorDiffPaintsByLineKind(t *testing.T) {
	plain := "diff --git a/f b/f\nindex 1..2 100644\n--- a/f\n+++ b/f\n@@ -1 +1 @@\n-old\n+new\n ctx\n\\ No newline at end of file\n"
	got := ColorDiff(plain)
	for _, want := range []string{
		"\x1b[1mdiff --git a/f b/f\nindex 1..2 100644\n--- a/f\n+++ b/f\n\x1b[0m",
		"\x1b[36m@@ -1 +1 @@\n\x1b[0m",
		"\x1b[31m-old\n\x1b[0m",
		"\x1b[32m+new\n\x1b[0m",
		" ctx\n\\ No newline at end of file\n",
	} {
		if !strings.Contains(got, want) {
			t.Errorf("missing %q in %q", want, got)
		}
	}
}

func TestSplitLinesAndBodyStart(t *testing.T) {
	if got := splitLines("a\r\nb\n\nc"); strings.Join(got, "|") != "a|b||c" {
		t.Errorf("%q", got)
	}
	if splitLines("") != nil || len(splitLines("\n")) != 1 {
		t.Error("empty")
	}
	if got := bodyStart("---\ntitle: t\n---\n\nbody\n"); got != 4 {
		t.Errorf("%d", got)
	}
	if got := bodyStart("no frontmatter\n"); got != 0 {
		t.Errorf("%d", got)
	}
}
