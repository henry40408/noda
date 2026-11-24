package notebook

import (
	"fmt"
	"os"
	"path/filepath"
	"slices"
	"sync"
	"testing"
	"time"

	"github.com/henry40408/noda/internal/note"
)

const (
	targetID   = "01abc234"
	targetFile = targetID + "-target.md"
	// sameSizeElsewhere names another note in as many bytes as targetFile, so
	// a body linking to it is as long as one linking to the target.
	sameSizeElsewhere = "09zzz999-target.md"
)

// bareNotebook is a directory, which is all LinkIndex reads: no repository.
func bareNotebook(t *testing.T) *Notebook {
	t.Helper()
	return &Notebook{Path: t.TempDir()}
}

func linkingTo(destination string) string { return fmt.Sprintf(noteText, destination) }

// settle gives a file a modification time long enough ago for the index to
// trust it, and returns that time.
func settle(t *testing.T, nb *Notebook, name string) time.Time {
	t.Helper()
	past := time.Now().Add(-time.Hour).Truncate(time.Second)
	if err := os.Chtimes(filepath.Join(nb.Path, name), past, past); err != nil {
		t.Fatal(err)
	}
	return past
}

func filenames(t *testing.T, found []NoteFile, err error) []string {
	t.Helper()
	if err != nil {
		t.Fatal(err)
	}
	names := make([]string, 0, len(found))
	for _, file := range found {
		names = append(names, note.FileName(file.ID, file.Slug))
	}
	return names
}

func backlinks(t *testing.T, index *LinkIndex, nb *Notebook) []string {
	t.Helper()
	found, err := index.BacklinksToNote(nb, targetID)
	return filenames(t, found, err)
}

func wantNames(t *testing.T, got []string, want ...string) {
	t.Helper()
	if !slices.Equal(got, want) {
		t.Fatalf("got %v, want %v", got, want)
	}
}

func TestTheIndexAnswersAsBacklinksToNoteDoes(t *testing.T) {
	nb := bareNotebook(t)
	write(t, nb, targetFile, linkingTo("02def567-b.md"))
	write(t, nb, "02def567-b.md", linkingTo(targetFile))
	write(t, nb, "03ghj890-a.md", linkingTo("./"+targetFile+"#section"))
	write(t, nb, "04kmn123-aa.md", linkingTo("%30"+targetFile[1:]))
	// What is not a note, or links nowhere here.
	write(t, nb, "05pqr456-mention.md", "---\ntitle: T\n---\n"+targetFile+" in prose\n")
	write(t, nb, "06stv789-no-frontmatter.md", "[x]("+targetFile+")\n")
	write(t, nb, "unnamed.md", linkingTo(targetFile))
	write(t, nb, "07wxy012-elsewhere.md", linkingTo("02def567-b.md"))
	write(t, nb, "08abc345-figure.png", linkingTo(targetFile))
	write(t, nb, ".hidden.md", linkingTo(targetFile))

	want, err := nb.BacklinksToNote(targetID)
	if err != nil {
		t.Fatal(err)
	}
	if len(want) != 3 {
		t.Fatalf("the fixture should have three backlinks, has %d", len(want))
	}
	for _, key := range []string{targetID, "01ABC234"} {
		got, err := (&LinkIndex{}).BacklinksToNote(nb, key)
		if err != nil {
			t.Fatal(err)
		}
		if !slices.EqualFunc(got, want, func(a, b NoteFile) bool {
			return a.ID == b.ID && a.Slug == b.Slug && a.Note.Title == b.Note.Title && a.Note.Body == b.Note.Body
		}) {
			t.Errorf("key %q: index %v, notebook %v", key, filenames(t, got, nil), filenames(t, want, nil))
		}
	}
}

func TestBacklinksComeInTheOrderOfTheirSlugs(t *testing.T) {
	nb := bareNotebook(t)
	write(t, nb, "01abc111-zebra.md", linkingTo(targetFile))
	write(t, nb, "02abc222-apple.md", linkingTo(targetFile))
	write(t, nb, "03abc333-apple.md", linkingTo(targetFile))
	wantNames(t, backlinks(t, &LinkIndex{}, nb), "02abc222-apple.md", "03abc333-apple.md", "01abc111-zebra.md")
}

func TestAnEditedNoteIsReadAgain(t *testing.T) {
	nb := bareNotebook(t)
	index := &LinkIndex{}
	write(t, nb, "02def567-b.md", linkingTo(targetFile))
	settle(t, nb, "02def567-b.md")
	wantNames(t, backlinks(t, index, nb), "02def567-b.md")

	write(t, nb, "02def567-b.md", linkingTo("02def567-b.md")+"and a longer body now\n")
	wantNames(t, backlinks(t, index, nb))

	write(t, nb, "02def567-b.md", linkingTo(targetFile))
	wantNames(t, backlinks(t, index, nb), "02def567-b.md")
}

func TestANewNoteARemovedOneAndARetitledOneAreSeen(t *testing.T) {
	nb := bareNotebook(t)
	index := &LinkIndex{}
	write(t, nb, "02def567-b.md", linkingTo(targetFile))
	settle(t, nb, "02def567-b.md")
	wantNames(t, backlinks(t, index, nb), "02def567-b.md")

	write(t, nb, "03ghj890-c.md", linkingTo(targetFile))
	wantNames(t, backlinks(t, index, nb), "02def567-b.md", "03ghj890-c.md")

	if err := os.Rename(filepath.Join(nb.Path, "02def567-b.md"), filepath.Join(nb.Path, "02def567-renamed.md")); err != nil {
		t.Fatal(err)
	}
	wantNames(t, backlinks(t, index, nb), "03ghj890-c.md", "02def567-renamed.md")

	if err := os.Remove(filepath.Join(nb.Path, "03ghj890-c.md")); err != nil {
		t.Fatal(err)
	}
	wantNames(t, backlinks(t, index, nb), "02def567-renamed.md")
}

// A rewrite of the same size, within the clock's tick of the last read, leaves
// size and time as they were; trusting them would answer from the old body. A
// time ahead of the clock is the extreme of "within a tick" and does not
// depend on how long the test takes to run.
func TestAFileReadWithinATickOfItsWriteIsNotTrusted(t *testing.T) {
	nb := bareNotebook(t)
	index := &LinkIndex{}
	write(t, nb, "02def567-b.md", linkingTo(sameSizeElsewhere))
	ahead := time.Now().Add(time.Hour)
	if err := os.Chtimes(filepath.Join(nb.Path, "02def567-b.md"), ahead, ahead); err != nil {
		t.Fatal(err)
	}
	wantNames(t, backlinks(t, index, nb))

	write(t, nb, "02def567-b.md", linkingTo(targetFile))
	if err := os.Chtimes(filepath.Join(nb.Path, "02def567-b.md"), ahead, ahead); err != nil {
		t.Fatal(err)
	}
	wantNames(t, backlinks(t, index, nb), "02def567-b.md")
}

// Making a file readable changes neither its size nor its time, so a failed
// read must not be remembered as "links nowhere".
func TestAFileThatCouldNotBeReadIsAskedAgain(t *testing.T) {
	nb := bareNotebook(t)
	index := &LinkIndex{}
	write(t, nb, "02def567-b.md", linkingTo(targetFile))
	past := settle(t, nb, "02def567-b.md")
	path := filepath.Join(nb.Path, "02def567-b.md")
	if err := os.Chmod(path, 0); err != nil {
		t.Fatal(err)
	}
	if _, err := os.ReadFile(path); err == nil {
		t.Skip("this user reads files whatever their mode")
	}
	wantNames(t, backlinks(t, index, nb))

	if err := os.Chmod(path, 0o644); err != nil {
		t.Fatal(err)
	}
	if err := os.Chtimes(path, past, past); err != nil {
		t.Fatal(err)
	}
	wantNames(t, backlinks(t, index, nb), "02def567-b.md")
}

// Pinned so that trusting a file is a decision someone changes on purpose: it
// is the whole saving. A file the index trusted can only hide a link, never
// invent one, because a note it names is read again before it is returned.
func TestAFileThatHasSettledIsNotReadAgainWhileItsSizeAndTimeHold(t *testing.T) {
	nb := bareNotebook(t)
	index := &LinkIndex{}
	write(t, nb, "02def567-b.md", linkingTo(sameSizeElsewhere))
	past := settle(t, nb, "02def567-b.md")
	wantNames(t, backlinks(t, index, nb))

	write(t, nb, "02def567-b.md", linkingTo(targetFile))
	if err := os.Chtimes(filepath.Join(nb.Path, "02def567-b.md"), past, past); err != nil {
		t.Fatal(err)
	}
	wantNames(t, backlinks(t, index, nb))
}

func TestANoteThatStoppedLinkingIsLeftOutAtOnceEvenIfTrusted(t *testing.T) {
	nb := bareNotebook(t)
	index := &LinkIndex{}
	write(t, nb, "02def567-b.md", linkingTo(targetFile))
	past := settle(t, nb, "02def567-b.md")
	wantNames(t, backlinks(t, index, nb), "02def567-b.md")

	write(t, nb, "02def567-b.md", linkingTo(sameSizeElsewhere))
	if err := os.Chtimes(filepath.Join(nb.Path, "02def567-b.md"), past, past); err != nil {
		t.Fatal(err)
	}
	wantNames(t, backlinks(t, index, nb))
}

func TestAChangedTimeAloneIsEnoughToReadAFileAgain(t *testing.T) {
	nb := bareNotebook(t)
	index := &LinkIndex{}
	write(t, nb, "02def567-b.md", linkingTo(sameSizeElsewhere))
	past := settle(t, nb, "02def567-b.md")
	wantNames(t, backlinks(t, index, nb))

	write(t, nb, "02def567-b.md", linkingTo(targetFile))
	later := past.Add(time.Minute)
	if err := os.Chtimes(filepath.Join(nb.Path, "02def567-b.md"), later, later); err != nil {
		t.Fatal(err)
	}
	wantNames(t, backlinks(t, index, nb), "02def567-b.md")
}

func TestHoldsComparesSizeTimeAndHowLongAfterTheWriteItWasRead(t *testing.T) {
	nb := bareNotebook(t)
	write(t, nb, "02def567-b.md", "12345")
	modified := settle(t, nb, "02def567-b.md")
	info, err := os.Lstat(filepath.Join(nb.Path, "02def567-b.md"))
	if err != nil {
		t.Fatal(err)
	}
	for _, c := range []struct {
		name  string
		entry indexedNote
		want  bool
	}{
		{"read a tick after the write", indexedNote{size: 5, modified: modified, read: modified.Add(timestampTick)}, true},
		{"read just under a tick after", indexedNote{size: 5, modified: modified, read: modified.Add(timestampTick - time.Nanosecond)}, false},
		{"read before the write's time", indexedNote{size: 5, modified: modified, read: modified.Add(-time.Hour)}, false},
		{"another size", indexedNote{size: 6, modified: modified, read: modified.Add(time.Hour)}, false},
		{"another time", indexedNote{size: 5, modified: modified.Add(time.Nanosecond), read: modified.Add(time.Hour)}, false},
		{"never read", indexedNote{}, false},
	} {
		if got := c.entry.holds(info); got != c.want {
			t.Errorf("%s: %v", c.name, got)
		}
	}
}

func TestAFileThatVanishedAfterItWasListedIsLeftOut(t *testing.T) {
	if _, ok := fresh(indexedNote{}, listing{name: "02def567-b.md", path: filepath.Join(t.TempDir(), "02def567-b.md")}); ok {
		t.Fatal("a file that is not there has an entry")
	}
}

func TestTwoNotebooksKeepTheirOwnRecords(t *testing.T) {
	first, second := bareNotebook(t), bareNotebook(t)
	index := &LinkIndex{}
	write(t, first, "02def567-b.md", linkingTo(targetFile))
	write(t, second, "02def567-b.md", linkingTo(sameSizeElsewhere))
	settle(t, first, "02def567-b.md")
	settle(t, second, "02def567-b.md")
	for range 2 {
		wantNames(t, backlinks(t, index, first), "02def567-b.md")
		wantNames(t, backlinks(t, index, second))
	}
}

func TestOnlyWhatTheRootHoldsIsRemembered(t *testing.T) {
	nb := bareNotebook(t)
	index := &LinkIndex{}
	write(t, nb, "02def567-b.md", linkingTo(targetFile))
	write(t, nb, "03ghj890-c.txt", "not a note")
	write(t, nb, "unnamed.md", linkingTo(targetFile))
	write(t, nb, "04kmn123-d.md", "no frontmatter")
	backlinks(t, index, nb)
	held := index.books[nb.Path]
	if len(held) != 2 {
		t.Fatalf("remembered %d files: %v", len(held), held)
	}
	if held["04kmn123-d.md"].linked != nil {
		t.Error("a file without frontmatter links nowhere")
	}
	if err := os.Remove(filepath.Join(nb.Path, "04kmn123-d.md")); err != nil {
		t.Fatal(err)
	}
	backlinks(t, index, nb)
	if _, kept := index.books[nb.Path]["04kmn123-d.md"]; kept {
		t.Error("a deleted file stays in the index")
	}
}

func TestAMissingNotebookIsAnError(t *testing.T) {
	gone := &Notebook{Path: filepath.Join(t.TempDir(), "gone")}
	if _, err := (&LinkIndex{}).BacklinksToNote(gone, targetID); err == nil {
		t.Fatal("a notebook that is not there answered")
	}
}

func TestRequestsMayAskTogetherWhileANoteIsWritten(t *testing.T) {
	nb := bareNotebook(t)
	index := &LinkIndex{}
	for i := range 20 {
		write(t, nb, fmt.Sprintf("%02dabc000-n.md", i+10), linkingTo(targetFile))
	}
	var wait sync.WaitGroup
	for range 8 {
		wait.Go(func() {
			for range 10 {
				if _, err := index.BacklinksToNote(nb, targetID); err != nil {
					t.Error(err)
				}
			}
		})
	}
	for i := range 10 {
		write(t, nb, "99zzz000-w.md", linkingTo(fmt.Sprintf("%d-x.md", i)))
	}
	wait.Wait()
}

// notebookOfNotes is the size the issue measured: each note links to three
// others, so the bodies cost what real ones do to parse.
func notebookOfNotes(b *testing.B, count int) *Notebook {
	b.Helper()
	nb := &Notebook{Path: b.TempDir()}
	name := func(i int) string { return fmt.Sprintf("%08d-note-%d.md", i, i) }
	past := time.Now().Add(-time.Hour)
	for i := range count {
		body := fmt.Sprintf("---\ntitle: Note %d\n---\nsee [a](%s), [b](%s) and [c](%s)\n\n%s\n",
			i, name((i+1)%count), name((i+7)%count), name((i+31)%count), "Some prose that stands for a paragraph of a real note. ")
		path := filepath.Join(nb.Path, name(i))
		if err := os.WriteFile(path, []byte(body), 0o644); err != nil {
			b.Fatal(err)
		}
		if err := os.Chtimes(path, past, past); err != nil {
			b.Fatal(err)
		}
	}
	return nb
}

func BenchmarkBacklinksToNote(b *testing.B) {
	nb := notebookOfNotes(b, 2000)
	b.Run("every body parsed", func(b *testing.B) {
		for range b.N {
			if _, err := nb.BacklinksToNote("00000100"); err != nil {
				b.Fatal(err)
			}
		}
	})
	b.Run("remembered", func(b *testing.B) {
		index := &LinkIndex{}
		for range b.N {
			if _, err := index.BacklinksToNote(nb, "00000100"); err != nil {
				b.Fatal(err)
			}
		}
	})
}
