package link

import (
	"bytes"
	"sort"
	"strings"

	"github.com/yuin/goldmark/ast"
	"github.com/yuin/goldmark/util"

	"github.com/henry40408/noda/internal/markdown"
)

// span is a half-open byte range of the body.
type span struct{ start, end int }

// Rewrite repoints every destination (inline or reference definition) resolving
// to old at new, touching only the destination's bytes. False when the body
// names old nowhere.
//
// A destination written with backslash escapes cannot be located in the source,
// so the caller re-checks with Targets.
func Rewrite(body, old, replacement string) (string, bool) {
	doc := markdown.Plain(body)
	var spans []span
	doc.Walk(func(node ast.Node) {
		destination, ok := destinationOf(node)
		if !ok || !namesFile(destination, old) {
			return
		}
		// A reference-style usage's bytes are in the definition, handled below.
		if found, ok := inlineDestination(doc.Source, node, destination); ok {
			spans = append(spans, pathPart(found, destination))
		}
	})
	for _, reference := range doc.References {
		destination := markdown.Unescape(reference.Destination())
		if !namesFile(destination, old) {
			continue
		}
		if found, ok := definitionDestination(doc, reference); ok {
			spans = append(spans, pathPart(found, destination))
		}
	}
	if len(spans) == 0 {
		return "", false
	}

	// Applied back to front so an earlier edit cannot move a later one's bytes.
	sort.Slice(spans, func(i, j int) bool { return spans[i].start < spans[j].start })
	encoded := encodeDestination(replacement)
	out := body
	previous := -1
	for i := len(spans) - 1; i >= 0; i-- {
		if spans[i].start == previous {
			continue
		}
		previous = spans[i].start
		out = out[:spans[i].start] + encoded + out[spans[i].end:]
	}
	return out, true
}

func namesFile(destination, old string) bool {
	path, ok := Target(destination)
	return ok && path == old
}

// pathPart narrows a destination's bytes to its path: a `#page=2` or `?v=2`
// suffix stays put.
func pathPart(found span, destination string) span {
	length := len(destination)
	if at := strings.IndexAny(destination, "#?"); at >= 0 {
		length = at
	}
	return span{found.start, found.start + length}
}

// inlineDestination locates where `[label](destination)` wrote its destination.
// goldmark gives no offsets for a link, so it starts from the end of the label's
// last text and takes the first `](` that is followed by this very destination.
func inlineDestination(source []byte, node ast.Node, destination string) (span, bool) {
	from, ok := labelEnd(node)
	if !ok {
		return span{}, false
	}
	for {
		at := bytes.Index(source[from:], []byte("]("))
		if at < 0 {
			return span{}, false
		}
		from += at + 2
		if found, ok := writtenDestination(source, from); ok && string(source[found.start:found.end]) == destination {
			return found, true
		}
	}
}

// labelEnd is where the last piece of the label's text stops. An empty label has
// no text to anchor on, so it is left for the caller's re-check.
func labelEnd(node ast.Node) (int, bool) {
	end, found := 0, false
	_ = ast.Walk(node, func(child ast.Node, entering bool) (ast.WalkStatus, error) {
		if text, ok := child.(*ast.Text); ok && entering && text.Segment.Stop > end {
			end, found = text.Segment.Stop, true
		}
		return ast.WalkContinue, nil
	})
	return end, found
}

// writtenDestination reads a link destination as CommonMark spells it starting
// at from: optional whitespace, then `<...>` or a run with balanced parentheses.
func writtenDestination(source []byte, from int) (span, bool) {
	at := skipSpace(source, from)
	if at >= len(source) {
		return span{}, false
	}
	if source[at] == '<' {
		return angleDestination(source, at+1)
	}
	return bareDestination(source, at)
}

func skipSpace(source []byte, at int) int {
	for at < len(source) && bytes.IndexByte([]byte(" \t\r\n"), source[at]) >= 0 {
		at++
	}
	return at
}

func angleDestination(source []byte, start int) (span, bool) {
	for at := start; at < len(source); at++ {
		switch source[at] {
		case '\\':
			at++
		case '\n', '<':
			return span{}, false
		case '>':
			return span{start, at}, true
		}
	}
	return span{}, false
}

func bareDestination(source []byte, start int) (span, bool) {
	depth := 0
	for at := start; at < len(source); at++ {
		switch ch := source[at]; {
		case ch == '\\':
			at++
		case ch == '(':
			depth++
		case ch == ')' && depth == 0:
			return span{start, at}, at > start
		case ch == ')':
			depth--
		case ch <= ' ':
			return span{start, at}, at > start
		}
	}
	return span{start, len(source)}, len(source) > start
}

// definitionDestination finds the `[label]: destination` line of a reference.
// goldmark drops definitions from the tree and keeps no offsets, so the source
// is scanned for the line, skipping anything inside a code block.
func definitionDestination(doc markdown.Document, reference interface {
	Label() []byte
	Destination() []byte
}) (span, bool) {
	code := codeRanges(doc)
	label := util.ToLinkReference(reference.Label())
	destination := markdown.Unescape(reference.Destination())
	for _, start := range lineStarts(doc.Source) {
		if inside(code, start) {
			continue
		}
		found, ok := definitionOnLine(doc.Source, start, label)
		if ok && string(doc.Source[found.start:found.end]) == destination {
			return found, true
		}
	}
	return span{}, false
}

// definitionOnLine reads `[label]:` at the start of a line (up to three spaces
// of indent) and returns the destination's bytes when the label matches.
func definitionOnLine(source []byte, lineStart int, label string) (span, bool) {
	at := lineStart
	for at < len(source) && source[at] == ' ' && at-lineStart < 3 {
		at++
	}
	if at >= len(source) || source[at] != '[' {
		return span{}, false
	}
	open := at + 1
	for at = open; at < len(source) && source[at] != ']'; at++ {
		if source[at] == '\\' {
			at++
		}
	}
	if at+1 >= len(source) || source[at+1] != ':' || util.ToLinkReference(source[open:at]) != label {
		return span{}, false
	}
	return writtenDestination(source, at+2)
}

func lineStarts(source []byte) []int {
	starts := []int{0}
	for at, ch := range source {
		if ch == '\n' && at+1 < len(source) {
			starts = append(starts, at+1)
		}
	}
	return starts
}

func codeRanges(doc markdown.Document) []span {
	var ranges []span
	doc.Walk(func(node ast.Node) {
		switch node.Kind() {
		case ast.KindFencedCodeBlock, ast.KindCodeBlock, ast.KindHTMLBlock:
		default:
			return
		}
		lines := node.Lines()
		if lines.Len() == 0 {
			return
		}
		ranges = append(ranges, span{lines.At(0).Start, lines.At(lines.Len() - 1).Stop})
	})
	return ranges
}

func inside(ranges []span, at int) bool {
	for _, r := range ranges {
		if at >= r.start && at < r.end {
			return true
		}
	}
	return false
}

// encodeDestination encodes what would otherwise end a destination or start its
// title.
func encodeDestination(name string) string {
	var out strings.Builder
	for _, ch := range name {
		switch ch {
		case '%':
			out.WriteString("%25")
		case ' ':
			out.WriteString("%20")
		case '(':
			out.WriteString("%28")
		case ')':
			out.WriteString("%29")
		case '<':
			out.WriteString("%3C")
		case '>':
			out.WriteString("%3E")
		case '"':
			out.WriteString("%22")
		default:
			out.WriteRune(ch)
		}
	}
	return out.String()
}
