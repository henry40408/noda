package markdown

import (
	"slices"
	"testing"

	"github.com/yuin/goldmark/ast"
	extast "github.com/yuin/goldmark/extension/ast"
)

// ticks is whether each box of the document is ticked, in order.
func ticks(doc Document) []bool {
	var found []bool
	doc.Walk(func(node ast.Node) {
		if box, ok := node.(*extast.TaskCheckBox); ok {
			found = append(found, box.IsChecked)
		}
	})
	return found
}

func TestABoxIsTickedByEitherCaseOfX(t *testing.T) {
	got := ticks(Tasks("- [ ] a\n- [x] b\n- [X] c\n"))
	if want := []bool{false, true, true}; !slices.Equal(got, want) {
		t.Errorf("got %v, want %v", got, want)
	}
}

func TestTasksPastHTMLReadsWhatALineOfHTMLWouldSwallow(t *testing.T) {
	cases := []struct {
		name       string
		body       string
		tasks      int
		pastTheTag int
	}{
		{"under a tag", "<div>\n- [ ] a\n", 0, 1},
		{"under two tags", "<details>\n<summary>s</summary>\n- [ ] a\n", 0, 1},
		{"under a tag CommonMark does not know", "<custom-tag>\n- [ ] a\n", 0, 1},
		{"a comment", "<!--\n- [ ] a\n-->\n", 0, 0},
		{"preformatted", "<pre>\n- [ ] a\n</pre>\n", 0, 0},
		{"no HTML", "- [ ] a\n", 1, 1},
	}
	for _, c := range cases {
		if got := len(ticks(Tasks(c.body))); got != c.tasks {
			t.Errorf("%s: Tasks found %d boxes, want %d", c.name, got, c.tasks)
		}
		if got := len(ticks(TasksPastHTML(c.body))); got != c.pastTheTag {
			t.Errorf("%s: TasksPastHTML found %d boxes, want %d", c.name, got, c.pastTheTag)
		}
	}
}
