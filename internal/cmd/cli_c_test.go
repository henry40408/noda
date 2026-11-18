package cmd

import (
	"context"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"testing"

	"github.com/mattn/go-runewidth"

	"github.com/henry40408/noda/internal/config"
	"github.com/henry40408/noda/internal/gitx"
	"github.com/henry40408/noda/internal/gitx/gittest"
	"github.com/henry40408/noda/internal/note"
	"github.com/henry40408/noda/internal/paths"
)

// cWorld is an initialised notebook under a throwaway root, with a directory
// elsewhere on disk to take files from.
type cWorld struct {
	env      Env
	root     string
	notebook string
}

// cInitialized is `initialized()` of the Rust tests: signing off, `init` run,
// the default notebook active.
func cInitialized(t *testing.T) cWorld {
	t.Helper()
	root := t.TempDir()
	where := paths.Rooted(root)
	if err := os.MkdirAll(where.ConfigDir(), 0o777); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(where.ConfigDir(), "config.toml"), []byte("sign = false\n"), 0o666); err != nil {
		t.Fatal(err)
	}
	e := Env{Paths: where, Git: gittest.Require(t)}
	if _, err := Init(e); err != nil {
		t.Fatal(err)
	}
	return cWorld{env: e, root: root, notebook: where.NotebookDir(config.DefaultNotebook)}
}

// cAdd adds a note and returns the summary line.
func cAdd(t *testing.T, w cWorld, title, body string, tags ...string) string {
	t.Helper()
	out, err := Add(w.env, AddOptions{Title: &title, Content: &body, Tags: tags})
	if err != nil {
		t.Fatal(err)
	}
	return out
}

// cParts is the id and slug from the `id  slug  [tags]` line mutating commands print.
func cParts(t *testing.T, summary string) (id, slug string) {
	t.Helper()
	id, rest, found := strings.Cut(summary, "  ")
	if !found {
		t.Fatalf("no id and slug in %q", summary)
	}
	slug, _, _ = strings.Cut(rest, "  ")
	return id, strings.TrimSpace(slug)
}

func cNoteFile(t *testing.T, summary string) string {
	t.Helper()
	id, slug := cParts(t, summary)
	return id + "-" + slug + ".md"
}

func cCommitCount(t *testing.T, w cWorld) int {
	t.Helper()
	result, err := w.env.Git.Run(context.Background(), gitx.Cmd{Dir: w.notebook, Args: []string{"rev-list", "--count", "HEAD"}, ReadOnly: true})
	if err != nil {
		t.Fatal(err)
	}
	count, err := strconv.Atoi(strings.TrimSpace(string(result.Stdout)))
	if err != nil {
		t.Fatal(err)
	}
	return count
}

func cPlantFile(t *testing.T, w cWorld, name string) {
	t.Helper()
	if err := os.WriteFile(filepath.Join(w.notebook, name), []byte("contents\n"), 0o666); err != nil {
		t.Fatal(err)
	}
}

// cWriteNote puts a note straight into the notebook, bypassing `add`.
func cWriteNote(t *testing.T, w cWorld, name, text string) {
	t.Helper()
	if err := os.WriteFile(filepath.Join(w.notebook, name), []byte(text), 0o666); err != nil {
		t.Fatal(err)
	}
}

func cReadFile(t *testing.T, path string) string {
	t.Helper()
	text, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	return string(text)
}

func cExists(path string) bool {
	_, err := os.Stat(path)
	return err == nil
}

// cNoteText is the note as it sits on disk; `show` dims the frontmatter.
func cNoteText(t *testing.T, w cWorld, key string) string {
	t.Helper()
	path, err := Path(w.env, key)
	if err != nil {
		t.Fatal(err)
	}
	return cReadFile(t, strings.TrimRight(path, "\n"))
}

// cTimes is the created and updated stamps of a note.
func cTimes(t *testing.T, w cWorld, key string) (created, updated *string) {
	t.Helper()
	parsed, err := note.Parse(cNoteText(t, w, key))
	if err != nil {
		t.Fatal(err)
	}
	return parsed.Created, parsed.Updated
}

// cSourceFile is a file elsewhere on disk to copy into a notebook.
func cSourceFile(t *testing.T, w cWorld, name string) string {
	t.Helper()
	dir := filepath.Join(w.root, "elsewhere")
	if err := os.MkdirAll(dir, 0o777); err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(dir, name)
	if err := os.WriteFile(path, []byte("contents\n"), 0o666); err != nil {
		t.Fatal(err)
	}
	return path
}

func cFileAdd(t *testing.T, w cWorld, rename string, sources ...string) string {
	t.Helper()
	out, err := FileAdd(w.env, sources, rename)
	if err != nil {
		t.Fatal(err)
	}
	return out
}

func cDoctorLinks(t *testing.T, w cWorld) string {
	t.Helper()
	return plain(Doctor(w.env, DoctorOptions{Links: true}))
}

func cLs(t *testing.T, w cWorld, options List) string {
	t.Helper()
	out, err := Ls(w.env, options)
	if err != nil {
		t.Fatal(err)
	}
	return out
}

// cLong is the long listing with the escapes stripped.
func cLong(t *testing.T, w cWorld) string {
	t.Helper()
	return plain(Ls(w.env, List{Long: true}))
}

// cLineWith is the first line containing text.
func cLineWith(t *testing.T, out, text string) string {
	t.Helper()
	for _, line := range strings.Split(out, "\n") {
		if strings.Contains(line, text) {
			return line
		}
	}
	t.Fatalf("no line with %q in:\n%s", text, out)
	return ""
}

// cLines is the lines of out, as Rust's `str::lines` splits them.
func cLines(out string) []string {
	if out == "" {
		return nil
	}
	return strings.Split(strings.TrimSuffix(out, "\n"), "\n")
}

func cEqual(t *testing.T, got, want []string, message string) {
	t.Helper()
	if strings.Join(got, "\x00") != strings.Join(want, "\x00") || len(got) != len(want) {
		t.Fatalf("%s: got %q, want %q", message, got, want)
	}
}

func cContains(t *testing.T, out, want, message string) {
	t.Helper()
	if !strings.Contains(out, want) {
		t.Fatalf("%s: %q not in %q", message, want, out)
	}
}

func cLacks(t *testing.T, out, unwanted, message string) {
	t.Helper()
	if strings.Contains(out, unwanted) {
		t.Fatalf("%s: %q is in %q", message, unwanted, out)
	}
}

func cErrorOf(t *testing.T, out string, err error) string {
	t.Helper()
	if err == nil {
		t.Fatalf("expected an error, got %q", out)
	}
	return err.Error()
}

// cSearch is a query as a shell hands it over: one token per word.
func cSearch(w cWorld, query string) (string, error) {
	return Search(w.env, strings.Fields(query))
}

func TestOnlyARealLinkCountsAsAReference(t *testing.T) {
	w := cInitialized(t)
	// In a fenced block: prose about a link, not a link.
	cAdd(t, w, "Alpha", "```\n![d](quoted.png)\n```\n")
	// Only at the bottom (a reference definition), which a search of the paragraph would miss.
	cAdd(t, w, "Beta", "see ![the diagram][d]\n\n[d]: referenced.png\n")
	cPlantFile(t, w, "quoted.png")
	cPlantFile(t, w, "referenced.png")

	out := cDoctorLinks(t, w)
	cContains(t, out, "quoted.png", "a link inside a fence references nothing")
	cLacks(t, out, "referenced.png", "a reference-style link is still a link")
}

// Outside the notebook or on another server: not a file this notebook can be missing.
func TestADestinationTheNotebookDoesNotOwnIsNeverBroken(t *testing.T) {
	w := cInitialized(t)
	cAdd(t, w, "Alpha", "[a](https://example.com/x.png) [b](#section) [c](mailto:me@example.com)\n")

	cContains(t, cDoctorLinks(t, w), "in order", "nothing here is broken")
}

func TestListingByTagDoesNotListTheNotebooksFiles(t *testing.T) {
	w := cInitialized(t)
	cAdd(t, w, "Alpha", "a\n", "work")
	cPlantFile(t, w, "receipt.pdf")

	cContains(t, cLs(t, w, List{}), "receipt.pdf", "untagged listing has the file")
	tagged := cLs(t, w, List{Tag: "work"})
	cLacks(t, tagged, "receipt.pdf", "tagged listing")
	cContains(t, tagged, "Alpha", "tagged listing")
}

// cListable is one tagged note and one file.
func cListable(t *testing.T) (w cWorld, summary string) {
	t.Helper()
	w = cInitialized(t)
	summary = cAdd(t, w, "Meeting Notes", "body\n", "work")
	cFileAdd(t, w, "", cSourceFile(t, w, "my diagram.png"))
	return w, summary
}

func TestLsJSONCarriesTheFilenameAsWellAsTheID(t *testing.T) {
	w, summary := cListable(t)
	id, slug := cParts(t, summary)

	out := cLs(t, w, List{Format: FormatJSON})
	if !strings.HasSuffix(out, "}\n") {
		t.Fatalf("one object, one line: %s", out)
	}
	cContains(t, out, `"id":"`+id+`"`, "id")
	cContains(t, out, `"slug":"`+slug+`"`, "slug")
	cContains(t, out, `"file":"`+id+"-"+slug+`.md"`, "the name a script needs next, not one it has to derive")
	cContains(t, out, `"title":"Meeting Notes"`, "title")
	cContains(t, out, `"tags":["work"]`, "tags")
	cContains(t, out, `"files":["my diagram.png"]`, "files")
	cContains(t, out, `"notebook":"default"`, "notebook")
}

// `--time` is about terminal width, and a program is not reading a terminal.
func TestLsJSONAlwaysCarriesTheTimes(t *testing.T) {
	w, _ := cListable(t)
	cWriteNote(t, w, "k3f9m2p1-undated.md", "---\ntitle: Undated\n---\n\nbody\n")

	out := cLs(t, w, List{Format: FormatJSON})

	if created := strings.Count(out, `"created":`); created != 2 {
		t.Fatalf("one per note, present or not: %d in %s", created, out)
	}
	cContains(t, out, `"created":null,"updated":null`, "a note with no times says so rather than dropping the keys")
	cContains(t, out, `"updated":"20`, "the dated note")
}

// The default row is narrow: the slug repeats the title, and two RFC 3339
// columns are forty characters.
func TestLsLongAddsTheSlugAndTheTimesAndSaysWhenThereAreNone(t *testing.T) {
	w := cInitialized(t)
	added := cAdd(t, w, "Alpha", "a\n")
	id, slug := cParts(t, added)
	cWriteNote(t, w, "k3f9m2p1-undated.md", "---\ntitle: Undated\n---\n\nbody\n")

	plainOut := plain(Ls(w.env, List{}))
	cLacks(t, plainOut, "Z", "no times by default")
	alpha := cLineWith(t, plainOut, "Alpha")
	cEqual(t, strings.Fields(alpha), []string{id, "Alpha"}, "the id and the title, and nothing that repeats the title: "+alpha)

	out := cLong(t, w)
	alpha = cLineWith(t, out, "Alpha")
	if columns := strings.Fields(alpha); columns[2] != slug {
		t.Fatalf("the slug comes back: %s", alpha)
	}
	if got := strings.Count(alpha, "Z"); got != 2 {
		t.Fatalf("created and updated: %s", alpha)
	}
	undated := cLineWith(t, out, "Undated")
	cEqual(t, strings.Fields(undated), []string{"k3f9m2p1", "Undated", "undated", "-", "-"},
		"a hole the eye would have to measure is filled in: "+undated)
}

// Padding goes outside the escapes: a cell measured with its escapes misaligns
// every column after it, and spaces before a reset escape `trim_end`.
func TestLsColoursTheColumnsWithoutMovingThem(t *testing.T) {
	w := cInitialized(t)
	_, short := cParts(t, cAdd(t, w, "Alpha", "a\n", "work"))
	_, longSlug := cParts(t, cAdd(t, w, "A Much Longer Title", "b\n"))
	// No times, so the last column is short on this row and full on the others —
	// the only way to get padding right of the rightmost cell.
	cWriteNote(t, w, "k3f9m2p1-undated.md", "---\ntitle: Undated\n---\n\nbody\n")

	out := cLs(t, w, List{Long: true})
	if !strings.Contains(out, "\x1b") {
		t.Fatalf("the listing is coloured: %q", out)
	}
	for _, line := range cLines(out) {
		if strings.HasSuffix(plain(line, nil), " ") {
			t.Fatalf("a row ends where its content ends: %q", line)
		}
	}

	stripped := plain(out, nil)
	slugAt := func(slug string) int {
		return strings.Index(cLineWith(t, stripped, slug), slug)
	}
	if slugAt(short) != slugAt(longSlug) {
		t.Fatalf("the slug column holds its place: %s", stripped)
	}
	if slugAt(short) != slugAt("undated") {
		t.Fatalf("%s", stripped)
	}
}

// Grey rather than `dim`, which a terminal may ignore. Read before `plain` strips it.
func TestLsGreysThePunctuationATagListIsWrittenWith(t *testing.T) {
	w := cInitialized(t)
	cAdd(t, w, "Alpha", "a\n", "work", "q3")

	out := cLs(t, w, List{})
	row := cLines(out)[0]
	const grey, cyan = "\x1b[90m", "\x1b[36m"
	for _, punctuation := range []string{"[", ", ", "]"} {
		cContains(t, row, grey+punctuation, punctuation+" is grey")
	}
	for _, tag := range []string{"work", "q3"} {
		cContains(t, row, cyan+tag, tag+" keeps the tag colour")
	}
	cLacks(t, row, "\x1b[2m", "nothing in the column leans on dim")
	cContains(t, plain(out, nil), "[work, q3]", "the tags")
}

// Emoji and CJK titles are wider than their char count; the slug column must
// still start at the same terminal column on every row.
func TestLsLongAlignsTheSlugColumnUnderEmojiAndCJKTitles(t *testing.T) {
	w := cInitialized(t)
	titles := []string{
		"plain-ascii",
		"✅ shipped",
		"⭐ starred",
		"\U0001f680 launch",
		"\U0001f468\u200d\U0001f469\u200d\U0001f467 family",
		"\U0001f44d\U0001f3fd approved",
		"會議筆記",
	}
	slugs := make([]string, len(titles))
	for i, title := range titles {
		_, slugs[i] = cParts(t, cAdd(t, w, title, "body\n"))
	}

	out := cLong(t, w)
	// The Rust ruler was unicode-width, with no use of DisplayWidth. No such
	// table is vendored here, so runewidth measures; the title column is 11
	// cells (`plain-ascii`) plus a 2-cell gutter, so the slug starts 13 cells in.
	ruler := runewidth.Condition{EastAsianWidth: false}
	starts := make([]int, len(titles))
	for i, title := range titles {
		row := cLineWith(t, out, title)
		from := strings.Index(row, title)
		after := from + len(title)
		// Past the title, so a slug that is a prefix of the title cannot match early.
		at := after + strings.Index(row[after:], slugs[i])
		starts[i] = ruler.StringWidth(row[from:at])
	}
	for _, start := range starts {
		if start != 13 {
			t.Fatalf("slug columns %v\n%s", starts, out)
		}
	}
}

// A script cutting the first two fields reads the same thing either way, and
// the one field a note may lack stays last in both.
func TestLsLongKeepsTheColumnsTheDefaultListingStartsWith(t *testing.T) {
	w := cInitialized(t)
	cAdd(t, w, "Alpha", "a\n", "work")
	cAdd(t, w, "Bravo", "b\n")

	head := func(long bool) []string {
		var heads []string
		for _, line := range cLines(plain(Ls(w.env, List{Long: long}))) {
			fields := strings.Fields(line)
			if len(fields) > 2 {
				fields = fields[:2]
			}
			heads = append(heads, strings.Join(fields, " "))
		}
		return heads
	}
	cEqual(t, head(false), head(true), "the id and the title, either way")

	long := cLong(t, w)
	alpha := cLineWith(t, long, "Alpha")
	if !strings.HasSuffix(strings.TrimRight(alpha, " \t"), "[work]") {
		t.Fatalf("tags stay at the end: %s", alpha)
	}
	bravo := cLineWith(t, long, "Bravo")
	if got := len(strings.Fields(bravo)); got != 5 {
		t.Fatalf("and a note without them ends one column earlier, not one column over: %s", bravo)
	}
}

// cTitles is the second column of each listed row.
func cTitles(t *testing.T, w cWorld, options List) []string {
	t.Helper()
	var titles []string
	for _, line := range cLines(cLs(t, w, options)) {
		if fields := strings.Fields(line); len(fields) > 1 {
			titles = append(titles, fields[1])
		}
	}
	return titles
}

// Parsed rather than compared as text: an imported note keeps its old offset.
func TestLsSortsByTimeAcrossTheOffsetsAnImportBrings(t *testing.T) {
	w := cInitialized(t)
	// 08:21Z, written as 16:21+08:00 — text order would put it last.
	for _, n := range []struct{ name, title, created string }{
		{"k3f9m2p1-middle.md", "Middle", "2019-03-14T16:21:00+08:00"},
		{"k3f9m2p2-oldest.md", "Oldest", "2019-03-14T07:00:00Z"},
		{"k3f9m2p3-newest.md", "Newest", "2019-03-14T09:00:00Z"},
	} {
		cWriteNote(t, w, n.name, "---\ntitle: "+n.title+"\ncreated: "+n.created+"\n---\n\nbody\n")
	}
	cWriteNote(t, w, "k3f9m2p4-undated.md", "---\ntitle: Undated\n---\n\nbody\n")

	cEqual(t, cTitles(t, w, List{Sort: SortCreated}), []string{"Newest", "Middle", "Oldest", "Undated"},
		"newest first, and a note with no time to sort by sorts last")
	cEqual(t, cTitles(t, w, List{Sort: SortTitle}), []string{"Middle", "Newest", "Oldest", "Undated"}, "by title")
	cEqual(t, cTitles(t, w, List{Sort: SortSlug}), []string{"Middle", "Newest", "Oldest", "Undated"},
		"by slug, which is what the walk already produced")
}

// Applied after the sort, so it needs no `--sort`.
func TestLsReverseTurnsWhicheverOrderWasAskedFor(t *testing.T) {
	w := cInitialized(t)
	for _, n := range []struct{ name, title, created string }{
		{"k3f9m2p1-alpha.md", "Alpha", "2019-03-14T07:00:00Z"},
		{"k3f9m2p2-bravo.md", "Bravo", "2019-03-14T09:00:00Z"},
	} {
		cWriteNote(t, w, n.name, "---\ntitle: "+n.title+"\ncreated: "+n.created+"\n---\n\nbody\n")
	}
	cWriteNote(t, w, "k3f9m2p3-undated.md", "---\ntitle: Undated\n---\n\nbody\n")

	cEqual(t, cTitles(t, w, List{Sort: SortCreated, Reverse: true}), []string{"Undated", "Alpha", "Bravo"},
		"oldest first, and the note with no time to sort by now leads")
	cEqual(t, cTitles(t, w, List{Sort: SortTitle, Reverse: true}), []string{"Undated", "Bravo", "Alpha"}, "Z to A")
	cEqual(t, cTitles(t, w, List{Sort: SortSlug, Reverse: true}), []string{"Undated", "Bravo", "Alpha"},
		"the default order turns too: that is what asking for it alone means")
	cEqual(t, cTitles(t, w, List{Sort: SortCreated}), []string{"Bravo", "Alpha", "Undated"},
		"and without the flag nothing moved")
}

// One listing, one order: notes and files reverse together.
func TestLsReverseTurnsTheFilesWithTheNotes(t *testing.T) {
	w := cInitialized(t)
	cAdd(t, w, "Alpha", "a\n")
	for _, name := range []string{"one.txt", "two.txt", "three.txt"} {
		path := filepath.Join(w.root, name)
		if err := os.WriteFile(path, []byte("x"), 0o666); err != nil {
			t.Fatal(err)
		}
		cFileAdd(t, w, "", path)
	}

	files := func(reverse bool) []string {
		var names []string
		for _, line := range cLines(plain(Ls(w.env, List{Only: OnlyFiles, Reverse: reverse}))) {
			line = strings.TrimSpace(line)
			if line != "" && line != "files" {
				names = append(names, line)
			}
		}
		return names
	}

	cEqual(t, files(false), []string{"one.txt", "three.txt", "two.txt"}, "forward")
	cEqual(t, files(true), []string{"two.txt", "three.txt", "one.txt"}, "reversed")
}

func TestLsJSONEscapesWhatWouldOtherwiseBreakTheDocument(t *testing.T) {
	w := cInitialized(t)
	cAdd(t, w, `He said "hi" \ bye`, "x\n")

	out := cLs(t, w, List{Format: FormatJSON})
	cContains(t, out, `"title":"He said \"hi\" \\ bye"`, "escaped title")
}

func TestLsJSONSaysSoWhenTheNotebookIsEmpty(t *testing.T) {
	w := cInitialized(t)
	out := cLs(t, w, List{Format: FormatJSON})
	if got := strings.TrimRight(out, "\n"); got != `{"notebook":"default","notes":[],"files":[]}` {
		t.Fatalf("an empty listing is still a document a program can parse: %s", out)
	}
}

// A note by its id and a file by its name, as the next command expects.
func TestLsQuietPrintsOneIdentifierPerRecord(t *testing.T) {
	w, summary := cListable(t)
	id, _ := cParts(t, summary)

	if out := cLs(t, w, List{Format: FormatQuiet}); out != id+"\nmy diagram.png\n" {
		t.Fatalf("got %q", out)
	}
}

// `noda file add` allows spaces in names, so newline-separated output is not
// safe for `xargs`.
func TestLsQuietCanSeparateWithNul(t *testing.T) {
	w, summary := cListable(t)
	id, _ := cParts(t, summary)

	if out := cLs(t, w, List{Format: FormatQuiet, Null: true}); out != id+"\x00my diagram.png\x00" {
		t.Fatalf("got %q", out)
	}
}

func TestLsCanLeaveOutEitherHalf(t *testing.T) {
	w, summary := cListable(t)
	id, _ := cParts(t, summary)

	if notes := cLs(t, w, List{Format: FormatQuiet, Only: OnlyNotes}); notes != id+"\n" {
		t.Fatalf("got %q", notes)
	}
	if files := cLs(t, w, List{Format: FormatQuiet, Only: OnlyFiles}); files != "my diagram.png\n" {
		t.Fatalf("got %q", files)
	}

	// The same subsetting in the other two formats.
	table := plain(Ls(w.env, List{Only: OnlyFiles}))
	cContains(t, table, "my diagram.png", "table")
	cLacks(t, table, "meeting-notes", "table")
}

// The Rust test ran the real binary, since `-0` is a promise about the bytes
// that leave the process. Here it is the Ls half of that promise: cmd/noda's
// tests cover the printing, and colour handling there keeps NUL.
func TestLsNullSeparatorsSurviveTheWayOutOfTheProcess(t *testing.T) {
	w, _ := cListable(t)

	out := cLs(t, w, List{Format: FormatQuiet, Null: true, Only: OnlyFiles})
	if out != "my diagram.png\x00" {
		t.Fatalf("stdout was %q", out)
	}
}

func TestReadmeWritesTheFrontPageAndCommitsIt(t *testing.T) {
	w := cInitialized(t)
	commits := cCommitCount(t, w)

	out := plain(Readme(w.env, false))
	cContains(t, out, "wrote README.md", "readme")
	cContains(t, out, config.DefaultNotebook, "and says where")

	written := cReadFile(t, filepath.Join(w.notebook, "README.md"))
	if !strings.HasPrefix(written, "# "+config.DefaultNotebook+"\n") {
		t.Fatalf("the notebook names itself: %s", written)
	}
	cContains(t, written, "<id>-<slug>.md", "and explains the filenames, which is what a stranger asks first")
	cContains(t, written, "noda use "+config.DefaultNotebook, "and how to work on it")
	if got := cCommitCount(t, w); got != commits+1 {
		t.Fatalf("one commit, revertible like every other change: %d -> %d", commits, got)
	}
}

func TestReadmeSaysNothingThatTheNextNoteWouldFalsify(t *testing.T) {
	w := cInitialized(t)
	cAdd(t, w, "Alpha", "body\n")
	plain(Readme(w.env, false))

	written := cReadFile(t, filepath.Join(w.notebook, "README.md"))
	cLacks(t, written, "Alpha", "no index of the notes: it would be stale from the next `noda add` onward, and `noda ls` is that list")
}

func TestReadmeWillNotOverwriteProseSomeoneWrote(t *testing.T) {
	w := cInitialized(t)
	plain(Readme(w.env, false))
	cWriteNote(t, w, "README.md", "# hand-written\n")
	commits := cCommitCount(t, w)

	out, err := Readme(w.env, false)
	message := cErrorOf(t, out, err)
	cContains(t, message, "already exists", "readme refusal")
	cContains(t, message, "--force", "and says how to get past it")
	if got := cReadFile(t, filepath.Join(w.notebook, "README.md")); got != "# hand-written\n" {
		t.Fatalf("the one already there is untouched: %q", got)
	}
	if got := cCommitCount(t, w); got != commits {
		t.Fatalf("and nothing was committed: %d -> %d", commits, got)
	}
}

func TestReadmeForceReplacesItInACommit(t *testing.T) {
	w := cInitialized(t)
	cWriteNote(t, w, "README.md", "# hand-written\n")
	commits := cCommitCount(t, w)

	out := plain(Readme(w.env, true))
	cContains(t, out, "rewrote README.md", "force")
	cContains(t, cReadFile(t, filepath.Join(w.notebook, "README.md")), "A [noda]", "the template replaced it")
	if got := cCommitCount(t, w); got != commits+1 {
		t.Fatalf("so `git revert` brings the old one back: %d -> %d", commits, got)
	}
}

func TestReadmeIsAFileRatherThanANoteAndNeverAnOrphan(t *testing.T) {
	w := cInitialized(t)
	cAdd(t, w, "Alpha", "body\n")
	plain(Readme(w.env, false))

	listed := plain(Ls(w.env, List{}))
	cContains(t, listed, "files\n  README.md", "it is one of the notebook's files, not one of its notes")

	out := cDoctorLinks(t, w)
	cLacks(t, out, "README.md", "and never reported as unlinked — the only way to clear that would be to link the front page from a note, which reads backwards")
}

func TestFileAddCopiesItInAndCommitsIt(t *testing.T) {
	w := cInitialized(t)
	source := cSourceFile(t, w, "diagram.png")
	commits := cCommitCount(t, w)

	out := plain(FileAdd(w.env, []string{source}, ""))
	if strings.TrimRight(out, "\n") != "added  diagram.png" {
		t.Fatalf("got %q", out)
	}
	if !cExists(filepath.Join(w.notebook, "diagram.png")) {
		t.Fatal("not copied in")
	}
	if !cExists(source) {
		t.Fatal("a copy, so the original stays put")
	}
	if got := cCommitCount(t, w); got != commits+1 {
		t.Fatalf("one commit, revertible like every other change: %d -> %d", commits, got)
	}
	cContains(t, plain(Ls(w.env, List{})), "files\n  diagram.png", "and it is listed")
}

func TestFileAddTakesSeveralAtOnceInOneCommit(t *testing.T) {
	w := cInitialized(t)
	sources := []string{cSourceFile(t, w, "a.png"), cSourceFile(t, w, "b.pdf")}
	commits := cCommitCount(t, w)

	out := plain(FileAdd(w.env, sources, ""))
	cContains(t, out, "added  a.png", "first")
	cContains(t, out, "added  b.pdf", "second")
	if got := cCommitCount(t, w); got != commits+1 {
		t.Fatalf("one commit, not two: %d -> %d", commits, got)
	}
}

func TestFileAddWillNotOverwriteWhatTheNotebookAlreadyHolds(t *testing.T) {
	w := cInitialized(t)
	source := cSourceFile(t, w, "diagram.png")
	cFileAdd(t, w, "", source)
	cWriteNote(t, w, "diagram.png", "the one already here\n")

	out, err := FileAdd(w.env, []string{source}, "")
	message := cErrorOf(t, out, err)
	cContains(t, message, "already holds diagram.png", "refusal")
	cContains(t, message, "--as", "and says how to get past it")
	if got := cReadFile(t, filepath.Join(w.notebook, "diagram.png")); got != "the one already here\n" {
		t.Fatalf("untouched: %q", got)
	}

	cFileAdd(t, w, "diagram-2.png", source)
	if !cExists(filepath.Join(w.notebook, "diagram-2.png")) {
		t.Fatal("the renamed copy is missing")
	}
}

// Every source is checked before anything is copied.
func TestFileAddCopiesNothingWhenOneOfThemCannotBeAdded(t *testing.T) {
	w := cInitialized(t)
	good := cSourceFile(t, w, "good.png")
	missing := filepath.Join(w.root, "elsewhere", "not-here.png")
	commits := cCommitCount(t, w)

	if out, err := FileAdd(w.env, []string{good, missing}, ""); err == nil {
		t.Fatalf("expected an error, got %q", out)
	}
	if cExists(filepath.Join(w.notebook, "good.png")) {
		t.Fatal("the one that could have been copied was not")
	}
	if got := cCommitCount(t, w); got != commits {
		t.Fatalf("committed: %d -> %d", commits, got)
	}
}

func TestFileAddRefusesTheNamesItCouldNotThenAccountFor(t *testing.T) {
	w := cInitialized(t)
	source := cSourceFile(t, w, "diagram.png")

	for _, c := range []struct{ rename, expected string }{
		{".hidden.png", "dotfiles"},
		{"sub/x.png", "cannot be a path"},
		// An id-and-slug `*.md` name reads as a note that lost its frontmatter.
		{"abcdefgh-hello.md", "claims a note's id"},
	} {
		out, err := FileAdd(w.env, []string{source}, c.rename)
		cContains(t, cErrorOf(t, out, err), c.expected, c.rename)
	}

	out, err := FileAdd(w.env, []string{source, source}, "x.png")
	cContains(t, cErrorOf(t, out, err), "cannot be given with several", "several")
}

// Links point at an attachment's name, so it is refused rather than cut like a
// slug — in noda's words, not an errno from the copy.
func TestFileAddRefusesANameTooLongToBeAFilename(t *testing.T) {
	w := cInitialized(t)
	source := cSourceFile(t, w, "diagram.png")
	cFileAdd(t, w, "", source)
	commits := cCommitCount(t, w)
	tooLong := strings.Repeat("a", 252) + ".png"
	if len(tooLong) != 256 {
		t.Fatal("the name is meant to be 256 bytes")
	}

	out, err := FileAdd(w.env, []string{source}, tooLong)
	message := cErrorOf(t, out, err)
	cContains(t, message, "has to fit in 255 bytes", "add")
	cContains(t, message, "is 256", "it says how far over")

	// The other command that writes a name.
	out, err = FileMv(w.env, "diagram.png", tooLong, false)
	cContains(t, cErrorOf(t, out, err), "has to fit in 255 bytes", "mv")

	if !cExists(filepath.Join(w.notebook, "diagram.png")) {
		t.Fatal("left where it was")
	}
	if got := cCommitCount(t, w); got != commits {
		t.Fatalf("and nothing committed: %d -> %d", commits, got)
	}
}

func TestFileAddRefusesADirectory(t *testing.T) {
	w := cInitialized(t)
	cSourceFile(t, w, "inside.png")

	out, err := FileAdd(w.env, []string{filepath.Join(w.root, "elsewhere")}, "")
	cContains(t, cErrorOf(t, out, err), "not a file", "directory")
}

func TestFileRmRemovesItAsACommit(t *testing.T) {
	w := cInitialized(t)
	cFileAdd(t, w, "", cSourceFile(t, w, "diagram.png"))
	commits := cCommitCount(t, w)

	out := plain(FileRm(w.env, "diagram.png"))
	if strings.TrimRight(out, "\n") != "removed  diagram.png" {
		t.Fatalf("got %q", out)
	}
	if cExists(filepath.Join(w.notebook, "diagram.png")) {
		t.Fatal("still there")
	}
	if got := cCommitCount(t, w); got != commits+1 {
		t.Fatalf("%d -> %d", commits, got)
	}
}

// A note has an identity to lose, so removing one is a different command.
func TestFileRmRefusesANoteAndSaysWhichCommandWantsIt(t *testing.T) {
	w := cInitialized(t)
	file := cNoteFile(t, cAdd(t, w, "Alpha", "a\n"))

	out, err := FileRm(w.env, file)
	message := cErrorOf(t, out, err)
	cContains(t, message, "is a note", "refusal")
	cContains(t, message, "`noda rm`", "refusal")
	if !cExists(filepath.Join(w.notebook, file)) {
		t.Fatal("and the note is still there")
	}
}

func TestFileRmSaysSoWhenThereIsNoSuchFile(t *testing.T) {
	w := cInitialized(t)
	out, err := FileRm(w.env, "nope.txt")
	cContains(t, cErrorOf(t, out, err), "no file called nope.txt", "refusal")
}

// The default: rename, then report the links that now point at nothing.
func TestFileMvRenamesAndReportsTheLinksItStranded(t *testing.T) {
	w := cInitialized(t)
	cFileAdd(t, w, "", cSourceFile(t, w, "old.png"))
	cAdd(t, w, "Alpha", "![a](old.png)\n")
	commits := cCommitCount(t, w)

	out := plain(FileMv(w.env, "old.png", "new.png", false))
	cContains(t, out, "renamed  old.png -> new.png", "mv")
	cContains(t, out, "1 note links to old.png", "mv")
	cContains(t, out, "alpha.md", "and says which")
	if !cExists(filepath.Join(w.notebook, "new.png")) || cExists(filepath.Join(w.notebook, "old.png")) {
		t.Fatal("the file did not move")
	}
	if got := cCommitCount(t, w); got != commits+1 {
		t.Fatalf("%d -> %d", commits, got)
	}
	cLacks(t, out, "updated", "the notes were not touched")
}

// A mechanical fixup; dating every note today would flatten their order.
func TestRenamingAFileDoesNotDateTheNotesThatLinkedToIt(t *testing.T) {
	w := cInitialized(t)
	cFileAdd(t, w, "", cSourceFile(t, w, "old.png"))
	cAdd(t, w, "Alpha", "![a](old.png)\n")
	createdBefore, updatedBefore := cTimes(t, w, "alpha")

	if _, err := FileMv(w.env, "old.png", "new.png", true); err != nil {
		t.Fatal(err)
	}

	cContains(t, cNoteText(t, w, "alpha"), "![a](new.png)", "the link was rewritten")
	created, updated := cTimes(t, w, "alpha")
	if *created != *createdBefore || *updated != *updatedBefore {
		t.Fatalf("the note was not edited: %v %v -> %v %v", *createdBefore, *updatedBefore, *created, *updated)
	}
}

// Opt-in, because it edits notes the command was not pointed at.
func TestFileMvUpdateLinksRewritesBothSpellingsAndLeavesItInOrder(t *testing.T) {
	w := cInitialized(t)
	cFileAdd(t, w, "", cSourceFile(t, w, "old.png"))
	first := cAdd(t, w, "Alpha", "Inline ![a](old.png) and a reference ![b][r].\n\n[r]: old.png\n")
	second := cAdd(t, w, "Beta", "![c](old.png#page=2)\n")
	commits := cCommitCount(t, w)

	out := plain(FileMv(w.env, "old.png", "new.png", true))
	cContains(t, out, "updated  2 notes", "mv")
	cLacks(t, out, "still link", "nothing was missed")

	alpha := cReadFile(t, filepath.Join(w.notebook, cNoteFile(t, first)))
	cContains(t, alpha, "![a](new.png)", "alpha inline")
	cContains(t, alpha, "[r]: new.png", "alpha reference")
	if !strings.HasPrefix(alpha, "---\ntitle: Alpha\n") {
		t.Fatalf("frontmatter kept: %s", alpha)
	}
	beta := cReadFile(t, filepath.Join(w.notebook, cNoteFile(t, second)))
	cContains(t, beta, "![c](new.png#page=2)", "the fragment says how to open it, not which file")

	cContains(t, cDoctorLinks(t, w), "in order", "no orphan and no broken link is left behind")
	if got := cCommitCount(t, w); got != commits+1 {
		t.Fatalf("the rename and the rewrites are one commit: %d -> %d", commits, got)
	}
}

// A backslash-escaped destination is not in the source literally, so it is
// reported rather than assumed fixed.
func TestFileMvSaysWhichNotesItCouldNotRewrite(t *testing.T) {
	w := cInitialized(t)
	cFileAdd(t, w, "", cSourceFile(t, w, "my(file).png"))
	cAdd(t, w, "Alpha", "[a](my\\(file\\).png)\n")

	out := plain(FileMv(w.env, "my(file).png", "new.png", true))
	cContains(t, out, "renamed", "mv")
	cContains(t, out, "1 note still links to my(file).png", "reported, not silently left")
}

func TestFileMvRefusesWhatItShouldNotRename(t *testing.T) {
	w := cInitialized(t)
	source := cSourceFile(t, w, "a.png")
	cFileAdd(t, w, "", source)
	cFileAdd(t, w, "b.png", source)
	summary := cAdd(t, w, "Alpha", "x\n")

	for _, c := range []struct{ old, new, expected string }{
		{"a.png", "b.png", "already holds b.png"},
		{"a.png", "a.png", "already is its name"},
		{"a.png", "abcdefgh-hello.md", "claims a note's id"},
		{"nope.png", "x.png", "no file called nope.png"},
	} {
		out, err := FileMv(w.env, c.old, c.new, false)
		cContains(t, cErrorOf(t, out, err), c.expected, c.old+" -> "+c.new)
	}

	out, err := FileMv(w.env, cNoteFile(t, summary), "x.png", false)
	message := cErrorOf(t, out, err)
	cContains(t, message, "is a note", "note refusal")
	cContains(t, message, "`noda mv`", "note refusal")
}

func TestPathPrintsTheNotebookANoteAndAFile(t *testing.T) {
	w := cInitialized(t)
	summary := cAdd(t, w, "Meeting Notes", "x\n")
	id, _ := cParts(t, summary)
	cFileAdd(t, w, "", cSourceFile(t, w, "diagram.png"))
	notePath := filepath.Join(w.notebook, cNoteFile(t, summary))

	for _, c := range []struct{ key, want, why string }{
		{"", w.notebook, "no argument is the notebook itself"},
		{"meeting-notes", notePath, "slug"},
		{id[:4], notePath, "an id prefix addresses a note here like everywhere else"},
		{"diagram.png", filepath.Join(w.notebook, "diagram.png"), "file"},
	} {
		out, err := Path(w.env, c.key)
		if err != nil || strings.TrimRight(out, "\n") != c.want {
			t.Fatalf("%s: got %q (%v), want %q", c.why, out, err, c.want)
		}
	}
}

func TestPathSaysSoWhenNothingAnswersToTheKey(t *testing.T) {
	w := cInitialized(t)
	out, err := Path(w.env, "nope")
	cContains(t, cErrorOf(t, out, err), "no note and no file", "refusal")
}

// noda never guesses between a note's slug and a file of the same name.
func TestPathRefusesAKeyThatNamesBothANoteAndAFile(t *testing.T) {
	w := cInitialized(t)
	cAdd(t, w, "Diagram", "x\n")
	cFileAdd(t, w, "", cSourceFile(t, w, "diagram"))

	out, err := Path(w.env, "diagram")
	message := cErrorOf(t, out, err)
	cContains(t, message, "names both a note and a file", "refusal")
	cContains(t, message, "diagram.md", "and lists them")
}

func TestSearchMatchesTheBodyTheTitleAndTheTags(t *testing.T) {
	w := cInitialized(t)
	cAdd(t, w, "Meeting Notes", "discuss the Q3 budget\nand the hiring plan\n", "work")
	cAdd(t, w, "Reading Log", "a book about budgets\n")
	search := func(query string) string {
		t.Helper()
		return plain(cSearch(w, query))
	}

	// A body hit quotes its line.
	out := search("Q3 BUDGET")
	lines := cLines(out)
	if len(lines) != 2 {
		t.Fatalf("one result and its excerpt: %s", out)
	}
	cContains(t, lines[0], "Meeting Notes", "result")
	cContains(t, lines[1], "discuss the Q3 budget", "excerpt")

	// A title or tag hit needs no excerpt.
	out = search("work")
	if len(cLines(out)) != 1 {
		t.Fatalf("%s", out)
	}
	cContains(t, out, "[work]", "tag hit")
	if got := cLines(search("reading")); len(got) != 1 {
		t.Fatalf("title hit: %q", got)
	}

	// Substring, not whole word: "budget" finds "budgets".
	out = search("budget")
	if len(cLines(out)) != 4 {
		t.Fatalf("%s", out)
	}
	cContains(t, out, "a book about budgets", "substring")
	if absent, err := cSearch(w, "absent"); err != nil || absent != "" {
		t.Fatalf("got %q, %v", absent, err)
	}
}

func TestSearchRequiresEveryTermButNotTheirOrder(t *testing.T) {
	w := cInitialized(t)
	cAdd(t, w, "Alpha", "budget for the offsite\n")
	cAdd(t, w, "Beta", "budget only\n")

	out := plain(cSearch(w, "offsite budget"))
	cContains(t, out, "Alpha", "both terms")
	cLacks(t, out, "beta", "both terms are required")

	if out, err := cSearch(w, "   "); err == nil {
		t.Fatalf("a query is required, got %q", out)
	}
}

func TestSearchWorksOnALanguageWithoutSpaces(t *testing.T) {
	w := cInitialized(t)
	cAdd(t, w, "會議記錄", "討論第三季預算與人力計畫\n")
	cAdd(t, w, "Reading Log", "unrelated\n")

	// No spaces to tokenise on, so substring matching is the point.
	out := plain(cSearch(w, "第三季預算"))
	if len(cLines(out)) != 2 {
		t.Fatalf("%s", out)
	}
	cContains(t, out, "討論第三季預算與人力計畫", "excerpt")
	cContains(t, plain(cSearch(w, "會議")), "會議記錄", "title")
}

func TestSearchOnlyLooksAtTheNoteNotTheFileAroundIt(t *testing.T) {
	w := cInitialized(t)
	added := cAdd(t, w, "Alpha", "body\n")
	id, _, _ := strings.Cut(added, "  ")

	// The frontmatter is not searchable. `text:---` because a leading `-` negates.
	if out, err := cSearch(w, "text:---"); err != nil || out != "" {
		t.Fatalf("got %q, %v", out, err)
	}
	if out, err := cSearch(w, "title:"); err == nil {
		t.Fatalf("a field needs a value, got %q", out)
	}
	// The id is the filename, not text in the note; `id:` asks for it.
	if out, err := cSearch(w, id); err != nil || out != "" {
		t.Fatalf("got %q, %v", out, err)
	}
	found, err := cSearch(w, "id:"+id[:4])
	if err != nil {
		t.Fatal(err)
	}
	cContains(t, found, "Alpha", "id: search")
}
