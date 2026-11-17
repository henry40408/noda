package cmd

import (
	"encoding/json"
	"os"
	"strings"
	"testing"

	"github.com/henry40408/noda/internal/gitx/gittest"
	"github.com/henry40408/noda/internal/note"
	"github.com/henry40408/noda/internal/notebook"
	"github.com/henry40408/noda/internal/paths"
	"github.com/henry40408/noda/internal/style"
)

func TestEveryOrderIsOnTheRingTheKeyWalks(t *testing.T) {
	at := SortSlug
	for _, sort := range AllSorts {
		if at != sort {
			t.Fatalf("the ring and the list came apart at %s", sort.Name())
		}
		at = at.Next()
	}
	if at != SortSlug {
		t.Fatal("the ring stopped coming round")
	}
	for _, sort := range AllSorts {
		if back, ok := SortNamed(sort.Name()); !ok || back != sort {
			t.Fatalf("%s did not read back", sort.Name())
		}
	}
}

func TestJSONStringRoundTripsWhatBreaksADocument(t *testing.T) {
	nasty := "q\" b\\ \n\r\t\b\f\x00\x1f\x7f    \U0001F600 é 日本"
	encoded := jsonString(nasty)
	for _, r := range encoded {
		if r < ' ' {
			t.Fatalf("control character left raw in %q", encoded)
		}
	}
	var back string
	if err := json.Unmarshal([]byte(encoded), &back); err != nil || back != nasty {
		t.Fatalf("round trip: %q, %v", back, err)
	}
	if got := jsonString("a\x01\"\\"); got != `"a\u0001\"\\"` {
		t.Fatalf("got %s", got)
	}
}

func aNote(id, title, updated string, pinned bool) notebook.NoteFile {
	file := notebook.NoteFile{ID: id, Slug: note.Slugify(title), Note: note.Note{Title: title, Created: &updated, Updated: &updated}}
	if pinned {
		value := note.PinnedValue
		file.Note.Pinned = &value
	}
	return file
}

func TestAPinFloatsToTheTopOfWhicheverOrderWasAskedFor(t *testing.T) {
	for _, by := range AllSorts {
		notes := []notebook.NoteFile{
			aNote("aaaa1111", "Alpha", "2026-01-01T00:00:00Z", false),
			aNote("bbbb2222", "Bravo", "2026-01-02T00:00:00Z", true),
			aNote("cccc3333", "Charlie", "2026-01-03T00:00:00Z", false),
			aNote("dddd4444", "Delta", "2026-01-04T00:00:00Z", true),
		}
		SortNotes(notes, by)
		if !notes[0].Note.IsPinned() || !notes[1].Note.IsPinned() {
			t.Fatalf("%s let a loose note above a pinned one", by.Name())
		}
		want := [2]string{"bbbb2222", "dddd4444"}
		if by == SortCreated || by == SortUpdated {
			want = [2]string{"dddd4444", "bbbb2222"}
		}
		if got := [2]string{notes[0].ID, notes[1].ID}; got != want {
			t.Fatalf("%s lost the order inside the pins: %v", by.Name(), got)
		}
	}
}

func TestStampsSortByInstantNotByText(t *testing.T) {
	notes := []notebook.NoteFile{
		aNote("aaaa1111", "Alpha", "2019-03-14T16:21:00+08:00", false),
		aNote("bbbb2222", "Bravo", "2019-03-14T09:00:00Z", false),
	}
	SortNotes(notes, SortUpdated)
	if notes[0].ID != "bbbb2222" {
		t.Fatalf("the later instant must come first: %s", notes[0].ID)
	}
}

func TestDeriveTitleSkipsBlankLinesAndHeadingMarkers(t *testing.T) {
	if title, ok := deriveTitle("\n\n## Deep Work\nbody"); !ok || title != "Deep Work" {
		t.Fatalf("got %q, %v", title, ok)
	}
	if _, ok := deriveTitle("   \n\n"); ok {
		t.Fatal("a blank body has no title")
	}
}

func TestCaseInsensitiveFindReturnsOffsetsIntoTheOriginal(t *testing.T) {
	line := "Discuss the Q3 Budget"
	start, end, ok := FindIgnoringCase(line, "q3 budget")
	if !ok || line[start:end] != "Q3 Budget" {
		t.Fatalf("got %d %d %v", start, end, ok)
	}

	// İ lowercases to two characters, changing the byte length.
	turkish := "aİb"
	start, end, ok = FindIgnoringCase(turkish, "b")
	if !ok || turkish[start:end] != "b" {
		t.Fatalf("got %d %d %v", start, end, ok)
	}

	if start, end, ok := FindIgnoringCase("會議紀錄", "紀錄"); !ok || start != 6 || end != 12 {
		t.Fatalf("got %d %d %v", start, end, ok)
	}
	if _, _, ok := FindIgnoringCase("nothing", "here"); ok {
		t.Fatal("found what is not there")
	}
}

func TestExcerptsAreCutAroundTheMatch(t *testing.T) {
	long := strings.Repeat("before ", 20) + " needle " + strings.Repeat("after ", 20)
	shown, ok := excerpt(long, []string{"needle"})
	shown = style.Strip(shown)
	if !ok || !strings.Contains(shown, "needle") {
		t.Fatalf("no match in %q", shown)
	}
	if !strings.HasPrefix(shown, "…") || !strings.HasSuffix(shown, "…") {
		t.Fatalf("both ends are cut: %q", shown)
	}
	if len([]rune(shown)) > excerptWidth+2 {
		t.Fatalf("too long: %q", shown)
	}

	shown, _ = excerpt("just the needle here", []string{"needle"})
	if style.Strip(shown) != "just the needle here" {
		t.Fatalf("got %q", shown)
	}
	if _, ok := excerpt("no hit", []string{"needle"}); ok {
		t.Fatal("matched nothing")
	}
}

func TestDimmingTheFrontmatterChangesNothingButTheEscapes(t *testing.T) {
	text := "---\nid: k3f9\ntitle: Alpha\n---\n\nbody, with --- in it\n"
	shown := dimFrontmatter(text)
	if shown == text && style.Muted.Paint("x") != "x" {
		t.Fatal("the frontmatter is styled")
	}
	if style.Strip(shown) != text {
		t.Fatalf("something else moved: %q", style.Strip(shown))
	}
	for _, plain := range []string{"no frontmatter\n", "---\nunterminated\n"} {
		if dimFrontmatter(plain) != plain {
			t.Fatalf("%q changed", plain)
		}
	}
}

func TestSummaryOmitsEmptyTags(t *testing.T) {
	if got := summary("k3f9", "notes", nil); got != "k3f9  notes" {
		t.Fatalf("got %q", got)
	}
	if got := summary("k3f9", "notes", []string{"work", "q3"}); got != "k3f9  notes  [work, q3]" {
		t.Fatalf("got %q", got)
	}
}

// aNotebook is an isolated home holding one active notebook called main.
func aNotebook(t *testing.T) (Env, *notebook.Notebook) {
	t.Helper()
	git := gittest.Require(t)
	where := paths.Rooted(t.TempDir())
	if err := where.CreateDirs(); err != nil {
		t.Fatal(err)
	}
	created, err := notebook.Create(git, where, "main")
	if err != nil {
		t.Fatal(err)
	}
	if err := where.SetActiveNotebook("main"); err != nil {
		t.Fatal(err)
	}
	return Env{Paths: where, Git: git}, created
}

func addNote(t *testing.T, e Env, title, content string, tags ...string) string {
	t.Helper()
	out, err := Add(e, AddOptions{Title: &title, Content: &content, Tags: tags})
	if err != nil {
		t.Fatal(err)
	}
	return strings.Fields(out)[0]
}

func TestAddTagPinListAndRemove(t *testing.T) {
	e, _ := aNotebook(t)
	id := addNote(t, e, "Meeting Notes", "agenda: budget")

	if out, err := Tag(e, id, []string{"+work", "+q3"}, TouchStamp); err != nil || !strings.HasSuffix(out, "[work, q3]") {
		t.Fatalf("tag: %q, %v", out, err)
	}
	if out, _ := Tag(e, id, []string{"+work"}, TouchStamp); !strings.HasSuffix(out, "(no change)") {
		t.Fatalf("a repeated tag is a no-op: %q", out)
	}
	if _, err := Tag(e, id, []string{"--no-touch"}, TouchStamp); err == nil || !strings.Contains(err.Error(), "flag has to come before") {
		t.Fatalf("a flag read as a tag: %v", err)
	}
	if _, err := Pin(e, id, true, TouchStamp); err != nil {
		t.Fatal(err)
	}

	listed, err := Ls(e, List{Format: FormatJSON})
	if err != nil {
		t.Fatal(err)
	}
	var document struct {
		Notes []struct {
			ID     string   `json:"id"`
			Pinned bool     `json:"pinned"`
			Tags   []string `json:"tags"`
		} `json:"notes"`
	}
	if err := json.Unmarshal([]byte(listed), &document); err != nil {
		t.Fatalf("%v: %s", err, listed)
	}
	if len(document.Notes) != 1 || !document.Notes[0].Pinned || len(document.Notes[0].Tags) != 2 {
		t.Fatalf("got %s", listed)
	}

	found, err := Search(e, []string{"BUDGET"})
	if err != nil || !strings.Contains(style.Strip(found), "agenda: budget") {
		t.Fatalf("search: %q, %v", found, err)
	}

	if _, err := Rm(e, id); err != nil {
		t.Fatal(err)
	}
	if quiet, _ := Ls(e, List{Format: FormatQuiet}); quiet != "" {
		t.Fatalf("still listed: %q", quiet)
	}
}

func TestRetitlingRewritesLinksOnlyWhenAsked(t *testing.T) {
	e, _ := aNotebook(t)
	target := addNote(t, e, "Old Name", "body")
	oldFile := note.FileName(target, "old-name")
	body := "see [it](" + oldFile + ")"
	linker := addNote(t, e, "Linker Two", body)

	out, err := Mv(e, Rename{Key: target, NewTitle: "New Name", Touch: TouchStamp})
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(out, "link") || !strings.Contains(out, "linker-two") {
		t.Fatalf("the stale link is reported: %q", out)
	}

	out, err = Mv(e, Rename{Key: target, NewTitle: "New Name", UpdateLinks: true, Touch: TouchStamp})
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(out, "updated  1 note") {
		t.Fatalf("the link is rewritten: %q", out)
	}
	text, err := os.ReadFile(notePath(t, e, linker))
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(string(text), note.FileName(target, "new-name")) {
		t.Fatalf("link not retargeted:\n%s", text)
	}
}

func notePath(t *testing.T, e Env, key string) string {
	t.Helper()
	opened, err := e.openActive()
	if err != nil {
		t.Fatal(err)
	}
	resolved, err := opened.Resolve(key)
	if err != nil {
		t.Fatal(err)
	}
	return opened.NotePath(resolved.ID, resolved.Slug)
}

func TestBulkMakesOneCommit(t *testing.T) {
	e, opened := aNotebook(t)
	first := addNote(t, e, "First", "a")
	second := addNote(t, e, "Second", "b")

	out, err := Bulk(e, []Step{
		{Keys: []string{first, second}, Change: Change{Tags: []string{"+x"}}},
		{Keys: []string{second}, Change: Change{Remove: true}},
	})
	if err != nil || !strings.Contains(out, "in one commit") {
		t.Fatalf("bulk: %q, %v", out, err)
	}
	if _, err := os.Stat(notePath(t, e, first)); err != nil {
		t.Fatal(err)
	}
	if _, err := Bulk(e, nil); err == nil {
		t.Fatal("an empty queue is refused")
	}
	if err := Check(Change{Tags: []string{"nope"}}); err == nil {
		t.Fatal("a bad tag is refused before sending")
	}
	_ = opened
}

func TestUseRefusesAnUnknownNotebook(t *testing.T) {
	e, _ := aNotebook(t)
	if _, err := Use(e, "ghost"); err == nil || !strings.Contains(err.Error(), "notebook not found: ghost") {
		t.Fatalf("got %v", err)
	}
	if out, err := Use(e, "main"); err != nil || out != "active notebook: main" {
		t.Fatalf("got %q, %v", out, err)
	}
}

func TestPathNamesANoteOrAFileAndRefusesBoth(t *testing.T) {
	e, opened := aNotebook(t)
	id := addNote(t, e, "Plan", "x")
	if out, err := Path(e, id); err != nil || !strings.HasSuffix(out, ".md\n") {
		t.Fatalf("note: %q, %v", out, err)
	}
	if err := os.WriteFile(opened.Path+"/data.csv", []byte("a"), 0o666); err != nil {
		t.Fatal(err)
	}
	if out, err := Path(e, "data.csv"); err != nil || !strings.HasSuffix(out, "data.csv\n") {
		t.Fatalf("file: %q, %v", out, err)
	}
	if _, err := Path(e, "zzzzzzzz"); err == nil || !strings.Contains(err.Error(), "nothing called") {
		t.Fatalf("got %v", err)
	}
}
