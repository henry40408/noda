// Package importer brings a notebook in from somewhere else.
//
// A source's reader produces Incoming; everything after is shared, so a new
// source is only a reader. This is the one place outside `internal/cmd` that
// writes notes.
package importer

import (
	"errors"
	"fmt"
	"io/fs"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"syscall"
	"unicode"
	"unicode/utf8"

	"github.com/henry40408/noda/internal/note"
	"github.com/henry40408/noda/internal/notebook"
	"github.com/henry40408/noda/internal/style"
)

// Unconverted is the frontmatter field naming what an importer did not
// translate; shared by every source so `doctor` has one check.
const Unconverted = "unconverted"

// SourceKey is what the source called this note, so re-importing it skips it.
const SourceKey = "source_key"

// Converter turns a source's body into Markdown, given a way to resolve the
// names notes use for each other.
type Converter func(body string, resolve Resolve) Converted

// Incoming is one note as a source hands it over, before noda has given it an
// identity.
type Incoming struct {
	Title string
	// Body is as the source held it; committed before conversion, so history
	// keeps it.
	Body string
	Tags []string
	// Created and Updated are RFC 3339, when the source had one to give.
	Created *string
	Updated *string
	// Extra is other source fields, carried through untouched.
	Extra []string
	// Key is what other notes link to it by; a tiddler's title.
	Key string
}

// Skipped is a note that was not imported, and why.
type Skipped struct{ Title, Reason string }

// Request is an import into the active notebook.
type Request struct {
	// Source names where the notes came from, as the summary and commits say.
	Source   string
	Incoming []Incoming
	Skipped  []Skipped
	// Convert is nil for a source already in Markdown, which gets only the
	// first commit.
	Convert Converter
}

type report struct {
	written   int
	converted int
	skipped   []Skipped
	// notConverted are notes written with the source's text whose conversion
	// could not be.
	notConverted []Skipped
	// left counts the constructs left in WikiText, by how many notes carry each.
	left map[string]int
}

// named is a note with the id and slug it will be written under.
type named struct {
	id, slug string
	note     Incoming
}

func (n named) file() string { return note.FileName(n.id, n.slug) }

// Write writes an import into the notebook, as two commits.
//
// The source's own text, then the conversion, so `noda diff` shows the
// conversion and `restore` reaches the original.
func Write(active *notebook.Notebook, req Request) (string, error) {
	r := &report{skipped: req.Skipped, left: map[string]int{}}

	pending, byKey, err := nameIncoming(active, req.Incoming, r)
	if err != nil {
		return "", err
	}
	if len(pending) == 0 {
		return r.summary(req.Source, false), nil
	}

	written := writeSourceText(active, pending, byKey, r)
	if len(written) == 0 {
		return r.summary(req.Source, false), nil
	}
	if err := commit(active, written, fmt.Sprintf("import: %d notes from %s", r.written, req.Source)); err != nil {
		return "", err
	}
	if req.Convert == nil {
		return r.summary(req.Source, false), nil
	}

	// Pass two: the conversion, once every name is resolvable.
	resolve := func(key string) (string, bool) {
		file, ok := byKey[key]
		return file, ok
	}
	changed := convertWritten(active, written, func(body string) Converted { return req.Convert(body, resolve) }, r)
	if len(changed) > 0 {
		message := fmt.Sprintf("import: convert %d notes from %s", r.converted, req.Source)
		if err := commit(active, changed, message); err != nil {
			return "", err
		}
	}
	return r.summary(req.Source, true), nil
}

// nameIncoming gives each note that will be imported its name, and says why
// the others will not be. It also returns where every source name ends up, for
// rewriting links; seeded with what is already held, since a wiki may be
// imported in pieces.
func nameIncoming(active *notebook.Notebook, incoming []Incoming, r *report) ([]named, map[string]string, error) {
	held, err := alreadyHere(active)
	if err != nil {
		return nil, nil, err
	}
	taken, err := active.TakenIDs()
	if err != nil {
		return nil, nil, err
	}
	byKey := make(map[string]string, len(held))
	for key, file := range held {
		byKey[key] = file
	}

	var pending []named
	for _, in := range incoming {
		if file, ok := held[in.Key]; ok {
			r.skip(in.Title, "already imported as "+file)
			continue
		}
		// Exports taken in pieces overlap; the first copy lands.
		if _, ok := byKey[in.Key]; ok {
			r.skip(in.Title, "given twice in this import")
			continue
		}
		if err := check(in); err != nil {
			r.skip(in.Title, err.Error())
			continue
		}
		id := note.MintID(taken)
		taken[note.NormalizeID(id)] = struct{}{}
		slug := note.Slugify(in.Title)
		byKey[in.Key] = note.FileName(id, slug)
		pending = append(pending, named{id: id, slug: slug, note: in})
	}
	return pending, byKey, nil
}

func (r *report) skip(title, reason string) {
	r.skipped = append(r.skipped, Skipped{Title: title, Reason: reason})
}

// writeSourceText is pass one: every note as the source wrote it. A write that
// fails is skipped rather than ending the run, which would leave the rest
// written but uncommitted.
func writeSourceText(active *notebook.Notebook, pending []named, byKey map[string]string, r *report) []named {
	var written []named
	for _, n := range pending {
		if err := os.WriteFile(filepath.Join(active.Path, n.file()), []byte(render(n.note, n.note.Body, nil)), 0o666); err != nil {
			delete(byKey, n.note.Key)
			r.skip(n.note.Title, Reason(err))
			continue
		}
		r.written++
		written = append(written, n)
	}
	return written
}

// convertWritten is pass two: it rewrites the notes the conversion changed,
// and returns their files.
func convertWritten(active *notebook.Notebook, written []named, convert func(string) Converted, r *report) []named {
	var changed []named
	for _, n := range written {
		converted := convert(n.note.Body)
		if converted.Text == n.note.Body && len(converted.Left) == 0 {
			continue
		}
		text := render(n.note, converted.Text, converted.Left)
		// As in pass one: reported, not fatal; the source's text is committed.
		if err := os.WriteFile(filepath.Join(active.Path, n.file()), []byte(text), 0o666); err != nil {
			r.notConverted = append(r.notConverted, Skipped{Title: n.note.Title, Reason: Reason(err)})
			continue
		}
		// Counted only once the `unconverted:` field is on disk.
		for _, name := range converted.Left {
			r.left[name]++
		}
		changed = append(changed, n)
		r.converted++
	}
	return changed
}

// render is the source's own fields carried through, plus `source_key` and
// `unconverted`.
func render(in Incoming, body string, left []string) string {
	extra := append(append([]string(nil), in.Extra...), SourceKey+": "+in.Key)
	if len(left) > 0 {
		extra = append(extra, Unconverted+": "+strings.Join(left, ", "))
	}
	return note.Note{
		Title:   in.Title,
		Tags:    in.Tags,
		Created: in.Created,
		Updated: in.Updated,
		Extra:   extra,
		Body:    body,
	}.Render()
}

// alreadyHere is what the notebook already holds, keyed by the name its source
// knew it as.
func alreadyHere(active *notebook.Notebook) (map[string]string, error) {
	notes, _, err := active.Inventory()
	if err != nil {
		return nil, err
	}
	held := map[string]string{}
	for _, file := range notes {
		for _, line := range file.Note.Extra {
			if key, ok := strings.CutPrefix(line, SourceKey+": "); ok {
				held[strings.TrimSpace(key)] = note.FileName(file.ID, file.Slug)
				break
			}
		}
	}
	return held, nil
}

// check refuses a note noda could not write faithfully, rather than write one
// that reads back as something else.
func check(in Incoming) error {
	if err := note.ValidateTitle(in.Title); err != nil {
		return err
	}
	for _, tag := range in.Tags {
		if err := note.ValidateTag(tag); err != nil {
			return err
		}
	}
	// A title with nothing alphanumeric has no slug.
	if note.Slugify(in.Title) == "" {
		return errors.New("the title makes no filename")
	}
	for _, line := range in.Extra {
		if strings.HasPrefix(line, SourceKey+":") || strings.HasPrefix(line, Unconverted+":") {
			return fmt.Errorf("carries its own `%s` or `%s` field, which the import would overwrite", SourceKey, Unconverted)
		}
	}
	return nil
}

func commit(active *notebook.Notebook, notes []named, message string) error {
	files := make([]string, len(notes))
	for i, n := range notes {
		files[i] = n.file()
	}
	return active.Commit(files, message)
}

// Reason is why a file operation failed, in the words the Rust version used
// (`Permission denied (os error 13)`), so one reason groups the same failures
// in the summary.
func Reason(err error) string {
	var pathErr *fs.PathError
	if errors.As(err, &pathErr) {
		err = pathErr.Err
	}
	var errno syscall.Errno
	if !errors.As(err, &errno) {
		return err.Error()
	}
	text := errno.Error()
	first, size := utf8.DecodeRuneInString(text)
	return fmt.Sprintf("%c%s (os error %d)", unicode.ToUpper(first), text[size:], int(errno))
}

func noun(n int) string {
	if n == 1 {
		return "note"
	}
	return "notes"
}

// reasons writes a heading and the reasons under it, counted rather than
// listing every title, which a large export would make unreadable.
func reasons(out *strings.Builder, heading string, entries []Skipped) {
	if len(entries) == 0 {
		return
	}
	why := map[string]int{}
	for _, entry := range entries {
		why[entry.Reason]++
	}
	out.WriteString("\n" + style.Muted.Paint(heading) + "\n")
	for _, reason := range sortedKeys(why) {
		fmt.Fprintf(out, "  %d %s\n", why[reason], reason)
	}
}

func sortedKeys(counts map[string]int) []string {
	keys := make([]string, 0, len(counts))
	for key := range counts {
		keys = append(keys, key)
	}
	slices.Sort(keys)
	return keys
}

// summary is what the import prints. converted says the second pass ran, which
// is when pointing at `noda diff` makes sense.
func (r *report) summary(source string, converted bool) string {
	var out strings.Builder
	fmt.Fprintf(&out, "imported  %d %s from %s\n", r.written, noun(r.written), source)
	if r.converted > 0 {
		fmt.Fprintf(&out, "converted %d %s\n", r.converted, noun(r.converted))
	}
	if len(r.left) > 0 {
		out.WriteString("\n" + style.Muted.Paint("left as WikiText, and named in each note's `unconverted:` field:") + "\n")
		for _, name := range sortedKeys(r.left) {
			fmt.Fprintf(&out, "  %d %s %s\n", r.left[name], noun(r.left[name]), name)
		}
	}
	reasons(&out, "not imported:", r.skipped)
	reasons(&out, "imported, but left as the source wrote them:", r.notConverted)
	if converted && r.converted > 0 {
		out.WriteString("\n" + style.Muted.Paint("`noda diff` shows the conversion; the commit before it holds the originals"))
	}
	return strings.TrimRightFunc(out.String(), unicode.IsSpace)
}
