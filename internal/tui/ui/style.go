// Package ui is the part of ratatui the TUI uses, drawn into a Buffer that a
// test can read and flush.go can send to a terminal. It has no widget the TUI
// does not draw with.
package ui

import "strings"

// Color is a palette slot, never RGB, so the terminal's own theme decides what
// each one looks like. Unset leaves a cell's colour alone; Reset puts back the
// terminal's default.
type Color uint8

const (
	Unset Color = iota
	Reset
	Black
	Red
	Green
	Yellow
	Blue
	Magenta
	Cyan
	Gray
	DarkGray
	LightRed
	LightGreen
	LightYellow
	LightBlue
	LightMagenta
	LightCyan
	White
)

var colorNames = [...]string{
	Unset: "", Reset: "Reset", Black: "Black", Red: "Red", Green: "Green",
	Yellow: "Yellow", Blue: "Blue", Magenta: "Magenta", Cyan: "Cyan",
	Gray: "Gray", DarkGray: "DarkGray", LightRed: "LightRed",
	LightGreen: "LightGreen", LightYellow: "LightYellow", LightBlue: "LightBlue",
	LightMagenta: "LightMagenta", LightCyan: "LightCyan", White: "White",
}

func (c Color) String() string { return colorNames[c] }

// Modifier is a set of text effects.
type Modifier uint8

const (
	Bold Modifier = 1 << iota
	Dim
	Reversed
)

// String names the set the way bitflags prints it, which is what the golden
// frames record.
func (m Modifier) String() string {
	var names []string
	for _, effect := range []struct {
		bit  Modifier
		name string
	}{{Bold, "BOLD"}, {Dim, "DIM"}, {Reversed, "REVERSED"}} {
		if m&effect.bit != 0 {
			names = append(names, effect.name)
		}
	}
	return strings.Join(names, " | ")
}

// Style is a change to apply to a cell: what it leaves unset, a cell keeps.
type Style struct {
	Fg, Bg Color
	Add    Modifier
	Sub    Modifier
}

// Patch is the style with other laid over it.
func (s Style) Patch(other Style) Style {
	if other.Fg != Unset {
		s.Fg = other.Fg
	}
	if other.Bg != Unset {
		s.Bg = other.Bg
	}
	s.Add = (s.Add | other.Add) &^ other.Sub
	s.Sub = (s.Sub | other.Sub) &^ other.Add
	return s
}

// WithAdd is the style with effects added.
func (s Style) WithAdd(m Modifier) Style { return s.Patch(Style{Add: m}) }
