package cmd

import (
	"cmp"
	"fmt"
	"slices"
	"strconv"
	"strings"
	"time"
	"unicode"

	"github.com/henry40408/noda/internal/note"
	"github.com/henry40408/noda/internal/notebook"
	"github.com/henry40408/noda/internal/style"
)

// Sort is the order `ls` lists notes in.
type Sort int

const (
	// SortSlug is what a notebook walk already produces.
	SortSlug Sort = iota
	// SortCreated is newest first, as is SortUpdated.
	SortCreated
	SortUpdated
	// SortTitle is alphabetical.
	SortTitle
)

// AllSorts is defined once so a page's list and the ring Next walks agree.
var AllSorts = []Sort{SortSlug, SortCreated, SortUpdated, SortTitle}

// Name is how `--sort` spells it, and what a screen calls it.
func (s Sort) Name() string {
	return [...]string{"slug", "created", "updated", "title"}[s]
}

// SortNamed is Name's inverse, for an order carried in a URL.
func SortNamed(said string) (Sort, bool) {
	for _, sort := range AllSorts {
		if sort.Name() == said {
			return sort, true
		}
	}
	return SortSlug, false
}

// Next is the next order in AllSorts, for a key that cycles through them.
func (s Sort) Next() Sort {
	return (s + 1) % Sort(len(AllSorts))
}

// Format is how a listing is written.
type Format int

const (
	// FormatTable is aligned columns for a person.
	FormatTable Format = iota
	// FormatJSON is one object, for a program.
	FormatJSON
	// FormatQuiet is one identifier per line and nothing else.
	FormatQuiet
)

// Touch is whether a command that changes a note moves its `updated`. TouchKeep
// is `--no-touch`: a fixed typo or an added tag is not a rewrite, and the user
// may own the field.
type Touch int

const (
	// TouchStamp sets `updated` to now.
	TouchStamp Touch = iota
	// TouchKeep leaves `updated` as it was found.
	TouchKeep
)

// Only narrows a listing to one of its halves.
type Only int

const (
	OnlyEverything Only = iota
	OnlyNotes
	OnlyFiles
)

// List is what `ls` is asked for.
type List struct {
	// Notebook is another notebook instead of the active one.
	Notebook string
	// Tag is anything more selective than one tag is `search`'s job.
	Tag    string
	Format Format
	Only   Only
	// Null NUL-separates FormatQuiet output, for `xargs -0` and names with
	// spaces.
	Null bool
	Sort Sort
	// Reverse is applied after Sort, as `ls -r` is; the files turn with the
	// notes.
	Reverse bool
	// Long adds the slug and both timestamps. Off by default because the slug
	// repeats the title.
	Long bool
}

// Ls lists notes as `id  title  [tags]`, aligned, then the notebook's files
// under their own heading. `-l` adds the slug and both timestamps.
func Ls(e Env, options List) (string, error) {
	name := options.Notebook
	if name == "" {
		active, err := notebook.ActiveName(e.Paths)
		if err != nil {
			return "", err
		}
		name = active
	}
	opened, err := e.open(name)
	if err != nil {
		return "", err
	}
	notes, files, err := opened.Inventory()
	if err != nil {
		return "", err
	}

	notes = selectNotes(notes, options)
	SortNotes(notes, options.Sort)

	// Asking for one tag is asking about notes.
	if options.Tag != "" || options.Only == OnlyNotes {
		files = nil
	}
	if options.Reverse {
		slices.Reverse(notes)
		slices.Reverse(files)
	}

	switch options.Format {
	case FormatJSON:
		return asJSON(name, notes, files), nil
	case FormatQuiet:
		return asIdentifiers(notes, files, options.Null), nil
	}
	return asTable(notes, files, options.Long), nil
}

func selectNotes(notes []notebook.NoteFile, options List) []notebook.NoteFile {
	if options.Only == OnlyFiles {
		return nil
	}
	if options.Tag == "" {
		return notes
	}
	var kept []notebook.NoteFile
	for _, file := range notes {
		if slices.Contains(file.Note.Tags, options.Tag) {
			kept = append(kept, file)
		}
	}
	return kept
}

// listed is one note's row as printed strings, so widths are measured over
// what is written.
type listed struct {
	id, slug, created, updated, title string
	tags                              []string
	pinned                            bool
}

// asTable is the aligned listing: a row per note, then the files under their
// heading.
func asTable(notes []notebook.NoteFile, files []string, long bool) string {
	rows := make([]listed, len(notes))
	for i, file := range notes {
		rows[i] = listed{
			id: file.ID, slug: file.Slug, title: file.Note.Title, tags: file.Note.Tags,
			pinned:  file.Note.IsPinned(),
			created: stampOrDash(file.Note.Created),
			updated: stampOrDash(file.Note.Updated),
		}
	}
	if len(rows) == 0 && len(files) == 0 {
		return ""
	}

	widest := func(of func(listed) string) int {
		cells := 0
		for _, row := range rows {
			cells = max(cells, DisplayWidth(of(row)))
		}
		return cells
	}
	idWidth := widest(func(r listed) string { return r.id })
	slugWidth := widest(func(r listed) string { return r.slug })
	createdWidth := widest(func(r listed) string { return r.created })
	updatedWidth := widest(func(r listed) string { return r.updated })
	titleWidth := widest(func(r listed) string { return r.title })

	var out strings.Builder
	for _, row := range rows {
		// `-l` extends the row rather than rearranging it, so a script cutting
		// id and title off the front reads the same either way. The optional
		// columns (tags, pin) go last so their absence shifts nothing.
		line := column(style.ID, row.id, idWidth)
		if long {
			line += fmt.Sprintf("  %s  %s  %s  %s", pad(row.title, titleWidth),
				column(style.Slug, row.slug, slugWidth),
				column(style.Muted, row.created, createdWidth),
				column(style.Muted, row.updated, updatedWidth))
		} else {
			line += "  " + row.title
		}
		if len(row.tags) > 0 {
			line += "  " + style.TagList(row.tags)
		}
		if row.pinned {
			line += "  " + style.Pin.Paint(style.PinMark)
		}
		out.WriteString(strings.TrimRightFunc(line, unicode.IsSpace))
		out.WriteByte('\n')
	}
	appendFiles(&out, files)
	return out.String()
}

// stampOrDash prints a missing stamp as `-` rather than a hole.
func stampOrDash(stamp *string) string {
	if stamp == nil {
		return "-"
	}
	return *stamp
}

// appendFiles puts them under a heading: a file has no id, title or tags to
// fill a row with.
func appendFiles(out *strings.Builder, files []string) {
	if len(files) == 0 {
		return
	}
	if out.Len() > 0 {
		out.WriteByte('\n')
	}
	out.WriteString(style.Muted.Paint("files") + "\n")
	for _, file := range files {
		out.WriteString("  " + file + "\n")
	}
}

// instant is the moment a stamp names, false when absent or unreadable. It is
// parsed rather than compared as text because an imported note keeps its old
// offset: `2019-03-14T16:21:00+08:00` sorts after `2019-03-14T09:00:00Z` as
// text but is earlier.
func instant(stamp *string) (time.Time, bool) {
	if stamp == nil {
		return time.Time{}, false
	}
	parsed, err := time.Parse(time.RFC3339, *stamp)
	return parsed, err == nil
}

// SortNotes is public so the browser sorts the same way. Reversing is the
// caller's, applied afterwards, so under `-r` pinned notes go to the bottom:
// the pin is part of the order.
func SortNotes(notes []notebook.NoteFile, by Sort) {
	switch by {
	case SortTitle:
		slices.SortStableFunc(notes, func(a, b notebook.NoteFile) int {
			return cmp.Or(cmp.Compare(a.Note.Title, b.Note.Title), cmp.Compare(a.ID, b.ID))
		})
	case SortCreated, SortUpdated:
		sortNewestFirst(notes, by)
	}
	// Last and stable: pins float to the top, keeping their order among
	// themselves.
	slices.SortStableFunc(notes, func(a, b notebook.NoteFile) int {
		switch {
		case a.Note.IsPinned() && !b.Note.IsPinned():
			return -1
		case !a.Note.IsPinned() && b.Note.IsPinned():
			return 1
		}
		return 0
	})
}

func sortNewestFirst(notes []notebook.NoteFile, by Sort) {
	stampOf := func(file notebook.NoteFile) *string {
		if by == SortCreated {
			return file.Note.Created
		}
		return file.Note.Updated
	}
	slices.SortStableFunc(notes, func(a, b notebook.NoteFile) int {
		left, leftOK := instant(stampOf(a))
		right, rightOK := instant(stampOf(b))
		// A missing or unreadable stamp goes past every real instant.
		if leftOK != rightOK {
			if leftOK {
				return -1
			}
			return 1
		}
		if leftOK && !left.Equal(right) {
			return right.Compare(left)
		}
		return cmp.Compare(a.ID, b.ID)
	})
}

// asJSON is assembled by hand around jsonString: a few flat fields need no
// reflection. Each note carries its filename so a script need not know the
// naming rule.
func asJSON(notebookName string, notes []notebook.NoteFile, files []string) string {
	var out strings.Builder
	out.WriteString(`{"notebook":` + jsonString(notebookName) + `,"notes":[`)
	for i, file := range notes {
		if i > 0 {
			out.WriteByte(',')
		}
		// The judgement, not the raw field (`pinned: yes` is not a pin).
		fmt.Fprintf(&out, `{"id":%s,"slug":%s,"file":%s,"title":%s,"created":%s,"updated":%s,"pinned":%t,"tags":[`,
			jsonString(file.ID), jsonString(file.Slug), jsonString(note.FileName(file.ID, file.Slug)),
			jsonString(file.Note.Title), jsonStamp(file.Note.Created), jsonStamp(file.Note.Updated),
			file.Note.IsPinned())
		out.WriteString(jsonList(file.Note.Tags))
		out.WriteString("]}")
	}
	out.WriteString(`],"files":[` + jsonList(files) + "]}\n")
	return out.String()
}

// jsonStamp is always present, `null` when absent, so no reader tests for the
// key.
func jsonStamp(stamp *string) string {
	if stamp == nil {
		return "null"
	}
	return jsonString(*stamp)
}

func jsonList(items []string) string {
	quoted := make([]string, len(items))
	for i, item := range items {
		quoted[i] = jsonString(item)
	}
	return strings.Join(quoted, ",")
}

// asIdentifiers is a note's id, a file's name: each half's identity.
func asIdentifiers(notes []notebook.NoteFile, files []string, null bool) string {
	separator := "\n"
	if null {
		separator = "\x00"
	}
	var out strings.Builder
	for _, file := range notes {
		out.WriteString(file.ID + separator)
	}
	for _, file := range files {
		out.WriteString(file + separator)
	}
	return out.String()
}

// jsonString is a JSON string literal, quotes included. It escapes what the
// grammar requires and nothing more, as serde_json does: encoding/json would
// also rewrite `<`, `>`, `&` and U+2028.
func jsonString(text string) string {
	var out strings.Builder
	out.WriteByte('"')
	for _, r := range text {
		switch {
		case r == '"':
			out.WriteString(`\"`)
		case r == '\\':
			out.WriteString(`\\`)
		case r == '\n':
			out.WriteString(`\n`)
		case r == '\r':
			out.WriteString(`\r`)
		case r == '\t':
			out.WriteString(`\t`)
		case r == '\b':
			out.WriteString(`\b`)
		case r == '\f':
			out.WriteString(`\f`)
		case r < 0x20:
			out.WriteString(`\u00` + strconv.FormatInt(int64(r)+0x100, 16)[1:])
		default:
			out.WriteRune(r)
		}
	}
	out.WriteByte('"')
	return out.String()
}
