package ui

import "github.com/gdamore/tcell/v2"

var tcellColors = map[Color]tcell.Color{
	Reset: tcell.ColorDefault, Black: tcell.PaletteColor(0), Red: tcell.PaletteColor(1),
	Green: tcell.PaletteColor(2), Yellow: tcell.PaletteColor(3), Blue: tcell.PaletteColor(4),
	Magenta: tcell.PaletteColor(5), Cyan: tcell.PaletteColor(6), Gray: tcell.PaletteColor(7),
	DarkGray: tcell.PaletteColor(8), LightRed: tcell.PaletteColor(9),
	LightGreen: tcell.PaletteColor(10), LightYellow: tcell.PaletteColor(11),
	LightBlue: tcell.PaletteColor(12), LightMagenta: tcell.PaletteColor(13),
	LightCyan: tcell.PaletteColor(14), White: tcell.PaletteColor(15),
}

func tcellStyle(cell Cell) tcell.Style {
	style := tcell.StyleDefault.Foreground(tcellColors[cell.Fg]).Background(tcellColors[cell.Bg])
	return style.Bold(cell.Modifier&Bold != 0).Dim(cell.Modifier&Dim != 0).Reverse(cell.Modifier&Reversed != 0)
}

// Flush writes the frame to the screen and shows or hides the cursor. It does
// not call Show: the caller decides when the screen is redrawn.
func Flush(screen tcell.Screen, frame *Frame) {
	buf := frame.Buffer
	for y := buf.Area.Y; y < buf.Area.Bottom(); y++ {
		for x := buf.Area.X; x < buf.Area.Right(); x++ {
			cell := buf.At(x, y)
			runes := []rune(cell.Symbol)
			screen.SetContent(x, y, runes[0], runes[1:], tcellStyle(*cell))
			// A wide character covers the next cell; drawing it too would cut
			// the character in half.
			x += max(0, textWidth(cell.Symbol)-1)
		}
	}
	if frame.Cursor == nil {
		screen.HideCursor()
		return
	}
	screen.ShowCursor(frame.Cursor.X, frame.Cursor.Y)
}
