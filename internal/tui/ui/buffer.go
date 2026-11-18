package ui

import (
	"strings"
	"unicode"

	"github.com/rivo/uniseg"

	"github.com/henry40408/noda/internal/cmd"
)

// Cell is one square of the screen.
type Cell struct {
	Symbol   string
	Fg, Bg   Color
	Modifier Modifier
}

func blankCell() Cell { return Cell{Symbol: " ", Fg: Reset, Bg: Reset} }

func (c *Cell) apply(style Style) {
	if style.Fg != Unset {
		c.Fg = style.Fg
	}
	if style.Bg != Unset {
		c.Bg = style.Bg
	}
	c.Modifier = (c.Modifier | style.Add) &^ style.Sub
}

// Style is what the cell looks like, for a test to compare.
func (c Cell) Style() Style { return Style{Fg: c.Fg, Bg: c.Bg, Add: c.Modifier} }

// Buffer is a screenful of cells.
type Buffer struct {
	Area  Rect
	Cells []Cell
}

// NewBuffer is an area of blank cells.
func NewBuffer(area Rect) *Buffer {
	cells := make([]Cell, area.Width*area.Height)
	for i := range cells {
		cells[i] = blankCell()
	}
	return &Buffer{Area: area, Cells: cells}
}

// At is the cell at a position, or nil outside the buffer.
func (b *Buffer) At(x, y int) *Cell {
	if x < b.Area.X || y < b.Area.Y || x >= b.Area.Right() || y >= b.Area.Bottom() {
		return nil
	}
	return &b.Cells[(y-b.Area.Y)*b.Area.Width+(x-b.Area.X)]
}

// SetStyle patches the style onto every cell of the area.
func (b *Buffer) SetStyle(area Rect, style Style) {
	area = area.Intersect(b.Area)
	for y := area.Y; y < area.Bottom(); y++ {
		for x := area.X; x < area.Right(); x++ {
			b.At(x, y).apply(style)
		}
	}
}

// Clear puts the cells of the area back to blank.
func (b *Buffer) Clear(area Rect) {
	area = area.Intersect(b.Area)
	for y := area.Y; y < area.Bottom(); y++ {
		for x := area.X; x < area.Right(); x++ {
			*b.At(x, y) = blankCell()
		}
	}
}

// Row is a row's symbols, as a terminal would show them.
func (b *Buffer) Row(y int) string {
	var row strings.Builder
	for x := b.Area.X; x < b.Area.Right(); x++ {
		row.WriteString(b.At(x, y).Symbol)
	}
	return row.String()
}

// String is every row, one to a line.
func (b *Buffer) String() string {
	rows := make([]string, 0, b.Area.Height)
	for y := b.Area.Y; y < b.Area.Bottom(); y++ {
		rows = append(rows, b.Row(y))
	}
	return strings.Join(rows, "\n")
}

type grapheme struct {
	symbol string
	width  int
}

// graphemes cuts text into what the terminal draws in one cell or two, leaving
// out control characters and anything with no width.
func graphemes(text string) []grapheme {
	var out []grapheme
	clusters := uniseg.NewGraphemes(text)
	for clusters.Next() {
		symbol := clusters.Str()
		if strings.IndexFunc(symbol, unicode.IsControl) >= 0 {
			continue
		}
		width := cmd.DisplayWidth(symbol)
		if width == 0 {
			continue
		}
		out = append(out, grapheme{symbol, width})
	}
	return out
}

// textWidth is the columns text occupies.
func textWidth(text string) int {
	width := 0
	for _, g := range graphemes(text) {
		width += g.width
	}
	return width
}

// setSpan writes text from (x, y), stopping at limit (a column), and patches the
// style onto each cell. The cells a wide character covers are blanked.
func (b *Buffer) setSpan(x, y, limit int, text string, style Style) {
	limit = min(limit, b.Area.Right())
	for _, g := range graphemes(text) {
		if x+g.width > limit {
			return
		}
		cell := b.At(x, y)
		if cell == nil {
			return
		}
		cell.Symbol = g.symbol
		cell.apply(style)
		for hidden := 1; hidden < g.width; hidden++ {
			*b.At(x+hidden, y) = blankCell()
		}
		x += g.width
	}
}
