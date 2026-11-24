package web

import (
	"strconv"
	"strings"

	"github.com/henry40408/noda/internal/style"
)

// terminal holds the slots the web palette needs. The three inks (text, dim,
// red) are looked up through fill, so a style that changes there follows here;
// the rest are surfaces, which a terminal never tells the program, so they are
// this file's to choose.
type terminal struct {
	text string
	dim  string
	red  string
	// Surfaces: the desk behind the page, the sheet the text sits on, the
	// sunken wells, the pressed state and the rules between things.
	desk  string
	sheet string
	sunk  string
	press string
	rule  string
	// A search match, the focus ring, and what a filled button wears.
	mark    string
	focus   string
	goInk   string
	redWash string
	// lightness is the L of every note colour: one list of hues serves both
	// themes, and the theme only decides how bright they are.
	lightness string
}

var lightTheme = terminal{
	text: "#161a22", dim: "#5b6473", red: "#b42318",
	desk: "#eef0f3", sheet: "#ffffff", sunk: "#f3f5f8", press: "#e3e7ed", rule: "#dde1e8",
	mark: "#ffe08a", focus: "#2f5bea", goInk: "#ffffff", redWash: "#fdecea",
	lightness: ".6",
}

var darkTheme = terminal{
	text: "#e4e7ec", dim: "#9aa3b2", red: "#ff8a80",
	desk: "#0d0f13", sheet: "#16191f", sunk: "#1e222a", press: "#272c36", rule: "#2b303a",
	mark: "#6b5310", focus: "#8aa7ff", goInk: "#12151a", redWash: "#3a1917",
	lightness: ".74",
}

// fill is a style as this theme draws it. Only foreground and dim cross over;
// bold is left to the markup, so it is said in one place.
func (t terminal) fill(s style.Style) string {
	if s.Fg == style.Red {
		return t.red
	}
	if s.Fg == style.NoColor && s.Dim {
		return t.dim
	}
	return t.text
}

// properties are the page's variables, named for what they are on a page (ink,
// sheet) rather than for a terminal colour.
func (t terminal) properties() string {
	var css strings.Builder
	for _, property := range [][2]string{
		{"--desk", t.desk},
		{"--sheet", t.sheet},
		{"--sunk", t.sunk},
		{"--press", t.press},
		{"--line", t.rule},
		{"--ink", t.fill(style.Style{})},
		{"--soft", t.fill(style.Muted)},
		{"--mark", t.mark},
		{"--focus", t.focus},
		// A filled button is the text colour with the sheet's colour on it.
		{"--go", t.fill(style.Style{})},
		{"--go-ink", t.goInk},
		// Red marks what failed or destroys, and nothing else.
		{"--bad", t.fill(style.Invalid)},
		{"--bad-wash", t.redWash},
		{"--sl", t.lightness},
	} {
		css.WriteString(property[0] + ":" + property[1] + ";")
	}
	return css.String()
}

// hueAngles is each note colour's chroma and angle in OKLCH. style.Hue picks the
// index. Fewer than style.Hues entries still compiles, so a colour added there
// and forgotten here is caught by asset_test, not by the build.
var hueAngles = [style.Hues][2]string{
	{".15", "20"}, {".15", "50"}, {".14", "80"}, {".15", "125"},
	{".14", "155"}, {".12", "185"}, {".12", "215"}, {".15", "245"},
	{".17", "270"}, {".18", "300"}, {".18", "330"}, {".17", "355"},
}

// hueClass is the class a name wears for its colour.
func hueClass(name string) string { return "h" + strconv.Itoa(style.Hue(name)) }

// hueRules is one rule a colour. The lightness is a variable, so the same rules
// serve both themes.
func hueRules() string {
	var css strings.Builder
	for index, angle := range hueAngles {
		css.WriteString(".h" + strconv.Itoa(index) + "{--s:oklch(var(--sl) " + angle[0] + " " + angle[1] + ")}")
	}
	return css.String()
}

// themeStylesheet is light on :root and dark inside the media query. No toggle
// and no stored preference: the reader already told their device.
func themeStylesheet() string {
	return ":root{color-scheme:light;" + lightTheme.properties() +
		"}@media (prefers-color-scheme:dark){:root{color-scheme:dark;" + darkTheme.properties() + "}}" +
		hueRules()
}
