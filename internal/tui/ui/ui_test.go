package ui

import (
	"strings"
	"testing"

	"github.com/gdamore/tcell/v2"
)

func rows(b *Buffer) []string {
	out := strings.Split(b.String(), "\n")
	for i := range out {
		out[i] = strings.TrimRight(out[i], " ")
	}
	return out
}

func equal(t *testing.T, got, want []string) {
	t.Helper()
	if strings.Join(got, "\n") != strings.Join(want, "\n") {
		t.Fatalf("got\n%s\nwant\n%s", strings.Join(got, "\n"), strings.Join(want, "\n"))
	}
}

func TestLayoutLengthsFirstThenTheRest(t *testing.T) {
	got := Layout{Direction: Vertical, Pieces: []Constraint{Length(5), Length(1), Min(0), Length(0), Length(1)}}.
		Split(Rect{0, 0, 10, 30})
	heights := []int{}
	for _, r := range got {
		heights = append(heights, r.Height)
	}
	want := []int{5, 1, 23, 0, 1}
	for i := range want {
		if heights[i] != want[i] {
			t.Fatalf("heights %v, want %v", heights, want)
		}
	}
	if got[4].Y != 29 {
		t.Fatalf("the status row is at %d", got[4].Y)
	}
}

func TestLayoutNeverHandsOutMoreThanThereIs(t *testing.T) {
	got := Layout{Pieces: []Constraint{Fill(1), Length(30)}}.Split(Rect{0, 0, 10, 1})
	if got[0].Width != 0 || got[1].Width != 10 {
		t.Fatalf("got %+v", got)
	}
}

func TestLayoutCentersAndSpaces(t *testing.T) {
	got := Layout{Flex: FlexCenter, Pieces: []Constraint{Length(66)}}.Split(Rect{0, 0, 100, 1})
	if got[0].X != 17 || got[0].Width != 66 {
		t.Fatalf("got %+v", got)
	}
	cols := Layout{Spacing: 2, Pieces: []Constraint{Length(3), Fill(1), Length(4)}}.Split(Rect{0, 0, 20, 1})
	if cols[1].X != 5 || cols[1].Width != 9 || cols[2].X != 16 {
		t.Fatalf("got %+v", cols)
	}
}

func draw(width, height int, p Paragraph) []string {
	buf := NewBuffer(Rect{0, 0, width, height})
	p.Render(buf, buf.Area)
	return rows(buf)
}

func TestParagraphWrapsAtWordsAndKeepsIndents(t *testing.T) {
	got := draw(10, 4, Paragraph{Wrap: true, Lines: []Line{LineRaw("the quick brown fox")}})
	equal(t, got, []string{"the quick", "brown fox", "", ""})
	got = draw(6, 3, Paragraph{Wrap: true, Lines: []Line{LineRaw("  abcdefghij")}})
	equal(t, got, []string{"  abcd", "efghij", ""})
}

func TestParagraphBreaksALongWordAndKeepsBlankLines(t *testing.T) {
	got := draw(4, 4, Paragraph{Wrap: true, Lines: []Line{LineRaw("abcdefghi"), {}, LineRaw("x")}})
	equal(t, got, []string{"abcd", "efgh", "i", ""})
}

func TestParagraphScrollsAndCutsWithoutWrap(t *testing.T) {
	lines := []Line{LineRaw("one"), LineRaw("two long line"), LineRaw("three")}
	got := draw(6, 2, Paragraph{Lines: lines, Scroll: 1})
	equal(t, got, []string{"two lo", "three"})
}

func TestParagraphPaddingAndWideCharacters(t *testing.T) {
	got := draw(8, 1, Paragraph{Lines: []Line{LineRaw("日本語")}, Block: Block{Padding: Padding{Left: 2}}})
	equal(t, got, []string{"  日 本 語"})
}

func TestBorderedBlockHasARoundedBorderAndTitle(t *testing.T) {
	buf := NewBuffer(Rect{0, 0, 12, 3})
	p := Paragraph{Lines: []Line{LineRaw("hi")}, Block: Block{Bordered: true, Title: Raw(" t ")}}
	p.Render(buf, buf.Area)
	equal(t, rows(buf), []string{"╭ t ───────╮", "│hi        │", "╰──────────╯"})
}

func TestSpanStylesPatchTheCell(t *testing.T) {
	buf := NewBuffer(Rect{0, 0, 4, 1})
	buf.SetStyle(buf.Area, Style{Add: Dim})
	LineOf(Styled("ab", Style{Fg: Yellow, Add: Bold})).Render(buf, buf.Area)
	cell := buf.At(0, 0)
	if cell.Fg != Yellow || cell.Modifier != Bold|Dim || cell.Bg != Reset {
		t.Fatalf("cell %+v", cell)
	}
	if got := cell.Style().Add.String(); got != "BOLD | DIM" {
		t.Fatalf("modifier prints as %q", got)
	}
}

func TestRightAlignedLineSitsAtTheEdge(t *testing.T) {
	buf := NewBuffer(Rect{0, 0, 8, 1})
	LineRaw("noda").Right().Render(buf, buf.Area)
	equal(t, rows(buf), []string{"    noda"})
}

func TestTableScrollsToTheSelectionAndReservesTheBar(t *testing.T) {
	var body []Row
	for _, name := range []string{"a", "b", "c", "d", "e"} {
		body = append(body, Row{Cells: []Line{LineRaw(name), LineRaw("x" + name)}})
	}
	table := Table{
		Rows: body, Header: &Row{Cells: []Line{LineRaw("N"), LineRaw("V")}},
		Widths: []Constraint{Length(1), Fill(1)}, ColumnSpacing: 2,
		HighlightStyle: Style{Add: Bold}, HighlightSymbol: Styled("> ", Style{Fg: Yellow}),
	}
	state := TableState{Selected: 4}
	buf := NewBuffer(Rect{0, 0, 10, 3})
	table.Render(buf, buf.Area, &state)
	equal(t, rows(buf), []string{"  N  V", "  d  xd", "> e  xe"})
	if state.Offset != 3 {
		t.Fatalf("offset %d", state.Offset)
	}
	if buf.At(5, 2).Modifier != Bold || buf.At(0, 2).Fg != Yellow || buf.At(0, 1).Modifier != 0 {
		t.Fatal("the selected row is not the bold one")
	}
	state.Selected = 0
	buf = NewBuffer(Rect{0, 0, 10, 3})
	table.Render(buf, buf.Area, &state)
	if state.Offset != 0 {
		t.Fatalf("offset %d after going back to the top", state.Offset)
	}
}

func TestScrollbarThumbFollowsPosition(t *testing.T) {
	bar := Scrollbar{Track: "│", Thumb: "█"}
	for _, c := range []struct {
		position int
		want     string
	}{{0, "█████│││││"}, {10, "│││││█████"}} {
		buf := NewBuffer(Rect{0, 0, 1, 10})
		bar.Render(buf, buf.Area, ScrollbarState{ContentLength: 10, Viewport: 10, Position: c.position})
		got := strings.ReplaceAll(buf.String(), "\n", "")
		if got != c.want {
			t.Fatalf("position %d drew %q, want %q", c.position, got, c.want)
		}
	}
}

func TestFlushWritesCellsAndCursorToASimulatedScreen(t *testing.T) {
	screen := tcell.NewSimulationScreen("UTF-8")
	if err := screen.Init(); err != nil {
		t.Fatal(err)
	}
	defer screen.Fini()
	screen.SetSize(6, 2)
	frame := NewFrame(6, 2)
	LineOf(Styled("日a", Style{Fg: Red, Add: Bold})).Render(frame.Buffer, frame.Area())
	frame.Cursor = &Point{3, 0}
	Flush(screen, frame)
	screen.Show()
	cells, width, _ := screen.GetContents()
	if width != 6 || string(cells[0].Runes) != "日" || string(cells[2].Runes) != "a" {
		t.Fatalf("cells %+v", cells[:4])
	}
	fg, _, attrs := cells[0].Style.Decompose()
	if fg != tcell.PaletteColor(1) || attrs&tcell.AttrBold == 0 {
		t.Fatalf("style %v %v", fg, attrs)
	}
}
