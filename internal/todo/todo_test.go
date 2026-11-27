package todo

import (
	"slices"
	"testing"
)

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

func TestOneSpaceIsStrippedFromEachSideOfACodeSpan(t *testing.T) {
	expectTexts(t, "- [ ] x` a `y\n", "xay")
	expectTexts(t, "- [ ] x`  a  `y\n", "xay")
	expectTexts(t, "- [ ] x` a`y\n", "x ay")
	expectTexts(t, "- [ ] x`a `y\n", "xa y")
	expectTexts(t, "- [ ] x`  `y\n", "x y")
}

func TestADueDateTakesDigitsOnly(t *testing.T) {
	for _, date := range []string{"2026-01-01", "0000-00-00", "9999-99-99"} {
		if !isDate(date) {
			t.Errorf("%s is a date", date)
		}
	}
	for _, date := range []string{"/026-01-01", ":026-01-01", "2026-0/-01", "2026-0:-01", "2026-01-0:", "2026-01-/1"} {
		if isDate(date) {
			t.Errorf("%s is not a date", date)
		}
	}
}

func TestHiddenIsWhatABlankLineWouldHaveListed(t *testing.T) {
	cases := []struct {
		name string
		body string
		want []string
	}{
		{"directly under a summary", "<details>\n<summary>s</summary>\n- [ ] a\n- [ ] b\n</details>\n", []string{"a", "b"}},
		{"a blank line only before the closing tag", "<details>\n<summary>s</summary>\n- [ ] a\n\n</details>\n", []string{"a"}},
		{"directly after a closing tag", "<details>\n<summary>s</summary>\n\n- [ ] seen\n\n</details>\n- [ ] after\n", []string{"after"}},
		{"under a tag CommonMark does not know", "<custom-tag>\n- [ ] a\n", []string{"a"}},
		{"under a line of text in the block", "<div>\ntext\n- [ ] a\n", []string{"a"}},
		{"an ordered list not starting at one", "<div>\n3. [ ] a\n", []string{"a"}},
		{"inside a list item", "- parent\n  <div>\n  - [ ] a\n", []string{"a"}},
		{"inside a quote", "> <div>\n> - [ ] a\n", []string{"a"}},
		{"the same words as a listed item", "- [ ] a\n\n<div>\n- [ ] a\n", []string{"a"}},
		{"properly spaced", "<details>\n<summary>s</summary>\n\n- [ ] a\n\n</details>\n", nil},
		{"no HTML", "- [ ] a\n", nil},
		{"inline HTML", "- [ ] a <b>b</b>\n", nil},
		{"ticked", "<div>\n- [x] a\n", nil},
		{"a comment", "<!--\n- [ ] a\n-->\n", nil},
		{"a comment inside a section", "<details>\n<!--\n- [ ] a\n-->\n</details>\n", nil},
		{"preformatted", "<pre>\n- [ ] a\n</pre>\n", nil},
		{"a fence under a tag", "<div>\n```\n- [ ] a\n```\n", nil},
		{"a fence around the tag", "```\n<div>\n- [ ] a\n```\n", nil},
		{"a fence opened in HTML and closed later", "<div>\n```\n</div>\n\ntext\n\n```\n- [ ] a\n```\n", nil},
		{"a comment after an unclosed pre in HTML", "<div>\n<pre>\n</div>\n\n<!--\n</pre>\n- [ ] a\n-->\n", nil},
		{"indented code after a list in HTML", "<div>\n- parent\n\n    - [ ] a\n", nil},
		{"after a fence left open in an earlier block", "<div>\n```\n</div>\n\n<div>\n- [ ] a\n</div>\n", []string{"a"}},
		{"after a pre left open in an earlier block", "<div>\n<pre>\n</div>\n\n<div>\n- [ ] a\n</div>\n", []string{"a"}},
	}
	for _, c := range cases {
		var got []string
		for _, item := range Hidden(c.body) {
			got = append(got, item.Text)
		}
		if !slices.Equal(got, c.want) {
			t.Errorf("%s: %q: got %q, want %q", c.name, c.body, got, c.want)
		}
	}
}
