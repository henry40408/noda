// Package markdown parses a note body into a syntax tree. Callers read the
// tree; they do not render it with goldmark, so the output stays under noda's
// control.
package markdown

import (
	"strings"

	"github.com/yuin/goldmark"
	"github.com/yuin/goldmark/ast"
	"github.com/yuin/goldmark/extension"
	"github.com/yuin/goldmark/parser"
	"github.com/yuin/goldmark/text"
	"github.com/yuin/goldmark/util"
)

// Document is a parsed body with the source its segments point into.
type Document struct {
	Root       ast.Node
	Source     []byte
	References []parser.Reference
}

// Plain is CommonMark and footnotes: what `link` reads, so a destination means
// what any other CommonMark reader takes it to mean. Footnotes are the one
// addition: the renderer draws `[^1]: Ibid.` as a footnote, and without them
// `link` would read it as a link to a file named `Ibid.`.
func Plain(body string) Document {
	return parse(body, goldmark.New(goldmark.WithExtensions(extension.Footnote)))
}

// Tasks adds task lists only: what `todo` reads, so `~~x~~` stays literal text
// in a todo as it does in noda's other readers of a body.
func Tasks(body string) Document {
	return parse(body, goldmark.New(goldmark.WithExtensions(taskList{})))
}

// Rich adds tables, strikethrough, footnotes and task lists, the dialect the
// renderer reads.
func Rich(body string) Document {
	return parse(body, goldmark.New(goldmark.WithExtensions(
		extension.Table, extension.Strikethrough, extension.Footnote, taskList{},
	)))
}

// TasksPastHTML is Tasks as if a blank line followed each line that opens raw
// HTML: how the body would read had its author left one under `<details>`.
// Comments, `<pre>` and the like still hold their text.
func TasksPastHTML(body string) Document {
	md := goldmark.New(goldmark.WithExtensions(taskList{}))
	// Ahead of goldmark's own HTML block parser, which is at 900.
	md.Parser().AddOptions(parser.WithBlockParsers(
		util.Prioritized(oneLineHTML{parser.NewHTMLBlockParser()}, 899),
	))
	return parse(body, md)
}

// oneLineHTML ends a block of raw HTML at its first line, where CommonMark runs
// it to the next blank one.
type oneLineHTML struct{ parser.BlockParser }

func (p oneLineHTML) Continue(node ast.Node, reader text.Reader, context parser.Context) parser.State {
	// The earlier kinds end at a marker of their own and hold text on purpose.
	if block, ok := node.(*ast.HTMLBlock); ok && block.HTMLBlockType >= ast.HTMLBlockType6 {
		return parser.Close
	}
	return p.BlockParser.Continue(node, reader, context)
}

func parse(body string, md goldmark.Markdown) Document {
	source := []byte(body)
	context := parser.NewContext()
	root := md.Parser().Parse(text.NewReader(source), parser.WithContext(context))
	return Document{Root: root, Source: source, References: context.References()}
}

// Unescape is what a reader sees of source text: backslash escapes and
// character references resolved. goldmark keeps them as written until it renders.
func Unescape(raw []byte) string {
	var out strings.Builder
	chunkStart := 0
	flush := func(end int) {
		out.Write(util.ResolveEntityNames(util.ResolveNumericReferences(raw[chunkStart:end])))
	}
	for at := 0; at < len(raw); at++ {
		if raw[at] != '\\' || at+1 >= len(raw) || !util.IsPunct(raw[at+1]) {
			continue
		}
		flush(at)
		out.WriteByte(raw[at+1])
		at++
		chunkStart = at + 1
	}
	flush(len(raw))
	return out.String()
}

// Walk visits every node in document order.
func (d Document) Walk(visit func(node ast.Node)) {
	_ = ast.Walk(d.Root, func(node ast.Node, entering bool) (ast.WalkStatus, error) {
		if entering {
			visit(node)
		}
		return ast.WalkContinue, nil
	})
}
