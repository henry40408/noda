package ui

// Padding is empty room inside a block.
type Padding struct{ Left, Right, Top, Bottom int }

// Block is the frame around a widget: an optional rounded border with a title,
// and padding inside it.
type Block struct {
	Padding     Padding
	Bordered    bool
	Title       Span
	BorderStyle Style
}

// Inner is the area left for contents.
func (b Block) Inner(area Rect) Rect {
	if b.Bordered {
		area = Rect{area.X + 1, area.Y + 1, max(0, area.Width-2), max(0, area.Height-2)}
	}
	return Rect{
		X:      area.X + b.Padding.Left,
		Y:      area.Y + b.Padding.Top,
		Width:  max(0, area.Width-b.Padding.Left-b.Padding.Right),
		Height: max(0, area.Height-b.Padding.Top-b.Padding.Bottom),
	}
}

// Render draws the border and title, when there are any.
func (b Block) Render(buf *Buffer, area Rect) {
	area = area.Intersect(buf.Area)
	if !b.Bordered || area.Width < 2 || area.Height < 2 {
		return
	}
	right, bottom := area.Right()-1, area.Bottom()-1
	for x := area.X + 1; x < right; x++ {
		b.put(buf, x, area.Y, "─")
		b.put(buf, x, bottom, "─")
	}
	for y := area.Y + 1; y < bottom; y++ {
		b.put(buf, area.X, y, "│")
		b.put(buf, right, y, "│")
	}
	b.put(buf, area.X, area.Y, "╭")
	b.put(buf, right, area.Y, "╮")
	b.put(buf, area.X, bottom, "╰")
	b.put(buf, right, bottom, "╯")
	buf.setSpan(area.X+1, area.Y, right, b.Title.Content, b.Title.Style)
}

func (b Block) put(buf *Buffer, x, y int, symbol string) {
	cell := buf.At(x, y)
	cell.Symbol = symbol
	cell.apply(b.BorderStyle)
}
