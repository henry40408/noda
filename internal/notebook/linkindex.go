package notebook

import (
	"cmp"
	"os"
	"slices"
	"strings"
	"sync"
	"time"

	"github.com/henry40408/noda/internal/link"
	"github.com/henry40408/noda/internal/note"
)

// timestampTick is the coarsest modification-time granularity a filesystem
// reports (FAT: two seconds). A file read within one tick of its own
// modification time may be written again inside that tick with the same size,
// and a stat would not show it.
const timestampTick = 2 * time.Second

// LinkIndex remembers which notes each note's body links to, so a process that
// answers many requests parses a body again only after its file changed.
// Without it, opening one note in `noda web` read and parsed every note in the
// notebook for the margin: 45 ms at 2,000 notes, paid on every view. A command
// that runs once has nothing to remember, and uses Notebook.BacklinksToNote.
//
// A file is trusted by its size and modification time, never by HEAD: notes
// are read from the working tree, and an editor changes it without a commit.
// The index lives in memory only. The notebook holds no index file by design,
// since a second copy of what the notes say could disagree with them.
//
// The zero value is ready, and one LinkIndex serves every notebook and request.
type LinkIndex struct {
	mutex sync.Mutex
	books map[string]map[string]indexedNote
}

// indexedNote is what one file said when it was read.
type indexedNote struct {
	size     int64
	modified time.Time
	read     time.Time
	// linked is the folded ids of the notes it links to; empty for a file that
	// is not a note.
	linked map[string]struct{}
}

// holds is whether the file is still as it was read. A file read within one
// tick of its modification may have changed since without a new time, so it is
// read again: the rule git uses for a racily clean file.
func (entry indexedNote) holds(info os.FileInfo) bool {
	unchanged := entry.size == info.Size() && entry.modified.Equal(info.ModTime())
	return unchanged && entry.read.Sub(entry.modified) >= timestampTick
}

// BacklinksToNote answers as Notebook.BacklinksToNote does, reading only the
// files that changed since the last call and the notes that link here.
func (x *LinkIndex) BacklinksToNote(n *Notebook, id string) ([]NoteFile, error) {
	wanted := note.NormalizeID(id)
	linking, err := x.filesLinkingTo(n, wanted)
	if err != nil {
		return nil, err
	}
	var found []NoteFile
	for _, file := range linking {
		parsed, ok := readNote(file.path)
		// The file may have changed since it was indexed.
		if !ok || !LinksToNote(parsed, wanted) {
			continue
		}
		stem, _ := strings.CutSuffix(file.name, ".md")
		fileID, slug, _ := note.SplitStem(stem)
		found = append(found, NoteFile{ID: fileID, Slug: slug, Note: parsed})
	}
	slices.SortStableFunc(found, func(a, b NoteFile) int { return cmp.Compare(a.Slug, b.Slug) })
	return found, nil
}

// filesLinkingTo brings the index up to date with the notebook's root and
// returns the files whose body links to the folded id.
func (x *LinkIndex) filesLinkingTo(n *Notebook, wanted string) ([]listing, error) {
	listed, err := n.rootFiles()
	if err != nil {
		return nil, err
	}
	x.mutex.Lock()
	defer x.mutex.Unlock()

	before := x.books[n.Path]
	after := make(map[string]indexedNote, len(before))
	var linking []listing
	for _, file := range listed {
		stem, isMarkdown := strings.CutSuffix(file.name, ".md")
		if _, _, named := note.SplitStem(stem); !isMarkdown || !named {
			continue
		}
		entry, ok := fresh(before[file.name], file)
		if !ok {
			continue
		}
		after[file.name] = entry
		if _, links := entry.linked[wanted]; links {
			linking = append(linking, file)
		}
	}
	if x.books == nil {
		x.books = map[string]map[string]indexedNote{}
	}
	// Replaced whole, so a file that is gone leaves the index with it.
	x.books[n.Path] = after
	return linking, nil
}

// fresh is the entry for a file: the remembered one while it holds, otherwise a
// new reading. It is not ok for a file that vanished since it was listed or
// that cannot be read: what a failed read says about the file is not recorded,
// since making it readable changes neither its size nor its time. The stat
// comes before the read, so a write in between is seen next time.
func fresh(remembered indexedNote, file listing) (indexedNote, bool) {
	info, err := os.Lstat(file.path)
	if err != nil {
		return indexedNote{}, false
	}
	if remembered.holds(info) {
		return remembered, true
	}
	entry := indexedNote{size: info.Size(), modified: info.ModTime(), read: time.Now()}
	text, err := os.ReadFile(file.path)
	if err != nil {
		return indexedNote{}, false
	}
	if parsed, isNote := parseNote(text); isNote {
		entry.linked = linkedNoteIDs(parsed)
	}
	return entry, true
}

// linkedNoteIDs is the folded id of every note the body links to.
func linkedNoteIDs(file note.Note) map[string]struct{} {
	ids := map[string]struct{}{}
	for _, target := range link.Targets(file.Body) {
		if id := LinkedNoteID(target); id != "" {
			ids[id] = struct{}{}
		}
	}
	return ids
}
