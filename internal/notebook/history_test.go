package notebook

import (
	"os"
	"regexp"
	"strconv"
	"strings"
	"testing"

	"github.com/henry40408/noda/internal/clock"
)

// numbered replaces each distinct short id with <OID:n> in order of appearance,
// in order of first appearance.
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

// renderDiff numbers the blob ids in order of first appearance; the all-zero id of a file
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

// trimmed ignores trailing blanks: expectations here keep none, while a diff's blank
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

func TestLinearLog(t *testing.T) {
	nb := fixture(t, "linear")
	all, err := nb.Log("", 0)
	if err != nil {
		t.Fatal(err)
	}
	same(t, "log", renderLog(all), []string{
		"  <OID:1>  2023-11-15 08:13  rm: Reading list",
		"  <OID:2>  2023-11-15 07:13  tag: Q3 budget review",
		"  <OID:3>  2023-11-15 06:13  add: Reading list",
		"  <OID:4>  2023-11-15 05:13  restore: Meeting notes",
		"  <OID:5>  2023-11-15 04:13  rm: Meeting notes",
		"  <OID:6>  2023-11-15 03:13  mv: Q3 budget review",
		"  <OID:7>  2023-11-15 02:13  edit: Budget review",
		"  <OID:8>  2023-11-15 01:13  add: Meeting notes",
		"  <OID:9>  2023-11-15 00:13  add: Budget review",
		"  <OID:10>  2023-11-14 23:13  chore: initialize notebook",
	})

	one, err := nb.Log("01abc234", 0)
	if err != nil {
		t.Fatal(err)
	}
	same(t, "log by id", renderLog(one), []string{
		"  <OID:1>  2023-11-15 07:13  tag: Q3 budget review",
		"  <OID:2>  2023-11-15 03:13  mv: Q3 budget review",
		"  <OID:3>  2023-11-15 02:13  edit: Budget review",
		"  <OID:4>  2023-11-15 00:13  add: Budget review",
	})

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
	same(t, "deleted", []string{line}, []string{"03ghj890  reading-list  2023-11-15 08:13  <OID:1>  Reading list"})
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

func TestLinearBlame(t *testing.T) {
	nb := fixture(t, "linear")
	lines, err := nb.Blame("02def567", "meeting-notes")
	if err != nil {
		t.Fatal(err)
	}
	same(t, "blame", renderBlame(lines), []string{
		"<OID:1>  2023-11-15 05:13  # Agenda",
		"<OID:1>  2023-11-15 05:13",
		"<OID:1>  2023-11-15 05:13  the budget, again",
		"<OID:1>  2023-11-15 05:13",
		"<OID:1>  2023-11-15 05:13  see [the budget](01abc234-budget-review.md)",
	})
}

func TestLinearDiffOfACleanNotebookIsTheLastCommit(t *testing.T) {
	nb := fixture(t, "linear")
	text, err := nb.Diff("")
	if err != nil {
		t.Fatal(err)
	}
	same(t, "diff", renderDiff(text), []string{
		"diff --git a/03ghj890-reading-list.md b/03ghj890-reading-list.md",
		"deleted file mode 100644",
		"index <OID:1>..0000000",
		"--- a/03ghj890-reading-list.md",
		"+++ /dev/null",
		"@@ -1,8 +0,0 @@",
		"----",
		"-title: Reading list",
		"-created: 2024-01-05T00:00:00Z",
		"-updated: 2024-01-05T00:00:00Z",
		"----",
		"-",
		"-a book",
		"-another book",
	})
}

func TestDirtyDiffLeavesUntrackedFilesOut(t *testing.T) {
	nb := fixture(t, "dirty")
	all, err := nb.Diff("")
	if err != nil {
		t.Fatal(err)
	}
	same(t, "diff", renderDiff(all), []string{
		"diff --git a/01abc234-budget-review.md b/01abc234-budget-review.md",
		"index <OID:1>..<OID:2> 100644",
		"--- a/01abc234-budget-review.md",
		"+++ b/01abc234-budget-review.md",
		"@@ -2,11 +2,12 @@",
		" title: Budget review",
		" tags: [work]",
		" created: 2024-01-01T00:00:00Z",
		"-updated: 2024-01-01T00:00:00Z",
		"+updated: 2024-01-09T00:00:00Z",
		" ---",
		" ",
		"-the q3 budget is late",
		"+the q3 budget is late again",
		" ",
		" - first line",
		"-- second line",
		"+- second line changed",
		" - third line",
		"+- fourth line",
	})
	one, err := nb.Diff("01abc234-budget-review.md")
	if err != nil {
		t.Fatal(err)
	}
	same(t, "diff note", renderDiff(one), []string{
		"diff --git a/01abc234-budget-review.md b/01abc234-budget-review.md",
		"index <OID:1>..<OID:2> 100644",
		"--- a/01abc234-budget-review.md",
		"+++ b/01abc234-budget-review.md",
		"@@ -2,11 +2,12 @@",
		" title: Budget review",
		" tags: [work]",
		" created: 2024-01-01T00:00:00Z",
		"-updated: 2024-01-01T00:00:00Z",
		"+updated: 2024-01-09T00:00:00Z",
		" ---",
		" ",
		"-the q3 budget is late",
		"+the q3 budget is late again",
		" ",
		" - first line",
		"-- second line",
		"+- second line changed",
		" - third line",
		"+- fourth line",
	})
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
	same(t, "log", renderLog(all), []string{
		"  <OID:1>  2023-11-15 04:13  edit: Budget review after merge",
		"  <OID:2>  2023-11-15 03:13  merge: side",
		"  <OID:3>  2023-11-15 02:13  edit: Budget review on main",
		"  <OID:4>  2023-11-15 01:13  edit: Meeting notes on side",
		"  <OID:5>  2023-11-15 00:13  add: two notes",
		"  <OID:6>  2023-11-14 23:13  chore: initialize notebook",
	})
	two, err := nb.Log("", 3)
	if err != nil {
		t.Fatal(err)
	}
	same(t, "log -n 3", renderLog(two), []string{
		"  <OID:1>  2023-11-15 04:13  edit: Budget review after merge",
		"  <OID:2>  2023-11-15 03:13  merge: side",
		"  <OID:3>  2023-11-15 02:13  edit: Budget review on main",
	})

	lines, err := nb.Blame("01abc234", "budget-review")
	if err != nil {
		t.Fatal(err)
	}
	same(t, "blame", renderBlame(lines), []string{
		"<OID:1>  2023-11-15 02:13  the q3 budget is late again",
		"<OID:2>  2023-11-15 00:13",
		"<OID:2>  2023-11-15 00:13  - first line",
		"<OID:1>  2023-11-15 02:13  - second line changed",
		"<OID:2>  2023-11-15 00:13  - third line",
		"<OID:1>  2023-11-15 02:13  - fourth line",
		"<OID:3>  2023-11-15 04:13  - after the merge",
	})
	text, err := nb.Diff("")
	if err != nil {
		t.Fatal(err)
	}
	same(t, "diff", renderDiff(text), []string{
		"diff --git a/01abc234-budget-review.md b/01abc234-budget-review.md",
		"index <OID:1>..<OID:2> 100644",
		"--- a/01abc234-budget-review.md",
		"+++ b/01abc234-budget-review.md",
		"@@ -2,7 +2,7 @@",
		" title: Budget review",
		" tags: [work]",
		" created: 2024-01-01T00:00:00Z",
		"-updated: 2024-01-04T00:00:00Z",
		"+updated: 2024-01-05T00:00:00Z",
		" ---",
		" ",
		" the q3 budget is late again",
		"@@ -11,3 +11,4 @@ the q3 budget is late again",
		" - second line changed",
		" - third line",
		" - fourth line",
		"+- after the merge",
	})
}

func TestRenameRewriteFollowsTheNoteById(t *testing.T) {
	nb := fixture(t, "rename-rewrite")
	entries, err := nb.Log("01abc234", 0)
	if err != nil {
		t.Fatal(err)
	}
	same(t, "log by id", renderLog(entries), []string{
		"  <OID:1>  2023-11-15 02:13  edit: Q3 budget review",
		"  <OID:2>  2023-11-15 01:13  mv: Q3 budget review, rewritten",
		"  <OID:3>  2023-11-15 00:13  add: Budget review",
	})
	text, err := nb.Diff("")
	if err != nil {
		t.Fatal(err)
	}
	same(t, "diff", renderDiff(text), []string{
		"diff --git a/01abc234-q3-budget-review.md b/01abc234-q3-budget-review.md",
		"index <OID:1>..<OID:2> 100644",
		"--- a/01abc234-q3-budget-review.md",
		"+++ b/01abc234-q3-budget-review.md",
		"@@ -2,7 +2,7 @@",
		" title: Q3 budget review",
		" tags: [work]",
		" created: 2024-01-01T00:00:00Z",
		"-updated: 2024-01-02T00:00:00Z",
		"+updated: 2024-01-03T00:00:00Z",
		" ---",
		" ",
		" one",
		"@@ -17,3 +17,4 @@ eight",
		" nine",
		" ten",
		" lima",
		"+eleven",
	})
}

// Rust follows a rename by id and so credits `golf` and `lima` to the note's
// first commit (<OID:2> in its record, the other kept lines being <OID:1>).
// git follows by content, and a rewrite this large is not one file to it. The
// user accepted the difference (CLAUDE.md, "Accepted differences from the Rust
// version"); this test pins what git does instead.
func TestRenameRewriteBlameDiffersFromRustAsKnown(t *testing.T) {
	nb := fixture(t, "rename-rewrite")
	lines, err := nb.Blame("01abc234", "q3-budget-review")
	if err != nil {
		t.Fatal(err)
	}
	same(t, "blame", renderBlame(lines), []string{
		"<OID:1>  2023-11-15 01:13  one",
		"<OID:1>  2023-11-15 01:13  two",
		"<OID:1>  2023-11-15 01:13  three",
		"<OID:1>  2023-11-15 01:13  four",
		"<OID:1>  2023-11-15 01:13  five",
		"<OID:1>  2023-11-15 01:13  six",
		"<OID:1>  2023-11-15 01:13  golf",
		"<OID:1>  2023-11-15 01:13  seven",
		"<OID:1>  2023-11-15 01:13  eight",
		"<OID:1>  2023-11-15 01:13  nine",
		"<OID:1>  2023-11-15 01:13  ten",
		"<OID:1>  2023-11-15 01:13  lima",
		"<OID:2>  2023-11-15 02:13  eleven",
	})
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
	same(t, "snapshots", got, []string{
		"clash       2023-11-15 01:13  <OID:1>  here it is the new commit",
		"local-only  2023-11-15 01:13  <OID:1>  only here",
		"shared      2023-11-15 00:13  <OID:2>  on both",
	})
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
	same(t, "diff --remote", renderDiff(text), []string{
		"diff --git a/03ghj890-reading-list.md b/03ghj890-reading-list.md",
		"new file mode 100644",
		"index 0000000..<OID:1>",
		"--- /dev/null",
		"+++ b/03ghj890-reading-list.md",
		"@@ -0,0 +1,9 @@",
		"+---",
		"+title: Reading list",
		"+tags: [books]",
		"+created: 2024-01-05T00:00:00Z",
		"+updated: 2024-01-06T00:00:00Z",
		"+---",
		"+",
		"+a book",
		"+another book",
	})
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

func TestBlameOfUncommittedLines(t *testing.T) {
	nb := fixture(t, "dirty")
	lines, err := nb.Blame("01abc234", "budget-review")
	if err != nil {
		t.Fatal(err)
	}
	same(t, "blame", renderBlame(lines), []string{
		"0000000  not committed     the q3 budget is late again",
		"<OID:1>  2023-11-15 00:13",
		"<OID:1>  2023-11-15 00:13  - first line",
		"0000000  not committed     - second line changed",
		"<OID:1>  2023-11-15 00:13  - third line",
		"0000000  not committed     - fourth line",
	})
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
