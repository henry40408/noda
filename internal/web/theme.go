package web

import (
	"strings"

	"github.com/henry40408/noda/internal/style"
)

// terminal holds only the slots noda's palette uses. A colour added to style
// and forgotten here falls through fill to the default foreground.
type terminal struct {
	text      string
	dim       string
	yellow    string
	yellowDim string
	cyan      string
	grey      string
	red       string
	magenta   string
	// Not a slot: a terminal never tells the program its background.
	background string
	sunk       string
	rule       string
	press      string
	mark       string
}

var lightTheme = terminal{
	text: "#17191d", dim: "#6b7280",
	// Unreadable on white, so round it to amber as light terminal themes do.
	yellow: "#8a6100", yellowDim: "#a98741",
	cyan: "#0e6f7a", grey: "#8b929c", red: "#b3261e", magenta: "#a626a4",
	background: "#ffffff", sunk: "#f4f5f7", rule: "#e3e5e9", press: "#ebedf0", mark: "#f3d98a",
}

var darkTheme = terminal{
	text: "#d8dce2", dim: "#868e9a",
	yellow: "#e0ac4d", yellowDim: "#9c7c3c",
	cyan: "#56b6c2", grey: "#6b7280", red: "#e06c75", magenta: "#c678dd",
	// Not black: under white text, pure black smears as the eye moves.
	background: "#14161a", sunk: "#1b1e24", rule: "#272b32", press: "#242830", mark: "#5a4a1e",
}

// fill is a style as this theme draws it. Only foreground and dim cross over;
// bold is left to the markup, so it is said in one place.
func (t terminal) fill(s style.Style) string {
	switch {
	case s.Fg == style.Yellow && s.Dim:
		return t.yellowDim
	case s.Fg == style.Yellow:
		return t.yellow
	case s.Fg == style.Cyan:
		return t.cyan
	case s.Fg == style.BrightBlack:
		return t.grey
	case s.Fg == style.Red:
		return t.red
	case s.Fg == style.Magenta:
		return t.magenta
	case s.Fg == style.NoColor && s.Dim:
		return t.dim
	}
	return t.text
}

// properties are named after style's variables (--tag, not --cyan), so a
// property is not reused for anything that merely wants its colour.
func (t terminal) properties() string {
	var css strings.Builder
	for _, property := range [][2]string{
		{"--bg", t.background},
		{"--bg-sunk", t.sunk},
		{"--press", t.press},
		{"--rule", t.rule},
		{"--mark", t.mark},
		{"--text", t.fill(style.Style{})},
		{"--muted", t.fill(style.Muted)},
		{"--id", t.fill(style.ID)},
		{"--id-dim", t.fill(style.Slug)},
		{"--tag", t.fill(style.Tags)},
		{"--punct", t.fill(style.TagsPunct)},
		{"--alert", t.fill(style.Invalid)},
		// Its own property though it is --alert's red: it marks meaning.
		{"--overdue", t.fill(style.Overdue)},
		{"--pin", t.fill(style.Pin)},
	} {
		css.WriteString(property[0] + ":" + property[1] + ";")
	}
	return css.String()
}

// themeStylesheet is light on :root and dark inside the media query. No toggle
// and no stored preference: the reader already told their device.
func themeStylesheet() string {
	return ":root{color-scheme:light;" + lightTheme.properties() +
		"}@media (prefers-color-scheme:dark){:root{color-scheme:dark;" + darkTheme.properties() + "}}"
}
