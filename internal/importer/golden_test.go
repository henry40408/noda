package importer

import (
	"encoding/json"
	"os"
	"slices"
	"testing"
)

func readGolden(t *testing.T, name string, into any) {
	t.Helper()
	data, err := os.ReadFile("../../testdata/golden/wikitext/" + name)
	if err != nil {
		t.Fatal(err)
	}
	if err := json.Unmarshal(data, into); err != nil {
		t.Fatalf("%s: %v", name, err)
	}
}

// The golden files were captured from the Rust converter; they are never
// edited to fit this one.
func TestWikitextMatchesTheRustGoldens(t *testing.T) {
	var cases []struct{ Name, Text string }
	var expected []struct {
		Name string
		Out  struct {
			Left []string
			Text string
		}
	}
	var known map[string]string
	readGolden(t, "cases.json", &cases)
	readGolden(t, "expected.json", &expected)
	readGolden(t, "known.json", &known)
	if len(cases) != len(expected) {
		t.Fatalf("%d cases, %d expected", len(cases), len(expected))
	}

	resolve := func(title string) (string, bool) {
		file, ok := known[title]
		return file, ok
	}
	for i, c := range cases {
		want := expected[i]
		if want.Name != c.Name {
			t.Fatalf("case %d is %s, expected %s", i, c.Name, want.Name)
		}
		got := Convert(c.Text, resolve)
		if got.Text != want.Out.Text {
			t.Errorf("%s text:\n got: %q\nwant: %q", c.Name, got.Text, want.Out.Text)
		}
		if !slices.Equal(got.Left, want.Out.Left) {
			t.Errorf("%s left: got %q, want %q", c.Name, got.Left, want.Out.Left)
		}
	}
}
