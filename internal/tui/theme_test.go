package tui

import (
	"testing"

	"github.com/henry40408/noda/internal/style"
	"github.com/henry40408/noda/internal/tui/ui"
)

func TestAnIDIsTheSameYellowInBothWriters(t *testing.T) {
	if theme(style.ID).Fg != ui.Yellow {
		t.Fatalf("id: %v", theme(style.ID))
	}
	if theme(style.Tags).Fg != ui.Cyan {
		t.Fatalf("tags: %v", theme(style.Tags))
	}
}

func TestEffectsSurviveTheCrossing(t *testing.T) {
	if theme(style.Muted).Add&ui.Dim == 0 {
		t.Fatal("muted lost dim")
	}
	if theme(style.Match).Add&ui.Bold == 0 {
		t.Fatal("match lost bold")
	}
	slug := theme(style.Slug)
	if slug.Fg != ui.Yellow || slug.Add&ui.Dim == 0 {
		t.Fatalf("slug: %v", slug)
	}
}

// A terminal that ignores dim would draw the whole column in one cyan.
func TestTagPunctuationStepsBackWithoutLeaningOnDim(t *testing.T) {
	punct := theme(style.TagsPunct)
	if punct.Fg != ui.DarkGray || punct.Add != 0 {
		t.Fatalf("punctuation: %v", punct)
	}
	if punct.Fg == theme(style.Tags).Fg {
		t.Fatal("punctuation and tags share a colour")
	}
}
