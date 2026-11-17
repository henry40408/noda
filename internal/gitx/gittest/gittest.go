// Package gittest is for tests that run git. They fail, not skip, when git is
// missing or too old: a skipped test hides a broken environment.
package gittest

import (
	"fmt"
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
	env["HOME"] = home
	env["XDG_CONFIG_HOME"] = filepath.Join(home, "xdg")
	env["GIT_CONFIG_NOSYSTEM"] = "1"
	env["GIT_CONFIG_GLOBAL"] = config
	WriteGlobalConfig(t, env, settings, false)
	return env
}

// WriteGlobalConfig replaces (or, with appendTo, extends) the isolated global
// git config. It refuses any path outside the sandbox home that Isolate made:
// a test once wrote a gitconfig into the real $HOME and broke the developer's git.
func WriteGlobalConfig(t *testing.T, env map[string]string, text string, appendTo bool) {
	t.Helper()
	path := env["GIT_CONFIG_GLOBAL"]
	if err := sandboxError(env, path); err != nil {
		t.Fatal(err)
	}
	flags := os.O_WRONLY | os.O_CREATE | os.O_TRUNC
	if appendTo {
		flags = os.O_WRONLY | os.O_CREATE | os.O_APPEND
	}
	file, err := os.OpenFile(path, flags, 0o600)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := file.WriteString(text); err != nil {
		t.Fatal(err)
	}
	if err := file.Close(); err != nil {
		t.Fatal(err)
	}
}

func sandboxError(env map[string]string, path string) error {
	home := env["HOME"]
	if path == "" || home == "" {
		return fmt.Errorf("refusing to write %q: the environment is not isolated", path)
	}
	if realHome, err := os.UserHomeDir(); err == nil && filepath.Clean(home) == filepath.Clean(realHome) {
		return fmt.Errorf("refusing to write %q: HOME is the real home directory", path)
	}
	relative, err := filepath.Rel(home, path)
	if err != nil || relative == ".." || strings.HasPrefix(relative, ".."+string(filepath.Separator)) {
		return fmt.Errorf("refusing to write %q: it is outside the sandbox home %q", path, home)
	}
	return nil
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
