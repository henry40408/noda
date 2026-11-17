package main

import (
	"bytes"
	"strings"
	"testing"
)

func TestVersionFlagsPrintTheVersion(t *testing.T) {
	for _, flag := range []string{"--version", "-V"} {
		var stdout, stderr bytes.Buffer
		if code := run([]string{flag}, &stdout, &stderr); code != 0 {
			t.Fatalf("%s: exit %d", flag, code)
		}
		if !strings.HasPrefix(stdout.String(), "noda ") || !strings.HasSuffix(stdout.String(), "\n") {
			t.Fatalf("%s: stdout %q", flag, stdout.String())
		}
	}
}

func TestAnythingElseFails(t *testing.T) {
	var stdout, stderr bytes.Buffer
	if code := run([]string{"ls"}, &stdout, &stderr); code != 1 {
		t.Fatalf("exit %d", code)
	}
	if stdout.Len() != 0 || stderr.Len() == 0 {
		t.Fatalf("stdout %q stderr %q", stdout.String(), stderr.String())
	}
}
