package cmd

import (
	"encoding/json"
	"path/filepath"
	"strconv"
	"strings"
	"testing"
	"time"

	"github.com/henry40408/noda/internal/note"
	"github.com/henry40408/noda/internal/notebook"
	"github.com/henry40408/noda/internal/style"
	"github.com/henry40408/noda/internal/todo"
)

// These tests pin boundaries and branches that the mutation shard for this
// package found unguarded.

func mustBeJSONWithItems(t *testing.T, text, key string, want int) {
	t.Helper()
	var document map[string]json.RawMessage
	if err := json.Unmarshal([]byte(text), &document); err != nil {
		t.Fatalf("not JSON: %v\n%s", err, text)
	}
	var items []json.RawMessage
	if err := json.Unmarshal(document[key], &items); err != nil || len(items) != want {
		t.Fatalf("%s: %d items, %v, wanted %d\n%s", key, len(items), err, want, text)
	}
}

func TestJSONListsSeparateTheirItemsWithCommasBetweenOnly(t *testing.T) {
	files := []notebook.NoteFile{aNote("k3f9", "One", "2026-01-01T00:00:00Z", false), aNote("m2p1", "Two", "2026-01-01T00:00:00Z", false)}
	mustBeJSONWithItems(t, backlinksJSON("main", "x.md", files), "backlinks", 2)

	gone := []notebook.Deleted{{ID: "k3f9", Slug: "one"}, {ID: "m2p1", Slug: "two"}}
	mustBeJSONWithItems(t, deletedJSON("main", gone), "deleted", 2)

	rows := []todoRow{{id: "k3f9", slug: "one", item: todo.Item{Text: "a"}}, {id: "m2p1", slug: "two", item: todo.Item{Text: "b"}}}
	mustBeJSONWithItems(t, todoJSON("main", rows), "todo", 2)
}

func TestDescribeSaysKeepingUpdatedOnlyForTouchKeep(t *testing.T) {
	keep := Step{Keys: []string{"a"}, Change: Change{Tags: []string{"+x"}, Touch: TouchKeep}}
	if got := keep.Describe(); got != "tag: +x (1 note, keeping updated)" {
		t.Fatalf("got %q", got)
	}
	stamp := Step{Keys: []string{"a", "b"}, Change: Change{Tags: []string{"+x"}, Touch: TouchStamp}}
	if got := stamp.Describe(); got != "tag: +x (2 notes)" {
		t.Fatalf("got %q", got)
	}
}

func TestCountIsSingularOnlyForOne(t *testing.T) {
	for n, want := range map[int]string{0: "0 notes", 1: "1 note", 2: "2 notes"} {
		if got := count(n, "note"); got != want {
			t.Errorf("count(%d) = %q, want %q", n, got, want)
		}
	}
}

func TestCommitMessageIsTheStepForOneAndASummaryForSeveral(t *testing.T) {
	one := Step{Keys: []string{"a"}, Change: Change{Remove: true}}
	if got := commitMessage([]Step{one}); got != "rm: 1 note" {
		t.Fatalf("got %q", got)
	}
	two := Step{Keys: []string{"a"}, Change: Change{Tags: []string{"+x"}}}
	want := "bulk: 2 changes over 1 note\n\nrm: 1 note\ntag: +x (1 note)"
	if got := commitMessage([]Step{one, two}); got != want {
		t.Fatalf("got %q, want %q", got, want)
	}
}

func TestBulkThatChangesNothingMakesNoCommit(t *testing.T) {
	e := dInitialized(t)
	dAdd(t, e, "Alpha", "a\n")
	commits := dCommitCount(t, e, dNotebookDir(e))

	out, err := Bulk(e, []Step{{Keys: []string{"nosuchnote"}, Change: Change{Tags: []string{"+x"}}}})
	if err != nil {
		t.Fatal(err)
	}
	if !strings.HasPrefix(out, "nothing to change") {
		t.Fatalf("got %q", out)
	}
	if got := dCommitCount(t, e, dNotebookDir(e)); got != commits {
		t.Fatalf("a commit was made: %d, was %d", got, commits)
	}
}

func TestDescribeTimesCallsANoteStaleOnlyPastTheLag(t *testing.T) {
	updated := "2020-01-01T00:00:00Z"
	file := aNote("k3f9m2p1", "Alpha", updated, false)
	base := mustInstant(t, updated).Unix()
	last := func(offset int64) map[string]int64 {
		return map[string]int64{note.NormalizeID(file.ID): base + offset}
	}
	notes := []notebook.NoteFile{file}

	if got := describeTimes(notes, last(commitLagSeconds)); got != "" {
		t.Fatalf("exactly the lag is not stale: %q", got)
	}
	if got := describeTimes(notes, last(commitLagSeconds+1)); !strings.Contains(got, "changed outside noda") {
		t.Fatalf("one second past the lag is stale: %q", got)
	}
}

func mustInstant(t *testing.T, stamp string) (parsed time.Time) {
	t.Helper()
	parsed, ok := instant(&stamp)
	if !ok {
		t.Fatalf("unreadable stamp %q", stamp)
	}
	return parsed
}

func TestDoctorKeepsItsExtraFindingsBesideAProblem(t *testing.T) {
	e := dInitialized(t)
	dir := dNotebookDir(e)
	dPlant(t, dir, "k3f9m2p3", "first")
	dPlant(t, dir, "k3f9m2p3", "second")
	dWrite(t, filepath.Join(dir, "k3f9m2p1-unreadable.md"),
		"---\ntitle: Unreadable\ncreated: last tuesday\n---\n\nbody\n")

	out := plain(Doctor(e, DoctorOptions{Times: true}))
	for _, want := range []string{"shared", "1 time cannot be read", "k3f9m2p1-unreadable.md created: last tuesday"} {
		dHas(t, out, want)
	}
}

func unconvertedNotes(n int) []notebook.NoteFile {
	notes := make([]notebook.NoteFile, n)
	for i := range notes {
		id := "k3f9m2p" + strconv.Itoa(i)
		notes[i] = notebook.NoteFile{ID: id, Slug: "n", Note: note.Note{Extra: []string{UnconvertedField + ": table"}}}
	}
	return notes
}

func TestUnconvertedListAnnouncesItsCapOnlyWhenCut(t *testing.T) {
	five := style.Strip(describeUnconverted(unconvertedNotes(5)))
	if strings.Contains(five, "more") {
		t.Fatalf("five fit under the cap:\n%s", five)
	}
	six := style.Strip(describeUnconverted(unconvertedNotes(6)))
	dHas(t, six, "and 1 more, each with its own `unconverted:` field")
}

func TestAdviceAboutSuspiciousFilesOnlyWhenThereAreSome(t *testing.T) {
	if got := advice(notebook.Scan{}); len(got) != 0 {
		t.Fatalf("an empty scan needs no advice: %q", got)
	}
	got := advice(notebook.Scan{Suspicious: []string{"k3f9m2p1-x.md"}})
	if len(got) != 1 || !strings.Contains(got[0], "holding no frontmatter") {
		t.Fatalf("got %q", got)
	}
}

func TestFileAddCopiesTheContentAndNamesTheCommit(t *testing.T) {
	e := dInitialized(t)
	source := filepath.Join(t.TempDir(), "photo.txt")
	dWrite(t, source, "hello")

	if _, err := FileAdd(e, []string{source}, ""); err != nil {
		t.Fatal(err)
	}
	dEqual(t, dRead(t, filepath.Join(dNotebookDir(e), "photo.txt")), "hello")
	dEqual(t, dGit(t, e, dNotebookDir(e), "log", "-1", "--format=%s"), "file: add photo.txt")

	other := filepath.Join(t.TempDir(), "other.txt")
	dWrite(t, other, "x")
	if _, err := FileAdd(e, []string{other, source + ".b"}, ""); err == nil {
		t.Fatal("a missing source is refused")
	}
	again := filepath.Join(t.TempDir(), "again.txt")
	dWrite(t, again, "y")
	if _, err := FileAdd(e, []string{other, again}, ""); err != nil {
		t.Fatal(err)
	}
	dEqual(t, dGit(t, e, dNotebookDir(e), "log", "-1", "--format=%s"), "file: add 2 files")
}

func TestBlameSaysNotCommittedOnlyForWhatIsNotCommitted(t *testing.T) {
	e := dInitialized(t)
	summary := dAdd(t, e, "Alpha", "committed line\n")
	path := filepath.Join(dNotebookDir(e), dNoteFile(summary))
	dWrite(t, path, dRead(t, path)+"fresh line\n")

	for _, line := range strings.Split(plain(Blame(e, "alpha")), "\n") {
		switch {
		case strings.Contains(line, "committed line"):
			if strings.Contains(line, "not committed") {
				t.Errorf("a committed line reads as uncommitted: %q", line)
			}
		case strings.Contains(line, "fresh line"):
			if !strings.Contains(line, "not committed") {
				t.Errorf("an uncommitted line has to say so: %q", line)
			}
		}
	}
}

func TestInitSetsTheFirstNotebookActiveAndSaysSo(t *testing.T) {
	e := dBare(t)
	out, err := Init(e)
	if err != nil {
		t.Fatal(err)
	}
	dHas(t, out, "active notebook: default")
	if got, err := NotebookCurrent(e); err != nil || got != "default" {
		t.Fatalf("active %q, %v", got, err)
	}
}

func TestConfigShowTakesSignAndAuthorFromGitWhenNothingElseSaysSo(t *testing.T) {
	e := dBare(t)
	shown := plain(ConfigShow(e))
	dHas(t, shown, "Test <test@example.com>")
	for _, line := range strings.Split(shown, "\n") {
		if strings.HasPrefix(line, "sign") && !strings.Contains(line, "(git config)") {
			t.Fatalf("git's commit.gpgsign is the source: %q", line)
		}
	}
}

func TestNotebookAddWithARemoteStillReportsTheNotebook(t *testing.T) {
	e := dInitialized(t)
	out, err := NotebookAdd(e, "work", "https://example.com/work.git")
	if err != nil || !strings.HasPrefix(out, "created notebook `work` at ") {
		t.Fatalf("got %q, %v", out, err)
	}
}

func TestNotebookRmAndRenameReportWhatTheyDid(t *testing.T) {
	e := dInitialized(t)
	dDone(NotebookAdd(e, "work", ""))
	out, err := NotebookRm(e, "work", true)
	if err != nil || !strings.HasPrefix(out, "removed notebook `work` and its history at ") {
		t.Fatalf("rm: %q, %v", out, err)
	}

	out, err = NotebookRename(e, "default", "renamed")
	if err != nil || out != "renamed notebook `default` to `renamed`" {
		t.Fatalf("rename: %q, %v", out, err)
	}
	if got, err := NotebookCurrent(e); err != nil || got != "renamed" {
		t.Fatalf("the active pointer follows: %q, %v", got, err)
	}
}

func TestNotebookLsMutesOnlyWhatNeedsNoAction(t *testing.T) {
	e := dInitialized(t)
	dDone(RemoteSet(e, dBareRemote(t, e, dBranchOf(t, e, "default"))))
	dAdd(t, e, "Alpha", "a\n")
	dDone(Push(e))

	muted := style.Muted.Paint("in sync")
	if listed := painted(NotebookLs(e)); !strings.Contains(listed, muted) {
		t.Fatalf("in sync is dimmed: %q", listed)
	}

	dAdd(t, e, "Beta", "b\n")
	listed := painted(NotebookLs(e))
	if strings.Contains(listed, style.Muted.Paint("1 to push")) || !strings.Contains(listed, "1 to push") {
		t.Fatalf("work waiting is not dimmed: %q", listed)
	}
}

func TestRmShowsTheTagsOfWhatItRemoved(t *testing.T) {
	e := dInitialized(t)
	dAdd(t, e, "Alpha", "a\n", "work")
	out, err := Rm(e, "alpha")
	if err != nil || !strings.Contains(out, "[work]") {
		t.Fatalf("got %q, %v", out, err)
	}
}

func TestAFlagReadAsATagHintsOnlyWhenTheNoteIsKnown(t *testing.T) {
	_, err := parseTag("--no-touch", "k3f9")
	if err == nil || !strings.Contains(err.Error(), "`noda tag k3f9 --no-touch +tag`") {
		t.Fatalf("got %v", err)
	}
	_, err = parseTag("--no-touch", "")
	if err == nil || strings.Contains(err.Error(), "noda tag") {
		t.Fatalf("no key, no hint: %v", err)
	}
}

func TestSearchQuotesTheBodyUnderTheTitleColumn(t *testing.T) {
	e, _ := aNotebook(t)
	id := addNote(t, e, "Plan", "needle in the first line")

	lines := strings.Split(plain(Search(e, []string{"needle"})), "\n")
	if lines[0] != id+"  Plan" {
		t.Fatalf("row: %q", lines[0])
	}
	if want := strings.Repeat(" ", len(id)+2) + "needle in the first line"; lines[1] != want {
		t.Fatalf("excerpt: %q, want %q", lines[1], want)
	}
}

func TestAMatchAtTheStartOfALineIsFound(t *testing.T) {
	start, end, ok := FindIgnoringCase("Needle", "needle")
	if !ok || start != 0 || end != 6 {
		t.Fatalf("got %d %d %v", start, end, ok)
	}
}

func TestCuttingKeepsTextExactlyAsLongAsTheLimit(t *testing.T) {
	if got := lastChars("abc", 3); got != "abc" {
		t.Errorf("lastChars = %q", got)
	}
	if got := firstChars("abc", 3); got != "abc" {
		t.Errorf("firstChars = %q", got)
	}
	if got := lastChars("abcd", 3); got != "…bcd" {
		t.Errorf("lastChars cut = %q", got)
	}
	if got := firstChars("abcd", 3); got != "abc…" {
		t.Errorf("firstChars cut = %q", got)
	}
}

func TestElideMarksACutOnlyBeyondThree(t *testing.T) {
	if got := elide([]string{"a", "b", "c"}); got != "a; b; c" {
		t.Errorf("three: %q", got)
	}
	if got := elide([]string{"a", "b", "c", "d"}); got != "a; b; c; …" {
		t.Errorf("four: %q", got)
	}
}

func TestAPinnedNoteWithoutTagsHasOneGapBeforeThePin(t *testing.T) {
	pinned := aNote("k3f9", "Alpha", "2026-01-01T00:00:00Z", true)
	want := "k3f9  Alpha  " + style.PinMark
	if got := style.Strip(asTable([]notebook.NoteFile{pinned}, nil, false)); got != want+"\n" {
		t.Fatalf("got %q, want %q", got, want+"\n")
	}
}

func TestFilesHeadingIsSeparatedFromRowsAndOnlyFromRows(t *testing.T) {
	var alone strings.Builder
	appendFiles(&alone, []string{"a.png"})
	if got := style.Strip(alone.String()); got != "files\n  a.png\n" {
		t.Errorf("alone: %q", got)
	}
	var below strings.Builder
	below.WriteString("row\n")
	appendFiles(&below, []string{"a.png"})
	if got := style.Strip(below.String()); got != "row\n\nfiles\n  a.png\n" {
		t.Errorf("below rows: %q", got)
	}
}

func TestDisplayWidthOfControlsAndKeycaps(t *testing.T) {
	cases := []struct {
		name, text string
		want       int
	}{
		{"DEL is a control", "\x7f", 1},
		{"C1 control", "\u0090", 1},
		{"hash keycap", "#️", 2},
		{"star keycap", "*️", 2},
		{"zero keycap", "0️", 2},
		{"nine keycap", "9️", 2},
		{"letter is not a keycap", "a️", 1},
		{"before zero is not a keycap", "/️", 1},
		{"after nine is not a keycap", ":️", 1},
	}
	for _, c := range cases {
		if got := DisplayWidth(c.text); got != c.want {
			t.Errorf("%s %q: %d, want %d", c.name, c.text, got, c.want)
		}
	}
}

func TestColumnPadsToTheWidthOutsideTheEscapes(t *testing.T) {
	if got := column(style.Muted, "ab", 5); got != style.Muted.Paint("ab")+"   " {
		t.Errorf("got %q", got)
	}
	if got := column(style.Muted, "日本", 6); got != style.Muted.Paint("日本")+"  " {
		t.Errorf("wide: %q", got)
	}
	if got := column(style.Muted, "long", 2); got != style.Muted.Paint("long") {
		t.Errorf("never cuts: %q", got)
	}
}
