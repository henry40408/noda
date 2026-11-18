package main

import (
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
)

// The release checklist asks for a hand check on a copy of a real notebook.
// This is the part a machine can do: a notebook built in a temp dir goes
// through every history command and a push/pull round trip with a bare remote.
func TestSmokeANotebookSurvivesTheHistoryAndSyncCommands(t *testing.T) {
	s := newSandbox(t)
	s.ok(t, "init")
	s.ok(t, "add", "Smoke note", "-c", "first line\n")
	s.ok(t, "tag", "smoke-note", "+demo")

	bare := filepath.Join(t.TempDir(), "remote.git")
	initBare := exec.Command("git", "init", "--bare", "-q", bare)
	initBare.Env = envList(s.env)
	if out, err := initBare.CombinedOutput(); err != nil {
		t.Fatalf("git init --bare: %v\n%s", err, out)
	}
	s.ok(t, "remote", "set", "file://"+bare)

	for _, args := range [][]string{
		{"status"}, {"log"}, {"blame", "smoke-note"}, {"diff"}, {"push"}, {"pull"},
	} {
		got := s.run(args...)
		if got.code != 0 {
			t.Errorf("noda %v: exit %d, stderr %q", args, got.code, got.stderr)
		}
	}
	if log := s.ok(t, "log"); !strings.Contains(log, "smoke-note") {
		t.Errorf("log does not name the note: %q", log)
	}
	if status := s.ok(t, "status"); !strings.Contains(status, "in sync") {
		t.Errorf("after push, status is not in sync: %q", status)
	}

	notebookDir := strings.TrimSpace(s.ok(t, "path"))
	for _, check := range [][]string{{"fsck", "--strict"}, {"cat-file", "-e", "HEAD"}} {
		command := exec.Command("git", append([]string{"-C", notebookDir}, check...)...)
		command.Env = envList(s.env)
		if out, err := command.CombinedOutput(); err != nil {
			t.Errorf("git %v: %v\n%s", check, err, out)
		}
	}
}

func envList(env map[string]string) []string {
	list := make([]string, 0, len(env))
	for name, value := range env {
		list = append(list, name+"="+value)
	}
	return list
}
