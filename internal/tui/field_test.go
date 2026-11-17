package tui

import "testing"

func typed(text string) *Field {
	field := &Field{}
	field.Set(text)
	return field
}

func expectText(t *testing.T, field *Field, want string) {
	t.Helper()
	if field.Text() != want {
		t.Fatalf("text %q, want %q", field.Text(), want)
	}
}

func expectBefore(t *testing.T, field *Field, want string) {
	t.Helper()
	if field.Before() != want {
		t.Fatalf("before %q, want %q", field.Before(), want)
	}
}

func expectEdit(t *testing.T, got, want Edit) {
	t.Helper()
	if got != want {
		t.Fatalf("edit %v, want %v", got, want)
	}
}

func TestTypingGoesInAtTheCursorAndNotAtTheEnd(t *testing.T) {
	field := typed("budget")
	field.Key(CtrlKey('a'))
	field.Key(Rune('a'))
	expectText(t, field, "abudget")
	expectBefore(t, field, "a")
}

func TestTheCursorWalksByCharacterAndStopsAtBothEnds(t *testing.T) {
	field := typed("ab")
	expectEdit(t, field.Key(CtrlKey('e')), Moved)
	for range 4 {
		field.Key(CtrlKey('b'))
	}
	expectBefore(t, field, "")
	for range 4 {
		field.Key(CtrlKey('f'))
	}
	expectBefore(t, field, "ab")
}

func TestACursorNeverLandsInsideACharacter(t *testing.T) {
	field := typed("預算")
	field.Key(Plain(CodeLeft))
	expectBefore(t, field, "預")
	field.Key(Plain(CodeBackspace))
	expectText(t, field, "算")
	field.Key(Rune('的'))
	expectText(t, field, "的算")
	expectBefore(t, field, "的")
}

func TestTheWordKeysStepOverLettersAndDigits(t *testing.T) {
	field := typed("tag:work budget")
	expectEdit(t, field.Key(AltKey('b')), Moved)
	expectBefore(t, field, "tag:work ")
	field.Key(AltKey('b'))
	expectBefore(t, field, "tag:")
	field.Key(AltKey('f'))
	expectBefore(t, field, "tag:work")
}

func TestCtrlWTakesAWholeTermHoweverItIsPunctuated(t *testing.T) {
	field := typed("tag:work tag:\"12.34\"")
	expectEdit(t, field.Key(CtrlKey('w')), Typed)
	expectText(t, field, "tag:work ")
	field.Key(CtrlKey('w'))
	expectText(t, field, "")
}

func TestTheKillsTakeEachEndOfTheLine(t *testing.T) {
	field := typed("budget review")
	field.Key(AltKey('b'))
	expectEdit(t, field.Key(CtrlKey('k')), Typed)
	expectText(t, field, "budget ")
	expectEdit(t, field.Key(CtrlKey('u')), Typed)
	expectText(t, field, "")
}

func TestAltDTakesTheWordInFrontOfTheCursor(t *testing.T) {
	field := typed("budget review")
	field.Key(CtrlKey('a'))
	expectEdit(t, field.Key(AltKey('d')), Typed)
	expectText(t, field, " review")
}

func TestWhatAKillTookCanBePutBack(t *testing.T) {
	field := typed("tag:work budget")
	field.Key(CtrlKey('w'))
	expectText(t, field, "tag:work ")
	expectEdit(t, field.Key(CtrlKey('y')), Typed)
	expectText(t, field, "tag:work budget")
	expectBefore(t, field, "tag:work budget")
	field.Key(CtrlKey('y'))
	expectText(t, field, "tag:work budgetbudget")
}

func TestAKillThatTookNothingKeepsWhatTheLastOneTook(t *testing.T) {
	field := typed("budget")
	field.Key(CtrlKey('u'))
	expectEdit(t, field.Key(CtrlKey('k')), NoEdit)
	expectEdit(t, field.Key(CtrlKey('u')), NoEdit)
	expectEdit(t, field.Key(CtrlKey('h')), NoEdit)
	field.Key(CtrlKey('y'))
	expectText(t, field, "budget")
}

func TestCtrlDDeletesForwardAndNothingElse(t *testing.T) {
	field := typed("budget")
	field.Key(CtrlKey('a'))
	expectEdit(t, field.Key(CtrlKey('d')), Typed)
	expectText(t, field, "udget")
	expectEdit(t, field.Key(Plain(CodeDelete)), Typed)
	expectText(t, field, "dget")
	// At the end: neither end-of-file nor the browser's delete.
	field.Key(CtrlKey('e'))
	expectEdit(t, field.Key(CtrlKey('d')), NoEdit)
	expectText(t, field, "dget")
}

func TestAChordThisDoesNotBindDoesNothingAtAll(t *testing.T) {
	field := typed("Trip")
	expectEdit(t, field.Key(CtrlKey('p')), NoEdit)
	expectEdit(t, field.Key(AltKey('x')), NoEdit)
	expectText(t, field, "Trip")
	expectEdit(t, field.Key(Key{Code: CodeChar, Char: 'Q', Shift: true}), Typed)
	expectText(t, field, "TripQ")
}

func TestAFieldWithNoCursorDrawnOnlyErasesFromTheEnd(t *testing.T) {
	field := typed("push")
	expectEdit(t, field.Erasing(CtrlKey('h')), Typed)
	expectText(t, field, "pus")
	expectEdit(t, field.Erasing(CtrlKey('u')), Typed)
	expectText(t, field, "")
	field.Set("push")
	expectEdit(t, field.Erasing(CtrlKey('a')), NoEdit)
	expectEdit(t, field.Erasing(AltKey('b')), NoEdit)
	expectEdit(t, field.Erasing(Plain(CodeLeft)), NoEdit)
	field.Erasing(Rune('x'))
	expectText(t, field, "pushx")
}

func TestWhatIsPutInTheFieldCanBeAddedToStraightAway(t *testing.T) {
	field := typed("Budget review")
	field.Key(Rune('!'))
	expectText(t, field, "Budget review!")
}
