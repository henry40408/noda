package tui

import (
	"github.com/henry40408/noda/internal/style"
	"github.com/henry40408/noda/internal/tui/ui"
)

// theme translates the palette's decisions for the screen; no colour is chosen
// here. Only foreground, bold and dim cross over: a background or underline
// added in the style package would silently do nothing.
func theme(s style.Style) ui.Style {
	out := ui.Style{Fg: paletteSlot(s.Fg)}
	if s.Bold {
		out.Add |= ui.Bold
	}
	if s.Dim {
		out.Add |= ui.Dim
	}
	return out
}

// paletteSlot is a slot, not RGB, so the terminal's theme still decides the
// colours.
func paletteSlot(colour style.Color) ui.Color {
	switch colour {
	case style.Red:
		return ui.Red
	case style.Green:
		return ui.Green
	case style.Yellow:
		return ui.Yellow
	case style.Magenta:
		return ui.Magenta
	case style.Cyan:
		return ui.Cyan
	case style.BrightBlack:
		return ui.DarkGray
	}
	return ui.Unset
}

var bold = ui.Style{Add: ui.Bold}

var reversed = ui.Style{Add: ui.Reversed}
