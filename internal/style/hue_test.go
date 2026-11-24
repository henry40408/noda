package style

import (
	"strconv"
	"testing"
)

// The pinned values are what the page has always drawn: changing the function
// recolours every note anybody has, so these must fail loudly.
func TestHueIsPinnedForKnownIdentities(t *testing.T) {
	for _, c := range []struct {
		identity string
		want     int
	}{
		{"01abc234", 10},
		{"02def567", 5},
		{"04kmn123", 0},
		{"a2b3c4d5", 4},
		{"zzzzzzzz", 0},
		{"default", 5},
		{"work", 7},
		{"a", 1},
		{"", 0},
		{"日本語", 4},
		{"個人筆記", 4},
	} {
		if got := Hue(c.identity); got != c.want {
			t.Errorf("Hue(%q) = %d, want %d", c.identity, got, c.want)
		}
	}
}

func TestHueStaysWithinTheTwelveAndUsesAllOfThem(t *testing.T) {
	seen := map[int]bool{}
	for n := range 2000 {
		got := Hue("id" + strconv.Itoa(n))
		if got < 0 || got >= Hues {
			t.Fatalf("Hue gave %d, outside 0..%d", got, Hues-1)
		}
		seen[got] = true
	}
	if len(seen) != Hues {
		t.Errorf("2000 ids reached %d of the %d colours", len(seen), Hues)
	}
}
