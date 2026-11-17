package importer

import (
	"fmt"
	"slices"
	"sort"
	"strings"
	"testing"
	"unicode"

	"github.com/yuin/goldmark/ast"

	"github.com/henry40408/noda/internal/markdown"
)

// resolver keeps a title's words in the filename, so the loss tests see them.
func resolver(known ...string) Resolve {
	return func(title string) (string, bool) {
		if !slices.Contains(known, title) {
			return "", false
		}
		slug := strings.Map(func(c rune) rune {
			if unicode.IsLetter(c) || unicode.IsNumber(c) {
				return c
			}
			return '-'
		}, title)
		return "k3f9m2p1-" + strings.ToLower(slug) + ".md", true
	}
}

func convertWith(text string, known ...string) Converted { return Convert(text, resolver(known...)) }

func textOf(text string) string { return convertWith(text).Text }

func expectText(t *testing.T, source, want string) {
	t.Helper()
	if got := textOf(source); got != want {
		t.Fatalf("%q\n got: %q\nwant: %q", source, got, want)
	}
}

func expectLeft(t *testing.T, out Converted, name string) {
	t.Helper()
	if !slices.Contains(out.Left, name) {
		t.Fatalf("%q not reported, left = %v", name, out.Left)
	}
}

func TestHeadingsTakeTheirLevelFromTheMarks(t *testing.T) {
	expectText(t, "!Top\n!!!Third\n", "# Top\n### Third\n")
	// Seven is not a heading in Markdown; `!` alone needs no escaping.
	expectText(t, "!!!!!!!Seven\n", "!!!!!!!Seven\n")
	expectText(t, "!!! Warning\n", "### Warning\n")
}

func TestADroppedClassIsReportedRatherThanLost(t *testing.T) {
	out := convertWith("!!!.myClass Heading\n")
	if out.Text != "### Heading\n" {
		t.Fatalf("%q", out.Text)
	}
	expectLeft(t, out, leftStyle)
}

func TestListsNestByTheLengthOfTheirPrefix(t *testing.T) {
	expectText(t, "* One\n** Two\n*# Numbered\n# Ordered\n", "- One\n  - Two\n  1. Numbered\n1. Ordered\n")
}

func TestEmphasisPairsConvertAndNest(t *testing.T) {
	expectText(t, "''bold''\n", "**bold**\n")
	expectText(t, "//italic//\n", "*italic*\n")
	expectText(t, "//''both''//\n", "***both***\n")
	expectText(t, "~~gone~~\n", "~~gone~~\n")
}

func TestAnUnmatchedDelimiterStaysText(t *testing.T) {
	expectText(t, "it cost 5'' of tape\n", "it cost 5'' of tape\n")
	expectText(t, "a // b\n", "a // b\n")
}

func TestAURLIsNotReadAsEmphasis(t *testing.T) {
	expectText(t, "see https://example.com/a//b for //this//\n", "see https://example.com/a//b for *this*\n")
}

func TestALinkBecomesTheFileItsTargetWasGiven(t *testing.T) {
	out := convertWith("see [[Meeting Notes]]\n", "Meeting Notes")
	if out.Text != "see [Meeting Notes](k3f9m2p1-meeting-notes.md)\n" || len(out.Left) != 0 {
		t.Fatalf("%q %v", out.Text, out.Left)
	}
}

func TestACaptionedLinkKeepsCaptionFirst(t *testing.T) {
	out := convertWith("[[the notes|Meeting Notes]]\n", "Meeting Notes")
	if out.Text != "[the notes](k3f9m2p1-meeting-notes.md)\n" {
		t.Fatalf("%q", out.Text)
	}
}

func TestALinkToNothingIsLeftAsItWasWritten(t *testing.T) {
	out := convertWith("see [[Missing Tiddler]]\n", "Meeting Notes")
	if out.Text != "see [[Missing Tiddler]]\n" {
		t.Fatalf("a link to a file that will not exist is worse than the WikiText: %q", out.Text)
	}
	expectLeft(t, out, leftLink)
}

func TestExternalLinksCarryTheirURL(t *testing.T) {
	expectText(t, "[[TW5|https://tiddlywiki.com/]]\n", "[TW5](https://tiddlywiki.com/)\n")
	// An ext link names a file, not a tiddler.
	expectText(t, "[ext[Open|index.html]]\n", "[Open](index.html)\n")
}

func TestAnImageKeepsItsTooltipAndReportsItsAttributes(t *testing.T) {
	expectText(t, "[img[a cat|cat.jpg]]\n", "![a cat](cat.jpg)\n")
	out := convertWith("[img width=32 [cat.jpg]]\n")
	if out.Text != "![](cat.jpg)\n" {
		t.Fatalf("%q", out.Text)
	}
	expectLeft(t, out, leftImageAttributes)
}

func TestABlockQuoteKeepsItsCitation(t *testing.T) {
	expectText(t, "<<<\nA bicycle for our minds\n<<< Steve Jobs\n", "> A bicycle for our minds\n> — Steve Jobs\n")
}

func TestAPlainTableBecomesAGFMTable(t *testing.T) {
	expectText(t, "|!One |!Two |\n|a |b |\n", "| One | Two |\n| --- | --- |\n| a | b |\n")
}

func TestATableMarkdownCannotHoldIsLeftAlone(t *testing.T) {
	for _, table := range []string{
		"|!One |!Two |\n|a |b |\n|Footer|Footer|f\n",
		"|a caption |c\n|a |b |\n",
		"|a |< |\n",
	} {
		out := convertWith(table)
		if out.Text != table {
			t.Fatalf("not left as written: %q", out.Text)
		}
		expectLeft(t, out, leftTable)
	}
}

func TestAFenceIsCopiedThroughUntouched(t *testing.T) {
	fenced := "```rust\nlet x = ''not bold'';\n```\n"
	expectText(t, fenced, fenced)
}

func TestInlineCodeIsLeftAloneToo(t *testing.T) {
	expectText(t, "run `a // b` now\n", "run `a // b` now\n")
}

func TestAMacroDefinitionStopsTheConversion(t *testing.T) {
	source := "\\define greet() hello\n\n!Not a heading now\n"
	out := convertWith(source)
	if out.Text != source {
		t.Fatalf("everything after a pragma is a program, not prose: %q", out.Text)
	}
	expectLeft(t, out, leftMacro)
}

func TestWhatMarkdownHasNoWordForIsKeptAndNamed(t *testing.T) {
	for source, name := range map[string]string{
		"a {{Transcluded}} here\n":      leftTransclusion,
		"a <<mymacro arg>> here\n":      leftMacro,
		"a <$list filter=\"x\"> here\n": leftWidget,
		"a <div class=\"x\"> here\n":    leftHTML,
		"an __underline__ here\n":       leftUnderline,
		"a ^^super^^ here\n":            leftSuperscript,
		"a ,,sub,, here\n":              leftSubscript,
	} {
		if out := convertWith(source); !slices.Contains(out.Left, name) {
			t.Fatalf("%s not reported for %q", name, source)
		}
	}
}

func TestCamelCaseLinksOnlyWhereItResolves(t *testing.T) {
	out := convertWith("See CamelCase and JavaScript\n", "CamelCase")
	if out.Text != "See [CamelCase](k3f9m2p1-camelcase.md) and JavaScript\n" {
		t.Fatalf("prose that names no tiddler stays prose: %q", out.Text)
	}
	expectText(t, "~CamelCase stays text\n", "CamelCase stays text\n")
	expectText(t, "HTML and IDs and Hello\n", "HTML and IDs and Hello\n")
}

func TestMarkdownSyntaxInProseIsEscaped(t *testing.T) {
	expectText(t, "2 * 3 * 4\n", "2 \\* 3 \\* 4\n")
	expectText(t, "see [1] there\n", "see \\[1] there\n")
	expectText(t, "a < b\n", "a \\< b\n")
}

// Every slice in this module is taken by byte.
func TestMultibyteTextSurvivesEverySlice(t *testing.T) {
	out := convertWith("!!標題\n* ''粗體''與//斜體//\n看 [[會議記錄]] 和 https://example.com/路徑\n", "會議記錄")
	want := "## 標題\n- **粗體**與*斜體*\n看 [會議記錄](k3f9m2p1-會議記錄.md) 和 https://example.com/路徑\n"
	if out.Text != want {
		t.Fatalf("got %q\nwant %q", out.Text, want)
	}
}

// everything is every construct in one document, for the loss invariant below.
const everything = "" +
	"!A Heading\n" +
	"!!!Another Heading\n" +
	"\n" +
	"Some prose with ''bold'', //italic//, ~~struck~~, __under__, ^^super^^ and ,,sub,,.\n" +
	"A link to [[Meeting Notes]] and one with [[a caption|Meeting Notes]].\n" +
	"An external https://example.com/page and [[named|https://example.com/other]].\n" +
	"An image [img[a photograph|photo.jpg]].\n" +
	"A transclusion {{Some Tiddler}} and a macro <<greeting name>>.\n" +
	"A widget <$list filter=\"whatever\"> and some <div class=\"raw\">html</div>.\n" +
	"\n" +
	"* first bullet\n" +
	"** nested bullet\n" +
	"# first number\n" +
	"\n" +
	"<<<\n" +
	"A quotation worth keeping\n" +
	"<<< Somebody Notable\n" +
	"\n" +
	"|!Heading one |!Heading two |\n" +
	"|table cell |another cell |\n" +
	"\n" +
	"; A term\n" +
	": Its definition\n" +
	"\n" +
	"```\n" +
	"fenced code stays exactly as written\n" +
	"```\n" +
	"\n" +
	"Prose in 中文，含有標點與空白。\n"

// words are runs of alphanumerics, which works for Chinese too. `img` and
// `ext` are syntax, not content, so they are excluded.
func words(text string) map[string]bool {
	found := map[string]bool{}
	for _, word := range strings.FieldsFunc(text, func(c rune) bool { return !unicode.IsLetter(c) && !unicode.IsNumber(c) }) {
		word = strings.ToLower(word)
		if len([]rune(word)) >= 3 && word != "img" && word != "ext" {
			found[word] = true
		}
	}
	return found
}

func missingFrom(before, after map[string]bool) []string {
	var missing []string
	for word := range before {
		if !after[word] {
			missing = append(missing, word)
		}
	}
	sort.Strings(missing)
	return missing
}

// Every source word is somewhere in the output: a swallowed paragraph, table
// row or delimiter's far side fails here and nowhere else.
func TestNoWordIsLost(t *testing.T) {
	out := convertWith(everything, "Meeting Notes", "Some Tiddler")
	if missing := missingFrom(words(everything), words(out.Text)); len(missing) > 0 {
		t.Fatalf("these words vanished: %v", missing)
	}
}

// renderedText is the text of a Markdown document as parsed rather than as
// spelled: what a reader would see, plus link destinations, which are carried
// in both languages and so count as survived.
func renderedText(body string) string {
	doc := markdown.Rich(body)
	var rendered strings.Builder
	add := func(text string) { rendered.WriteString(text + " ") }
	lines := func(n ast.Node) {
		for i := 0; i < n.Lines().Len(); i++ {
			segment := n.Lines().At(i)
			add(string(segment.Value(doc.Source)))
		}
	}
	_ = ast.Walk(doc.Root, func(n ast.Node, entering bool) (ast.WalkStatus, error) {
		if !entering {
			return ast.WalkContinue, nil
		}
		switch n := n.(type) {
		case *ast.Text:
			add(markdown.Unescape(n.Segment.Value(doc.Source)))
		case *ast.String:
			add(string(n.Value))
		case *ast.RawHTML:
			for i := 0; i < n.Segments.Len(); i++ {
				segment := n.Segments.At(i)
				add(string(segment.Value(doc.Source)))
			}
		case *ast.FencedCodeBlock, *ast.CodeBlock:
			lines(n)
		case *ast.HTMLBlock:
			lines(n)
		case *ast.Link:
			add(string(n.Destination))
		case *ast.Image:
			add(string(n.Destination))
		}
		return ast.WalkContinue, nil
	})
	return rendered.String()
}

// The same, against the Markdown as parsed rather than as spelled.
func TestNoWordIsLostOnceMarkdownIsParsed(t *testing.T) {
	out := convertWith(everything, "Meeting Notes", "Some Tiddler")
	missing := missingFrom(words(everything), words(renderedText(out.Text)))
	if len(missing) > 0 {
		t.Fatalf("these words are in the file but not in what it renders: %v", missing)
	}
}

func TestSplitLinesIsRustLines(t *testing.T) {
	for text, want := range map[string][]string{
		"":         nil,
		"a":        {"a"},
		"a\n":      {"a"},
		"a\n\nb":   {"a", "", "b"},
		"a\r\nb\r": {"a", "b\r"},
		"a\r\n":    {"a"},
	} {
		if got := splitLines(text); fmt.Sprint(got) != fmt.Sprint(want) {
			t.Fatalf("%q: got %q, want %q", text, got, want)
		}
	}
}
