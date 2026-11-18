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

// A bare tag moves under the build, past the dependency cooldown.
func TestEveryBaseImageIsPinnedByVersionAndDigest(t *testing.T) {
	froms := regexp.MustCompile(`(?m)^FROM .*$`).FindAllString(readDockerfile(t), -1)
	if len(froms) == 0 {
		t.Fatal("no FROM line")
	}
	pinned := regexp.MustCompile(`\S+:\d\S*@sha256:[0-9a-f]{64}|\S+:[a-z]\S*@sha256:[0-9a-f]{64}`)
	golang := regexp.MustCompile(`golang:1\.\d+\.\d+-bookworm@sha256:`)
	for _, from := range froms {
		if !pinned.MatchString(from) {
			t.Errorf("not pinned by tag and digest: %s", from)
		}
		if strings.Contains(from, "golang:") && !golang.MatchString(from) {
			t.Errorf("the Go image needs a full version: %s", from)
		}
	}
}
