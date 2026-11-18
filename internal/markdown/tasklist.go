package markdown

import (
	"regexp"

	"github.com/yuin/goldmark"
	"github.com/yuin/goldmark/ast"
	extast "github.com/yuin/goldmark/extension/ast"
	"github.com/yuin/goldmark/parser"
	"github.com/yuin/goldmark/text"
	"github.com/yuin/goldmark/util"
)

// taskList is goldmark's GFM task list with one difference: the box must be
// followed by whitespace, as in GFM and pulldown-cmark. goldmark's own pattern
// accepts `- [ ]text`, which would turn a bracket in prose into a todo.
type taskList struct{}

func (taskList) Extend(md goldmark.Markdown) {
	md.Parser().AddOptions(parser.WithInlineParsers(util.Prioritized(taskBoxParser{}, 0)))
}

var taskBoxPattern = regexp.MustCompile(`^\[([\sxX])\]\s`)

type taskBoxParser struct{}

func (taskBoxParser) Trigger() []byte { return []byte{'['} }

func (taskBoxParser) Parse(parent ast.Node, block text.Reader, _ parser.Context) ast.Node {
	// Only the first thing in a list item's first block can be a box.
	if parent.Parent() == nil || parent.Parent().FirstChild() != parent || parent.HasChildren() {
		return nil
	}
	if _, ok := parent.Parent().(*ast.ListItem); !ok {
		return nil
	}
	line, _ := block.PeekLine()
	match := taskBoxPattern.FindSubmatchIndex(line)
	if match == nil {
		return nil
	}
	mark := line[match[2]]
	block.Advance(match[1])
	return extast.NewTaskCheckBox(mark == 'x' || mark == 'X')
}

func (taskBoxParser) CloseBlock(ast.Node, parser.Context) {}
