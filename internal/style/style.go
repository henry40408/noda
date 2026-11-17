// Package style is the palette. Colour marks what a line is, never what it
// means, and never a note's own text.
//
// Whether colour is wanted is not decided where text is built: commands always
// emit escapes, and the output path calls Wanted and Strip, so a piped
// `noda show` writes exactly the bytes on disk.
package style

import (
	"strconv"
	"strings"
)

// Color is one of the ANSI foreground colours, or none. The values are the SGR
// parameters.
type Color uint8

const (
	NoColor     Color = 0
	Red         Color = 31
	Green       Color = 32
	Yellow      Color = 33
	Magenta     Color = 35
	Cyan        Color = 36
	BrightBlack Color = 90
)

const (
	sgrBold  = "\x1b[1m"
	sgrDim   = "\x1b[2m"
	sgrReset = "\x1b[0m"
)

// Style is a foreground colour with optional bold and dim.
type Style struct {
	Fg   Color
	Bold bool
	Dim  bool
}

var (
	Commit = Style{Fg: Yellow}
	// ID is Commit's yellow: both are the short string you copy into the next
	// command.
	ID = Commit
	// Slug is ID a step down: side by side they are the filename
	// `<id>-<slug>.md`.
	Slug = Style{Fg: Yellow, Dim: true}
	Tags = Style{Fg: Cyan}
	// TagsPunct is the `[`, `,` and `]` around and between the tags. It is grey,
	// not Tags dimmed: a terminal may ignore `dim`, and interleaved with the
	// tags the difference would vanish.
	TagsPunct = Style{Fg: BrightBlack}
	// Muted is for timestamps and other supporting detail.
	Muted   = Style{Dim: true}
	Added   = Style{Fg: Green}
	Removed = Style{Fg: Red}
	// Hunk is for `@@` hunk headers.
	Hunk = Style{Fg: Cyan}
	// Heading is for file headers in a diff.
	Heading = Style{Bold: true}
	// Match is the part of a search result that matched.
	Match = Style{Fg: Yellow, Bold: true}
	// Invalid is why what has been typed is not a query yet, in the TUI's
	// search line (the CLI just fails on a bad query).
	Invalid = Style{Fg: Red}
	// Overdue is a due date that has passed, in `todo`. An exception to "colour
	// never means anything": it is what anybody scans a todo list for.
	Overdue = Style{Fg: Red}
	// Column is the names along the top of a table, in the TUI only. Grey steps
	// back by hue, as TagsPunct does; bold keeps it from reading as data.
	Column = Style{Fg: BrightBlack, Bold: true}
	// Pin is the mark on a pinned row, seen from across the listing.
	Pin = Style{Fg: Magenta}
	// Cursor is the bar down the left of the row the cursor is on, in the TUI.
	// The row is only emboldened: reverse video would invert the columns'
	// colours on the one row being looked at.
	Cursor = Style{Fg: Yellow}
)

// PinMark is what a pin says, shared by `ls`, the TUI and the web page. It is a
// word, not an emoji, whose width depends on the terminal's font and would
// misalign the row.
const PinMark = "pinned"

// Paint wraps text in the style's escape sequences. Effects come before the
// colour, as anstyle writes them, so output stays byte-identical to the Rust
// version. A style with nothing set leaves the text alone.
func (s Style) Paint(text string) string {
	var open strings.Builder
	if s.Bold {
		open.WriteString(sgrBold)
	}
	if s.Dim {
		open.WriteString(sgrDim)
	}
	if s.Fg != NoColor {
		open.WriteString("\x1b[" + strconv.Itoa(int(s.Fg)) + "m")
	}
	if open.Len() == 0 {
		return text
	}
	return open.String() + text + sgrReset
}

// Piece is a run of text in one style.
type Piece struct {
	Style Style
	Text  string
}

// TagPieces cuts a tag list into differently coloured pieces, so `ls` (escapes)
// and the TUI (spans) cut it the same way. No tags give no pieces, not `[]`.
func TagPieces(tags []string) []Piece {
	if len(tags) == 0 {
		return nil
	}
	pieces := []Piece{{TagsPunct, "["}}
	for i, tag := range tags {
		if i > 0 {
			pieces = append(pieces, Piece{TagsPunct, ", "})
		}
		pieces = append(pieces, Piece{Tags, tag})
	}
	return append(pieces, Piece{TagsPunct, "]"})
}

// TagList is the pieces painted and joined: the tag list as a listing writes
// it.
func TagList(tags []string) string {
	var b strings.Builder
	for _, piece := range TagPieces(tags) {
		b.WriteString(piece.Style.Paint(piece.Text))
	}
	return b.String()
}
