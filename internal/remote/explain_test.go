package remote

import (
	"strings"
	"testing"
)

func TestExplainAddsTheWayOutOfAFailedLogin(t *testing.T) {
	cases := map[string]struct{ stderr, url, hint string }{
		"ssh": {
			"git@example.com: Permission denied (publickey).\nfatal: Could not read from remote repository.\n\nPlease make sure you have the correct access rights\n",
			"git@example.com:team/notes.git", "ssh-add -l",
		},
		"host key": {"Host key verification failed.\nfatal: Could not read from remote repository.\n", "ssh://example.com/x", "ssh-add -l"},
		"https": {
			"fatal: could not read Username for 'https://example.com': terminal prompts disabled\n",
			"https://example.com/x.git", "git credential approve",
		},
		"rejected token": {
			"remote: Invalid username or password.\nfatal: Authentication failed for 'https://tok@example.com/x.git/'\n",
			"https://tok@example.com/x.git", "git credential approve",
		},
	}
	for name, c := range cases {
		message, ok := Explain(c.stderr, c.url)
		if !ok || !strings.Contains(message, c.hint) {
			t.Errorf("%s: %q, %v", name, message, ok)
		}
		if strings.Contains(message, "tok@") {
			t.Errorf("%s: the token is shown: %q", name, message)
		}
	}
}

func TestExplainLeavesOtherFailuresWithoutAHint(t *testing.T) {
	message, ok := Explain("fatal: unable to access 'https://tok@host/x/': Could not resolve host: host\n", "https://tok@host/x")
	if !ok || strings.Contains(message, "\n") || strings.Contains(message, "tok@") {
		t.Fatalf("%q, %v", message, ok)
	}
	if message, ok := Explain("", "x"); ok {
		t.Fatalf("%q", message)
	}
}

func TestNameFromURL(t *testing.T) {
	for url, want := range map[string]string{
		"https://example.com/team/notes.git": "notes",
		"git@example.com:team/notes.git/":    "notes",
		"git@example.com:notes":              "notes",
		"/srv/notes.git.git":                 "notes",
		"https://example.com/":               "example.com",
		"":                                   "",
	} {
		if got := NameFromURL(url); got != want {
			t.Errorf("NameFromURL(%q) = %q, want %q", url, got, want)
		}
	}
}

func TestNameFromURLOfAnAbsolutePath(t *testing.T) {
	if got := NameFromURL("/notes.git"); got != "notes" {
		t.Errorf("got %q", got)
	}
}
