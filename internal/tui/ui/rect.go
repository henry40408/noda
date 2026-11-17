package ui

// Rect is a block of cells.
type Rect struct{ X, Y, Width, Height int }

func (r Rect) Right() int  { return r.X + r.Width }
func (r Rect) Bottom() int { return r.Y + r.Height }

// Intersect is the cells both rectangles hold.
func (r Rect) Intersect(o Rect) Rect {
	x, y := max(r.X, o.X), max(r.Y, o.Y)
	right, bottom := min(r.Right(), o.Right()), min(r.Bottom(), o.Bottom())
	return Rect{X: x, Y: y, Width: max(0, right-x), Height: max(0, bottom-y)}
}

type constraintKind uint8

const (
	lengthKind constraintKind = iota
	minKind
	fillKind
)

// Constraint sizes one piece of a split.
type Constraint struct {
	kind constraintKind
	n    int
}

// Length is exactly n cells, fewer when there are not that many left.
func Length(n int) Constraint { return Constraint{lengthKind, max(0, n)} }

// Min is whatever is left, and at least n when that many are.
func Min(n int) Constraint { return Constraint{minKind, max(0, n)} }

// Fill is a share of what is left, in proportion to weight.
func Fill(weight int) Constraint { return Constraint{fillKind, max(0, weight)} }

// Direction is the axis a split runs along.
type Direction uint8

const (
	Horizontal Direction = iota
	Vertical
)

// Flex is where the room a split leaves over goes.
type Flex uint8

const (
	FlexStart Flex = iota
	FlexCenter
)

// Layout cuts a rectangle into pieces along one axis.
type Layout struct {
	Direction Direction
	Flex      Flex
	Spacing   int
	Pieces    []Constraint
}

// Split lays the pieces out in order. Lengths are met first, from the start;
// Min and Fill then share what is left.
func (l Layout) Split(area Rect) []Rect {
	total := area.Width
	if l.Direction == Vertical {
		total = area.Height
	}
	sizes := l.sizes(total)
	start := 0
	if l.Flex == FlexCenter {
		// The odd column goes to the leading spacer, as ratatui rounds it.
		start = (max(0, total-l.spanOf(sizes)) + 1) / 2
	}
	out := make([]Rect, len(sizes))
	at := start
	for i, size := range sizes {
		if l.Direction == Horizontal {
			out[i] = Rect{X: area.X + at, Y: area.Y, Width: size, Height: area.Height}
		} else {
			out[i] = Rect{X: area.X, Y: area.Y + at, Width: area.Width, Height: size}
		}
		at += size + l.Spacing
	}
	return out
}

func (l Layout) spanOf(sizes []int) int {
	span := l.Spacing * max(0, len(sizes)-1)
	for _, size := range sizes {
		span += size
	}
	return span
}

func (l Layout) sizes(total int) []int {
	left := max(0, total-l.Spacing*max(0, len(l.Pieces)-1))
	sizes := make([]int, len(l.Pieces))
	weights, weighed := 0, 0
	for i, piece := range l.Pieces {
		switch piece.kind {
		case lengthKind:
			sizes[i] = min(piece.n, left)
			left -= sizes[i]
		case minKind:
			weights++
		case fillKind:
			weights += piece.n
		}
	}
	for i, piece := range l.Pieces {
		if piece.kind == lengthKind || weights == 0 {
			continue
		}
		weight := 1
		if piece.kind == fillKind {
			weight = piece.n
		}
		weighed += weight
		share := left*weighed/weights - left*(weighed-weight)/weights
		sizes[i] = share
		if piece.kind == minKind {
			sizes[i] = max(share, min(piece.n, left))
		}
	}
	return sizes
}
