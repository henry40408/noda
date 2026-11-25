package web

import (
	"strconv"
	"strings"
	"testing"
)

func TestBody(t *testing.T) {
	where := AroundOf("notes & more", []Named{{ID: "01abc234", Slug: "hello"}, {ID: "02abc234", Slug: "other thing"}})
	for _, c := range []struct{ name, markdown, want string }{
		{"a paragraph is wrapped in p", "plain paragraph",
			"<p>plain paragraph</p>\n"},
		{"headings keep their level", "# H1\n\n## H2\n\n### H3\n",
			"<h1>H1</h1>\n<h2>H2</h2>\n<h3>H3</h3>\n"},
		{"emphasis strong strike and code span", "*em* **strong** ~~strike~~ `code`",
			"<p><em>em</em> <strong>strong</strong> <del>strike</del> <code>code</code></p>\n"},
		{"a soft line break stays a newline", "line one\nline two",
			"<p>line one\nline two</p>\n"},
		{"two trailing spaces make a hard break", "line one  \nhard break",
			"<p>line one<br />\nhard break</p>\n"},
		{"nested bullet list", "- a\n- b\n  - c\n",
			"<ul>\n<li>a</li>\n<li>b\n<ul>\n<li>c</li>\n</ul>\n</li>\n</ul>\n"},
		{"ordered list", "1. a\n2. b\n",
			"<ol>\n<li>a</li>\n<li>b</li>\n</ol>\n"},
		{"ordered list keeps its start number", "3. starts at three\n4. next\n",
			"<ol start=\"3\">\n<li>starts at three</li>\n<li>next</li>\n</ol>\n"},
		{"task list checkboxes are disabled", "- [ ] todo\n- [x] done\n",
			"<ul>\n<li><input disabled=\"\" type=\"checkbox\"/>\ntodo</li>\n<li><input disabled=\"\" type=\"checkbox\" checked=\"\"/>\ndone</li>\n</ul>\n"},
		{"table column alignments", "| l | c | r | n |\n|:--|:-:|--:|---|\n| 1 | 2 | 3 | 4 |\n",
			"<table><thead><tr><th style=\"text-align: left\">l</th><th style=\"text-align: center\">c</th><th style=\"text-align: right\">r</th><th>n</th></tr></thead><tbody>\n<tr><td style=\"text-align: left\">1</td><td style=\"text-align: center\">2</td><td style=\"text-align: right\">3</td><td>4</td></tr>\n</tbody></table>\n"},
		{"block quote", "> quote\n> more\n",
			"<blockquote>\n<p>quote\nmore</p>\n</blockquote>\n"},
		{"fenced code keeps its language", "```rust\nfn main() {}\n```\n",
			"<pre><code class=\"language-rust\">fn main() {}\n</code></pre>\n"},
		{"fenced code without a language", "```\nno lang\n```\n",
			"<pre><code>no lang\n</code></pre>\n"},
		{"tilde fence", "~~~\ntilde fence\n~~~\n",
			"<pre><code>tilde fence\n</code></pre>\n"},
		{"indented code block", "    indented code\n",
			"<pre><code>indented code\n</code></pre>\n"},
		{"thematic break", "---\n",
			"<hr />\n"},
		{"setext heading", "Setext\n======\n",
			"<h1>Setext</h1>\n"},
		{"loose list item wraps paragraphs", "- item\n\n  para in item\n",
			"<ul>\n<li>\n<p>item</p>\n<p>para in item</p>\n</li>\n</ul>\n"},
		{"list inside a block quote", "> - quoted list\n",
			"<blockquote>\n<ul>\n<li>quoted list</li>\n</ul>\n</blockquote>\n"},
		{"raw script block is shown as code not run", "<script>alert(1)</script>",
			"<pre><code class=\"language-html\">&lt;script&gt;alert(1)&lt;/script&gt;</code></pre>\n"},
		{"raw html block then a paragraph", "<script>alert(1)</script>\n\ntext",
			"<pre><code class=\"language-html\">&lt;script&gt;alert(1)&lt;/script&gt;\n</code></pre>\n<p>text</p>\n"},
		{"inline html is shown as code spans", "text <b>inline html</b> text",
			"<p>text <code>&lt;b&gt;</code>inline html<code>&lt;/b&gt;</code> text</p>\n"},
		{"html comment is shown as code", "<!-- comment -->\n\ntext",
			"<pre><code class=\"language-html\">&lt;!-- comment --&gt;\n</code></pre>\n<p>text</p>\n"},
		{"inline html in a table cell is escaped", "| a |\n|---|\n| <b>x</b> |\n",
			"<table><thead><tr><th>a</th></tr></thead><tbody>\n<tr><td><code>&lt;b&gt;</code>x<code>&lt;/b&gt;</code></td></tr>\n</tbody></table>\n"},
		{"html block split by blank lines", "<details><summary>s</summary>\n\nbody\n\n</details>",
			"<pre><code class=\"language-html\">&lt;details&gt;&lt;summary&gt;s&lt;/summary&gt;\n</code></pre>\n<p>body</p>\n<pre><code class=\"language-html\">&lt;/details&gt;</code></pre>\n"},
		{"javascript link keeps only its text", "[x](javascript:alert(1))",
			"<p>x</p>\n"},
		{"javascript scheme is case insensitive", "[x](JaVaScRiPt:alert(1))",
			"<p>x</p>\n"},
		{"data link keeps only its text", "[x](data:text/html,<script>alert(1)</script>)",
			"<p>x</p>\n"},
		{"vbscript link keeps only its text", "[x](vbscript:x)",
			"<p>x</p>\n"},
		{"file link keeps only its text", "[x](file:///etc/passwd)",
			"<p>x</p>\n"},
		{"ftp link keeps only its text", "[x](ftp://example.com)",
			"<p>x</p>\n"},
		{"https link opens in a new tab safely", "[x](https://example.com)",
			"<p><a href=\"https://example.com\" target=\"_blank\" rel=\"noopener noreferrer\">x</a></p>\n"},
		{"link href escapes ampersand", "[x](http://example.com/a?b=1&c=2)",
			"<p><a href=\"http://example.com/a?b=1&amp;c=2\" target=\"_blank\" rel=\"noopener noreferrer\">x</a></p>\n"},
		{"mailto link", "[x](mailto:a@b.c)",
			"<p><a href=\"mailto:a@b.c\">x</a></p>\n"},
		{"tel link", "[x](tel:+886)",
			"<p><a href=\"tel:+886\">x</a></p>\n"},
		{"fragment link", "[x](#frag)",
			"<p><a href=\"#frag\">x</a></p>\n"},
		{"note link routes to the note page", "[x](01abc234-hello.md)",
			"<p><a class=\"note h10\" href=\"/nb/notes%20%26%20more/n/01abc234\">x</a></p>\n"},
		{"note link drops dot slash and fragment", "[x](./01abc234-hello.md#frag)",
			"<p><a class=\"note h10\" href=\"/nb/notes%20%26%20more/n/01abc234\">x</a></p>\n"},
		{"note link drops its title", "[x](01abc234-hello.md \"a title\")",
			"<p><a class=\"note h10\" href=\"/nb/notes%20%26%20more/n/01abc234\">x</a></p>\n"},
		{"unknown md file routes to the file page", "[x](unknown.md)",
			"<p><a href=\"/nb/notes%20%26%20more/f/unknown.md\">x</a></p>\n"},
		{"file path with directory", "[x](sub/dir.md)",
			"<p><a href=\"/nb/notes%20%26%20more/f/sub/dir.md\">x</a></p>\n"},
		{"non md file routes to the file page", "[x](notes.txt)",
			"<p><a href=\"/nb/notes%20%26%20more/f/notes.txt\">x</a></p>\n"},
		{"reference style link", "[x][r]\n\n[r]: https://example.com \"T\"\n",
			"<p><a href=\"https://example.com\" target=\"_blank\" rel=\"noopener noreferrer\">x</a></p>\n"},
		{"reference to a note", "[r]\n\n[r]: 01abc234-hello.md\n",
			"<p><a class=\"note h10\" href=\"/nb/notes%20%26%20more/n/01abc234\">r</a></p>\n"},
		{"external and note links side by side", "[x](https://example.com)\n\n[y](01abc234-hello.md)\n",
			"<p><a href=\"https://example.com\" target=\"_blank\" rel=\"noopener noreferrer\">x</a></p>\n<p><a class=\"note h10\" href=\"/nb/notes%20%26%20more/n/01abc234\">y</a></p>\n"},
		{"image of a local file", "![alt](pic.png)",
			"<p><img src=\"/nb/notes%20%26%20more/f/pic.png\" alt=\"alt\" /></p>\n"},
		{"image of an external url", "![alt](https://example.com/p.png)",
			"<p><img src=\"https://example.com/p.png\" alt=\"alt\" /></p>\n"},
		{"image of a note keeps only alt", "![alt](01abc234-hello.md)",
			"<p>alt</p>\n"},
		{"image alt flattens markup and keeps title", "![alt *em*](x.png \"title\")",
			"<p><img src=\"/nb/notes%20%26%20more/f/x.png\" alt=\"alt em\" title=\"title\" /></p>\n"},
		{"javascript image keeps only alt", "![alt](javascript:x)",
			"<p>alt</p>\n"},
		{"image with empty alt", "![](x.png)",
			"<p><img src=\"/nb/notes%20%26%20more/f/x.png\" alt=\"\" /></p>\n"},
		{"image of a pdf is a viewer with the link as its fallback", "![the *spec*](spec.pdf)",
			"<p><object class=\"pdf\" data=\"/nb/notes%20%26%20more/f/spec.pdf\" type=\"application/pdf\"><a href=\"/nb/notes%20%26%20more/f/spec.pdf\">the spec</a></object></p>\n"},
		{"image of a pdf keeps its title", "![](spec.PDF \"Q3 <spec>\")",
			"<p><object class=\"pdf\" data=\"/nb/notes%20%26%20more/f/spec.PDF\" type=\"application/pdf\" title=\"Q3 &lt;spec&gt;\"><a href=\"/nb/notes%20%26%20more/f/spec.PDF\">spec.PDF</a></object></p>\n"},
		{"image of a pdf ignores the fragment", "![a & b](spec.pdf#page=2)",
			"<p><object class=\"pdf\" data=\"/nb/notes%20%26%20more/f/spec.pdf\" type=\"application/pdf\"><a href=\"/nb/notes%20%26%20more/f/spec.pdf\">a &amp; b</a></object></p>\n"},
		{"image of an external pdf is a link that leaves, never an object", "![x <y>](https://example.com/s.pdf)",
			"<p><a href=\"https://example.com/s.pdf\" target=\"_blank\" rel=\"noopener noreferrer\">x &lt;y&gt;</a></p>\n"},
		{"image of a pdf in a link is its words, so the link stays whole", "[![x](spec.pdf)](https://example.com)",
			"<p><a href=\"https://example.com\" target=\"_blank\" rel=\"noopener noreferrer\">x</a></p>\n"},
		{"image of an external pdf in a link is its words", "[![](https://example.com/s.pdf)](spec.md)",
			"<p><a href=\"/nb/notes%20%26%20more/f/spec.md\">https://example.com/s.pdf</a></p>\n"},
		{"image of a file only named like a pdf stays an image", "![x](spec.pdf.png)",
			"<p><img src=\"/nb/notes%20%26%20more/f/spec.pdf.png\" alt=\"x\" /></p>\n"},
		{"a link to a pdf is still a link", "[x](spec.pdf)",
			"<p><a href=\"/nb/notes%20%26%20more/f/spec.pdf\">x</a></p>\n"},
		{"bare url becomes a link", "https://example.com/bare",
			"<p><a href=\"https://example.com/bare\" target=\"_blank\" rel=\"noopener noreferrer\">https://example.com/bare</a></p>\n"},
		{"bare url drops trailing sentence dot", "see https://example.com/bare. next",
			"<p>see <a href=\"https://example.com/bare\" target=\"_blank\" rel=\"noopener noreferrer\">https://example.com/bare</a>. next</p>\n"},
		{"bare url drops closing paren", "(https://example.com/bare)",
			"<p>(<a href=\"https://example.com/bare\" target=\"_blank\" rel=\"noopener noreferrer\">https://example.com/bare</a>)</p>\n"},
		{"bare url stops before angle bracket", "https://example.com/a?b=1&c=<2>",
			"<p><a href=\"https://example.com/a?b=1&amp;c=\" target=\"_blank\" rel=\"noopener noreferrer\">https://example.com/a?b=1&amp;c=</a>&lt;2&gt;</p>\n"},
		{"bare ftp url stays text", "ftp://example.com/nope",
			"<p>ftp://example.com/nope</p>\n"},
		{"two bare urls", "http://example.com and https://two.example",
			"<p><a href=\"http://example.com\" target=\"_blank\" rel=\"noopener noreferrer\">http://example.com</a> and <a href=\"https://two.example\" target=\"_blank\" rel=\"noopener noreferrer\">https://two.example</a></p>\n"},
		{"explicit and bare link to the same url", "[x](https://a.com) https://a.com",
			"<p><a href=\"https://a.com\" target=\"_blank\" rel=\"noopener noreferrer\">x</a> <a href=\"https://a.com\" target=\"_blank\" rel=\"noopener noreferrer\">https://a.com</a></p>\n"},
		{"url in code span is not linked", "`https://example.com/in-code`",
			"<p><code>https://example.com/in-code</code></p>\n"},
		{"autolink in angle brackets", "<https://example.com/angle>",
			"<p><a href=\"https://example.com/angle\" target=\"_blank\" rel=\"noopener noreferrer\">https://example.com/angle</a></p>\n"},
		{"email autolink", "<a@b.c>",
			"<p><a href=\"mailto:a@b.c\">a@b.c</a></p>\n"},
		{"entities are decoded", "&amp; &lt; &copy; &#35;",
			"<p>&amp; &lt; © #</p>\n"},
		{"quotes and stray ampersand", "\"quotes\" 'single' <angle> &amp",
			"<p>\"quotes\" 'single' <code>&lt;angle&gt;</code> &amp;amp</p>\n"},
		{"japanese text with a note link", "日本語 **強調** [リンク](01abc234-hello.md)",
			"<p>日本語 <strong>強調</strong> <a class=\"note h10\" href=\"/nb/notes%20%26%20more/n/01abc234\">リンク</a></p>\n"},
		{"emoji", "emoji 😀 text",
			"<p>emoji 😀 text</p>\n"},
		{"angle bracket destination with a space", "[x](<with space.md>)",
			"<p><a href=\"/nb/notes%20%26%20more/f/with%20space.md\">x</a></p>\n"},
		{"percent encoded space", "[x](with%20space.md)",
			"<p><a href=\"/nb/notes%20%26%20more/f/with%20space.md\">x</a></p>\n"},
		{"space in destination is not a link", "[x](https://example.com/a b)",
			"<p>[x](<a href=\"https://example.com/a\" target=\"_blank\" rel=\"noopener noreferrer\">https://example.com/a</a> b)</p>\n"},
		{"quoted destination is escaped", "[x](\"quoted\")",
			"<p><a href=\"/nb/notes%20%26%20more/f/%22quoted%22\">x</a></p>\n"},
		{"quote in url is escaped", "[x](https://example.com/\"onmouseover=\"x)",
			"<p><a href=\"https://example.com/&quot;onmouseover=&quot;x\" target=\"_blank\" rel=\"noopener noreferrer\">x</a></p>\n"},
		{"link with empty text", "[](https://example.com)",
			"<p><a href=\"https://example.com\" target=\"_blank\" rel=\"noopener noreferrer\"></a></p>\n"},
		{"link with blank text", "[ ](01abc234-hello.md)",
			"<p><a class=\"note h10\" href=\"/nb/notes%20%26%20more/n/01abc234\"> </a></p>\n"},
		{"footnote", "footnote[^1]\n\n[^1]: the note\n",
			"<p>footnote" + footnoteRef("fnref:", 1) + "</p>\n" +
				footnoteList("<li id=\"fn:1\">\n<p>the note"+footnoteBack("fnref:1", "")+"</p>\n</li>\n")},
		{"footnotes are numbered by first citation", "b[^2] a[^1]\n\n[^1]: one\n[^2]: two\n",
			"<p>b" + footnoteRef("fnref:", 1) + " a" + footnoteRef("fnref:", 2) + "</p>\n" +
				footnoteList("<li id=\"fn:1\">\n<p>two"+footnoteBack("fnref:1", "")+"</p>\n</li>\n"+
					"<li id=\"fn:2\">\n<p>one"+footnoteBack("fnref:2", "")+"</p>\n</li>\n")},
		{"a footnote cited twice returns to both", "a[^x] b[^x]\n\n[^x]: shared\n",
			"<p>a" + footnoteRef("fnref:", 1) + " b" + footnoteRef("fnref1:", 1) + "</p>\n" +
				footnoteList("<li id=\"fn:1\">\n<p>shared"+footnoteBack("fnref:1", "")+footnoteBack("fnref1:1", "<sup>2</sup>")+"</p>\n</li>\n")},
		{"a footnote of two paragraphs returns from the last", "a[^1]\n\n[^1]: first\n\n    second\n",
			"<p>a" + footnoteRef("fnref:", 1) + "</p>\n" +
				footnoteList("<li id=\"fn:1\">\n<p>first</p>\n<p>second"+footnoteBack("fnref:1", "")+"</p>\n</li>\n")},
		{"a footnote ending in code returns after it", "a[^1]\n\n[^1]: code\n\n        x\n",
			"<p>a" + footnoteRef("fnref:", 1) + "</p>\n" +
				footnoteList("<li id=\"fn:1\">\n<p>code</p>\n<pre><code>x\n</code></pre>\n"+footnoteBack("fnref:1", "")+"</li>\n")},
		{"a footnote is read as markdown", "a[^1]\n\n[^1]: *em* and <b>\n",
			"<p>a" + footnoteRef("fnref:", 1) + "</p>\n" +
				footnoteList("<li id=\"fn:1\">\n<p><em>em</em> and <code>&lt;b&gt;</code>"+footnoteBack("fnref:1", "")+"</p>\n</li>\n")},
		{"a footnote cited in a table cell", "| h |\n|---|\n| c[^1] |\n\n[^1]: n\n",
			"<table><thead><tr><th>h</th></tr></thead><tbody>\n<tr><td>c" + footnoteRef("fnref:", 1) + "</td></tr>\n</tbody></table>\n" +
				footnoteList("<li id=\"fn:1\">\n<p>n"+footnoteBack("fnref:1", "")+"</p>\n</li>\n")},
		// The id stays so the arrow under the footnote still has somewhere to return to.
		{"a footnote cited inside a link is a bare number", "[a[^1]](https://example.com)\n\n[^1]: n\n",
			"<p><a href=\"https://example.com\" target=\"_blank\" rel=\"noopener noreferrer\">a<sup id=\"fnref:1\">1</sup></a></p>\n" +
				footnoteList("<li id=\"fn:1\">\n<p>n"+footnoteBack("fnref:1", "")+"</p>\n</li>\n")},
		{"a citation with no definition is text", "orphan[^nope]\n",
			"<p>orphan[^nope]</p>\n"},
		{"a definition nothing cites is dropped", "[^unused]: nobody\n\ntext\n",
			"<p>text</p>\n"},
		{"a definition in a code fence is code", "```\n[^1]: x\n```\n",
			"<pre><code>[^1]: x\n</code></pre>\n"},
		{"backslash escapes", "a\\*b\\_c",
			"<p>a*b_c</p>\n"},
		{"nul byte survives", "\u0000null",
			"<p>\u0000null</p>\n"},
	} {
		t.Run(c.name, func(t *testing.T) {
			if got := Body(c.markdown, where); got != c.want {
				t.Errorf("markdown %q\n got %q\nwant %q", c.markdown, got, c.want)
			}
		})
	}
}

// A link to a note carries the hue of the note it names, so it matches that
// note's row in the listing without asking the server which note it is.
func TestANoteLinkWearsTheHueOfItsTarget(t *testing.T) {
	where := AroundOf("work", []Named{{ID: "01abc234", Slug: "a"}, {ID: "04kmn123", Slug: "b"}})
	for id, slug := range map[string]string{"01abc234": "a", "04kmn123": "b"} {
		want := `<a class="note ` + hueClass(id) + `" href="/nb/work/n/` + id + `">x</a>`
		if got := Body("[x]("+id+"-"+slug+".md)", where); !strings.Contains(got, want) {
			t.Errorf("%s: %q lacks %q", id, got, want)
		}
	}
	if hueClass("01abc234") == hueClass("04kmn123") {
		t.Fatal("the two ids were chosen to differ")
	}
}

func TestBodyNeverEmitsRawHTMLOrAScriptLink(t *testing.T) {
	where := AroundOf("work", nil)
	for _, source := range []string{
		"<script>alert(1)</script>",
		"text <img src=x onerror=alert(1)> text",
		"[x](javascript:alert(1))",
		"[x](JaVaScRiPt:alert(1))",
		"![x](data:text/html,<script>alert(1)</script>)",
		"<https://example.com/\"onmouseover=\"x>",
		"[x](https://example.com/\"onmouseover=\"x)",
	} {
		out := Body(source, where)
		for _, bad := range []string{"<script", "<img", "javascript:", "href=\"data:"} {
			if strings.Contains(strings.ToLower(out), bad) {
				t.Errorf("%q rendered %q, which holds %q", source, out, bad)
			}
		}
	}
}

func TestEveryWrittenAnchorIsClosed(t *testing.T) {
	out := Body("[a](https://a.example) [b](k-n.md) [c](f.png) https://bare.example/x.", AroundOf("work", []Named{{ID: "k", Slug: "n"}}))
	if strings.Count(out, "<a ") != strings.Count(out, "</a>") {
		t.Errorf("unbalanced anchors: %s", out)
	}
}

func TestNoScriptCanEndItsOwnElement(t *testing.T) {
	for _, asset := range allAssets {
		if strings.Contains(strings.ToLower(asset.body()), "</script") {
			t.Errorf("%s holds </script", asset.Href())
		}
	}
}

// footnoteRef is the number a footnote leaves in the text; id is where the citation is
// named, "fnref:" for a note's first and "fnref1:" for its second.
func footnoteRef(id string, note int) string {
	n := strconv.Itoa(note)
	return "<sup id=\"" + id + n + "\"><a href=\"#fn:" + n + "\" class=\"footnote-ref\" role=\"doc-noteref\">" + n + "</a></sup>"
}

// footnoteBack is the arrow that returns from a footnote to the citation id names.
func footnoteBack(id, mark string) string {
	return "&#160;<a href=\"#" + id + "\" class=\"footnote-backref\" role=\"doc-backlink\">&#x21a9;&#xfe0e;" + mark + "</a>"
}

func footnoteList(items string) string {
	return "<div class=\"footnotes\" role=\"doc-endnotes\">\n<hr />\n<ol>\n" + items + "</ol>\n</div>\n"
}
