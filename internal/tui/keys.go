package tui

// Code is which key was pressed, apart from the modifiers held.
type Code uint8

const (
	CodeChar Code = iota
	CodeEnter
	CodeEsc
	CodeTab
	CodeBackTab
	CodeUp
	CodeDown
	CodeLeft
	CodeRight
	CodePageUp
	CodePageDown
	CodeHome
	CodeEnd
	CodeBackspace
	CodeDelete
)

// Key is a key press. It is the TUI's own type, not the terminal library's, so
// the state machine does not depend on one and a test can build keys freely. A
// chord arrives as its letter: Ctrl-d is Char 'd' with Ctrl set.
type Key struct {
	Code  Code
	Char  rune
	Ctrl  bool
	Alt   bool
	Shift bool
}

// Plain is a key with no modifier.
func Plain(code Code) Key { return Key{Code: code} }

// Rune is a typed character.
func Rune(r rune) Key { return Key{Code: CodeChar, Char: r} }

// CtrlKey is Ctrl held with a letter.
func CtrlKey(r rune) Key { return Key{Code: CodeChar, Char: r, Ctrl: true} }

// AltKey is Alt held with a letter.
func AltKey(r rune) Key { return Key{Code: CodeChar, Char: r, Alt: true} }
