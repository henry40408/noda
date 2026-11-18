package cmd

import (
	"strings"

	"github.com/mattn/go-runewidth"

	"github.com/henry40408/noda/internal/style"
)

// ambiguousAsNarrow counts East Asian ambiguous characters as one cell, as
// unicode-width does. The package default would follow the locale, so the same
// table would align differently from one terminal to the next.
var ambiguousAsNarrow = runewidth.Condition{EastAsianWidth: false}

const (
	variationSelector16 = '\uFE0F'
	zeroWidthJoiner     = '\u200D'
)

// DisplayWidth is the terminal cells text occupies, so a CJK or emoji slug does
// not misalign a table. It follows unicode-width, which the Rust tables were
// aligned with, where runewidth differs: a control character takes a cell, and
// U+FE0F widens a lone narrow character to emoji presentation.
func DisplayWidth(text string) int {
	runes := []rune(text)
	var printable strings.Builder
	cells := 0
	for i, r := range runes {
		switch {
		case isControl(r):
			cells++
		case r == variationSelector16 && widensToEmoji(runes, i):
			cells++
			printable.WriteRune(r)
		default:
			printable.WriteRune(r)
		}
	}
	return cells + ambiguousAsNarrow.StringWidth(printable.String())
}

func isControl(r rune) bool {
	return r < 0x20 || (r >= 0x7f && r < 0xa0)
}

// widensToEmoji is whether the U+FE0F at runes[i] turns a character that is one
// cell in text presentation into a two-cell emoji. Inside a joined sequence the
// whole cluster is already measured as one emoji, so nothing is added there.
func widensToEmoji(runes []rune, i int) bool {
	if i == 0 {
		return false
	}
	previous := runes[i-1]
	joined := i+1 < len(runes) && runes[i+1] == zeroWidthJoiner
	if joined || previous == zeroWidthJoiner {
		return false
	}
	isKeycapBase := previous == '#' || previous == '*' || (previous >= '0' && previous <= '9')
	if previous < 0x80 && !isKeycapBase {
		return false
	}
	return ambiguousAsNarrow.RuneWidth(previous) == 1
}

func pad(text string, cells int) string {
	return text + strings.Repeat(" ", max(0, cells-DisplayWidth(text)))
}

// column pads outside the escape sequences, so the width ignores them and the
// caller's trailing trim can reach the spaces.
func column(s style.Style, text string, cells int) string {
	return s.Paint(text) + strings.Repeat(" ", max(0, cells-DisplayWidth(text)))
}
