package main

import (
	"os"
	"regexp"
	"strings"
	"testing"
)

func readDockerfile(t *testing.T) string {
	t.Helper()
	text, err := os.ReadFile("../../Dockerfile")
	if err != nil {
		t.Fatal(err)
	}
	return string(text)
}

// Without an init process, a grandchild of git (ssh, a credential helper) that
// is re-parented to PID 1 is never reaped.
func TestTheImageRunsNodaUnderAnInitProcess(t *testing.T) {
	dockerfile := readDockerfile(t)
	if !strings.Contains(dockerfile, `ENTRYPOINT ["/usr/bin/tini", "--", "/noda"]`) {
		t.Error("ENTRYPOINT does not run noda under tini")
	}
	if !regexp.MustCompile(`apt-get install[^\n]*\btini\b`).MatchString(strings.ReplaceAll(dockerfile, "\\\n", " ")) {
		t.Error("tini is not installed in the runtime stage")
	}
}

// A bare `golang` or `debian` tag resolves to trixie, a silent Debian major
// bump, and the runtime stage must stay on bookworm's git 2.39.
func TestEveryBaseImageStaysOnBookworm(t *testing.T) {
	froms := regexp.MustCompile(`(?m)^FROM .*$`).FindAllString(readDockerfile(t), -1)
	if len(froms) == 0 {
		t.Fatal("no FROM line")
	}
	for _, from := range froms {
		if !strings.Contains(from, "bookworm") {
			t.Errorf("not on bookworm: %s", from)
		}
	}
}
