package tui

import (
	"unicode"
	"unicode/utf8"
)

// Edit tells a query's owner what a key did: a query reruns when its text
// changes, not when the cursor moves.
type Edit uint8

const (
	// NoEdit is a key the field leaves to its caller, an unbound chord
	// included: it must not type its letter.
	NoEdit Edit = iota
	Typed
	Moved
)

// word is readline's two kinds of word: the Alt keys stop at punctuation
// (tag:work), while Ctrl-w stops only at whitespace and takes a whole
// tag:"12.34".
type word uint8

const (
	alnumWord word = iota
	blankWord
)

func (w word) holds(r rune) bool {
	if w == alnumWord {
		return unicode.IsLetter(r) || unicode.IsDigit(r) || unicode.IsNumber(r)
	}
	return !unicode.IsSpace(r)
}

// Field is a one-line text field with readline's keys. The kill ring is one
// entry deep and consecutive kills replace: enough for Ctrl-y to undo a Ctrl-w,
// in a line rarely longer than a title.
type Field struct {
	text string
	// at is a byte offset, since every use slices the text; it is only ever
	// moved by walking characters, so it is always on a character boundary.
	at int
	// cut is kept across Set and Clear.
	cut string
}

// Text is everything typed.
func (f *Field) Text() string { return f.text }

// IsEmpty is whether nothing is typed.
func (f *Field) IsEmpty() bool { return f.text == "" }

// Before is the text left of the cursor, which places the terminal's cursor.
func (f *Field) Before() string { return f.text[:f.at] }

// Set replaces the text and leaves the cursor at the end.
func (f *Field) Set(text string) {
	f.text = text
	f.at = len(text)
}

// Clear empties the field.
func (f *Field) Clear() {
	f.text = ""
	f.at = 0
}

// Take empties the field and returns what it held.
func (f *Field) Take() string {
	text := f.text
	f.Clear()
	return text
}

// Key applies a key.
func (f *Field) Key(key Key) Edit {
	switch {
	case key.Code == CodeChar && key.Ctrl:
		return f.chord(key.Char)
	case key.Code == CodeChar && key.Alt:
		return f.meta(key.Char)
	case key.Code == CodeChar:
		return f.insert(key.Char)
	case key.Code == CodeBackspace && key.Alt:
		return f.kill(f.wordBack(alnumWord), f.at)
	case key.Code == CodeBackspace:
		return f.kill(f.back(), f.at)
	case key.Code == CodeDelete:
		return f.kill(f.at, f.forward())
	case key.Code == CodeLeft && (key.Ctrl || key.Alt):
		return f.to(f.wordBack(alnumWord))
	case key.Code == CodeRight && (key.Ctrl || key.Alt):
		return f.to(f.wordForward(alnumWord))
	case key.Code == CodeLeft:
		return f.to(f.back())
	case key.Code == CodeRight:
		return f.to(f.forward())
	case key.Code == CodeHome:
		return f.to(0)
	case key.Code == CodeEnd:
		return f.to(len(f.text))
	}
	return NoEdit
}

// Erasing is for a field whose cursor is not drawn: only keys that keep the
// cursor at the end, so nothing moves the insertion point out of sight.
func (f *Field) Erasing(key Key) Edit {
	switch {
	case key.Code == CodeChar && key.Ctrl && key.Char == 'w':
		return f.kill(f.wordBack(blankWord), f.at)
	// The cursor is at the end, so both kill everything.
	case key.Code == CodeChar && key.Ctrl && (key.Char == 'u' || key.Char == 'k'):
		return f.kill(0, f.at)
	case key.Code == CodeChar && key.Ctrl && key.Char == 'h':
		return f.kill(f.back(), f.at)
	case key.Code == CodeChar && (key.Ctrl || key.Alt):
		return NoEdit
	case key.Code == CodeChar:
		return f.insert(key.Char)
	case key.Code == CodeBackspace && key.Alt:
		return f.kill(f.wordBack(alnumWord), f.at)
	case key.Code == CodeBackspace:
		return f.kill(f.back(), f.at)
	}
	return NoEdit
}

func (f *Field) chord(c rune) Edit {
	switch c {
	case 'a':
		return f.to(0)
	case 'e':
		return f.to(len(f.text))
	case 'b':
		return f.to(f.back())
	case 'f':
		return f.to(f.forward())
	case 'h':
		return f.kill(f.back(), f.at)
	// Never readline's end-of-file: outside a field Ctrl-d deletes a note, so it
	// must not depend on what has been typed.
	case 'd':
		return f.kill(f.at, f.forward())
	case 'w':
		return f.kill(f.wordBack(blankWord), f.at)
	case 'u':
		return f.kill(0, f.at)
	case 'k':
		return f.kill(f.at, len(f.text))
	case 'y':
		return f.yank()
	}
	return NoEdit
}

func (f *Field) meta(c rune) Edit {
	switch c {
	case 'b':
		return f.to(f.wordBack(alnumWord))
	case 'f':
		return f.to(f.wordForward(alnumWord))
	case 'd':
		return f.kill(f.at, f.wordForward(alnumWord))
	}
	return NoEdit
}

func (f *Field) insert(c rune) Edit {
	f.text = f.text[:f.at] + string(c) + f.text[f.at:]
	f.at += utf8.RuneLen(c)
	return Typed
}

// kill removes nothing when from is not before to, which keeps the previous
// cut: an idle Ctrl-k at the end of a line does not lose what Ctrl-w took.
func (f *Field) kill(from, to int) Edit {
	if from >= to {
		return NoEdit
	}
	f.cut = f.text[from:to]
	f.text = f.text[:from] + f.text[to:]
	f.at = from
	return Typed
}

// yank keeps the cut, so it can be yanked again.
func (f *Field) yank() Edit {
	if f.cut == "" {
		return NoEdit
	}
	f.text = f.text[:f.at] + f.cut + f.text[f.at:]
	f.at += len(f.cut)
	return Typed
}

func (f *Field) to(at int) Edit {
	f.at = at
	return Moved
}

func (f *Field) back() int {
	r, size := utf8.DecodeLastRuneInString(f.text[:f.at])
	if r == utf8.RuneError && size == 0 {
		return f.at
	}
	return f.at - size
}

func (f *Field) forward() int {
	r, size := utf8.DecodeRuneInString(f.text[f.at:])
	if r == utf8.RuneError && size == 0 {
		return f.at
	}
	return f.at + size
}

// wordBack skips separators first, then the word, so it works from inside
// trailing spaces.
func (f *Field) wordBack(w word) int {
	at, into := f.at, false
	for at > 0 {
		r, size := utf8.DecodeLastRuneInString(f.text[:at])
		if w.holds(r) {
			into = true
		} else if into {
			break
		}
		at -= size
	}
	return at
}

func (f *Field) wordForward(w word) int {
	at, into := f.at, false
	for at < len(f.text) {
		r, size := utf8.DecodeRuneInString(f.text[at:])
		if w.holds(r) {
			into = true
		} else if into {
			break
		}
		at += size
	}
	return at
}
