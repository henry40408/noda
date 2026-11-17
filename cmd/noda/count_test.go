package main

import (
	"strings"
	"testing"
)

func TestParseCountReadsAUsizeAsRustDid(t *testing.T) {
	for text, want := range map[string]int{
		"0": 0, "5": 5, "+5": 5, "007": 7, "+0": 0,
		"2147483647": maxCount, "2147483648": maxCount, "9223372036854775808": maxCount, "18446744073709551615": maxCount,
	} {
		if got, err := parseCount(text); err != nil || got != want {
			t.Errorf("%q: got %d, %v; want %d", text, got, err, want)
		}
	}
	for text, want := range map[string]string{
		"":                     "cannot parse integer from empty string",
		"18446744073709551616": "number too large to fit in target type",
		"-5":                   "invalid digit found in string",
		"+":                    "invalid digit found in string",
		"-":                    "invalid digit found in string",
		"++5":                  "invalid digit found in string",
		" 5":                   "invalid digit found in string",
		"5x":                   "invalid digit found in string",
		"1.5":                  "invalid digit found in string",
		"1_0":                  "invalid digit found in string",
	} {
		if _, err := parseCount(text); err == nil || err.Error() != want {
			t.Errorf("%q: got %v; want %s", text, err, want)
		}
	}
}

func TestLogCountFlowsThroughTheCommandLine(t *testing.T) {
	s := newSandbox(t)
	s.ok(t, "init")
	s.ok(t, "add", "A", "-c", "x")
	s.ok(t, "add", "B", "-c", "y")
	lines := func(args ...string) int {
		return strings.Count(s.ok(t, args...), "\n")
	}
	// 0 shows one commit, as it did in Rust, not all of them.
	if got := lines("log", "-n", "0"); got != 1 {
		t.Errorf("-n 0 shows %d", got)
	}
	if got := lines("log", "-n", "2"); got != 2 {
		t.Errorf("-n 2 shows %d", got)
	}
	if got := lines("log", "--max-count=+2"); got != 2 {
		t.Errorf("--max-count=+2 shows %d", got)
	}
	for _, text := range []string{"99999999999", "18446744073709551615"} {
		if got := lines("log", "--max-count="+text); got != 3 {
			t.Errorf("--max-count=%s shows %d", text, got)
		}
	}
	got := s.run("log", "-n", "18446744073709551616")
	if got.code != 2 || !strings.Contains(got.stderr, "'18446744073709551616' for '--max-count <COUNT>': number too large to fit in target type") {
		t.Errorf("%+v", got)
	}
	if got := s.run("log", "-n="); got.code != 2 || !strings.Contains(got.stderr, "cannot parse integer from empty string") {
		t.Errorf("%+v", got)
	}
}
