package ui

// Frame is a buffer being drawn, and where the cursor goes when it is shown.
type Frame struct {
	Buffer *Buffer
	// Cursor is nil when the frame wants none.
	Cursor *Point
}

// Point is a position on the screen.
type Point struct{ X, Y int }

// NewFrame is a blank frame of the given size.
func NewFrame(width, height int) *Frame {
	return &Frame{Buffer: NewBuffer(Rect{0, 0, width, height})}
}

// Area is the whole frame.
func (f *Frame) Area() Rect { return f.Buffer.Area }

// Clear blanks the area, so a card does not show what is under it.
func (f *Frame) Clear(area Rect) { f.Buffer.Clear(area) }

// Line draws a line in the area.
func (f *Frame) Line(area Rect, line Line) { line.Render(f.Buffer, area) }

// Paragraph draws a paragraph in the area.
func (f *Frame) Paragraph(area Rect, p Paragraph) { p.Render(f.Buffer, area) }

// Table draws a table in the area.
func (f *Frame) Table(area Rect, t Table, state *TableState) { t.Render(f.Buffer, area, state) }

// Scrollbar draws a scrollbar in the area.
func (f *Frame) Scrollbar(area Rect, s Scrollbar, state ScrollbarState) {
	s.Render(f.Buffer, area, state)
}

// Block draws a block's border in the area.
func (f *Frame) Block(area Rect, b Block) { b.Render(f.Buffer, area) }
