package config

import (
	"fmt"
	"sort"
	"strings"
	"unicode"

	"github.com/pelletier/go-toml/v2/unstable"
)

// The editor below changes one top-level key of a TOML file and leaves every
// other byte alone, so a hand-edited config keeps its comments and layout. It
// follows toml_edit, which the Rust version used, in the places its output is
// visible: a replaced value loses its trailing comment, a removed key takes the
// comment lines above it, and every line ending of a key or header is `\n`.

// entry is one `key = value` of the document.
type entry struct {
	name  string // the first part of the key
	plain bool   // the key has exactly one part
	top   bool   // it comes before the first table header
	eq    int    // offset of the `=`
	end   int    // offset just past the line the value ends on
}

type layout struct {
	entries []entry
	// lineEnds are the offsets past each key-value and header line.
	lineEnds []int
	// names are the first parts of every key and header: the document's root.
	names map[string]struct{}
}

func scan(text string) (layout, error) {
	lay := layout{names: map[string]struct{}{}}
	var parser unstable.Parser
	parser.Reset([]byte(text))
	seenHeader := false
	for parser.NextExpression() {
		node := parser.Expression()
		switch node.Kind {
		case unstable.Table, unstable.ArrayTable:
			seenHeader = true
			first, _ := keyParts(node)
			lay.names[first.name] = struct{}{}
			lay.lineEnds = append(lay.lineEnds, lineEnd(text, first.offset))
		case unstable.KeyValue:
			lay.addEntry(text, node, !seenHeader)
		}
	}
	if err := parser.Error(); err != nil {
		return layout{}, err
	}
	return lay, nil
}

func (lay *layout) addEntry(text string, node *unstable.Node, top bool) {
	first, parts := keyParts(node)
	valueEnd := int(node.Raw.Offset + node.Raw.Length)
	for valueEnd > 0 && unicode.IsSpace(rune(text[valueEnd-1])) {
		valueEnd--
	}
	eq := first.endOffset
	for eq < len(text) && text[eq] != '=' {
		eq++
	}
	end := lineEnd(text, valueEnd)
	lay.entries = append(lay.entries, entry{name: first.name, plain: parts == 1, top: top, eq: eq, end: end})
	lay.lineEnds = append(lay.lineEnds, end)
	if top {
		lay.names[first.name] = struct{}{}
	}
}

type keyStart struct {
	name      string
	offset    int
	endOffset int
}

// keyParts returns the first part of a node's key and how many parts it has.
func keyParts(node *unstable.Node) (keyStart, int) {
	var first keyStart
	count := 0
	for it := node.Key(); it.Next(); {
		part := it.Node()
		if count == 0 {
			first = keyStart{string(part.Data), int(part.Raw.Offset), int(part.Raw.Offset + part.Raw.Length)}
		}
		count++
	}
	return first, count
}

// lineEnd is the offset past the newline of the line holding offset.
func lineEnd(text string, offset int) int {
	newline := strings.IndexByte(text[offset:], '\n')
	if newline < 0 {
		return len(text)
	}
	return offset + newline + 1
}

// withLFEndings drops the `\r` of every key-value and header line.
func withLFEndings(text string, lay layout) string {
	var drop []int
	for _, end := range lay.lineEnds {
		if end >= 2 && text[end-2:end] == "\r\n" {
			drop = append(drop, end-2)
		}
	}
	sort.Ints(drop)
	var out strings.Builder
	previous := 0
	for _, at := range drop {
		out.WriteString(text[previous:at])
		previous = at + 1
	}
	out.WriteString(text[previous:])
	return out.String()
}

// editable normalises line endings, then scans, so offsets match the text the
// edit works on.
func editable(text string) (string, layout, error) {
	lay, err := scan(text)
	if err != nil {
		return "", layout{}, err
	}
	text = withLFEndings(text, lay)
	lay, err = scan(text)
	return text, lay, err
}

// topEntry is the existing top-level key-value for name, if any.
func (lay layout) topEntry(name string) (entry, bool) {
	for _, e := range lay.entries {
		if e.top && e.name == name {
			return e, true
		}
	}
	return entry{}, false
}

// hasKey reports whether name is already a key or a table of the root.
func (lay layout) hasKey(name string) bool {
	_, found := lay.names[name]
	return found
}

// setKey writes `key = value` (value already in TOML syntax).
func setKey(text, key, value string) (string, error) {
	text, lay, err := editable(text)
	if err != nil {
		return "", err
	}
	line := key + " = " + value + "\n"
	if existing, found := lay.topEntry(key); found && existing.plain {
		return text[:existing.eq+1] + " " + value + "\n" + text[existing.end:], nil
	}
	// A table or dotted key of that name would turn a second `key =` into
	// invalid TOML, so it is left for the user.
	if lay.hasKey(key) {
		return "", fmt.Errorf("%s is a table in config.toml, not a setting: fix it by hand", key)
	}
	return ensureFinalNewline(insertKey(text, lay, line)), nil
}

// insertKey puts a new line after the last top-level key, or at the top when
// there is none. The first key added to a file of nothing but comments takes
// them with it: the starter config is all comments, and the key lands under
// them, not above.
func insertKey(text string, lay layout, line string) string {
	if len(lay.names) == 0 && strings.TrimSpace(text) != "" {
		return strings.TrimRight(text, "\n") + "\n" + line
	}
	at := 0
	for _, e := range lay.entries {
		if e.top {
			at = e.end
		}
	}
	prefix := text[:at]
	if prefix != "" && !strings.HasSuffix(prefix, "\n") {
		prefix += "\n"
	}
	return prefix + line + text[at:]
}

func ensureFinalNewline(text string) string {
	if text == "" || strings.HasSuffix(text, "\n") {
		return text
	}
	return text + "\n"
}

// removeKey deletes a top-level key with the comment lines above it, as
// toml_edit does. False when the key is not there.
func removeKey(text, key string) (string, bool, error) {
	text, lay, err := editable(text)
	if err != nil {
		return "", false, err
	}
	if _, found := lay.topEntry(key); !found {
		return text, false, nil
	}
	var out strings.Builder
	previousEnd := 0
	kept := 0
	for _, e := range lay.entries {
		if e.top && e.name == key {
			out.WriteString(text[kept:previousEnd])
			kept = e.end
		}
		previousEnd = e.end
	}
	out.WriteString(text[kept:])
	return ensureFinalNewline(out.String()), true, nil
}

// tomlString writes a string the way toml_edit picks its form: a literal string
// when it holds a backslash or a double quote and nothing a literal cannot
// carry, else a basic one, multi-line when it holds a newline.
func tomlString(value string) string {
	multiline := strings.Contains(value, "\n")
	if canBeLiteral(value) {
		if multiline {
			return "'''\n" + value + "'''"
		}
		return "'" + value + "'"
	}
	escaped := escapeBasic(value, multiline)
	if multiline {
		return "\"\"\"\n" + escaped + "\"\"\""
	}
	return "\"" + escaped + "\""
}

func canBeLiteral(value string) bool {
	if !strings.ContainsAny(value, "\\\"") {
		return false
	}
	if strings.Contains(value, "'''") || strings.HasSuffix(value, "'") {
		return false
	}
	// A single-line literal string ends at the first quote, so one inside
	// would cut the value short and leave config.toml unparseable.
	if strings.Contains(value, "'") && !strings.Contains(value, "\n") {
		return false
	}
	for _, ch := range value {
		if ch != '\n' && ch != '\t' && (ch < 0x20 || ch == 0x7f) {
			return false
		}
	}
	return true
}

func escapeBasic(value string, multiline bool) string {
	var out strings.Builder
	for _, ch := range value {
		switch {
		case ch == '"':
			out.WriteString(`\"`)
		case ch == '\\':
			out.WriteString(`\\`)
		case ch == '\n' && multiline:
			out.WriteByte('\n')
		case ch == '\n':
			out.WriteString(`\n`)
		case ch == '\t':
			out.WriteString(`\t`)
		case ch == '\r':
			out.WriteString(`\r`)
		case ch == '\b':
			out.WriteString(`\b`)
		case ch == '\f':
			out.WriteString(`\f`)
		case ch < 0x20 || ch == 0x7f:
			out.WriteString(`\u00`)
			out.WriteString(strings.ToUpper(string("0123456789abcdef"[ch>>4]) + string("0123456789abcdef"[ch&15])))
		default:
			out.WriteRune(ch)
		}
	}
	return out.String()
}
