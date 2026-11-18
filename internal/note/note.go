// Package note is a note: its Markdown file, and the identity its filename
// spells out.
//
// The id lives in the filename: git forbids two entries at one path, so
// uniqueness is structural, and two machines each adding a note merge cleanly.
//
// The frontmatter's presence marks a file as a note. noda interprets its own
// fields and no others; any other field survives a write-back untouched.
package note

import (
	"errors"
	"strings"
	"time"
	"unicode"
)

// Note is a parsed note. The id is not here: it is the filename.
type Note struct {
	Title string
	Tags  []string
	// Created is RFC 3339, exactly as found and never respelled: an imported
	// `+08:00` keeps its offset, so `noda show` still matches the file.
	Created *string
	Updated *string
	// Pinned is kept as written; IsPinned judges it. It is absent rather than
	// `false` when unpinned, so pin + unpin leaves the file byte for byte.
	Pinned *string
	// Extra holds the frontmatter lines noda does not interpret, in order, so a
	// write-back keeps them. None can be `---` (it would have closed the
	// block), so Render never writes what Parse cannot read.
	Extra []string
	Body  string
}

// PinnedValue is what `pin` writes. `unpin` writes no line at all.
const PinnedValue = "true"

var errNoFrontmatter = errors.New("note has no frontmatter block")

// Render writes the block even when empty: it is what marks a note.
func (n Note) Render() string {
	var out strings.Builder
	out.WriteString("---\n")
	out.WriteString("title: " + n.Title + "\n")
	if len(n.Tags) > 0 {
		out.WriteString("tags: [" + strings.Join(n.Tags, ", ") + "]\n")
	}
	for _, field := range []struct {
		key   string
		value *string
	}{{"created", n.Created}, {"updated", n.Updated}, {"pinned", n.Pinned}} {
		if field.value != nil {
			out.WriteString(field.key + ": " + *field.value + "\n")
		}
	}
	// After noda's own fields: their order is kept, their position is not.
	for _, line := range n.Extra {
		out.WriteString(line + "\n")
	}
	out.WriteString("---\n\n")
	out.WriteString(n.Body)
	text := out.String()
	if !strings.HasSuffix(text, "\n") {
		text += "\n"
	}
	return text
}

// Parse reads a note. Fields it does not know are kept in Extra.
func Parse(text string) (Note, error) {
	frontmatter, body, ok := SplitFrontmatter(text)
	if !ok {
		return Note{}, errNoFrontmatter
	}
	var note Note
	for _, line := range lines(frontmatter) {
		key, value, found := strings.Cut(line, ":")
		if !found {
			note.Extra = append(note.Extra, line)
			continue
		}
		value = strings.TrimSpace(value)
		switch strings.TrimSpace(key) {
		case "title":
			note.Title = value
		case "tags":
			note.Tags = parseTags(value)
		// Kept as written rather than refusing the note over a typo;
		// `doctor --times` reports it.
		case "created":
			note.Created = &value
		case "updated":
			note.Updated = &value
		case "pinned":
			note.Pinned = &value
		default:
			note.Extra = append(note.Extra, line)
		}
	}
	note.Body = strings.TrimLeft(body, "\n")
	return note, nil
}

// IsPinned is `true`, case-folded; any other value is not a pin but stays in
// the file.
func (n Note) IsPinned() bool {
	return n.Pinned != nil && strings.EqualFold(*n.Pinned, "true")
}

// SplitFrontmatter splits text into the frontmatter body and everything after
// the closing `---`.
func SplitFrontmatter(text string) (frontmatter, body string, ok bool) {
	rest, found := strings.CutPrefix(text, "---\n")
	if !found {
		return "", "", false
	}
	position := 0
	for position < len(rest) {
		end := len(rest)
		if newline := strings.IndexByte(rest[position:], '\n'); newline >= 0 {
			end = position + newline + 1
		}
		if trimEnd(rest[position:end]) == "---" {
			return rest[:position], rest[end:], true
		}
		position = end
	}
	return "", "", false
}

// trimEnd drops trailing Unicode whitespace, as the closing fence tolerates
// `---  ` and `---\r`.
func trimEnd(line string) string {
	return strings.TrimRightFunc(line, unicode.IsSpace)
}

// Now is RFC 3339, UTC, whole seconds: machine-independent and fixed-width, so
// it sorts as text.
func Now() string {
	return time.Now().UTC().Format("2006-01-02T15:04:05Z")
}

// SetField sets one frontmatter field, leaving every other byte alone, unlike
// Render, which moves unknown fields below noda's own. So `edit` does not
// reorder a block somebody just arranged by hand.
func SetField(text, key, value string) (string, bool) {
	frontmatter, body, ok := SplitFrontmatter(text)
	if !ok {
		return "", false
	}
	var out strings.Builder
	out.WriteString("---\n")
	written := false
	for _, line := range lines(frontmatter) {
		name, _, found := strings.Cut(line, ":")
		if !found || strings.TrimSpace(name) != key {
			out.WriteString(line + "\n")
			continue
		}
		// Parse reads one value, so a duplicate is dropped.
		if !written {
			out.WriteString(key + ": " + value + "\n")
			written = true
		}
	}
	if !written {
		out.WriteString(key + ": " + value + "\n")
	}
	out.WriteString("---\n")
	out.WriteString(body)
	return out.String(), true
}

// SetBody replaces the body and leaves the frontmatter block exactly as found
// (for SetField's reason). CRLF becomes LF: the caller is a browser, and an
// HTML form submits CRLF.
func SetBody(text, body string) (string, bool) {
	frontmatter, _, ok := SplitFrontmatter(text)
	if !ok {
		return "", false
	}
	var out strings.Builder
	out.WriteString("---\n")
	out.WriteString(frontmatter)
	// Render's shape.
	out.WriteString("---\n\n")
	out.WriteString(strings.TrimLeft(strings.ReplaceAll(body, "\r\n", "\n"), "\n"))
	result := out.String()
	if !strings.HasSuffix(result, "\n") {
		result += "\n"
	}
	return result, true
}

func parseTags(value string) []string {
	value = strings.TrimLeft(value, "[")
	value = strings.TrimRight(value, "]")
	var tags []string
	for _, tag := range strings.Split(value, ",") {
		if tag = strings.TrimSpace(tag); tag != "" {
			tags = append(tags, tag)
		}
	}
	return tags
}

// SplitStem splits `<id>-<slug>`. The id alphabet has no `-`, so the first one
// is the boundary however long ids grow. The length floor stops `c-vs-rust`
// reading as the id `c`.
func SplitStem(stem string) (id, slug string, ok bool) {
	id, slug, found := strings.Cut(stem, "-")
	if !found || slug == "" || !IsIDShaped(id) {
		return "", "", false
	}
	return id, slug, true
}

// NamesANote reports whether a file name is `<id>-<slug>.md`. `.md` alone does
// not decide it (`README.md`). It is the test the notebook inventory applies,
// for anything deciding from a name alone.
func NamesANote(name string) bool {
	stem, found := strings.CutSuffix(name, ".md")
	if !found {
		return false
	}
	_, _, ok := SplitStem(stem)
	return ok
}

// ValidateTitle refuses a line break: a title goes into the frontmatter
// verbatim, so it is refused rather than escaped, keeping Render and Parse
// inverse.
func ValidateTitle(title string) error {
	if strings.ContainsAny(title, "\n\r") {
		return errors.New("a title has to fit on one line")
	}
	return nil
}

// ValidateTag refuses what would read back differently: `,`, `[`, `]` or a
// line break.
func ValidateTag(tag string) error {
	if tag == "" {
		return errors.New("a tag needs a name")
	}
	at := strings.IndexAny(tag, "\n\r,[]")
	if at < 0 {
		return nil
	}
	bad := "`" + tag[at:at+1] + "`"
	if tag[at] == '\n' || tag[at] == '\r' {
		bad = "a line break"
	}
	return errors.New("a tag cannot contain " + bad + ": " + tag)
}
