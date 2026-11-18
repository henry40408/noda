package todo

import (
	"encoding/json"
	"os"
	"reflect"
	"slices"
	"testing"
)

func readJSON(t *testing.T, path string, into any) {
	t.Helper()
	raw, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	if err := json.Unmarshal(raw, into); err != nil {
		t.Fatal(err)
	}
}

func TestTodosMatchTheRustGolden(t *testing.T) {
	var cases []struct{ Name, Body string }
	var expected []struct {
		Name string `json:"name"`
		Out  []any  `json:"out"`
	}
	readJSON(t, "../../testdata/golden/todo/cases.json", &cases)
	readJSON(t, "../../testdata/golden/todo/expected.json", &expected)
	if len(cases) != len(expected) || len(cases) != 39 {
		t.Fatalf("%d cases, %d expected", len(cases), len(expected))
	}
	for i, c := range cases {
		if expected[i].Name != c.Name {
			t.Fatalf("case %d is %s, expected %s", i, c.Name, expected[i].Name)
		}
		got := []any{}
		for _, item := range Items(c.Body) {
			var due any
			if item.Due != nil {
				due = *item.Due
			}
			got = append(got, map[string]any{"text": item.Text, "due": due})
		}
		if !reflect.DeepEqual(got, expected[i].Out) {
			t.Errorf("%s: %q\n got  %#v\n want %#v", c.Name, c.Body, got, expected[i].Out)
		}
	}
}

func texts(body string) []string {
	var found []string
	for _, item := range Items(body) {
		found = append(found, item.Text)
	}
	return found
}

func expectTexts(t *testing.T, body string, want ...string) {
	t.Helper()
	if got := texts(body); !slices.Equal(got, want) {
		t.Errorf("%q: got %q, want %q", body, got, want)
	}
}

func TestOnlyUntickedBoxesComeBack(t *testing.T) {
	expectTexts(t, "- [ ] open\n- [x] done\n- [ ] also open\n", "open", "also open")
}

func TestAListItemWithoutABoxIsNotATodo(t *testing.T) {
	expectTexts(t, "- just a bullet\n\nand a paragraph\n")
}

func TestABoxInsideACodeBlockIsProseAboutABox(t *testing.T) {
	expectTexts(t, "```\n- [ ] not mine\n```\n")
	expectTexts(t, "    - [ ] not mine either\n")
}

func TestEveryBulletAndEveryDepthCounts(t *testing.T) {
	expectTexts(t, "- [ ] dash\n\n* [ ] star\n\n+ [ ] plus\n\n1. [ ] ordered\n", "dash", "star", "plus", "ordered")
	expectTexts(t, "- [ ] outer\n  - [ ] inner\n", "outer", "inner")
}

func TestInlineMarkupIsFlattened(t *testing.T) {
	expectTexts(t, "- [ ] read [the spec](spec.md) **today**\n", "read the spec today")
	expectTexts(t, "- [ ] run `cargo test`\n", "run cargo test")
}

func TestAnItemStopsAtItsFirstParagraph(t *testing.T) {
	expectTexts(t, "- [ ] the task\n\n  a note about it\n", "the task")
}

func TestALineBreakInsideAnItemBecomesASpace(t *testing.T) {
	expectTexts(t, "- [ ] one\n  two\n", "one two")
}

func TestAnEmptyBoxIsNotAnItem(t *testing.T) {
	expectTexts(t, "- [ ]\n")
}

func TestEscapesAndCharacterReferencesReadAsTheReaderSeesThem(t *testing.T) {
	expectTexts(t, "- [ ] fish &amp; chips \\*not bold\\*\n", "fish & chips *not bold*")
}

func TestADueDateIsLiftedOutOfTheText(t *testing.T) {
	found := Items("- [ ] send the contract due:2026-08-10 to legal\n")
	if found[0].Text != "send the contract to legal" || found[0].Due == nil || *found[0].Due != "2026-08-10" {
		t.Fatalf("%+v", found[0])
	}
}

func TestATermThatIsNotADateStaysInTheProse(t *testing.T) {
	found := Items("- [ ] ask about due:tomorrow\n")
	if found[0].Text != "ask about due:tomorrow" || found[0].Due != nil {
		t.Fatalf("%+v", found[0])
	}
}

func TestTheLastDueDateWins(t *testing.T) {
	found := Items("- [ ] due:2026-01-01 moved due:2026-03-01\n")
	if found[0].Text != "moved" || found[0].Due == nil || *found[0].Due != "2026-03-01" {
		t.Fatalf("%+v", found[0])
	}
}

func TestOverdueComparesTheDateAsText(t *testing.T) {
	due := "2026-08-10"
	item := Item{Text: "x", Due: &due}
	if !item.Overdue("2026-08-11") || item.Overdue("2026-08-10") || item.Overdue("2026-08-09") {
		t.Fatal("overdue is strictly before today")
	}
	if (Item{Text: "x"}).Overdue("2099-01-01") {
		t.Fatal("an undated item is never overdue")
	}
}

func TestCompareSortsSoonestFirstUndatedLastThenBySlug(t *testing.T) {
	early, late := "2026-01-01", "2026-02-01"
	cases := []struct {
		leftSlug  string
		left      Item
		rightSlug string
		right     Item
		want      int
	}{
		{"b", Item{Due: &early}, "a", Item{Due: &late}, -1},
		{"a", Item{Due: &late}, "b", Item{Due: &early}, 1},
		{"a", Item{Due: &early}, "b", Item{}, -1},
		{"a", Item{}, "b", Item{Due: &early}, 1},
		{"a", Item{Due: &early}, "b", Item{Due: &early}, -1},
		{"b", Item{}, "a", Item{}, 1},
		{"a", Item{}, "a", Item{}, 0},
	}
	for _, c := range cases {
		if got := Compare(c.leftSlug, c.left, c.rightSlug, c.right); got != c.want {
			t.Errorf("%+v: %d, want %d", c, got, c.want)
		}
	}
}
