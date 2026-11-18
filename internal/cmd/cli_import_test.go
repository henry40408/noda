package cmd

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/henry40408/noda/internal/note"
)

// iExport writes a small export to a file, as a browser's "export all" would.
func iExport(t *testing.T, dir, name, tiddlers string) string {
	t.Helper()
	path := filepath.Join(dir, name)
	if err := os.WriteFile(path, []byte(tiddlers), 0o666); err != nil {
		t.Fatal(err)
	}
	return path
}

// iExportInHome is an export outside the notebook, in the test's own root.
func iExportInHome(t *testing.T, e Env, name, tiddlers string) string {
	t.Helper()
	return iExport(t, filepath.Dir(e.Paths.ConfigDir()), name, tiddlers)
}

func iImport(t *testing.T, e Env, convert bool, files ...string) string {
	t.Helper()
	return plain(ImportTiddlywiki(e, files, convert))
}

// iNoteFiles is the files in the notebook whose names say they are notes.
func iNoteFiles(t *testing.T, dir string) []string {
	t.Helper()
	entries, err := os.ReadDir(dir)
	if err != nil {
		t.Fatal(err)
	}
	var names []string
	for _, entry := range entries {
		if note.NamesANote(entry.Name()) {
			names = append(names, entry.Name())
		}
	}
	return names
}

const iTiddlers = `[
  {"title":"Meeting Notes","text":"See [[Reading Log]] and ''this''.\n","tags":"work [[two words]]",
   "created":"20190314082100000","modified":"20241102164012123","creator":"henry",
   "type":"text/vnd.tiddlywiki"},
  {"title":"Reading Log","text":"A {{Transclusion}} nobody can translate.\n","tags":"",
   "created":"20200101000000000","modified":"20200101000000000"},
  {"title":"$:/config/Something","text":"not a note"},
  {"title":"A Picture","text":"aGk=","type":"image/png"}
]`

// Whatever the conversion did, the export's text is in history, one command away.
func TestImportWritesTheOriginalsFirstAndTheConversionSecond(t *testing.T) {
	e, dir := aInit(t)
	file := iExportInHome(t, e, "wiki.json", iTiddlers)
	before := aCommitCount(t, e, dir)

	out := iImport(t, e, true, file)
	aContains(t, out, "imported  2 notes from tiddlywiki")
	aEqual(t, aCommitCount(t, e, dir), before+2, "one commit for the notes as written, one for the conversion")

	aContains(t, aNoteText(t, e, "meeting-notes"), "**this**")
	output, err := Restore(e, "meeting-notes", "HEAD~1", TouchKeep)
	original := aOK(t, output, err)
	aLacks(t, original, "(no change)")
	aContains(t, aNoteText(t, e, "meeting-notes"), "''this''")
}

func TestImportCarriesTheTimesAndFieldsTheWikiHad(t *testing.T) {
	e, _ := aInit(t)
	iImport(t, e, true, iExportInHome(t, e, "wiki.json", iTiddlers))

	text := aNoteText(t, e, "meeting-notes")
	aContains(t, text, "created: 2019-03-14T08:21:00.000Z")
	aContains(t, text, "updated: 2024-11-02T16:40:12.123Z")
	aContains(t, text, "creator: henry")
	aContains(t, text, "source_key: Meeting Notes")
	aEqual(t, aStampText(aTimes(t, e, "meeting-notes").created), "2019-03-14T08:21:00.000Z", "noda reads back what the wiki wrote")
	// A title list keeps the spaces inside its double brackets.
	aContains(t, plain(Ls(e, List{Tag: "two words"})), "Meeting Notes")
}

func TestImportKeepsAMultiLineFieldWithoutItReadingAsAnother(t *testing.T) {
	e, _ := aInit(t)
	iImport(t, e, true, iExportInHome(t, e, "wiki.json", `[{"title":"Kept","text":"body","caption":"one\ntitle: two"}]`))

	aContains(t, aNoteText(t, e, "kept"), `caption: "one\ntitle: two"`)
	out := plain(Ls(e, List{}))
	aContains(t, out, "Kept")
	aLacks(t, out, "two")
}

// A title too long for a filename used to abort the whole import, leaving pass
// one's notes uncommitted.
func TestImportTakesATiddlerWhoseTitleIsTooLongForAFilename(t *testing.T) {
	e, dir := aInit(t)
	title := "GitHub - coding-horror/basic-computer-games: An updated version of the " +
		"classic Basic Computer Games book, with well-written examples in a variety of " +
		"common MEMORY SAFE, SCRIPTING programming languages. See " +
		"https://coding-horror.github.io/basic-computer-games/"
	file := iExportInHome(t, e, "wiki.json", fmt.Sprintf(`[{"title":%q,"text":"body\n"}]`, title))

	aContains(t, iImport(t, e, true, file), "imported  1 note from tiddlywiki")

	names := iNoteFiles(t, dir)
	aEqual(t, len(names), 1, fmt.Sprint(names))
	if len(names[0]) > 255 {
		t.Fatalf("%d bytes, %s", len(names[0]), names[0])
	}
	text, err := os.ReadFile(filepath.Join(dir, names[0]))
	if err != nil {
		t.Fatal(err)
	}
	aContains(t, string(text), "title: "+title)
}

// A refused write used to end the run, leaving everything so far uncommitted.
// It is now a per-note reason, like a title `check` rejected.
func TestImportReportsANoteItCouldNotWriteInsteadOfEndingTheRun(t *testing.T) {
	e, dir := aInit(t)
	file := iExportInHome(t, e, "wiki.json", iTiddlers)
	commits := aCommitCount(t, e, dir)

	// A single write cannot be made to fail (the filename has a minted id, so no
	// collision can be planted), so every write fails: the directory is read-only.
	if err := os.Chmod(dir, 0o555); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = os.Chmod(dir, 0o755) })
	if err := os.WriteFile(filepath.Join(dir, "probe"), []byte("x"), 0o666); err == nil {
		// Running as root, or on a filesystem that ignores the mode.
		t.Skip("the directory stayed writable")
	}

	out := iImport(t, e, true, file)

	aContains(t, out, "imported  0 notes from tiddlywiki")
	aContains(t, out, "not imported:")
	aContains(t, out, "2 Permission denied")
	aEqual(t, aCommitCount(t, e, dir), commits, "nothing committed")
	aEqual(t, len(iNoteFiles(t, dir)), 0, "and nothing left in the working tree")
}

// The target file is not named until its id is minted, so links are rewritten
// once every note exists: the second pass.
func TestImportPointsLinksAtTheFilesTheNotesBecame(t *testing.T) {
	e, _ := aInit(t)
	iImport(t, e, true, iExportInHome(t, e, "wiki.json", iTiddlers))

	output, err := Path(e, "reading-log")
	reading := aOK(t, output, err)
	name := filepath.Base(strings.TrimSpace(reading))
	aContains(t, aNoteText(t, e, "meeting-notes"), fmt.Sprintf("[Reading Log](%s)", name))
	aContains(t, plain(Backlinks(e, "reading-log", FormatTable)), "Meeting Notes")
}

func TestWhatIsNotANoteIsReportedRatherThanImported(t *testing.T) {
	e, _ := aInit(t)
	out := iImport(t, e, true, iExportInHome(t, e, "wiki.json", iTiddlers))
	aContains(t, out, "1 system tiddler")
	aContains(t, out, "1 not text (image/png)")
}

// `--no-convert` is the first pass only.
func TestImportCanLeaveTheWikitextAsItStands(t *testing.T) {
	e, dir := aInit(t)
	file := iExportInHome(t, e, "wiki.json", iTiddlers)
	before := aCommitCount(t, e, dir)

	iImport(t, e, false, file)

	aEqual(t, aCommitCount(t, e, dir), before+1, "one commit, no conversion")
	aContains(t, aNoteText(t, e, "meeting-notes"), "''this''")
}

func TestASecondImportOfTheSameExportChangesNothing(t *testing.T) {
	e, dir := aInit(t)
	file := iExportInHome(t, e, "wiki.json", iTiddlers)
	iImport(t, e, true, file)
	afterFirst := aCommitCount(t, e, dir)

	out := iImport(t, e, true, file)

	aContains(t, out, "imported  0 notes")
	aContains(t, out, "already imported as")
	aEqual(t, aCommitCount(t, e, dir), afterFirst, "and nothing committed")
}

// A frontmatter field rather than a tag, because tags belong to whoever writes
// the notes; `doctor` makes the field findable.
func TestDoctorReportsTheNotesAnImportCouldNotFinish(t *testing.T) {
	e, _ := aInit(t)
	iImport(t, e, true, iExportInHome(t, e, "wiki.json", iTiddlers))

	aContains(t, aNoteText(t, e, "reading-log"), "unconverted: transclusion")
	out := plain(Doctor(e, DoctorOptions{}))
	aContains(t, out, "1 note carries text an importer did not convert")
	aContains(t, out, "1 note transclusion")
}

func TestAFileThatIsNotAnExportIsRefusedByName(t *testing.T) {
	e, _ := aInit(t)
	file := iExportInHome(t, e, "notes.md", "# just some markdown\n")
	_, err := ImportTiddlywiki(e, []string{file}, true)
	aContains(t, aFail(t, "", err), "no tiddler store")
}

// Links run between the pieces of a split export, so several files are one
// import.
func TestSeveralExportsAreReadAsOneImport(t *testing.T) {
	e, _ := aInit(t)
	first := iExportInHome(t, e, "one.json", `[{"title":"Alpha","text":"points at [[Beta]]\n"}]`)
	second := iExportInHome(t, e, "two.json", `[{"title":"Beta","text":"and back at [[Alpha]]\n"}]`)

	aContains(t, iImport(t, e, true, first, second), "imported  2 notes")

	alpha := aNoteText(t, e, "alpha")
	if !strings.Contains(alpha, "](") || strings.Contains(alpha, "[[Beta]]") {
		t.Fatalf("a link across the two files resolves: %s", alpha)
	}
	aContains(t, plain(Backlinks(e, "alpha", FormatTable)), "Beta")
}

// Overlapping pieces: the first copy lands and the second is reported.
func TestANoteGivenTwiceInOneImportArrivesOnce(t *testing.T) {
	e, _ := aInit(t)
	first := iExportInHome(t, e, "one.json", `[{"title":"Alpha","text":"first\n"}]`)
	second := iExportInHome(t, e, "two.json", `[{"title":"Alpha","text":"again\n"}]`)

	out := iImport(t, e, true, first, second)

	aContains(t, out, "imported  1 note")
	aContains(t, out, "given twice in this import")
	aContains(t, aNoteText(t, e, "alpha"), "first")
}

// The resolver starts from what the notebook already holds, so a wiki can be
// imported over several sittings.
func TestALaterImportLinksToWhatAnEarlierOneBrought(t *testing.T) {
	e, _ := aInit(t)
	iImport(t, e, true, iExportInHome(t, e, "one.json", `[{"title":"Alpha","text":"a\n"}]`))

	iImport(t, e, true, iExportInHome(t, e, "two.json", `[{"title":"Beta","text":"points at [[Alpha]]\n"}]`))

	aLacks(t, aNoteText(t, e, "beta"), "[[Alpha]]")
	aContains(t, plain(Backlinks(e, "alpha", FormatTable)), "Beta")
}

// Every file is read before anything is written.
func TestAFileThatCannotBeReadStopsTheImportBeforeItWrites(t *testing.T) {
	e, dir := aInit(t)
	good := iExportInHome(t, e, "one.json", `[{"title":"Alpha","text":"a\n"}]`)
	bad := iExportInHome(t, e, "two.json", "not an export at all\n")
	before := aCommitCount(t, e, dir)

	_, err := ImportTiddlywiki(e, []string{good, bad}, true)

	aContains(t, aFail(t, "", err), "two.json")
	aEqual(t, aCommitCount(t, e, dir), before, "and wrote nothing")
	listed, err := Ls(e, List{})
	aEqual(t, aOK(t, listed, err), "", "and the notebook is empty")
}
