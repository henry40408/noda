package ui

// Row is one row of a table, a line to a cell.
type Row struct{ Cells []Line }

// Table is rows in columns under an optional heading row, with a cursor bar
// beside the selected row.
type Table struct {
	Rows            []Row
	Header          *Row
	Widths          []Constraint
	ColumnSpacing   int
	HighlightStyle  Style
	HighlightSymbol Span
}

// TableState is which row the cursor is on and which is the first one drawn.
type TableState struct {
	// Selected is -1 when no row is.
	Selected int
	Offset   int
}

// NewTableState is a state with nothing selected, at the top.
func NewTableState() TableState { return TableState{Selected: -1} }

// Render draws the table and moves the state's offset to keep the selection in
// view. The cursor bar's column is reserved on every row, selected or not, so
// columns do not shift.
func (t Table) Render(buf *Buffer, area Rect, state *TableState) {
	area = area.Intersect(buf.Area)
	if area.Width == 0 || area.Height == 0 {
		return
	}
	columns := t.columns(area.Width)
	rowsTop, rowsHeight := area.Y, area.Height
	if t.Header != nil {
		t.drawCells(buf, area, area.Y, columns, *t.Header)
		rowsTop, rowsHeight = area.Y+1, max(0, area.Height-1)
	}
	if len(t.Rows) == 0 {
		state.Selected = -1
		return
	}
	state.Selected = min(state.Selected, len(t.Rows)-1)
	start, end := t.visible(*state, rowsHeight)
	state.Offset = start
	for i := start; i < end; i++ {
		y := rowsTop + i - start
		selected := i == state.Selected
		if selected {
			t.HighlightSymbol.renderIn(buf, Rect{area.X, y, t.HighlightSymbol.Width(), 1})
		}
		t.drawCells(buf, area, y, columns, t.Rows[i])
		if selected {
			buf.SetStyle(Rect{area.X, y, area.Width, 1}, t.HighlightStyle)
		}
	}
}

func (s Span) renderIn(buf *Buffer, area Rect) {
	buf.setSpan(area.X, area.Y, area.Right(), s.Content, s.Style)
}

type column struct{ x, width int }

func (t Table) columns(width int) []column {
	bar := t.HighlightSymbol.Width()
	pieces := Layout{Spacing: t.ColumnSpacing, Pieces: t.Widths}.Split(Rect{bar, 0, max(0, width-bar), 1})
	columns := make([]column, len(pieces))
	for i, piece := range pieces {
		columns[i] = column{piece.X, piece.Width}
	}
	return columns
}

func (t Table) drawCells(buf *Buffer, area Rect, y int, columns []column, row Row) {
	for i, cell := range row.Cells {
		if i >= len(columns) {
			return
		}
		cell.Render(buf, Rect{area.X + columns[i].x, y, columns[i].width, 1})
	}
}

// visible is the rows [start, end) that fit under the offset, moved the least
// that shows the selection.
func (t Table) visible(state TableState, height int) (start, end int) {
	start = min(state.Offset, len(t.Rows)-1)
	end = start + min(height, len(t.Rows)-start)
	selected := min(max(state.Selected, 0), len(t.Rows)-1)
	if selected >= end {
		end = selected + 1
		start = max(start, end-height)
	}
	if selected < start {
		start = selected
		end = min(end, start+height)
	}
	return start, end
}
