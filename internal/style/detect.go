package style

import "strings"

// Wanted decides whether colour goes to a stream, with the rules anstream uses:
// NO_COLOR (non-empty) wins, then CLICOLOR_FORCE (non-empty), then CLICOLOR=0.
// Otherwise only a terminal gets colour, and only when TERM is usable, CLICOLOR
// is set, or the run is in CI.
func Wanted(isTerminal bool, getenv func(string) (string, bool)) bool {
	if value, _ := getenv("NO_COLOR"); value != "" {
		return false
	}
	if value, _ := getenv("CLICOLOR_FORCE"); value != "" {
		return true
	}
	clicolor, clicolorSet := getenv("CLICOLOR")
	if clicolorSet && clicolor == "0" {
		return false
	}
	if !isTerminal {
		return false
	}
	term, termSet := getenv("TERM")
	_, inCI := getenv("CI")
	return (termSet && term != "dumb") || clicolorSet || inCI
}

// Strip removes the escape sequences Paint writes (and any other CSI sequence),
// for a stream where colour is not wanted.
func Strip(text string) string {
	if !strings.Contains(text, "\x1b[") {
		return text
	}
	var b strings.Builder
	for i := 0; i < len(text); {
		if text[i] == '\x1b' && i+1 < len(text) && text[i+1] == '[' {
			i = endOfSequence(text, i+2)
			continue
		}
		b.WriteByte(text[i])
		i++
	}
	return b.String()
}

// endOfSequence returns the index after the final byte (0x40-0x7e) of a CSI
// sequence whose parameters start at from. An unterminated sequence runs to the
// end of the text.
func endOfSequence(text string, from int) int {
	for i := from; i < len(text); i++ {
		if text[i] >= 0x40 && text[i] <= 0x7e {
			return i + 1
		}
	}
	return len(text)
}
