// Package todo reads the action items in a note's body: GFM checkboxes, so any
// other Markdown reader renders them too. Parsed rather than grepped: `- [ ]`
// inside a fence is not a todo.
package todo

import (
	"cmp"
	"strings"

	"github.com/yuin/goldmark/ast"
	extast "github.com/yuin/goldmark/extension/ast"

	"github.com/henry40408/noda/internal/markdown"
)

// Item is one unticked checkbox, as `noda todo` reports it.
type Item struct {
	// Text has its inline markup flattened and `due:` lifted out.
	Text string
	// Due is `YYYY-MM-DD`, when the item named one.
	Due *string
}

// Overdue is a string comparison, valid because `YYYY-MM-DD` sorts as text the
// way it sorts as a date. today is the local date, not UTC.
func (i Item) Overdue(today string) bool {
	return i.Due != nil && *i.Due < today
}

// Items is every unticked checkbox in body, in order. The text stops at the end
// of the item's first paragraph and inline markup is flattened, so
// `[the spec](spec.md)` reads as `the spec`.
func Items(body string) []Item {
	return unticked(markdown.Tasks(body))
}

// Hidden is the items Items misses because raw HTML sits above them with no
// blank line between, which makes them part of the HTML.
func Hidden(body string) []Item {
	// No raw HTML starts without one.
	if !strings.Contains(body, "<") {
		return nil
	}
	doc := markdown.Tasks(body)
	var hidden []Item
	doc.Walk(func(node ast.Node) {
		block, ok := node.(*ast.HTMLBlock)
		// The earlier kinds are comments, `<pre>` and the like: a box in one is
		// meant as text.
		if !ok || block.HTMLBlockType < ast.HTMLBlockType6 {
			return
		}
		// Each block alone, so that a fence left open in one changes nothing
		// outside it.
		inside := string(block.Lines().Value(doc.Source))
		hidden = append(hidden, unticked(markdown.TasksPastHTML(inside))...)
	})
	return hidden
}

func unticked(doc markdown.Document) []Item {
	var found []Item
	doc.Walk(func(node ast.Node) {
		box, ok := node.(*extast.TaskCheckBox)
		if !ok || box.IsChecked {
			return
		}
		if item, ok := itemAfter(doc.Source, box); ok {
			found = append(found, item)
		}
	})
	return found
}

// itemAfter reads what follows the checkbox up to the end of its paragraph. An
// empty `- [ ]` is not an item.
func itemAfter(source []byte, box *extast.TaskCheckBox) (Item, bool) {
	var text strings.Builder
	for sibling := box.NextSibling(); sibling != nil; sibling = sibling.NextSibling() {
		flatten(source, sibling, &text)
	}
	words, due := splitDue(strings.TrimSpace(text.String()))
	if words == "" {
		return Item{}, false
	}
	return Item{Text: words, Due: due}, true
}

// flatten appends the text of an inline node and everything inside it.
func flatten(source []byte, node ast.Node, out *strings.Builder) {
	switch n := node.(type) {
	case *ast.Text:
		out.WriteString(markdown.Unescape(n.Segment.Value(source)))
		if n.SoftLineBreak() || n.HardLineBreak() {
			out.WriteByte(' ')
		}
		return
	case *ast.String:
		out.Write(n.Value)
		return
	case *ast.AutoLink:
		out.Write(n.Label(source))
		return
	case *ast.CodeSpan:
		out.WriteString(codeSpanText(source, n))
		return
	case *ast.RawHTML:
		return
	}
	for child := node.FirstChild(); child != nil; child = child.NextSibling() {
		flatten(source, child, out)
	}
}

// codeSpanText is a code span's content as CommonMark reads it: line endings
// become spaces, and one space is stripped from each side when both are there
// and the content is not all spaces.
func codeSpanText(source []byte, span *ast.CodeSpan) string {
	var raw strings.Builder
	for child := span.FirstChild(); child != nil; child = child.NextSibling() {
		if text, ok := child.(*ast.Text); ok {
			raw.Write(text.Segment.Value(source))
		}
	}
	content := strings.ReplaceAll(raw.String(), "\n", " ")
	if len(content) >= 2 && content[0] == ' ' && content[len(content)-1] == ' ' && strings.Trim(content, " ") != "" {
		return content[1 : len(content)-1]
	}
	return content
}

// Compare orders soonest first, undated last, ties by slug so a listing does
// not reshuffle. Shared so `noda todo` and the browser's todo screen agree.
func Compare(leftSlug string, left Item, rightSlug string, right Item) int {
	switch {
	case left.Due != nil && right.Due != nil:
		if byDue := cmp.Compare(*left.Due, *right.Due); byDue != 0 {
			return byDue
		}
	case left.Due != nil:
		return -1
	case right.Due != nil:
		return 1
	}
	return cmp.Compare(leftSlug, rightSlug)
}

// splitDue lifts a todo.txt-style `due:YYYY-MM-DD` term out of the printed text
// (the file is untouched). The last one wins: two dates means the author moved
// it.
func splitDue(text string) (string, *string) {
	var due *string
	var kept []string
	for _, word := range strings.Fields(text) {
		date, found := strings.CutPrefix(word, "due:")
		if found && isDate(date) {
			due = &date
			continue
		}
		// Includes `due:tomorrow`: a term noda cannot read stays prose.
		kept = append(kept, word)
	}
	return strings.Join(kept, " "), due
}

// isDate checks the shape only, not a date parse: the shape is what makes it
// sort as text, and `2026-13-99` is still the author's word.
func isDate(text string) bool {
	if len(text) != 10 || text[4] != '-' || text[7] != '-' {
		return false
	}
	for _, at := range [...]int{0, 1, 2, 3, 5, 6, 8, 9} {
		if text[at] < '0' || text[at] > '9' {
			return false
		}
	}
	return true
}
