package notebook

import (
	"cmp"
	"fmt"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"unicode/utf8"

	"github.com/henry40408/noda/internal/note"
)

// NoteFile is a note with the name that identifies it.
type NoteFile struct {
	ID   string
	Slug string
	Note note.Note
}

// Problem is something in the notebook that noda will not settle on its own.
// Reported by kind, because it usually goes wrong wholesale (a directory copied
// in). The order is the order they are reported in.
type Problem int

const (
	// SharedID is one id on more than one file: two machines minted the same
	// id, and the filenames differ, so git merged them silently.
	SharedID Problem = iota
	// Unnamed is frontmatter but no id in the name: a hand-written note
	// awaiting adoption.
	Unnamed
	// Suspicious is an id over a file with no frontmatter: a note that lost
	// its block, or never a note.
	Suspicious
)

// Describe says how many of this kind there are.
func (p Problem) Describe(count int) string {
	one := count == 1
	switch {
	case p == SharedID && one:
		return "1 id is carried by more than one note"
	case p == SharedID:
		return fmt.Sprintf("%d ids are carried by more than one note", count)
	case p == Unnamed && one:
		return "1 note has no id in its filename"
	case p == Unnamed:
		return fmt.Sprintf("%d notes have no id in their filenames", count)
	case one:
		return "1 file is named like a note but has no frontmatter"
	default:
		return fmt.Sprintf("%d files are named like notes but have no frontmatter", count)
	}
}

// ProblemGroup is every subject of one kind of problem.
type ProblemGroup struct {
	Kind     Problem
	Subjects []string
}

// IDSlug is a note's name taken apart.
type IDSlug struct{ ID, Slug string }

// compareIDSlug orders by id, then slug.
func compareIDSlug(a, b IDSlug) int {
	return cmp.Or(cmp.Compare(a.ID, b.ID), cmp.Compare(a.Slug, b.Slug))
}

// Scan sorts every file into the four cases a filename and a frontmatter block
// produce: the block says "a note", the id prefix says "adopted", and a file
// with neither is left alone.
type Scan struct {
	Notes []IDSlug
	// Unnamed has frontmatter but no id: adoptable.
	Unnamed []string
	// Suspicious has an id but no frontmatter: ambiguous.
	Suspicious []string
	// Files is everything else. Which note uses one is AuditLinks's question.
	Files []string
}

// Problems groups what Scan found that needs a person.
func (s Scan) Problems() []ProblemGroup {
	var groups []ProblemGroup

	count := map[string]int{}
	for _, found := range s.Notes {
		count[note.NormalizeID(found.ID)]++
	}
	var shared []string
	for id, n := range count {
		if n > 1 {
			shared = append(shared, id)
		}
	}
	groups = appendGroup(groups, SharedID, shared)
	groups = appendGroup(groups, Unnamed, s.Unnamed)
	return appendGroup(groups, Suspicious, s.Suspicious)
}

func appendGroup(groups []ProblemGroup, kind Problem, subjects []string) []ProblemGroup {
	if len(subjects) == 0 {
		return groups
	}
	subjects = append([]string(nil), subjects...)
	slices.Sort(subjects)
	deduped := subjects[:1]
	for _, subject := range subjects[1:] {
		if subject != deduped[len(deduped)-1] {
			deduped = append(deduped, subject)
		}
	}
	return append(groups, ProblemGroup{Kind: kind, Subjects: deduped})
}

// listing is a visible regular file of the notebook's root.
type listing struct {
	name string
	path string
}

// rootFiles is the regular files at the notebook's root, dotfiles (repo
// configuration) and names that are not UTF-8 (which no link can match) left
// out.
func (n *Notebook) rootFiles() ([]listing, error) {
	entries, err := os.ReadDir(n.Path)
	if err != nil {
		return nil, err
	}
	var files []listing
	for _, entry := range entries {
		name := entry.Name()
		if !entry.Type().IsRegular() || strings.HasPrefix(name, ".") || !utf8.ValidString(name) {
			continue
		}
		files = append(files, listing{name: name, path: filepath.Join(n.Path, name)})
	}
	return files, nil
}

// Scan sorts every file into the four cases.
func (n *Notebook) Scan() (Scan, error) {
	files, err := n.rootFiles()
	if err != nil {
		return Scan{}, err
	}
	var scan Scan
	for _, file := range files {
		stem, isMarkdown := strings.CutSuffix(file.name, ".md")
		if !isMarkdown {
			scan.Files = append(scan.Files, file.name)
			continue
		}
		text, err := os.ReadFile(file.path)
		if err != nil {
			return Scan{}, err
		}
		if !utf8.Valid(text) {
			return Scan{}, fmt.Errorf("%s: stream did not contain valid UTF-8", file.path)
		}
		_, parseErr := note.Parse(string(text))
		declared := parseErr == nil
		id, slug, named := note.SplitStem(stem)
		switch {
		case named && declared:
			scan.Notes = append(scan.Notes, IDSlug{ID: id, Slug: slug})
		case named:
			scan.Suspicious = append(scan.Suspicious, file.name)
		case declared:
			scan.Unnamed = append(scan.Unnamed, file.name)
		default:
			scan.Files = append(scan.Files, file.name)
		}
	}
	slices.SortFunc(scan.Notes, compareIDSlug)
	slices.Sort(scan.Unnamed)
	slices.Sort(scan.Suspicious)
	slices.Sort(scan.Files)
	return scan, nil
}

// Notes is every adopted note, sorted by slug, each file read once. A file that
// will not parse is skipped.
func (n *Notebook) Notes() ([]NoteFile, error) {
	notes, _, err := n.Inventory()
	return notes, err
}

// Inventory is the notes and the other files from one walk, classified as Scan
// does; unnamed and suspicious files are in neither list, since Scan reports
// them.
func (n *Notebook) Inventory() ([]NoteFile, []string, error) {
	listed, err := n.rootFiles()
	if err != nil {
		return nil, nil, err
	}
	var notes []NoteFile
	var files []string
	for _, file := range listed {
		stem, isMarkdown := strings.CutSuffix(file.name, ".md")
		if !isMarkdown {
			files = append(files, file.name)
			continue
		}
		parsed, ok := readNote(file.path)
		id, slug, named := note.SplitStem(stem)
		switch {
		case named && ok:
			notes = append(notes, NoteFile{ID: id, Slug: slug, Note: parsed})
		case named || ok:
		default:
			files = append(files, file.name)
		}
	}
	slices.SortStableFunc(notes, func(a, b NoteFile) int { return cmp.Compare(a.Slug, b.Slug) })
	slices.Sort(files)
	return notes, files, nil
}

func readNote(path string) (note.Note, bool) {
	text, err := os.ReadFile(path)
	if err != nil || !utf8.Valid(text) {
		return note.Note{}, false
	}
	parsed, err := note.Parse(string(text))
	return parsed, err == nil
}

// NamedFiles is every `(id, slug)` a filename spells out, readable file or not;
// it opens nothing. More forgiving than Scan, so `rm`, `log`, `diff` and
// `restore` work on a broken file.
func (n *Notebook) NamedFiles() ([]IDSlug, error) {
	entries, err := os.ReadDir(n.Path)
	if err != nil {
		return nil, err
	}
	var found []IDSlug
	for _, entry := range entries {
		if !entry.Type().IsRegular() {
			continue
		}
		stem, isMarkdown := strings.CutSuffix(entry.Name(), ".md")
		if !isMarkdown {
			continue
		}
		if id, slug, named := note.SplitStem(stem); named {
			found = append(found, IDSlug{ID: id, Slug: slug})
		}
	}
	slices.SortFunc(found, compareIDSlug)
	return found, nil
}

// TakenIDs is every id spoken for, folded, from the filenames alone.
func (n *Notebook) TakenIDs() (map[string]struct{}, error) {
	named, err := n.NamedFiles()
	if err != nil {
		return nil, err
	}
	taken := make(map[string]struct{}, len(named))
	for _, found := range named {
		taken[note.NormalizeID(found.ID)] = struct{}{}
	}
	return taken, nil
}
