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
	spans := append(inlineSpans(doc, old), definitionSpans(doc, old)...)
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

// inlineSpans locates the destination of every inline link or image naming old.
// Nodes are visited on the way out, the order their `](` appear in the source:
// an image inside a link closes first. Each search starts after the previous
// hit in the same block, which is what anchors a link with an empty label.
func inlineSpans(doc markdown.Document, old string) []span {
	searchable := maskCodeSpans(doc)
	cursors := map[ast.Node]int{}
	var spans []span
	_ = ast.Walk(doc.Root, func(node ast.Node, entering bool) (ast.WalkStatus, error) {
		if entering {
			return ast.WalkContinue, nil
		}
		destination, ok := destinationOf(node)
		if !ok || !namesFile(destination, old) {
			return ast.WalkContinue, nil
		}
		block, window, ok := enclosingBlock(node)
		if !ok {
			return ast.WalkContinue, nil
		}
		from := max(cursors[block], window.start, labelEnd(node))
		found, ok := inlineDestination(searchable[:window.end], from, destination)
		if !ok {
			return ast.WalkContinue, nil
		}
		cursors[block] = found.end
		spans = append(spans, pathPart(found, destination))
		return ast.WalkContinue, nil
	})
	return spans
}

// maskCodeSpans blanks what is inside code spans, so a `[b](x.md)` written in
// code is never taken for the link's destination. Offsets stay the same.
func maskCodeSpans(doc markdown.Document) []byte {
	masked := bytes.Clone(doc.Source)
	doc.Walk(func(node ast.Node) {
		if node.Kind() != ast.KindCodeSpan {
			return
		}
		for child := node.FirstChild(); child != nil; child = child.NextSibling() {
			text, ok := child.(*ast.Text)
			if !ok {
				continue
			}
			for at := text.Segment.Start; at < text.Segment.Stop; at++ {
				masked[at] = ' '
			}
		}
	})
	return masked
}

// enclosingBlock is the nearest block with source lines, and the bytes they
// cover: a link's destination is never outside it.
func enclosingBlock(node ast.Node) (ast.Node, span, bool) {
	for block := node.Parent(); block != nil; block = block.Parent() {
		if block.Type() != ast.TypeBlock {
			continue
		}
		lines := block.Lines()
		if lines.Len() > 0 {
			return block, span{lines.At(0).Start, lines.At(lines.Len() - 1).Stop}, true
		}
	}
	return nil, span{}, false
}

// inlineDestination locates where `[label](destination)` wrote its destination.
// goldmark gives no offsets for a link, so it takes the first `](` at or after
// from that is followed by this very destination. source ends where the
// enclosing block does.
func inlineDestination(source []byte, from int, destination string) (span, bool) {
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

// labelEnd is where the last piece of the label's text stops, or 0 for a label
// with no text (`![](pic.png)`), which the caller anchors on the previous hit.
func labelEnd(node ast.Node) int {
	end := 0
	_ = ast.Walk(node, func(child ast.Node, entering bool) (ast.WalkStatus, error) {
		if text, ok := child.(*ast.Text); ok && entering {
			end = max(end, text.Segment.Stop)
		}
		return ast.WalkContinue, nil
	})
	return end
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

func definitionSpans(doc markdown.Document, old string) []span {
	var spans []span
	definitions := definitionStarts(doc.Root)
	for _, reference := range doc.References {
		destination := markdown.Unescape(reference.Destination())
		if !namesFile(destination, old) {
			continue
		}
		label := util.ToLinkReference(reference.Label())
		if found, ok := definitionDestination(doc.Source, definitions, label, destination); ok {
			spans = append(spans, pathPart(found, destination))
		}
	}
	return spans
}

// definitionStarts is where each `[label]: destination` definition begins.
// goldmark keeps a node for a definition, with its lines, but not the
// destination's offset. A line that only looks like one (in a fence, or as a
// paragraph's lazy continuation) has no such node.
func definitionStarts(root ast.Node) []int {
	var starts []int
	_ = ast.Walk(root, func(node ast.Node, entering bool) (ast.WalkStatus, error) {
		if entering && node.Kind() == ast.KindLinkReferenceDefinition && node.Lines().Len() > 0 {
			starts = append(starts, node.Lines().At(0).Start)
		}
		return ast.WalkContinue, nil
	})
	return starts
}

// definitionDestination finds the destination bytes of the first definition
// with this label, the one a reader uses.
func definitionDestination(source []byte, starts []int, label, destination string) (span, bool) {
	for _, start := range starts {
		found, ok := definitionOnLine(source, start, label)
		if ok && string(source[found.start:found.end]) == destination {
			return found, true
		}
	}
	return span{}, false
}

// definitionOnLine reads `[label]:` at the given offset and returns the
// destination's bytes when the label matches.
func definitionOnLine(source []byte, at int, label string) (span, bool) {
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
