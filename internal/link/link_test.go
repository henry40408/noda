package link

import (
	"slices"
	"testing"
)

func targetsOf(body string) []string { return Targets(body) }

func TestInlineLinksAndImagesBothCount(t *testing.T) {
	got := targetsOf("![diagram](diagram.png) and [the spec](spec.pdf)\n")
	if !slices.Equal(got, []string{"diagram.png", "spec.pdf"}) {
		t.Fatal(got)
	}
}

func TestAReferenceStyleLinkIsResolved(t *testing.T) {
	got := targetsOf("See ![the diagram][d] for the shape.\n\n[d]: diagram.png\n")
	if !slices.Equal(got, []string{"diagram.png"}) {
		t.Fatal(got)
	}
}

func TestALinkInsideACodeFenceIsNotALink(t *testing.T) {
	body := "```markdown\n![diagram](diagram.png)\n```\n\nand `[x](y.png)` inline\n"
	if got := targetsOf(body); len(got) != 0 {
		t.Fatal(got)
	}
}

// The renderer draws `[^1]: Ibid.` as a footnote. Read as a link reference
// definition it would name a file called `Ibid.`, and doctor would report it
// broken.
func TestAFootnoteIsNotALinkToAFile(t *testing.T) {
	if got := targetsOf("Said before.[^1]\n\n[^1]: Ibid.\n"); len(got) != 0 {
		t.Fatal(got)
	}
}

func TestALinkInsideAFootnoteIsALink(t *testing.T) {
	body := "Source.[^1]\n\n[^1]: [Other](k3f9m2p1-other.md)\n"
	if got := targetsOf(body); !slices.Equal(got, []string{"k3f9m2p1-other.md"}) {
		t.Fatal(got)
	}
	want := "Source.[^1]\n\n[^1]: [Other](m4n5p6q7-renamed.md)\n"
	if got := rewritten(t, body, "k3f9m2p1-other.md", "m4n5p6q7-renamed.md"); got != want {
		t.Fatal(got)
	}
}

func TestDestinationsThatNameNoFileInTheNotebookAreDropped(t *testing.T) {
	body := "[a](https://example.com/x.png)\n[b](http://example.com)\n" +
		"[c](mailto:me@example.com)\n[d](#section)\n[e](/etc/passwd)\n[f](../outside.png)\n"
	if got := targetsOf(body); len(got) != 0 {
		t.Fatal(got)
	}
}

func TestADestinationIsNormalizedBeforeItIsCompared(t *testing.T) {
	cases := map[string]string{
		"[a](./diagram.png)\n":                                    "diagram.png",
		"[a](my%20file.pdf)\n":                                    "my file.pdf",
		"[a](diagram.png#page=2)\n":                               "diagram.png",
		"[a](sub/./dir/../f.png)\n":                               "sub/f.png",
		"[a](k3f9m2p1-other.md)\n":                                "k3f9m2p1-other.md",
		"[a](./diagram.png) [b](diagram.png) ![c](diagram.png)\n": "diagram.png",
	}
	for body, want := range cases {
		if got := targetsOf(body); !slices.Equal(got, []string{want}) {
			t.Errorf("%q: %v, want %s", body, got, want)
		}
	}
}

func rewritten(t *testing.T, body, old, replacement string) string {
	t.Helper()
	out, ok := Rewrite(body, old, replacement)
	if !ok {
		t.Fatalf("%q: nothing rewritten", body)
	}
	return out
}

func TestRewriteReplacesOnlyTheDestination(t *testing.T) {
	body := "See ![diagram.png](diagram.png \"the shape\") and [x](other.png)\n"
	want := "See ![diagram.png](shape.png \"the shape\") and [x](other.png)\n"
	if got := rewritten(t, body, "diagram.png", "shape.png"); got != want {
		t.Fatal(got)
	}
}

func TestRewriteReachesAReferenceDefinitionAtTheBottom(t *testing.T) {
	body := "See ![the diagram][d].\n\n[d]: diagram.png\n"
	got := rewritten(t, body, "diagram.png", "shape.png")
	if got != "See ![the diagram][d].\n\n[d]: shape.png\n" {
		t.Fatal(got)
	}
	if !slices.Equal(Targets(got), []string{"shape.png"}) {
		t.Fatal(Targets(got))
	}
}

func TestRewriteFindsEverySpellingOfTheSameFile(t *testing.T) {
	body := "![a](diagram.png) [b](./diagram.png) [c](diagram.png#page=2)\n"
	got := rewritten(t, body, "diagram.png", "shape.png")
	if !slices.Equal(Targets(got), []string{"shape.png"}) {
		t.Fatal(got)
	}
	if got != "![a](shape.png) [b](shape.png) [c](shape.png#page=2)\n" {
		t.Fatal(got)
	}
}

func TestRewriteLeavesABodyThatNeverNamedItAlone(t *testing.T) {
	if _, ok := Rewrite("![a](other.png)\n", "diagram.png", "shape.png"); ok {
		t.Fatal("rewrote an unrelated body")
	}
	if _, ok := Rewrite("```\n![a](diagram.png)\n```\n", "diagram.png", "shape.png"); ok {
		t.Fatal("a link inside a fence is not a link here either")
	}
	if _, ok := Rewrite("```\n[d]: diagram.png\n```\n\n[d]: other.png\n", "diagram.png", "shape.png"); ok {
		t.Fatal("a definition inside a fence is not a definition")
	}
}

func TestRewriteEncodesANewNameThatWouldNotSurviveVerbatim(t *testing.T) {
	got := rewritten(t, "![a](diagram.png)\n", "diagram.png", "my shape (v2).png")
	if got != "![a](my%20shape%20%28v2%29.png)\n" {
		t.Fatal(got)
	}
	if !slices.Equal(Targets(got), []string{"my shape (v2).png"}) {
		t.Fatal(Targets(got))
	}
}

func TestTheLinkTextIsNotMistakenForTheDestination(t *testing.T) {
	got := rewritten(t, "[diagram.png](diagram.png)\n", "diagram.png", "shape.png")
	if got != "[diagram.png](shape.png)\n" {
		t.Fatal(got)
	}
}

func TestADestinationThatCannotBeLocatedIsLeftForTheCallerToNotice(t *testing.T) {
	body := "[a](my\\(file\\).png)\n"
	if !slices.Equal(Targets(body), []string{"my(file).png"}) {
		t.Fatal(Targets(body))
	}
	if _, ok := Rewrite(body, "my(file).png", "shape.png"); ok {
		t.Fatal("an escaped destination cannot be located")
	}
}

func TestALinkNestedInAnImageIsRewrittenAtItsOwnDestination(t *testing.T) {
	got := rewritten(t, "[![alt](pic.png)](page.md)\n", "page.md", "next.md")
	if got != "[![alt](pic.png)](next.md)\n" {
		t.Fatal(got)
	}
	got = rewritten(t, "[![alt](pic.png)](page.md)\n", "pic.png", "next.png")
	if got != "[![alt](next.png)](page.md)\n" {
		t.Fatal(got)
	}
}

// The cases below are what the Rust rewrite does (it searches only the event's
// own range); the review that found these gaps listed each outcome.
func TestRewriteTouchesTheLinkNotTheCodeThatLooksLikeOne(t *testing.T) {
	cases := []struct{ name, body, old, want string }{
		{"code span after an escaped link", "[a](x\\_y.md) and `[b](x_y.md)`\n", "x_y.md", ""},
		{"code span after the real link", "[a](x_y.md) and `[b](x_y.md)`\n", "x_y.md", "[a](z.md) and `[b](x_y.md)`\n"},
		{"code span before the real link", "`[b](x_y.md)` then [a](x_y.md)\n", "x_y.md", "`[b](x_y.md)` then [a](z.md)\n"},
		{"fence after the paragraph", "[a](x_y.md)\n\n```\n[b](x_y.md)\n```\n", "x_y.md", "[a](z.md)\n\n```\n[b](x_y.md)\n```\n"},
		{"fence only", "```\n[b](x_y.md)\n```\n", "x_y.md", ""},
	}
	for _, c := range cases {
		got, ok := Rewrite(c.body, c.old, "z.md")
		if c.want == "" {
			if ok {
				t.Errorf("%s: rewrote %q", c.name, got)
			}
			continue
		}
		if !ok || got != c.want {
			t.Errorf("%s: got %q (%v), want %q", c.name, got, ok, c.want)
		}
	}
}

func TestRewriteFindsTheRealDefinitionNotALazyContinuation(t *testing.T) {
	body := "intro\n[r]: x.md\n\n[r]: x.md\n\n[see][r]\n"
	got, ok := Rewrite(body, "x.md", "z.md")
	if !ok || got != "intro\n[r]: x.md\n\n[r]: z.md\n\n[see][r]\n" {
		t.Fatalf("%q %v", got, ok)
	}
}

func TestRewriteSkipsRawHTMLAndAutolinksBeforeAnEmptyLabel(t *testing.T) {
	cases := []struct{ name, body, want string }{
		{"raw html", "<i title=\"](p.png)\"></i>![](p.png)\n", "<i title=\"](p.png)\"></i>![](z.md)\n"},
		{"autolink", "x <https://e.com/](p.png)> ![](p.png)\n", "x <https://e.com/](p.png)> ![](z.md)\n"},
	}
	for _, c := range cases {
		got, ok := Rewrite(c.body, "p.png", "z.md")
		if !ok || got != c.want {
			t.Errorf("%s: got %q (%v), want %q", c.name, got, ok, c.want)
		}
	}
}

func TestRewriteReachesDestinationsWithAnEmptyLabel(t *testing.T) {
	cases := []struct{ name, body, old, want string }{
		{"empty link", "[](x.md)\n", "x.md", "[](z.md)\n"},
		{"empty alt", "![](pic.png)\n", "pic.png", "![](z.md)\n"},
		{"empty alt in a link, outer", "[![](t.png)](f.png)\n", "f.png", "[![](t.png)](z.md)\n"},
		{"empty alt in a link, inner", "[![](t.png)](f.png)\n", "t.png", "[![](z.md)](f.png)\n"},
		{"two in a row", "![](p.png) ![](p.png)\n", "p.png", "![](z.md) ![](z.md)\n"},
		{"after a labelled one", "[a](p.png) then ![](p.png)\n", "p.png", "[a](z.md) then ![](z.md)\n"},
		{"in a blockquote definition", "> [r]: x.md\n\n[see][r]\n", "x.md", "> [r]: z.md\n\n[see][r]\n"},
		{"in a list definition", "- [r]: x.md\n\n[see][r]\n", "x.md", "- [r]: z.md\n\n[see][r]\n"},
	}
	for _, c := range cases {
		got, ok := Rewrite(c.body, c.old, "z.md")
		if !ok || got != c.want {
			t.Errorf("%s: got %q (%v), want %q", c.name, got, ok, c.want)
		}
	}
}

func TestSchemeNeedsALetterFirstAndOnlySchemeCharacters(t *testing.T) {
	cases := map[string]string{"https://x": "https", "mailto:a@b": "mailto", "a+b-c.d:x": "a+b-c.d"}
	for dest, want := range cases {
		if got, ok := Scheme(dest); !ok || got != want {
			t.Errorf("%s: %q %v", dest, got, ok)
		}
	}
	for _, dest := range []string{"", ":x", "1a:x", "a b:c", "file.md", "日本:x"} {
		if got, ok := Scheme(dest); ok {
			t.Errorf("%s: %q", dest, got)
		}
	}
}

func TestSchemeLettersAreExactlyTheASCIILetters(t *testing.T) {
	for _, dest := range []string{"a:x", "z:x", "A:x", "Z:x", "bz:x", "bZ:x", "ba:x", "bA:x"} {
		if _, ok := Scheme(dest); !ok {
			t.Errorf("%s is a scheme", dest)
		}
	}
	for _, dest := range []string{"`:x", "{:x", "@:x", "[:x", "b`:x", "b{:x", "b@:x", "b[:x"} {
		if got, ok := Scheme(dest); ok {
			t.Errorf("%s: %q", dest, got)
		}
	}
}

func TestAPercentSignThatCutsOffTheTextIsKept(t *testing.T) {
	for _, dest := range []string{"file%", "file%4", "file%41", "file%4g.md"} {
		want := map[string]string{"file%41": "fileA"}[dest]
		if want == "" {
			want = dest
		}
		if got, ok := Target(dest); !ok || got != want {
			t.Errorf("%s: %q %v, want %q", dest, got, ok, want)
		}
	}
}
