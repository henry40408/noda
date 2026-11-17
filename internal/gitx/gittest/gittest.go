// Package gittest is for tests that run git. They fail, not skip, when git is
// missing or too old: a skipped test hides a broken environment.
package gittest

import (
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/henry40408/noda/internal/gitx"
)

// Isolate is an environment with the developer's own git settings (signing,
// hooks, aliases, `diff.*`) out of reach.
func Isolate(t *testing.T) map[string]string {
	t.Helper()
	env := map[string]string{}
	for _, pair := range os.Environ() {
		if name, value, found := strings.Cut(pair, "="); found {
			env[name] = value
		}
	}
	home := t.TempDir()
	config := filepath.Join(home, "gitconfig")
	settings := "[user]\n\tname = Test\n\temail = test@example.com\n[init]\n\tdefaultBranch = main\n[commit]\n\tgpgsign = false\n"
	if err := os.WriteFile(config, []byte(settings), 0o644); err != nil {
		t.Fatal(err)
	}
	env["HOME"] = home
	env["XDG_CONFIG_HOME"] = filepath.Join(home, "xdg")
	env["GIT_CONFIG_NOSYSTEM"] = "1"
	env["GIT_CONFIG_GLOBAL"] = config
	return env
}

// Require returns a Git running in the isolated environment.
func Require(t *testing.T) *gitx.Git {
	t.Helper()
	git, err := gitx.New(gitx.Options{Env: Isolate(t)})
	if err != nil {
		t.Fatalf("these tests need git %s or later: %v", gitx.MinVersion, err)
	}
	return git
}
