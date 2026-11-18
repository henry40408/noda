package main

import (
	"bytes"
	"strings"
	"testing"

	"github.com/henry40408/noda/internal/cmd"
)

func TestConfirmAcceptsOnlyYesAnswers(t *testing.T) {
	cases := []struct {
		answer string
		want   bool
	}{
		{"y\n", true},
		{"YES\n", true},
		{" yes \n", true},
		{"y", true}, // end of input without a newline still answers
		{"n\n", false},
		{"\n", false},
		{"", false},
		{"yep\n", false},
	}
	for _, c := range cases {
		var asked bytes.Buffer
		p := process{stdin: strings.NewReader(c.answer), stderr: &asked}
		got, err := p.confirm("sure? ")
		if err != nil || got != c.want {
			t.Errorf("answer %q: got %v, %v; want %v", c.answer, got, err, c.want)
		}
		if asked.String() != "sure? " {
			t.Errorf("the question goes to stderr, got %q", asked.String())
		}
	}
}

func TestSortOfMapsEveryKeyAndDefaultsToSlug(t *testing.T) {
	want := map[string]cmd.Sort{
		"created": cmd.SortCreated,
		"updated": cmd.SortUpdated,
		"title":   cmd.SortTitle,
		"slug":    cmd.SortSlug,
		"":        cmd.SortSlug,
	}
	for name, sort := range want {
		if got := sortOf(name); got != sort {
			t.Errorf("sortOf(%q) = %v, want %v", name, got, sort)
		}
	}
}

func TestLsSortsByTitleAndReverses(t *testing.T) {
	s := newSandbox(t)
	s.ok(t, "init")
	s.ok(t, "add", "Banana", "-c", "b\n")
	s.ok(t, "add", "Apple", "-c", "a\n")

	forward := s.ok(t, "ls", "--sort", "title")
	if strings.Index(forward, "Apple") > strings.Index(forward, "Banana") {
		t.Fatalf("by title, Apple comes first:\n%s", forward)
	}
	backward := s.ok(t, "ls", "--sort", "title", "--reverse")
	if strings.Index(backward, "Banana") > strings.Index(backward, "Apple") {
		t.Fatalf("reversed, Banana comes first:\n%s", backward)
	}
}

func TestLsOnlyNotesOrOnlyFilesNarrowsTheListing(t *testing.T) {
	s := newSandbox(t)
	s.ok(t, "init")
	s.ok(t, "add", "A Note", "-c", "x\n")

	notes := s.ok(t, "ls", "--notes-only", "--json")
	files := s.ok(t, "ls", "--files-only", "--json")
	if !strings.Contains(notes, "A Note") {
		t.Errorf("--notes-only lost the note: %s", notes)
	}
	if strings.Contains(files, "A Note") {
		t.Errorf("--files-only listed a note: %s", files)
	}
}

func TestSnapshotListsWhenNamelessAndTakesOneWhenNamed(t *testing.T) {
	s := newSandbox(t)
	s.ok(t, "init")
	s.ok(t, "add", "Kept", "-c", "x\n")

	if got := s.ok(t, "snapshot"); !strings.Contains(got, "no snapshots") {
		t.Fatalf("an empty list says so: %q", got)
	}
	if got := s.ok(t, "snapshot", "before-trip"); !strings.Contains(got, "snapshot: before-trip") {
		t.Fatalf("taking one reports it: %q", got)
	}
	if got := s.ok(t, "snapshot"); !strings.Contains(got, "before-trip") {
		t.Fatalf("the list names it: %q", got)
	}
}

func TestRemovingANotebookAtATerminalAsksFirst(t *testing.T) {
	s := newSandbox(t)
	s.ok(t, "init")
	s.ok(t, "notebook", "add", "scratch")

	remove := func(answer string) result {
		var stdout, stderr bytes.Buffer
		code := run(process{args: []string{"notebook", "rm", "scratch"}, env: s.env, stdin: strings.NewReader(answer),
			stdout: &stdout, stderr: &stderr, stdinIsTerminal: true})
		return result{code, stdout.String(), stderr.String()}
	}

	declined := remove("n\n")
	if !strings.Contains(declined.stderr, "[y/N]") {
		t.Fatalf("the prompt goes to stderr: %q", declined.stderr)
	}
	if !strings.Contains(s.ok(t, "notebook", "ls"), "scratch") {
		t.Fatal("answering no removed the notebook")
	}
	if got := remove("y\n"); got.code != 0 {
		t.Fatalf("answering yes failed: %+v", got)
	}
	if strings.Contains(s.ok(t, "notebook", "ls"), "scratch") {
		t.Fatal("answering yes kept the notebook")
	}
}
