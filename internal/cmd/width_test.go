package cmd

import (
	"encoding/json"
	"os"
	"testing"
)

func TestDisplayWidthMatchesRustGolden(t *testing.T) {
	var cases []struct{ Name, Text string }
	var expected []struct {
		Name string
		Out  int
	}
	readJSON(t, "../../testdata/golden/width/cases.json", &cases)
	readJSON(t, "../../testdata/golden/width/expected.json", &expected)
	if len(cases) != len(expected) {
		t.Fatalf("%d cases, %d expected", len(cases), len(expected))
	}
	for i, c := range cases {
		if got := DisplayWidth(c.Text); got != expected[i].Out {
			t.Errorf("%s %q: width %d, Rust said %d", c.Name, c.Text, got, expected[i].Out)
		}
	}
}

func TestPadAndColumnIgnoreEscapes(t *testing.T) {
	if got := pad("日本", 6); got != "日本  " {
		t.Errorf("pad = %q", got)
	}
	if got := pad("long", 2); got != "long" {
		t.Errorf("pad never cuts: %q", got)
	}
}

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
