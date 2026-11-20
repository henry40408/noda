package importer

import (
	"slices"
	"testing"
)

const otherFile = "k3f9m2p1-other-tiddler.md"

// Each case pins one conversion edge; the name says which.
func TestWikitextConversionEdges(t *testing.T) {
	cases := []struct {
		name   string
		source string
		want   string
		left   []string
	}{
		{"heading six is the deepest level", "!!!!!! Six\n", "###### Six\n", nil},
		{"heading without a space after the marks", "!No space\n", "# No space\n", nil},
		{"empty heading keeps its marker", "! \n", "# \n", nil},
		{"empty list item keeps its marker", "* \n", "- \n", nil},
		{"mixed list prefixes nest by length", "#* mixed\n*# mixed\n", "  - mixed\n  1. mixed\n", nil},
		{"deep unordered list indents two spaces per level", "* a\n** b\n*** c\n", "- a\n  - b\n    - c\n", nil},
		{"list items convert their inline markup", "* ''bold'' and [[Other Tiddler]]\n", "- **bold** and [Other Tiddler](" + otherFile + ")\n", nil},
		{"bold-italic quadruple quotes", "''''x''''\n", "****x****\n", nil},
		{"triple quotes leave a stray apostrophe, as the Rust converter did", "'''a''' ''b''\n", "**'a**' **b**\n", nil},
		{"unclosed bold stays text", "''unclosed\n", "''unclosed\n", nil},
		{"triple quote closed by a double quote", "''' not closed ''\n", "**' not closed **\n", nil},
		{"strike is kept while underline and super/sub are reported", "__u__ ^^s^^ ,,b,, ~~x~~\n", "u s b ~~x~~\n", []string{leftSubscript, leftSuperscript, leftUnderline}},
		{"highlight loses its style", "@@mark@@\n", "mark\n", []string{leftStyle}},
		{"inline code is copied through", "`a` b\n", "`a` b\n", nil},
		{"code span whose first backtick closes at once", "``a ` b``\n", "``a ` b`\\`\n", nil},
		{"a lone backtick is escaped as text", "a ` b\n", "a \\` b\n", nil},
		{"fenced block is copied through", "```\n* not a list\n```\n", "```\n* not a list\n```\n", nil},
		{"bare URL stays as written", "https://example.com/bare\n", "https://example.com/bare\n", nil},
		{"insecure and non-http schemes stay as written", "http://a.example ftp://b.example mailto:c@d.e\n", "http://a.example ftp://b.example mailto:c@d.e\n", nil},
		{"link to a known tiddler", "[[Other Tiddler]]\n", "[Other Tiddler](" + otherFile + ")\n", nil},
		{"captioned link to a known tiddler", "[[label|Other Tiddler]]\n", "[label](" + otherFile + ")\n", nil},
		{"link to a URL", "[[label|https://example.com]]\n", "[label](https://example.com)\n", nil},
		{"unknown link target is reported", "[[Missing]]\n", "[[Missing]]\n", []string{leftLink}},
		{"link title is case sensitive", "[[other tiddler]]\n", "[[other tiddler]]\n", []string{leftLink}},
		{"caption with an unknown target is reported", "[[Other Tiddler|Missing]]\n", "[[Other Tiddler|Missing]]\n", []string{leftLink}},
		{"two pipes are not a caption", "[[a|b|c]]\n", "[[a|b|c]]\n", []string{leftLink}},
		{"padding inside a link is part of the title", "[[ Other Tiddler ]]\n", "[ Other Tiddler ](" + otherFile + ")\n", nil},
		{"trailing bracket after a link is text", "[[Other Tiddler]]]\n", "[Other Tiddler](" + otherFile + ")]\n", nil},
		{"unclosed link is escaped", "[[unclosed\n", "\\[\\[unclosed\n", nil},
		{"image with a tooltip", "[img[tip|pic.png]]\n", "![tip](pic.png)\n", nil},
		{"image with attributes is reported", "[img width=10 [pic.png]]\n", "![](pic.png)\n", []string{leftImageAttributes}},
		{"transclusion is kept and reported", "{{Embedded||Template}}\n", "{{Embedded||Template}}\n", []string{leftTransclusion}},
		{"inline preformatted is reported as a transclusion", "{{{ a }}} then ``b``\n", "{{{ a }}} then ``b``\n", []string{leftTransclusion}},
		{"macro with quoted parameters", "<<m \"quoted param\" n:\"x\">>\n", "<<m \"quoted param\" n:\"x\">>\n", []string{leftMacro}},
		{"macro definition line", "\\define m()\nbody\n\\end\n", "\\define m()\nbody\n\\end\n", []string{leftMacro}},
		{"widget closing tag is escaped", "<$link to=\"x\">y</$link>\n", "<$link to=\"x\">y\\</$link>\n", []string{leftWidget}},
		{"html block is kept and reported", "<div class=\"x\">y</div>\n", "<div class=\"x\">y</div>\n", []string{leftHTML}},
		{"angle-bracket URL counts as html", "<http://angle>\n", "<http://angle>\n", []string{leftHTML}},
		{"html comment opener is escaped", "<!-- c -->\n", "\\<!-- c -->\n", nil},
		{"plain table gets alignment-free GFM", "|a|b|\n|c|d|\n", "| a | b |\n| c | d |\n", nil},
		{"ragged table rows stay a table", "|a|\n|b|c|\n", "| a |\n| b | c |\n", nil},
		{"table flag row is left alone", "|a|b|h\n", "|a|b|h\n", []string{leftTable}},
		{"table cell markers are not flags", "|>a|\n|<b|\n", "| >a |\n| \\<b |\n", nil},
		{"definition list is kept and reported", "; term\n: def\n", "; term\n: def\n", []string{leftDefinitionList}},
		{"block quote with a citation", "<<<.cls\ntext\n<<< cite\n", "> text\n> — cite\n", nil},
		{"line-start quote marker and rule are escaped", "> q\n\n---\n", "\\> q\n\n\\---\n", nil},
		{"markdown star at line start is escaped", "*not a list\n", "\\*not a list\n", nil},
		{"star after leading space is escaped", " * x\n", " \\* x\n", nil},
		{"star at line start after other text is escaped", "x\n*not a list\n", "x\n\\*not a list\n", nil},
		{"mid-line pipe and star", "a | b and * c\n", "a | b and \\* c\n", nil},
		{"trailing backslash is doubled", "end\\\n", "end\\\\\n", nil},
		{"wiki escape of a star keeps both backslashes escaped", "\\*x\n", "\\\\\\*x\n", nil},
		{"comment delimiters are escaped", "/*\nc\n*/\n", "/\\*\nc\n\\*/\n", nil},
		{"tilde stops a camel-case link", "~WikiWord ~NotLinked\n", "WikiWord NotLinked\n", nil},
		{"html entities are untouched", "&nbsp; &amp; &lt;\n", "&nbsp; &amp; &lt;\n", nil},
		{"multibyte text around markup", "日本語 ''強調'' [[Other Tiddler]]\n", "日本語 **強調** [Other Tiddler](" + otherFile + ")\n", nil},
		{"empty input", "", "", nil},
		{"whitespace-only line is preserved", "   \n", "   \n", nil},
		{"paragraph breaks are preserved", "one\ntwo\n\nthree\n", "one\ntwo\n\nthree\n", nil},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			got := convertWith(c.source, "Other Tiddler")
			if got.Text != c.want {
				t.Errorf("text:\n got: %q\nwant: %q", got.Text, c.want)
			}
			if !slices.Equal(got.Left, c.left) {
				t.Errorf("left: got %q, want %q", got.Left, c.left)
			}
		})
	}
}
