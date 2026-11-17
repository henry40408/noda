package notebook

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/henry40408/noda/internal/gitx"
	"github.com/henry40408/noda/internal/gitx/gittest"
)

func TestBlameRunsNoTextconvOfTheUsersConfig(t *testing.T) {
	nb := fixture(t, "linear")
	want, err := nb.Blame("02def567", "meeting-notes")
	if err != nil {
		t.Fatal(err)
	}

	env := gittest.Isolate(t)
	env["TZ"] = "UTC"
	attributes := filepath.Join(filepath.Dir(env["GIT_CONFIG_GLOBAL"]), "attributes")
	if err := os.WriteFile(attributes, []byte("*.md diff=evil\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	gittest.WriteGlobalConfig(t, env, "[core]\n\tattributesFile = "+attributes+
		"\n[diff \"evil\"]\n\ttextconv = sh -c 'echo CONVERTED; echo textconv-ran >&2'\n", true)
	git, err := gitx.New(gitx.Options{Env: env})
	if err != nil {
		t.Fatal(err)
	}
	nb.git = git

	got, err := nb.Blame("02def567", "meeting-notes")
	if err != nil {
		t.Fatalf("blame with a textconv configured: %v", err)
	}
	same(t, "blame", renderBlame(got), renderBlame(want))
}
