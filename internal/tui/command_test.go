package tui

import (
	"slices"
	"testing"
)

func TestEverySpellingIsItsOwn(t *testing.T) {
	seen := map[string]bool{}
	for _, spec := range Commands {
		for _, spelling := range append([]string{spec.Name}, spec.Aliases...) {
			if seen[spelling] {
				t.Fatalf("%q is spelled twice", spelling)
			}
			seen[spelling] = true
		}
	}
}

func TestACommandIsFoundByItsNameOrBySpellingOfIt(t *testing.T) {
	for _, spelling := range []string{"open", "o", "show"} {
		if spec, ok := FindCommand(spelling); !ok || spec.Name != "open" {
			t.Fatalf("%q found %v", spelling, spec.Name)
		}
	}
	for _, missing := range []string{"", "pu", "stat"} {
		if _, ok := FindCommand(missing); ok {
			t.Fatalf("%q should find nothing", missing)
		}
	}
}

func names(specs []Spec) []string {
	var out []string
	for _, spec := range specs {
		out = append(out, spec.Name)
	}
	return out
}

func TestTheListIsSearchedByWhatACommandDoesAsWellAsByItsName(t *testing.T) {
	if got := names(MatchingCommands("tag")); !slices.Equal(got, []string{"tags", "tag"}) {
		t.Fatalf("got %v", got)
	}
	described := names(MatchingCommands("remote"))
	if !slices.Contains(described, "push") || !slices.Contains(described, "pull") {
		t.Fatalf("got %v", described)
	}
	if len(MatchingCommands("")) != len(Commands) || len(MatchingCommands("zzz")) != 0 {
		t.Fatal("empty matches all, junk none")
	}
}

func TestWhatACommandTakesReadsAsOneLine(t *testing.T) {
	open, _ := FindCommand("open")
	quit, _ := FindCommand("quit")
	if open.Usage() != "open <note>" || quit.Usage() != "quit" {
		t.Fatalf("%q %q", open.Usage(), quit.Usage())
	}
	if len(Commands) != 30 {
		t.Fatalf("%d commands", len(Commands))
	}
}
