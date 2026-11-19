package importer

import (
	"encoding/json"
	"errors"
	"fmt"
	"slices"
	"strings"
)

// Reading a TiddlyWiki 5 export: the JSON array "export all" writes, or a saved
// single-file wiki carrying that array in a `<script>`. Bodies are not
// converted here; the import commits them as the wiki held them first.
//
//   - `created` / `modified` are `YYYYMMDDhhmmssXXX` in UTC and become RFC 3339.
//   - `tags` is space-separated, with `[[double brackets]]` around a tag that
//     contains a space.
//   - Other string fields are carried through, except the wiki's bookkeeping
//     (`revision`, `bag`, `type`). A multi-line value becomes one double-quoted
//     line with JSON escapes, which YAML reads back as the same string; a block
//     scalar would not survive, because noda reads frontmatter a line at a time.

// claimed are the fields noda interprets or leaves out.
var claimed = map[string]bool{
	"title": true, "text": true, "tags": true, "created": true, "modified": true,
	"revision": true, "bag": true, "type": true,
}

const storeOpen = `<script class="tiddlywiki-tiddler-store" type="application/json">`

// Export is what a TiddlyWiki file held.
type Export struct {
	Notes []Incoming
	// Skipped is what was not imported, and why.
	Skipped []Skipped
}

// ReadTiddlyWiki reads an export, from either a `.json` file or a saved
// single-file wiki.
func ReadTiddlyWiki(text string) (Export, error) {
	js := strings.TrimSpace(text)
	if !strings.HasPrefix(js, "[") {
		var err error
		if js, err = store(text); err != nil {
			return Export{}, err
		}
	}
	var parsed any
	if err := json.Unmarshal([]byte(js), &parsed); err != nil {
		return Export{}, fmt.Errorf("this does not look like a TiddlyWiki export: %v\n"+
			"expected the JSON array that `export all` writes, or a saved wiki", err)
	}
	tiddlers, ok := parsed.([]any)
	if !ok {
		return Export{}, errors.New("a TiddlyWiki export is a JSON array of tiddlers")
	}

	var export Export
	for _, tiddler := range tiddlers {
		title := stringField(tiddler, "title")
		note, reason := incoming(tiddler, title)
		if reason != "" {
			export.Skipped = append(export.Skipped, Skipped{Title: title, Reason: reason})
			continue
		}
		export.Notes = append(export.Notes, note)
	}
	return export, nil
}

// store is the JSON inside a saved wiki's store. Script content is raw text
// and must not be unescaped, or a tiddler's own `&amp;` changes. Of several
// stores, the biggest is the wiki.
func store(text string) (string, error) {
	best := ""
	rest := text
	for {
		at := strings.Index(rest, storeOpen)
		if at < 0 {
			break
		}
		from := rest[at+len(storeOpen):]
		end := strings.Index(from, "</script>")
		if end < 0 {
			break
		}
		if end > len(best) {
			best = strings.TrimSpace(from[:end])
		}
		rest = from[end:]
	}
	if best == "" {
		return "", errors.New("no tiddler store in this file\n" +
			"a saved TiddlyWiki holds one; a rendered copy of a wiki does not")
	}
	return best, nil
}

// textTypes are the tiddler types whose text is a note.
var textTypes = map[string]bool{
	"": true, "text/vnd.tiddlywiki": true, "text/plain": true,
	"text/markdown": true, "text/x-markdown": true,
}

// incoming is one tiddler as a note, or the reason it is not one.
func incoming(tiddler any, title string) (Incoming, string) {
	if title == "" {
		return Incoming{}, "no title"
	}
	// TiddlyWiki's own namespace.
	if strings.HasPrefix(title, "$:/") {
		return Incoming{}, "system tiddler"
	}
	if kind := stringField(tiddler, "type"); !textTypes[kind] {
		return Incoming{}, fmt.Sprintf("not text (%s)", kind)
	}
	body := stringField(tiddler, "text")
	if strings.TrimSpace(body) == "" {
		return Incoming{}, "empty"
	}
	return Incoming{
		Title:   title,
		Body:    strings.TrimLeft(body, "\n"),
		Tags:    splitTags(stringField(tiddler, "tags")),
		Created: stamp(stringField(tiddler, "created")),
		Updated: stamp(stringField(tiddler, "modified")),
		Extra:   carriedFields(tiddler),
		Key:     title,
	}, ""
}

// carriedFields are the string fields noda does not claim, in name order as
// the Rust map held them.
func carriedFields(tiddler any) []string {
	fields, ok := tiddler.(map[string]any)
	if !ok {
		return nil
	}
	names := make([]string, 0, len(fields))
	for name := range fields {
		names = append(names, name)
	}
	slices.Sort(names)

	var extra []string
	for _, name := range names {
		value, isString := fields[name].(string)
		if claimed[name] || !isString {
			continue
		}
		if strings.ContainsAny(value, "\n\r") {
			value = quoted(value)
		}
		extra = append(extra, name+": "+value)
	}
	return extra
}

// quoted is value as a JSON string, escaped as serde_json writes it: only what
// JSON requires.
func quoted(value string) string {
	var out strings.Builder
	out.WriteByte('"')
	for _, r := range value {
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
			fmt.Fprintf(&out, `\u%04x`, r)
		default:
			out.WriteRune(r)
		}
	}
	out.WriteByte('"')
	return out.String()
}

func stringField(tiddler any, field string) string {
	fields, ok := tiddler.(map[string]any)
	if !ok {
		return ""
	}
	value, _ := fields[field].(string)
	return value
}

// splitTags splits a TiddlyWiki tag list: space-separated, `[[double
// brackets]]` around a tag with a space; deduplicated.
func splitTags(field string) []string {
	var found []string
	seen := map[string]bool{}
	rest := strings.TrimSpace(field)
	for rest != "" {
		tag, next := nextTag(rest)
		tag = strings.TrimSpace(tag)
		if tag != "" && !seen[tag] {
			seen[tag] = true
			found = append(found, tag)
		}
		rest = trimStart(next)
	}
	return found
}

func nextTag(rest string) (tag, next string) {
	if after, ok := strings.CutPrefix(rest, "[["); ok {
		// Unclosed: keep the rest as a tag rather than lose it.
		tag, next, closed := strings.Cut(after, "]]")
		if !closed {
			return after, ""
		}
		return tag, next
	}
	tag, next, _ = strings.Cut(rest, " ")
	return tag, next
}

// stamp turns `YYYYMMDDhhmmssXXX` in UTC to RFC 3339, keeping the
// milliseconds, which would otherwise be lost for good. It is nil when value
// is no timestamp.
func stamp(value string) *string {
	digits := strings.TrimSpace(value)
	if len(digits) < 14 || strings.Trim(digits, "0123456789") != "" {
		return nil
	}
	millis := ""
	if len(digits) >= 17 {
		millis = "." + digits[14:17]
	}
	text := fmt.Sprintf("%s-%s-%sT%s:%s:%s%sZ",
		digits[0:4], digits[4:6], digits[6:8], digits[8:10], digits[10:12], digits[12:14], millis)
	return &text
}
