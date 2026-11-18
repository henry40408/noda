package ui

import "math"

// Scrollbar is a track and thumb down the right edge, with no arrows.
type Scrollbar struct {
	Track      string
	TrackStyle Style
	Thumb      string
}

// ScrollbarState is how far through the content the view has got.
type ScrollbarState struct {
	// ContentLength is how far the view can scroll; Viewport is how much it shows.
	ContentLength int
	Viewport      int
	Position      int
}

// Render draws the bar the height of area.
func (s Scrollbar) Render(buf *Buffer, area Rect, state ScrollbarState) {
	area = area.Intersect(buf.Area)
	if area.Width == 0 || area.Height == 0 {
		return
	}
	start, length := s.thumb(float64(area.Height), state)
	for i := range area.Height {
		symbol, style := s.Track, s.TrackStyle
		if i >= start && i < start+length {
			symbol, style = s.Thumb, Style{}
		}
		buf.setSpan(area.X, area.Y+i, area.Right(), symbol, style)
	}
}

// The thumb scales over content plus a viewport less one row, so the last
// position still leaves the thumb a row on the track.
func (Scrollbar) thumb(track float64, state ScrollbarState) (start, length int) {
	viewport := float64(state.Viewport)
	if viewport == 0 {
		viewport = track
	}
	content := float64(state.ContentLength)
	position := float64(min(state.Position, max(0, state.ContentLength-1)))
	scrollable := content + viewport - 1
	first := math.Round(position * track / scrollable)
	last := math.Round((position + viewport) * track / scrollable)
	start = int(math.Min(math.Max(first, 0), track-1))
	end := int(math.Min(math.Max(last, 0), track))
	return start, max(0, end-start)
}
