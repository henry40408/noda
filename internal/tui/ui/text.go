package ui

// Span is a run of text in one style.
type Span struct {
	Content string
	Style   Style
}

// Raw is text with no style of its own.
func Raw(content string) Span { return Span{Content: content} }

// Styled is text in a style.
func Styled(content string, style Style) Span { return Span{content, style} }

// Width is the columns the span occupies.
func (s Span) Width() int { return textWidth(s.Content) }

// Alignment is which side of its area a line sits against.
type Alignment uint8

const (
	AlignLeft Alignment = iota
	AlignRight
)

// Line is spans on one row.
type Line struct {
	Spans     []Span
	Alignment Alignment
}

// LineOf is a line of the spans.
func LineOf(spans ...Span) Line { return Line{Spans: spans} }

// LineRaw is a line of unstyled text.
func LineRaw(content string) Line { return Line{Spans: []Span{Raw(content)}} }

// Width is the columns the line occupies.
func (l Line) Width() int {
	width := 0
	for _, span := range l.Spans {
		width += span.Width()
	}
	return width
}

// String is the line's text with no styles.
func (l Line) String() string {
	text := ""
	for _, span := range l.Spans {
		text += span.Content
	}
	return text
}

// Right is the line aligned to the right edge of its area.
func (l Line) Right() Line {
	l.Alignment = AlignRight
	return l
}

// Render draws the line on the first row of the area, cut off at its edge.
func (l Line) Render(b *Buffer, area Rect) {
	area = area.Intersect(b.Area)
	if area.Width == 0 || area.Height == 0 {
		return
	}
	x := area.X
	if l.Alignment == AlignRight {
		x += max(0, area.Width-l.Width())
	}
	for _, span := range l.Spans {
		if x >= area.Right() {
			return
		}
		b.setSpan(x, area.Y, area.Right(), span.Content, span.Style)
		x += span.Width()
	}
}
