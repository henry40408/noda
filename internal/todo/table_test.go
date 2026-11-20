package todo

import (
	"reflect"
	"testing"
)

func TestItemsTable(t *testing.T) {
	d := func(s string) *string { return &s }
	cases := []struct {
		name string
		body string
		want []Item
	}{
		{"empty body", "", nil},
		{"no tasks", "no tasks here", nil},
		{"ticked boxes are skipped", "- [ ] one\n- [x] two\n- [ ] three\n", []Item{{Text: "one"}, {Text: "three"}}},
		{"capital X is ticked", "- [X] capital done\n", nil},
		{"open box under a ticked parent", "- [x] outer done\n  - [ ] inner open\n", []Item{{Text: "inner open"}}},
		{"nested boxes count at every depth", "- [ ] outer\n  - [ ] inner\n  - [x] done inner\n", []Item{{Text: "outer"}, {Text: "inner"}}},
		{"bullet markers and ordered lists", "* [ ] star\n+ [ ] plus\n1. [ ] numbered\n", []Item{{Text: "star"}, {Text: "plus"}, {Text: "numbered"}}},
		{"items split by prose all count", "- [ ] a\n- [ ] b\n\ntext\n\n- [ ] c\n", []Item{{Text: "a"}, {Text: "b"}, {Text: "c"}}},
		{"box in a fence is prose", "```\n- [ ] in fence\n```\n", nil},
		{"box in indented code is prose", "    - [ ] indented code\n", nil},
		{"box in a blockquote counts", "> - [ ] quoted\n", []Item{{Text: "quoted"}}},
		{"box in a table cell is prose", "| a |\n|---|\n| - [ ] x |\n", nil},
		{"brackets without a space are no box", "- [] not a task\n", nil},
		{"two spaces after the marker", "-  [ ] two spaces\n", []Item{{Text: "two spaces"}}},
		{"box glued to text is no box", "- [ ]no space\n", nil},
		{"trailing space alone is empty", "- [ ] \n", nil},
		{"emphasis and code flatten", "- [ ] **bold** and *em* and `code`\n", []Item{{Text: "bold and em and code"}}},
		{"link flattens to its label", "- [ ] [the spec](spec.md) review\n", []Item{{Text: "the spec review"}}},
		{"image flattens to its alt text", "- [ ] ![img](a.png) caption\n", []Item{{Text: "img caption"}}},
		{"raw html tags are dropped", "- [ ] <b>html</b> text\n", []Item{{Text: "html text"}}},
		{"autolink keeps its label", "- [ ] <https://example.com> auto\n", []Item{{Text: "https://example.com auto"}}},
		{"strikethrough markers stay", "- [ ] ~~strike~~ x\n", []Item{{Text: "~~strike~~ x"}}},
		{"pipe stays in an item", "- [ ] a | b\n", []Item{{Text: "a | b"}}},
		{"soft line break is a space", "- [ ] line one\n  line two\n", []Item{{Text: "line one line two"}}},
		{"hard line break is a space", "- [ ] hard  \n  break\n", []Item{{Text: "hard break"}}},
		{"second paragraph is not part of the item", "- [ ] para one\n\n  para two\n", []Item{{Text: "para one"}}},
		{"due at the start", "- [ ] due:2024-01-02 pay\n", []Item{{Text: "pay", Due: d("2024-01-02")}}},
		{"due at the end", "- [ ] pay due:2024-01-02\n", []Item{{Text: "pay", Due: d("2024-01-02")}}},
		{"due in the middle", "- [ ] pay due:2024-01-02 now\n", []Item{{Text: "pay now", Due: d("2024-01-02")}}},
		{"due only is an empty item", "- [ ] due:2024-01-02\n", nil},
		{"last due wins", "- [ ] due:2024-01-02 and due:2025-01-01\n", []Item{{Text: "and", Due: d("2025-01-01")}}},
		{"month 13 is still a date by shape", "- [ ] due:2024-13-45 pay\n", []Item{{Text: "pay", Due: d("2024-13-45")}}},
		{"due with unpadded digits stays prose", "- [ ] due:2024-1-2 pay\n", []Item{{Text: "due:2024-1-2 pay"}}},
		{"due with a word stays prose", "- [ ] due:soon pay\n", []Item{{Text: "due:soon pay"}}},
		{"due key is case sensitive", "- [ ] DUE:2024-01-02 caps\n", []Item{{Text: "DUE:2024-01-02 caps"}}},
		{"space after the colon stays prose", "- [ ] due: 2024-01-02 spaced\n", []Item{{Text: "due: 2024-01-02 spaced"}}},
		{"trailing comma stays prose", "- [ ] text due:2024-01-02, comma\n", []Item{{Text: "text due:2024-01-02, comma"}}},
		{"unicode text with a due date", "- [ ] 日本語 due:2024-05-06 タスク\n", []Item{{Text: "日本語 タスク", Due: d("2024-05-06")}}},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			if got := Items(c.body); !reflect.DeepEqual(got, c.want) {
				t.Errorf("Items(%q):\n got  %+v\n want %+v", c.body, got, c.want)
			}
		})
	}
}
