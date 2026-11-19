package ui

import (
	"reflect"
	"testing"

	"github.com/gdamore/tcell/v2"
)

func widthsOf(rects []Rect) []int {
	widths := make([]int, len(rects))
	for i, r := range rects {
		widths[i] = r.Width
	}
	return widths
}

func TestBlockInnerSubtractsTheBorderAndEachPaddingSide(t *testing.T) {
	block := Block{Bordered: true, Padding: Padding{Left: 1, Right: 2, Top: 3, Bottom: 4}}
	got := block.Inner(Rect{5, 6, 20, 30})
	if want := (Rect{7, 10, 15, 21}); got != want {
		t.Fatalf("inner %+v, want %+v", got, want)
	}
	got = Block{Padding: Padding{Left: 1, Right: 2, Top: 3, Bottom: 4}}.Inner(Rect{5, 6, 20, 30})
	if want := (Rect{6, 9, 17, 23}); got != want {
		t.Fatalf("unbordered inner %+v, want %+v", got, want)
	}
}

func TestBlockDrawsABorderOnlyWhenTwoCellsWideAndTwoHigh(t *testing.T) {
	border := Block{Bordered: true}
	for _, c := range []struct {
		area Rect
		want []string
	}{
		{Rect{0, 0, 2, 2}, []string{"╭╮", "╰╯", "", ""}},
		{Rect{0, 0, 2, 3}, []string{"╭╮", "││", "╰╯", ""}},
		{Rect{0, 0, 3, 2}, []string{"╭─╮", "╰─╯", "", ""}},
		{Rect{0, 0, 1, 3}, []string{"", "", "", ""}},
		{Rect{0, 0, 3, 1}, []string{"", "", "", ""}},
	} {
		buf := NewBuffer(Rect{0, 0, 6, 4})
		border.Render(buf, c.area)
		equal(t, rows(buf), c.want)
	}
}

func TestBufferAtIsNilOnTheFirstCellPastEachEdge(t *testing.T) {
	buf := NewBuffer(Rect{2, 3, 4, 2})
	for _, c := range []struct{ x, y int }{{6, 3}, {2, 5}, {1, 3}, {2, 2}} {
		if buf.At(c.x, c.y) != nil {
			t.Fatalf("At(%d, %d) is inside the buffer", c.x, c.y)
		}
	}
}

func TestBufferAtMapsAnOffsetAreaOntoItsCells(t *testing.T) {
	buf := NewBuffer(Rect{2, 3, 4, 2})
	buf.At(3, 4).Symbol = "x"
	if buf.Cells[1*4+1].Symbol != "x" {
		t.Fatal("(3, 4) is not the second cell of the second row")
	}
	buf.At(5, 3).Symbol = "y"
	if buf.Cells[3].Symbol != "y" {
		t.Fatal("(5, 3) is not the last cell of the first row")
	}
}

func TestALeadingControlCharacterIsLeftOutOfTheRow(t *testing.T) {
	equal(t, draw(4, 1, Paragraph{Lines: []Line{LineRaw("\x01a")}}), []string{"a"})
}

func TestTextWidthAddsEachGraphemeUp(t *testing.T) {
	for text, want := range map[string]int{"": 0, "ab": 2, "a界": 3, "界界": 4} {
		if got := textWidth(text); got != want {
			t.Fatalf("textWidth(%q) = %d, want %d", text, got, want)
		}
	}
}

func TestParagraphStopsAtTheAreaNotTheBuffer(t *testing.T) {
	buf := NewBuffer(Rect{0, 0, 6, 6})
	lines := []Line{LineRaw("one"), LineRaw("two"), LineRaw("three"), LineRaw("four")}
	Paragraph{Lines: lines}.Render(buf, Rect{0, 0, 6, 2})
	equal(t, rows(buf), []string{"one", "two", "", "", "", ""})
	buf = NewBuffer(Rect{0, 0, 6, 6})
	Paragraph{Lines: lines, Scroll: 1}.Render(buf, Rect{0, 0, 6, 2})
	equal(t, rows(buf), []string{"two", "three", "", "", "", ""})
}

func TestParagraphCutsALongLineAtTheAreaNotTheBuffer(t *testing.T) {
	buf := NewBuffer(Rect{0, 0, 10, 1})
	Paragraph{Lines: []Line{LineRaw("abcdef")}}.Render(buf, Rect{0, 0, 3, 1})
	equal(t, rows(buf), []string{"abc"})
	buf = NewBuffer(Rect{0, 0, 10, 1})
	Paragraph{Lines: []Line{LineRaw("a界c")}}.Render(buf, Rect{0, 0, 3, 1})
	equal(t, rows(buf), []string{"a界"})
}

func TestParagraphWrapKeepsAnEmptyLineAsABlankRow(t *testing.T) {
	got := draw(5, 4, Paragraph{Wrap: true, Lines: []Line{LineRaw("a"), {}, LineRaw("b")}})
	equal(t, got, []string{"a", "", "b", ""})
}

func TestParagraphWrapTreatsANoBreakSpaceAsPartOfAWord(t *testing.T) {
	got := draw(5, 3, Paragraph{Wrap: true, Lines: []Line{LineRaw("ab cd ef")}})
	equal(t, got, []string{"ab cd", "ef", ""})
	got = draw(5, 3, Paragraph{Wrap: true, Lines: []Line{LineRaw("ab cd ef")}})
	equal(t, got, []string{"ab cd", "ef", ""})
}

func TestParagraphWrapFitsAWideCharacterThatIsExactlyAsWideAsTheRow(t *testing.T) {
	got := draw(2, 3, Paragraph{Wrap: true, Lines: []Line{LineRaw("界a")}})
	equal(t, got, []string{"界", "a", ""})
	got = draw(1, 2, Paragraph{Wrap: true, Lines: []Line{LineRaw("界a")}})
	equal(t, got, []string{"a", ""})
}

func TestIntersectWidthIsTheOverlapNotTheSum(t *testing.T) {
	got := Rect{2, 1, 10, 6}.Intersect(Rect{5, 3, 10, 10})
	if want := (Rect{5, 3, 7, 4}); got != want {
		t.Fatalf("intersect %+v, want %+v", got, want)
	}
}

func TestCenteredSplitCountsTheSpacingBetweenPieces(t *testing.T) {
	two := Layout{Flex: FlexCenter, Spacing: 2, Pieces: []Constraint{Length(3), Length(3)}}.Split(Rect{0, 0, 20, 1})
	if two[0].X != 6 || two[1].X != 11 {
		t.Fatalf("two pieces at %d and %d", two[0].X, two[1].X)
	}
	one := Layout{Flex: FlexCenter, Spacing: 2, Pieces: []Constraint{Length(4)}}.Split(Rect{0, 0, 20, 1})
	if one[0].X != 8 {
		t.Fatalf("one piece at %d, want 8", one[0].X)
	}
}

func TestFillSharesWhatIsLeftByWeightAndGivesTheRemainderToTheLast(t *testing.T) {
	for _, c := range []struct {
		pieces []Constraint
		total  int
		want   []int
	}{
		{[]Constraint{Fill(1), Fill(2), Fill(3)}, 10, []int{1, 4, 5}},
		{[]Constraint{Fill(1), Fill(1), Fill(1)}, 7, []int{2, 2, 3}},
		{[]Constraint{Fill(1), Fill(100)}, 10, []int{0, 10}},
	} {
		got := widthsOf(Layout{Pieces: c.pieces}.Split(Rect{0, 0, c.total, 1}))
		if !reflect.DeepEqual(got, c.want) {
			t.Fatalf("%d cells gave %v, want %v", c.total, got, c.want)
		}
	}
}

func TestMinTakesAtLeastItsLengthAndFillTakesOnlyItsShare(t *testing.T) {
	got := widthsOf(Layout{Pieces: []Constraint{Min(5), Fill(1)}}.Split(Rect{0, 0, 8, 1}))
	if want := []int{5, 4}; !reflect.DeepEqual(got, want) {
		t.Fatalf("got %v, want %v", got, want)
	}
}

func TestScrollbarThumbCoversTheViewportShareOfTheTrack(t *testing.T) {
	for _, c := range []struct {
		state         ScrollbarState
		start, length int
	}{
		{ScrollbarState{ContentLength: 20, Viewport: 5, Position: 0}, 0, 2},
		{ScrollbarState{ContentLength: 20, Viewport: 5, Position: 15}, 6, 2},
		{ScrollbarState{ContentLength: 20, Viewport: 5, Position: 100}, 8, 2},
		{ScrollbarState{ContentLength: 100, Viewport: 1, Position: 99}, 9, 1},
		{ScrollbarState{ContentLength: 20, Viewport: 0, Position: 0}, 0, 3},
	} {
		start, length := Scrollbar{}.thumb(10, c.state)
		if start != c.start || length != c.length {
			t.Fatalf("%+v: thumb (%d, %d), want (%d, %d)", c.state, start, length, c.start, c.length)
		}
	}
}

func TestTableKeepsASelectionPastTheLastRowOnTheLastRow(t *testing.T) {
	table := Table{Rows: []Row{{Cells: []Line{LineRaw("a")}}, {Cells: []Line{LineRaw("b")}}, {Cells: []Line{LineRaw("c")}}},
		Widths: []Constraint{Fill(1)}}
	state := TableState{Selected: 10}
	table.Render(NewBuffer(Rect{0, 0, 4, 3}), Rect{0, 0, 4, 3}, &state)
	if state.Selected != 2 {
		t.Fatalf("selected %d, want 2", state.Selected)
	}
}

func TestTableColumnsLieRightOfTheBarAndFillWhatIsLeft(t *testing.T) {
	table := Table{Widths: []Constraint{Fill(1)}, HighlightSymbol: Raw("> ")}
	got := table.columns(10)
	if want := []column{{2, 8}}; !reflect.DeepEqual(got, want) {
		t.Fatalf("columns %+v, want %+v", got, want)
	}
}

func TestTableIgnoresCellsBeyondItsColumns(t *testing.T) {
	table := Table{
		Rows:   []Row{{Cells: []Line{LineRaw("a"), LineRaw("b"), LineRaw("c")}}},
		Widths: []Constraint{Length(2), Length(2)},
	}
	state := NewTableState()
	buf := NewBuffer(Rect{0, 0, 12, 1})
	table.Render(buf, buf.Area, &state)
	equal(t, rows(buf), []string{"a b"})
}

func TestTableVisibleWindowMovesTheLeastThatShowsTheSelection(t *testing.T) {
	table := Table{Rows: make([]Row, 10)}
	for _, c := range []struct {
		name       string
		state      TableState
		height     int
		start, end int
	}{
		{"offset past the end", TableState{Selected: 0, Offset: 20}, 3, 0, 3},
		{"offset past the end on the last row", TableState{Selected: 9, Offset: 20}, 3, 9, 10},
		{"offset near the end", TableState{Selected: 9, Offset: 8}, 3, 8, 10},
		{"window from the top", TableState{Selected: 0, Offset: 0}, 3, 0, 3},
		{"selection just below", TableState{Selected: 3, Offset: 0}, 3, 1, 4},
		{"selection far below", TableState{Selected: 50, Offset: 0}, 3, 7, 10},
		{"selection above", TableState{Selected: 2, Offset: 5}, 3, 2, 5},
		{"selection at the top", TableState{Selected: 5, Offset: 5}, 3, 5, 8},
	} {
		start, end := table.visible(c.state, c.height)
		if start != c.start || end != c.end {
			t.Fatalf("%s: window [%d, %d), want [%d, %d)", c.name, start, end, c.start, c.end)
		}
	}
}

func flushedCell(t *testing.T, cell Cell) tcell.Style {
	t.Helper()
	screen := tcell.NewSimulationScreen("UTF-8")
	if err := screen.Init(); err != nil {
		t.Fatal(err)
	}
	defer screen.Fini()
	screen.SetSize(2, 1)
	frame := NewFrame(2, 1)
	*frame.Buffer.At(0, 0) = cell
	Flush(screen, frame)
	screen.Show()
	cells, _, _ := screen.GetContents()
	return cells[0].Style
}

func TestFlushPassesEachModifierOnAndOnlyThatOne(t *testing.T) {
	for _, c := range []struct {
		modifier Modifier
		attr     tcell.AttrMask
	}{{Bold, tcell.AttrBold}, {Dim, tcell.AttrDim}, {Reversed, tcell.AttrReverse}, {0, 0}} {
		cell := blankCell()
		cell.Modifier = c.modifier
		_, _, attrs := flushedCell(t, cell).Decompose()
		if attrs != c.attr {
			t.Fatalf("modifier %v flushed as %v, want %v", c.modifier, attrs, c.attr)
		}
	}
}

func TestFlushHidesTheCursorWhenTheFrameWantsNone(t *testing.T) {
	screen := tcell.NewSimulationScreen("UTF-8")
	if err := screen.Init(); err != nil {
		t.Fatal(err)
	}
	defer screen.Fini()
	screen.SetSize(4, 2)
	frame := NewFrame(4, 2)
	frame.Cursor = &Point{2, 1}
	Flush(screen, frame)
	screen.Show()
	if x, y, visible := screen.GetCursor(); !visible || x != 2 || y != 1 {
		t.Fatalf("cursor (%d, %d) visible %v", x, y, visible)
	}
	frame.Cursor = nil
	Flush(screen, frame)
	screen.Show()
	if _, _, visible := screen.GetCursor(); visible {
		t.Fatal("the cursor is still shown")
	}
}

func TestLineStopsAtTheAreaEdgeNotTheBufferEdge(t *testing.T) {
	buf := NewBuffer(Rect{0, 0, 10, 1})
	LineRaw("abcdef").Render(buf, Rect{0, 0, 3, 1})
	equal(t, rows(buf), []string{"abc"})
	buf = NewBuffer(Rect{0, 0, 10, 1})
	LineRaw("a界").Render(buf, Rect{0, 0, 2, 1})
	equal(t, rows(buf), []string{"a"})
}

func TestParagraphWrapAddsNoBlankRowAfterTrailingGraphemesThatCannotFit(t *testing.T) {
	got := draw(1, 3, Paragraph{Wrap: true, Lines: []Line{LineRaw("a 界"), LineRaw("b")}})
	equal(t, got, []string{"a", "b", ""})
}
