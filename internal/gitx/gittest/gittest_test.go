package gittest

import (
	"os"
	"path/filepath"
	"testing"
)

func TestWriteGlobalConfigStaysInTheSandbox(t *testing.T) {
	env := Isolate(t)
	WriteGlobalConfig(t, env, "[user]\n\tname = Again\n", false)
	got, err := os.ReadFile(env["GIT_CONFIG_GLOBAL"])
	if err != nil || string(got) != "[user]\n\tname = Again\n" {
		t.Fatalf("%q %v", got, err)
	}
}

func TestSandboxErrorRefusesOutsideAndRealHome(t *testing.T) {
	realHome, err := os.UserHomeDir()
	if err != nil {
		t.Skip("no home directory")
	}
	sandbox := t.TempDir()
	refused := []map[string]string{
		{"HOME": sandbox, "GIT_CONFIG_GLOBAL": filepath.Join(realHome, ".gitconfig")},
		{"HOME": realHome, "GIT_CONFIG_GLOBAL": filepath.Join(realHome, ".gitconfig")},
		{"HOME": sandbox, "GIT_CONFIG_GLOBAL": ""},
		{"HOME": "", "GIT_CONFIG_GLOBAL": filepath.Join(sandbox, "x")},
		{"HOME": sandbox, "GIT_CONFIG_GLOBAL": filepath.Join(sandbox, "..", "x")},
	}
	for _, env := range refused {
		if sandboxError(env, env["GIT_CONFIG_GLOBAL"]) == nil {
			t.Errorf("not refused: %v", env)
		}
	}
}

func TestIsolateDropsInheritedGitRepositoryVariables(t *testing.T) {
	t.Setenv("GIT_DIR", "/elsewhere/.git")
	t.Setenv("GIT_WORK_TREE", "/elsewhere")

	env := Isolate(t)

	for _, name := range []string{"GIT_DIR", "GIT_WORK_TREE"} {
		if value, found := env[name]; found {
			t.Errorf("%s = %q leaked into the isolated environment", name, value)
		}
	}
}
