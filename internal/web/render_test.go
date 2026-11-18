package web

import (
	"encoding/json"
	"os"
	"strings"
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

func TestRenderGolden(t *testing.T) {
	var cases []struct{ Name, Markdown string }
	var expected []struct{ Name, Out string }
	var around struct {
		Book  string
		Named [][2]string
	}
	readJSON(t, "../../testdata/golden/render/cases.json", &cases)
	readJSON(t, "../../testdata/golden/render/expected.json", &expected)
	readJSON(t, "../../testdata/golden/render/around.json", &around)
	named := []Named{}
	for _, pair := range around.Named {
		named = append(named, Named{ID: pair[0], Slug: pair[1]})
	}
	where := AroundOf(around.Book, named)
	for i, c := range cases {
		if expected[i].Name != c.Name {
			t.Fatalf("order differs at %d", i)
		}
		t.Run(c.Name, func(t *testing.T) {
			if got := Body(c.Markdown, where); got != expected[i].Out {
				t.Errorf("markdown %q\n got %q\nwant %q", c.Markdown, got, expected[i].Out)
			}
		})
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
