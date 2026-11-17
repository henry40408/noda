package ui

import (
	"strings"
	"unicode"
)

// Paragraph is lines of text, optionally wrapped at word boundaries and
// scrolled down, inside a block.
type Paragraph struct {
	Lines  []Line
	Wrap   bool
	Scroll int
	Block  Block
}

type styledGrapheme struct {
	grapheme
	style Style
}

// Render draws the lines. Text that does not fit is cut off, never squeezed.
func (p Paragraph) Render(buf *Buffer, area Rect) {
	area = area.Intersect(buf.Area)
	p.Block.Render(buf, area)
	inner := p.Block.Inner(area).Intersect(buf.Area)
	if inner.Width == 0 || inner.Height == 0 {
		return
	}
	y := 0
	for _, line := range p.Lines {
		for _, row := range p.rowsOf(line, inner.Width) {
			if y >= p.Scroll+inner.Height {
				return
			}
			if y >= p.Scroll {
				drawRow(buf, inner.X, inner.Y+y-p.Scroll, row)
			}
			y++
		}
	}
}

func (p Paragraph) rowsOf(line Line, width int) [][]styledGrapheme {
	var all []styledGrapheme
	for _, span := range line.Spans {
		for _, g := range graphemes(span.Content) {
			all = append(all, styledGrapheme{g, span.Style})
		}
	}
	if p.Wrap {
		return wrapRow(all, width)
	}
	return [][]styledGrapheme{truncateRow(all, width)}
}

// drawRow leaves the cell a wide character covers as it was.
func drawRow(buf *Buffer, x, y int, row []styledGrapheme) {
	for _, g := range row {
		cell := buf.At(x, y)
		if cell == nil {
			return
		}
		cell.Symbol = g.symbol
		cell.apply(g.style)
		x += g.width
	}
}

func truncateRow(row []styledGrapheme, width int) []styledGrapheme {
	used := 0
	for i, g := range row {
		if used+g.width > width {
			return row[:i]
		}
		used += g.width
	}
	return row
}

// A no-break space is part of a word, as it is for ratatui.
func isBreakSpace(symbol string) bool {
	if symbol == " " {
		return false
	}
	return strings.IndexFunc(symbol, func(r rune) bool { return !unicode.IsSpace(r) }) < 0
}

// wrapRow breaks one line at word boundaries, keeping leading spaces; a word
// longer than the width is broken where it overflows. An empty line is one
// empty row.
func wrapRow(line []styledGrapheme, width int) [][]styledGrapheme {
	if width == 0 {
		return nil
	}
	var rows [][]styledGrapheme
	rest := line
	for {
		row, remainder := fillRow(rest, width)
		if len(row) == 0 && len(remainder) == 0 && len(rows) > 0 {
			return rows
		}
		rows = append(rows, row)
		if len(remainder) == 0 {
			return rows
		}
		rest = remainder
	}
}

// fillRow takes graphemes from the front of rest until the row is full, and
// returns the row and what is left, less the spaces the break swallowed.
func fillRow(rest []styledGrapheme, width int) (row, remainder []styledGrapheme) {
	used, lastEnd := 0, 0
	previousSpace := false
	for taken := 0; taken < len(rest); {
		g := rest[taken]
		taken++
		if g.width > width {
			continue
		}
		space := isBreakSpace(g.symbol)
		if space && !previousSpace {
			lastEnd = len(row)
		}
		previousSpace = space
		row = append(row, g)
		used += g.width
		if used <= width {
			continue
		}
		cutAt := lastEnd
		if cutAt == 0 {
			cutAt = len(row) - 1
		}
		carry := row[cutAt:]
		row = row[:cutAt:cutAt]
		for i, c := range carry {
			if !isBreakSpace(c.symbol) {
				remainder = append(append([]styledGrapheme{}, carry[i:]...), rest[taken:]...)
				return row, remainder
			}
		}
		return row, append([]styledGrapheme{}, rest[taken:]...)
	}
	return row, nil
}
