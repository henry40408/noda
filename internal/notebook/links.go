package notebook

import (
	"cmp"
	"slices"
	"strings"

	"github.com/henry40408/noda/internal/link"
	"github.com/henry40408/noda/internal/note"
)

// Stale is a link whose path is gone but whose id is held: a note retitled
// after being linked to.
type Stale struct{ Note, Destination, Current string }

// Broken is a link naming nothing the notebook holds.
type Broken struct{ Note, Destination string }

// Audit is what AuditLinks found. It is not part of Scan: it parses every
// body, which `status` must not pay for.
type Audit struct {
	// Orphans are files no note links to, the README exempt.
	Orphans []string
	Stale   []Stale
	Broken  []Broken
}

// AuditLinks finds orphaned files and dangling links. It parses every body, so
// it runs only on request.
//
// Links are checked against the filesystem, so one into a subdirectory
// resolves; orphans are reported at the root only. A dangling link is stale
// when it still names an id the notebook holds (noda knows the right name) and
// broken otherwise.
func (n *Notebook) AuditLinks() (Audit, error) {
	notes, files, err := n.Inventory()
	if err != nil {
		return Audit{}, err
	}
	current := make(map[string]string, len(notes))
	for _, file := range notes {
		current[note.NormalizeID(file.ID)] = note.FileName(file.ID, file.Slug)
	}

	var audit Audit
	referenced := map[string]struct{}{}
	for _, file := range notes {
		name := note.FileName(file.ID, file.Slug)
		for _, target := range link.Targets(file.Note.Body) {
			if n.fileExists(target) {
				referenced[target] = struct{}{}
				continue
			}
			if now, found := current[LinkedNoteID(target)]; found {
				audit.Stale = append(audit.Stale, Stale{Note: name, Destination: target, Current: now})
			} else {
				audit.Broken = append(audit.Broken, Broken{Note: name, Destination: target})
			}
		}
	}

	for _, file := range files {
		if _, linked := referenced[file]; file != ReadmeFile && !linked {
			audit.Orphans = append(audit.Orphans, file)
		}
	}
	slices.SortFunc(audit.Stale, func(a, b Stale) int {
		return cmp.Or(cmp.Compare(a.Note, b.Note), cmp.Compare(a.Destination, b.Destination), cmp.Compare(a.Current, b.Current))
	})
	slices.SortFunc(audit.Broken, func(a, b Broken) int {
		return cmp.Or(cmp.Compare(a.Note, b.Note), cmp.Compare(a.Destination, b.Destination))
	})
	return audit, nil
}

// BacklinksToNote is the notes whose bodies link to the note id names. They
// are matched on the id, not the filename, so a link survives `noda mv`
// retitling its target. A note linking to itself is listed.
func (n *Notebook) BacklinksToNote(id string) ([]NoteFile, error) {
	return n.notesWhere(func(file note.Note) bool { return LinksToNote(file, id) })
}

// BacklinksToFile is matched on the whole name: an attachment has no id.
func (n *Notebook) BacklinksToFile(name string) ([]NoteFile, error) {
	return n.notesWhere(func(file note.Note) bool { return LinksToFile(file, name) })
}

func (n *Notebook) notesWhere(keep func(note.Note) bool) ([]NoteFile, error) {
	notes, err := n.Notes()
	if err != nil {
		return nil, err
	}
	var kept []NoteFile
	for _, file := range notes {
		if keep(file.Note) {
			kept = append(kept, file)
		}
	}
	return kept, nil
}

// LinkedNoteID is the folded id in a destination's filename, empty when it has
// none. Only the root holds notes, so a destination into a subdirectory has
// none.
func LinkedNoteID(target string) string {
	if strings.Contains(target, "/") {
		return ""
	}
	stem, isMarkdown := strings.CutSuffix(target, ".md")
	if !isMarkdown {
		return ""
	}
	id, _, named := note.SplitStem(stem)
	if !named {
		return ""
	}
	return note.NormalizeID(id)
}

// TagCount is a tag and how many notes carry it.
type TagCount struct {
	Tag   string
	Notes int
}

// TagTally is every tag with its note count, commonest first, then
// alphabetical. It is decided here so the TUI and web agree, and it takes the
// notes so a caller holding them does not walk the directory again.
func TagTally(notes []NoteFile) []TagCount {
	counted := map[string]int{}
	for _, file := range notes {
		for _, tag := range file.Note.Tags {
			counted[tag]++
		}
	}
	tallies := make([]TagCount, 0, len(counted))
	for tag, count := range counted {
		tallies = append(tallies, TagCount{Tag: tag, Notes: count})
	}
	slices.SortFunc(tallies, func(a, b TagCount) int {
		return cmp.Or(cmp.Compare(b.Notes, a.Notes), cmp.Compare(a.Tag, b.Tag))
	})
	return tallies
}

// LinksToNote is whether this note's body links to the note id names. It is a
// plain function for the TUI, which already holds every note; BacklinksToNote
// reads them from disk.
func LinksToNote(file note.Note, id string) bool {
	wanted := note.NormalizeID(id)
	for _, target := range link.Targets(file.Body) {
		if linked := LinkedNoteID(target); linked != "" && linked == wanted {
			return true
		}
	}
	return false
}

// LinksToFile is whether this note's body links to the notebook file name
// names.
func LinksToFile(file note.Note, name string) bool {
	for _, target := range link.Targets(file.Body) {
		if target == name {
			return true
		}
	}
	return false
}
