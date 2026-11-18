package style

import (
	"slices"
	"strings"
	"testing"
)

func TestATagListIsCutBetweenTheTagsAndThePunctuation(t *testing.T) {
	if got := TagPieces(nil); got != nil {
		t.Fatalf("no tags gave %v", got)
	}
	got := TagPieces([]string{"work", "q3"})
	want := []Piece{
		{TagsPunct, "["}, {Tags, "work"}, {TagsPunct, ", "}, {Tags, "q3"}, {TagsPunct, "]"},
	}
	if !slices.Equal(got, want) {
		t.Fatalf("got %v", got)
	}
	var plain strings.Builder
	for _, piece := range got {
		plain.WriteString(piece.Text)
	}
	if plain.String() != "[work, q3]" {
		t.Fatalf("plain %q", plain.String())
	}
}

func TestPaintWritesEffectsBeforeTheColourAndResets(t *testing.T) {
	cases := []struct {
		style Style
		want  string
	}{
		{Added, "\x1b[32mx\x1b[0m"},
		{Slug, "\x1b[2m\x1b[33mx\x1b[0m"},
		{Match, "\x1b[1m\x1b[33mx\x1b[0m"},
		{Column, "\x1b[1m\x1b[90mx\x1b[0m"},
		{Muted, "\x1b[2mx\x1b[0m"},
		{Heading, "\x1b[1mx\x1b[0m"},
		{Style{}, "x"},
	}
	for _, c := range cases {
		if got := c.style.Paint("x"); got != c.want {
			t.Errorf("%+v: got %q, want %q", c.style, got, c.want)
		}
	}
}

func TestTagListPaintsEachPiece(t *testing.T) {
	got := TagList([]string{"a"})
	want := "\x1b[90m[\x1b[0m\x1b[36ma\x1b[0m\x1b[90m]\x1b[0m"
	if got != want {
		t.Fatalf("got %q", got)
	}
	if TagList(nil) != "" {
		t.Fatal("no tags must print nothing")
	}
}

func env(pairs ...string) func(string) (string, bool) {
	m := map[string]string{}
	for i := 0; i < len(pairs); i += 2 {
		m[pairs[i]] = pairs[i+1]
	}
	return func(key string) (string, bool) {
		v, ok := m[key]
		return v, ok
	}
}

func TestWantedFollowsAnstream(t *testing.T) {
	cases := []struct {
		name     string
		terminal bool
		env      []string
		want     bool
	}{
		{"pipe", false, []string{"TERM", "xterm"}, false},
		{"terminal", true, []string{"TERM", "xterm"}, true},
		{"dumb terminal", true, []string{"TERM", "dumb"}, false},
		{"no TERM", true, nil, false},
		{"NO_COLOR beats force", true, []string{"NO_COLOR", "1", "CLICOLOR_FORCE", "1"}, false},
		{"empty NO_COLOR is unset", true, []string{"NO_COLOR", "", "TERM", "xterm"}, true},
		{"force on a pipe", false, []string{"CLICOLOR_FORCE", "1"}, true},
		{"empty force is unset", false, []string{"CLICOLOR_FORCE", ""}, false},
		{"CLICOLOR=0", true, []string{"CLICOLOR", "0", "TERM", "xterm"}, false},
		{"CLICOLOR=1 without TERM", true, []string{"CLICOLOR", "1"}, true},
		{"CI without TERM", true, []string{"CI", "true"}, true},
		{"CI on a pipe", false, []string{"CI", "true"}, false},
	}
	for _, c := range cases {
		if got := Wanted(c.terminal, env(c.env...)); got != c.want {
			t.Errorf("%s: got %v", c.name, got)
		}
	}
}

func TestStripRemovesSequencesAndKeepsText(t *testing.T) {
	cases := map[string]string{
		"plain":                        "plain",
		Added.Paint("a") + " b":        "a b",
		"\x1b[1m\x1b[33mhi\x1b[0m\x00": "hi\x00",
		"x\x1b[38;5;196my":             "xy",
		"tail\x1b[3":                   "tail",
		"é\x1b[0m日本":                   "é日本",
	}
	for in, want := range cases {
		if got := Strip(in); got != want {
			t.Errorf("%q: got %q, want %q", in, got, want)
		}
	}
}
