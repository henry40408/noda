package tui

import (
	"reflect"
	"slices"
	"testing"

	"github.com/henry40408/noda/internal/cmd"
	"github.com/henry40408/noda/internal/note"
	"github.com/henry40408/noda/internal/notebook"
)

func typing(app *App, text string) {
	for _, c := range text {
		app.OnKey(Rune(c))
	}
}

// readIt is what the runtime does between the keystroke and the frame.
func readIt(t *testing.T, app *App, text string) {
	t.Helper()
	app.OnKey(Plain(CodeEnter))
	view := app.View()
	if need := app.Wanted(); need == nil || need.Kind != NeedNote {
		t.Fatal("a note screen wants its file")
	}
	app.Supply(view, Content{Kind: ContentNote, Text: text})
}

// supplied supplies content as refresh would.
func supplied(t *testing.T, app *App, content Content) {
	t.Helper()
	view := app.View()
	if app.Wanted() == nil {
		t.Fatalf("%v asked for nothing", view)
	}
	app.Supply(view, content)
}

// aSession is nothing but these notes, and a fixed date so "overdue" holds
// still.
func aSession(status notebook.Status, notes []notebook.NoteFile) Session {
	return Session{Status: status, Notes: notes, Notebooks: []string{"personal"}, Today: "2026-08-09"}
}

func aStatus() notebook.Status {
	return notebook.Status{Branch: "main", Notes: 3}
}

func aNote(id, slug, title string, tags []string, body string) notebook.NoteFile {
	return notebook.NoteFile{ID: id, Slug: slug, Note: note.Note{Title: title, Tags: tags, Body: body}}
}

// anApp has fixed ids, so assertions are stable.
func anApp() *App {
	return NewApp("personal", "/notebook", aSession(aStatus(), []notebook.NoteFile{
		aNote("aaaa1111", "budget-review", "Budget review", []string{"work"}, "the q3 budget"),
		aNote("bbbb2222", "meeting-notes", "Meeting notes", []string{"work", "q3"}, "agenda"),
		aNote("cccc3333", "reading-list", "Reading list", nil, "a book about budgets"),
	}))
}

func selectedID(app *App) string {
	if file := app.Selected(); file != nil {
		return file.ID
	}
	return ""
}

func wantSelected(t *testing.T, app *App, id string) {
	t.Helper()
	if got := selectedID(app); got != id {
		t.Fatalf("selected %q, want %q", got, id)
	}
}

func wantAction(t *testing.T, got *Action, want Action) {
	t.Helper()
	if got == nil || !reflect.DeepEqual(*got, want) {
		t.Fatalf("action %+v, want %+v", got, want)
	}
}

func wantNoAction(t *testing.T, got *Action) {
	t.Helper()
	if got != nil {
		t.Fatalf("action %+v, want none", got)
	}
}

func wantInt(t *testing.T, what string, got, want int) {
	t.Helper()
	if got != want {
		t.Fatalf("%s = %d, want %d", what, got, want)
	}
}

func wantMode(t *testing.T, app *App, want Mode) {
	t.Helper()
	if app.Mode != want {
		t.Fatalf("mode %v, want %v", app.Mode, want)
	}
}

func wantText(t *testing.T, what, got, want string) {
	t.Helper()
	if got != want {
		t.Fatalf("%s = %q, want %q", what, got, want)
	}
}

func repeatKey(app *App, key Key, times int) {
	for range times {
		app.OnKey(key)
	}
}

func TestItOpensOnTheListingAndTheFirstNote(t *testing.T) {
	app := anApp()
	if app.View() != (View{Kind: ViewNotes}) {
		t.Fatal("not on the listing")
	}
	wantInt(t, "depth", app.Depth(), 1)
	wantSelected(t, app, "aaaa1111")
	wantInt(t, "shown", app.Shown(), 3)
}

func TestTheCursorStopsAtBothEnds(t *testing.T) {
	app := anApp()
	repeatKey(app, Rune('j'), 10)
	wantSelected(t, app, "cccc3333")
	repeatKey(app, Rune('k'), 10)
	wantSelected(t, app, "aaaa1111")
}

func TestGAndShiftGReachTheEndsInOneKey(t *testing.T) {
	app := anApp()
	app.OnKey(Rune('G'))
	wantSelected(t, app, "cccc3333")
	app.OnKey(Rune('g'))
	wantSelected(t, app, "aaaa1111")
}

func TestTheQueryNarrowsAsItIsTyped(t *testing.T) {
	app := anApp()
	app.OnKey(Rune('/'))
	wantMode(t, app, ModeSearch)

	typing(app, "budget")
	// The title of one and the body of another: a bare word is `text:`.
	wantInt(t, "shown", app.Shown(), 2)

	typing(app, " tag:work")
	wantInt(t, "shown", app.Shown(), 1)
	wantSelected(t, app, "aaaa1111")
}

func TestAHalfTypedQueryKeepsTheLastGoodResult(t *testing.T) {
	app := anApp()
	app.OnKey(Rune('/'))
	typing(app, "tag:work")
	wantInt(t, "shown", app.Shown(), 2)

	typing(app, " O")
	lastGood := app.Shown()
	if _, has := app.Error(); has {
		t.Fatal("error too early")
	}

	// `OR` with nothing after it breaks the grammar.
	typing(app, "R")
	if _, has := app.Error(); !has {
		t.Fatal("expected an error")
	}
	wantInt(t, "shown (an unfinished query leaves the list where it was)", app.Shown(), lastGood)

	typing(app, " tag:q3")
	if _, has := app.Error(); has {
		t.Fatal("error should clear")
	}
	wantInt(t, "shown", app.Shown(), 2)
}

func TestEscapeGivesTheWholeNotebookBack(t *testing.T) {
	app := anApp()
	app.OnKey(Rune('/'))
	typing(app, "tag:q3")
	wantInt(t, "shown", app.Shown(), 1)

	app.OnKey(Plain(CodeEsc))
	wantMode(t, app, ModeBrowse)
	wantInt(t, "shown", app.Shown(), 3)
	wantText(t, "search", app.Search(), "")
}

func TestEnterKeepsTheQueryAndHandsBackTheKeyboard(t *testing.T) {
	app := anApp()
	app.OnKey(Rune('/'))
	typing(app, "tag:q3")
	app.OnKey(Plain(CodeEnter))

	wantMode(t, app, ModeBrowse)
	wantInt(t, "depth (leaving the field is not opening a note)", app.Depth(), 1)
	wantInt(t, "shown", app.Shown(), 1)
}

func TestAQueryThatMatchesNothingLeavesNoCursor(t *testing.T) {
	app := anApp()
	app.OnKey(Rune('/'))
	typing(app, "tag:nothing")
	wantInt(t, "shown", app.Shown(), 0)
	if app.Selected() != nil {
		t.Fatal("a cursor with nothing shown")
	}
	app.OnKey(Plain(CodeEnter))
	app.OnKey(Plain(CodeEnter))
	wantInt(t, "depth", app.Depth(), 1)
	if app.Wanted() != nil {
		t.Fatal("wanted something")
	}
}

func TestEnterOpensTheNoteAndEscapeClosesItAgain(t *testing.T) {
	app := anApp()
	app.OnKey(Rune('j'))

	app.OnKey(Plain(CodeEnter))
	if app.View() != NoteView("bbbb2222") {
		t.Fatalf("view %v", app.View())
	}
	wantInt(t, "depth", app.Depth(), 2)
	wantNeed(t, app.Wanted(), Need{Kind: NeedNote, ID: "bbbb2222", Path: "/notebook/bbbb2222-meeting-notes.md"})
	supplied(t, app, Content{Kind: ContentNote, Text: "agenda\n"})
	if app.Wanted() != nil {
		t.Fatal("still wanted")
	}

	app.OnKey(Plain(CodeEsc))
	if app.View() != (View{Kind: ViewNotes}) {
		t.Fatalf("view %v", app.View())
	}
	wantInt(t, "depth", app.Depth(), 1)
	wantSelected(t, app, "bbbb2222")
}

func wantNeed(t *testing.T, got *Need, want Need) {
	t.Helper()
	if got == nil || *got != want {
		t.Fatalf("need %+v, want %+v", got, want)
	}
}

func TestTheListingIsNeverPopped(t *testing.T) {
	app := anApp()
	repeatKey(app, Plain(CodeEsc), 5)
	wantInt(t, "depth", app.Depth(), 1)
	if app.View() != (View{Kind: ViewNotes}) {
		t.Fatal("not on the listing")
	}
}

func TestANoteOpenedFromAQueryKeepsWhatTheQueryPickedOut(t *testing.T) {
	app := anApp()
	app.OnKey(Rune('/'))
	typing(app, "budget")
	app.OnKey(Plain(CodeEnter))
	if !slices.Equal(app.Terms(), []string{"budget"}) {
		t.Fatalf("terms %v", app.Terms())
	}

	app.OnKey(Plain(CodeEnter))
	wantInt(t, "depth", app.Depth(), 2)
	if !slices.Equal(app.Terms(), []string{"budget"}) {
		t.Fatalf("terms %v", app.Terms())
	}
}

func TestANoteOnItsOwnScreenScrollsAndStopsAtTheEnd(t *testing.T) {
	app := anApp()
	readIt(t, app, "one\ntwo\nthree\nfour\n")
	wantInt(t, "scroll", app.Scroll(), 0)

	app.OnKey(Rune('j'))
	wantInt(t, "scroll", app.Scroll(), 1)
	repeatKey(app, Rune('j'), 20)
	wantInt(t, "scroll (it stops rather than running into blanks)", app.Scroll(), 3)

	app.OnKey(Rune('g'))
	wantInt(t, "scroll", app.Scroll(), 0)
}

func TestTheListingKeepsItsCursorWhileANoteIsBeingRead(t *testing.T) {
	app := anApp()
	readIt(t, app, "one\ntwo\nthree\nfour\n")
	repeatKey(app, Rune('j'), 3)
	wantInt(t, "scroll (the note scrolled)", app.Scroll(), 3)

	app.OnKey(Plain(CodeEsc))
	wantSelected(t, app, "aaaa1111")
}

func TestANoteStartsAtItsTop(t *testing.T) {
	app := anApp()
	readIt(t, app, "one\ntwo\nthree\n")
	app.OnKey(Rune('j'))
	wantInt(t, "scroll", app.Scroll(), 1)

	app.Supply(app.View(), Content{Kind: ContentNote, Text: "agenda\n"})
	wantInt(t, "scroll", app.Scroll(), 0)
}

func TestCtrlFMovesByHalfOfWhatIsOnScreen(t *testing.T) {
	app := anApp()
	app.SetPage(4)
	app.OnKey(CtrlKey('f'))
	wantSelected(t, app, "cccc3333")
	app.OnKey(CtrlKey('b'))
	wantSelected(t, app, "aaaa1111")
}

func TestQuittingAndReloadingAreTheRuntimesBusiness(t *testing.T) {
	app := anApp()
	wantAction(t, app.OnKey(Rune('q')), Action{Kind: ActQuit})
	wantAction(t, app.OnKey(Rune('r')), Action{Kind: ActReload})
	wantAction(t, app.OnKey(CtrlKey('c')), Action{Kind: ActQuit})
}

func TestTheKeysThatChangeANoteMeanTheSameOnTheNoteItself(t *testing.T) {
	app := anApp()
	readIt(t, app, "the q3 budget\n")

	wantAction(t, app.OnKey(Rune('e')), Action{Kind: ActEdit, Key: "aaaa1111", Touch: cmd.TouchStamp})
	app.OnKey(Rune('m'))
	wantMode(t, app, ModeAsk)
	if app.Asking != AskRetitle {
		t.Fatal("not a retitle")
	}
	wantText(t, "input", app.Input.Text(), "Budget review")
	app.OnKey(Plain(CodeEsc))

	app.OnKey(CtrlKey('d'))
	wantMode(t, app, ModeConfirm)
	if app.Confirming != WhatDelete {
		t.Fatal("not a delete")
	}
	wantAction(t, app.OnKey(Rune('y')), Action{Kind: ActRemove, Key: "aaaa1111"})
}

func TestDeletingTheNoteYouAreReadingClosesItsScreen(t *testing.T) {
	app := anApp()
	readIt(t, app, "the q3 budget\n")
	wantInt(t, "depth", app.Depth(), 2)

	// As after a delete: the re-read notebook lacks the note on screen.
	app.Replace(aSession(aStatus(), []notebook.NoteFile{
		aNote("bbbb2222", "meeting-notes", "Meeting notes", []string{"work"}, "x"),
		aNote("cccc3333", "reading-list", "Reading list", nil, "a book"),
	}))
	wantInt(t, "depth (a screen with nothing behind it is closed)", app.Depth(), 1)
	if app.View() != (View{Kind: ViewNotes}) {
		t.Fatal("not on the listing")
	}
}

func TestCtrlCLeavesFromInsideAQueryToo(t *testing.T) {
	app := anApp()
	app.OnKey(Rune('/'))
	typing(app, "budget")
	wantAction(t, app.OnKey(CtrlKey('c')), Action{Kind: ActQuit})
}

func TestAChordDoesNotTypeItsOwnLetterIntoAQuery(t *testing.T) {
	app := anApp()
	app.OnKey(Rune('/'))
	typing(app, "budget")

	// Ctrl-R arrives as Char 'r' with a modifier; an unbound chord neither
	// types nor reloads.
	app.OnKey(CtrlKey('r'))
	app.OnKey(CtrlKey('o'))
	wantText(t, "search", app.Search(), "budget")
	wantMode(t, app, ModeSearch)

	// Shift is not a chord.
	app.OnKey(Key{Code: CodeChar, Char: 'Q', Shift: true})
	wantText(t, "search", app.Search(), "budgetQ")
}

func TestTheQueryAnswersTheKeysAShellPromptAnswers(t *testing.T) {
	app := anApp()
	app.OnKey(Rune('/'))
	typing(app, "tag:work budget")
	wantInt(t, "shown (one note has both)", app.Shown(), 1)

	// The keys are tested in field; this checks the query is re-run.
	app.OnKey(CtrlKey('w'))
	wantText(t, "search", app.Search(), "tag:work ")
	wantInt(t, "shown", app.Shown(), 2)

	// Including an edit mid-line.
	app.OnKey(CtrlKey('a'))
	typing(app, "id:aaaa1111 ")
	wantText(t, "search", app.Search(), "id:aaaa1111 tag:work ")
	wantInt(t, "shown", app.Shown(), 1)
}

func TestTheCursorStillWalksTheListingWhileAQueryIsTyped(t *testing.T) {
	app := anApp()
	app.OnKey(Rune('/'))
	typing(app, "tag:work")
	wantSelected(t, app, "aaaa1111")
	app.OnKey(Plain(CodeDown))
	wantSelected(t, app, "bbbb2222")
	app.OnKey(CtrlKey('p'))
	wantSelected(t, app, "aaaa1111")
	wantText(t, "search", app.Search(), "tag:work")
}

func TestQIsALetterWhileAQueryIsBeingTyped(t *testing.T) {
	app := anApp()
	app.OnKey(Rune('/'))
	wantNoAction(t, app.OnKey(Rune('q')))
	wantText(t, "search", app.Search(), "q")
}

func TestHelpIsDismissedByAnything(t *testing.T) {
	app := anApp()
	app.OnKey(Rune('?'))
	wantMode(t, app, ModeHelp)
	app.OnKey(Rune('x'))
	wantMode(t, app, ModeBrowse)
}

func TestAReloadKeepsTheQueryAndTheNoteUnderTheCursor(t *testing.T) {
	app := anApp()
	app.OnKey(Rune('/'))
	typing(app, "tag:work")
	app.OnKey(Plain(CodeEnter))
	app.OnKey(Rune('j'))
	wantSelected(t, app, "bbbb2222")

	app.Replace(aSession(aStatus(), []notebook.NoteFile{
		aNote("aaaa1111", "budget-review", "Budget review", []string{"work"}, "the q3 budget"),
		aNote("bbbb2222", "meeting-notes", "Meeting notes", []string{"work", "q3"}, "agenda, revised"),
		aNote("cccc3333", "reading-list", "Reading list", nil, "a book about budgets"),
		aNote("dddd4444", "trip-plan", "Trip plan", []string{"work"}, "flights"),
	}))

	wantText(t, "search", app.Search(), "tag:work")
	wantInt(t, "shown", app.Shown(), 3)
	wantSelected(t, app, "bbbb2222")
}

func TestAReloadDropsTheCopyOfTheNoteThatWasOnScreen(t *testing.T) {
	app := anApp()
	readIt(t, app, "the q3 budget\n")
	if app.Wanted() != nil {
		t.Fatal("it has been read")
	}

	app.Replace(aSession(aStatus(), []notebook.NoteFile{
		aNote("aaaa1111", "budget-review", "Budget review", []string{"work"}, "the q3 budget, revised"),
	}))
	if app.Wanted() == nil {
		t.Fatal("should want a fresh read")
	}
}

func TestAReloadThatRemovesTheSelectedNoteLandsSomewhereReal(t *testing.T) {
	app := anApp()
	app.OnKey(Rune('G'))
	wantSelected(t, app, "cccc3333")

	app.Replace(aSession(aStatus(), []notebook.NoteFile{
		aNote("aaaa1111", "budget-review", "Budget review", []string{"work"}, "q3"),
	}))
	wantSelected(t, app, "aaaa1111")
}

func TestAnEmptyNotebookHasNowhereToPutTheCursor(t *testing.T) {
	app := NewApp("personal", "/notebook", aSession(aStatus(), nil))
	if app.Selected() != nil {
		t.Fatal("selected")
	}
	app.OnKey(Rune('j'))
	app.OnKey(Rune('G'))
	if app.Selected() != nil {
		t.Fatal("selected")
	}
}

func TestEAsksForTheNoteUnderTheCursorById(t *testing.T) {
	app := anApp()
	app.OnKey(Rune('j'))
	wantAction(t, app.OnKey(Rune('e')), Action{Kind: ActEdit, Key: "bbbb2222", Touch: cmd.TouchStamp})
	wantMode(t, app, ModeBrowse)
}

func TestANewNoteIsTitledAtThePrompt(t *testing.T) {
	app := anApp()
	wantNoAction(t, app.OnKey(Rune('a')))
	wantMode(t, app, ModeAsk)
	if app.Asking != AskTitle {
		t.Fatal("not a title")
	}

	typing(app, "Trip plan")
	wantText(t, "input", app.Input.Text(), "Trip plan")
	wantAction(t, app.OnKey(Plain(CodeEnter)), Action{Kind: ActAdd, Title: "Trip plan", HasTitle: true})
	wantMode(t, app, ModeBrowse)
	if !app.Input.IsEmpty() {
		t.Fatal("input not cleared")
	}
}

func TestANewNoteWithNoTitleLeavesItToTheBody(t *testing.T) {
	app := anApp()
	app.OnKey(Rune('a'))
	wantAction(t, app.OnKey(Plain(CodeEnter)), Action{Kind: ActAdd})
}

func TestARetitleStartsFromTheTitleItHas(t *testing.T) {
	app := anApp()
	app.OnKey(Rune('m'))
	wantMode(t, app, ModeAsk)
	if app.Asking != AskRetitle {
		t.Fatal("not a retitle")
	}
	wantText(t, "input", app.Input.Text(), "Budget review")

	repeatKey(app, Plain(CodeBackspace), len("review"))
	typing(app, "revision")
	wantAction(t, app.OnKey(Plain(CodeEnter)),
		Action{Kind: ActRetitle, Key: "aaaa1111", Title: "Budget revision", Touch: cmd.TouchStamp})
}

func choiceTags(app *App) []string {
	var names []string
	for _, c := range app.Choices() {
		names = append(names, c.Tag)
	}
	return names
}

func TestThePickerOpensOnEveryTagWithTheNotesOwnTicked(t *testing.T) {
	app := anApp()
	app.OnKey(Rune('#'))
	wantMode(t, app, ModeTagging)

	// The tags screen's list and order, whether or not this note has them.
	if got := choiceTags(app); !slices.Equal(got, []string{"work", "q3"}) {
		t.Fatalf("choices %v", got)
	}
	wantInt(t, "picking notes", app.PickingNotes(), 1)
	if file := app.PickingNote(); file == nil || file.ID != "aaaa1111" {
		t.Fatalf("picking %v", file)
	}

	wantText(t, "tick", app.Choices()[0].Tick(1), "[x]")
	wantInt(t, "notes", app.Choices()[0].Notes, 2)
	wantText(t, "tick", app.Choices()[1].Tick(1), "[ ]")
}

func TestTabWalksATagThroughTheStatesThatWouldChangeSomething(t *testing.T) {
	app := anApp()
	app.OnKey(Rune('#'))

	// Two states over one note: a tag it carries cannot be given again.
	app.OnKey(Plain(CodeTab))
	if app.Choices()[0].Mark != MarkRemove {
		t.Fatal("want remove")
	}
	app.OnKey(Plain(CodeTab))
	if app.Choices()[0].Mark != MarkLeave {
		t.Fatal("want leave")
	}

	app.OnKey(Plain(CodeDown))
	app.OnKey(Plain(CodeTab))
	if app.Choices()[1].Mark != MarkAdd {
		t.Fatal("want add")
	}
	wantText(t, "tick", app.Choices()[1].Tick(1), "[+]")
}
