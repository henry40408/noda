package gitx

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestParseVersionReadsVendorSuffixes(t *testing.T) {
	cases := map[string]Version{
		"git version 2.39.5\n":                 {2, 39, 5},
		"git version 2.39.5 (Apple Git-154)\n": {2, 39, 5},
		"git version 2.47.1.windows.1\n":       {2, 47, 1},
		"git version 2.50.0.rc1\n":             {2, 50, 0},
		"git version 2.38.0\n":                 {2, 38, 0},
		"git version 3.0\n":                    {3, 0, 0},
	}
	for output, want := range cases {
		got, err := parseVersion(output)
		if err != nil || got != want {
			t.Errorf("%q: %v %v", output, got, err)
		}
	}
	if _, err := parseVersion("not git"); err == nil {
		t.Error("an unreadable version is an error")
	}
}

func TestVersionsOrderByEachNumber(t *testing.T) {
	if !(Version{2, 38, 9}).Less(MinVersion) || (Version{2, 39, 0}).Less(MinVersion) || (Version{3, 0, 0}).Less(MinVersion) {
		t.Fatal("2.39.0 is the floor")
	}
}

func TestSanitizedEnvDropsWhatRedirectsGit(t *testing.T) {
	base := map[string]string{
		"GIT_DIR": "/elsewhere", "GIT_WORK_TREE": "/w", "GIT_INDEX_FILE": "/i", "GIT_CONFIG_PARAMETERS": "'a=b'",
		"GIT_CONFIG_COUNT": "1", "GIT_CONFIG_KEY_0": "k", "GIT_CONFIG_VALUE_0": "v", "LANGUAGE": "ja",
		"HOME": "/home/u", "PATH": "/bin", "GIT_SSH_COMMAND": "ssh -i k", "LC_ALL": "ja_JP.UTF-8",
	}
	env := strings.Join(sanitizedEnv(base, false, map[string]string{"GIT_AUTHOR_DATE": "1 +0000"}), "\n")
	for _, gone := range []string{"GIT_DIR=", "GIT_WORK_TREE=", "GIT_INDEX_FILE=", "GIT_CONFIG_PARAMETERS=", "GIT_CONFIG_COUNT=", "GIT_CONFIG_KEY_0=", "GIT_CONFIG_VALUE_0=", "LANGUAGE=", "GIT_OPTIONAL_LOCKS="} {
		if strings.Contains(env, gone) {
			t.Errorf("%s survived", gone)
		}
	}
	for _, kept := range []string{"HOME=/home/u", "PATH=/bin", "GIT_SSH_COMMAND=ssh -i k", "LC_ALL=C", "GIT_TERMINAL_PROMPT=0", "GIT_AUTHOR_DATE=1 +0000"} {
		if !strings.Contains(env, kept) {
			t.Errorf("%s is missing", kept)
		}
	}
	if !strings.Contains(strings.Join(sanitizedEnv(base, true, nil), "\n"), "GIT_OPTIONAL_LOCKS=0") {
		t.Error("a read-only run takes no optional locks")
	}
}

func fakeGit(t *testing.T, script string) map[string]string {
	t.Helper()
	dir := t.TempDir()
	if err := os.WriteFile(filepath.Join(dir, "git"), []byte("#!/bin/sh\n"+script+"\n"), 0o755); err != nil {
		t.Fatal(err)
	}
	return map[string]string{"PATH": dir}
}

func TestNewSaysWhyGitIsUnusable(t *testing.T) {
	_, err := New(Options{Env: map[string]string{"PATH": t.TempDir()}})
	if err == nil || err.Error() != "noda needs git 2.39 or later on PATH: git was not found" {
		t.Errorf("missing git: %v", err)
	}
	_, err = New(Options{Env: fakeGit(t, "echo git version 2.38.0")})
	if err == nil || err.Error() != "noda needs git 2.39 or later, found 2.38.0" {
		t.Errorf("old git: %v", err)
	}
	_, err = New(Options{Env: fakeGit(t, "echo hello")})
	if err == nil || !strings.Contains(err.Error(), "cannot read a version") {
		t.Errorf("odd git: %v", err)
	}
	git, err := New(Options{Env: fakeGit(t, "echo git version 2.39.5 '(Apple Git-154)'")})
	if err != nil || git.Version != (Version{2, 39, 5}) {
		t.Errorf("exactly the floor: %v %v", git, err)
	}
}

func TestErrorIsTheLastLineGitPrintedWithoutASecret(t *testing.T) {
	err := &Error{Args: []string{"fetch"}, Code: 128, Stderr: "remote: hint\nfatal: unable to access 'https://tok:en@example.com/a.git/': 403\n"}
	if got := err.Error(); got != "unable to access 'https://***@example.com/a.git/': 403" {
		t.Error(got)
	}
	err = &Error{Args: []string{"rev-parse", "--verify"}, Code: 1}
	if got := err.Error(); got != "git rev-parse exited with code 1" {
		t.Error(got)
	}
	err = &Error{Code: 2, Stderr: "error: something broke\n\n"}
	if got := err.Error(); got != "something broke" {
		t.Error(got)
	}
}

func TestIsExitLooksThroughWrapping(t *testing.T) {
	err := &Error{Code: 1}
	if !IsExit(err, 1) || IsExit(err, 2) || IsExit(nil, 1) {
		t.Fatal("IsExit compares the code")
	}
}
