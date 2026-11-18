package gitx_test

import (
	"context"
	"encoding/json"
	"os"
	"testing"

	"github.com/henry40408/noda/internal/gitx"
	"github.com/henry40408/noda/internal/gitx/gittest"
)

// TestMergeFileMatchesTheRustGolden replays the cases libgit2's merge_file was
// captured on, with a user's diff3 setting that must not leak into the result.
func TestMergeFileMatchesTheRustGolden(t *testing.T) {
	var cases []struct{ Name, Base, Ours, Theirs string }
	var expected []struct {
		Name string
		Out  struct {
			Clean bool
			Text  string
		}
	}
	readJSON(t, "../../testdata/golden/merge3/cases.json", &cases)
	readJSON(t, "../../testdata/golden/merge3/expected.json", &expected)

	env := gittest.Isolate(t)
	git, err := gitx.New(gitx.Options{Env: env})
	if err != nil {
		t.Fatal(err)
	}
	appendGlobalConfig(t, git, "[merge]\n\tconflictStyle = diff3\n")
	for i, c := range cases {
		got, err := gitx.MergeFile(context.Background(), git, gitx.MergeInput{
			Base: c.Base, Ours: c.Ours, Theirs: c.Theirs,
			OursLabel: "what you wrote", TheirsLabel: "saved since",
		})
		if err != nil {
			t.Fatalf("%s: %v", c.Name, err)
		}
		want := expected[i]
		if got.Clean != want.Out.Clean || got.Text != want.Out.Text {
			t.Errorf("%s: got clean=%v %q, want clean=%v %q", c.Name, got.Clean, got.Text, want.Out.Clean, want.Out.Text)
		}
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
