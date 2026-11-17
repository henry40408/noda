package tui

import (
	"errors"
	"slices"
	"strings"
	"testing"

	"github.com/henry40408/noda/internal/cmd"
	"github.com/henry40408/noda/internal/notebook"
	"github.com/henry40408/noda/internal/style"
)

func tagAction(key string, touch cmd.Touch, changes ...string) Action {
	return Action{Kind: ActTag, Key: key, Changes: changes, Touch: touch}
}

// tagWith adds a tag the notebook lacks through the picker: one Tab is a `+`.
func tagWith(app *App, tag string) {
	app.OnKey(Rune('#'))
	typing(app, tag)
	app.OnKey(Plain(CodeTab))
	app.OnKey(Plain(CodeEnter))
}

// mark marks the notes with Space, as a user would.
func mark(app *App, ids ...string) {
	for _, id := range ids {
		app.SelectID(id)
		app.OnKey(Rune(' '))
	}
}

func wantProposal(t *testing.T, got *Proposal, want Proposal) {
	t.Helper()
	if got == nil || *got != want {
		t.Fatalf("proposal %+v, want %+v", got, want)
	}
}

func TestWhatThePickerSendsIsTheNotationTheCommandTakes(t *testing.T) {
	app := anApp()
	app.OnKey(Rune('#'))
	app.OnKey(Plain(CodeTab))
	app.OnKey(Plain(CodeDown))
	app.OnKey(Plain(CodeTab))

	wantAction(t, app.OnKey(Plain(CodeEnter)), tagAction("aaaa1111", cmd.TouchStamp, "-work", "+q3"))
	wantMode(t, app, ModeBrowse)
	wantInt(t, "choices left behind", len(app.Choices()), 0)
}

func TestChoosingNothingIsAWayOutRatherThanACommand(t *testing.T) {
	app := anApp()
	app.OnKey(Rune('#'))
	wantNoAction(t, app.OnKey(Plain(CodeEnter)))
	wantMode(t, app, ModeBrowse)
	if app.Message != nil {
		t.Fatal("nothing is said about it")
	}
}

func TestEscLeavesThePickerWithNothingAskedOfTheNotebook(t *testing.T) {
	app := anApp()
	app.OnKey(Rune('#'))
	app.OnKey(Plain(CodeTab))
	wantNoAction(t, app.OnKey(Plain(CodeEsc)))
	wantMode(t, app, ModeBrowse)
	wantInt(t, "choices", len(app.Choices()), 0)
	wantInt(t, "queue", len(app.Queue), 0)
}

func TestATagTheNotebookDoesNotHaveIsARowOfItsOwn(t *testing.T) {
	app := anApp()
	app.OnKey(Rune('#'))
	typing(app, "urgent")

	wantInt(t, "shown tags", len(app.ShownTags()), 0)
	wantProposal(t, app.Proposal(), Proposal{Tag: "urgent"})
	wantInt(t, "picker rows", app.PickerRows(), 1)

	app.OnKey(Plain(CodeTab))
	if app.Proposal() != nil {
		t.Fatal("it is one of the tags now")
	}
	choices := app.Choices()
	if choices[len(choices)-1].Mark != MarkAdd {
		t.Fatal("want add")
	}
	wantAction(t, app.OnKey(Plain(CodeEnter)), tagAction("aaaa1111", cmd.TouchStamp, "+urgent"))
}

func TestANewTagAKeystrokeFromAnOldOneSaysWhichOne(t *testing.T) {
	app := anApp()
	app.OnKey(Rune('#'))
	for _, typed := range []string{"wrok", "Work", "wrk", "workk"} {
		typing(app, typed)
		wantProposal(t, app.Proposal(), Proposal{Tag: typed, Near: "work", NearNotes: 2})
		repeatKey(app, Plain(CodeBackspace), len(typed))
	}

	// No warning for a tag that is nothing like one.
	typing(app, "budget")
	wantProposal(t, app.Proposal(), Proposal{Tag: "budget"})
}

func TestATagTheNotebookWouldRefuseIsRefusedWhereItIsTyped(t *testing.T) {
	app := anApp()
	app.OnKey(Rune('#'))
	typing(app, "a,b")
	proposal := app.Proposal()
	if proposal == nil || proposal.Refused == "" {
		t.Fatal("a comma is not a tag")
	}
	if !strings.Contains(proposal.Refused, ",") {
		t.Fatalf("in the words cmd uses: %s", proposal.Refused)
	}

	app.OnKey(Plain(CodeTab))
	wantNoAction(t, app.OnKey(Plain(CodeEnter)))
}

func TestOverAMarkedSetATagSomeOfThemCarryHasThreeStates(t *testing.T) {
	app := anApp()
	app.OnKey(Rune('*'))
	app.OnKey(Rune('#'))
	wantInt(t, "picking notes", app.PickingNotes(), 3)
	if app.PickingNote() != nil {
		t.Fatal("it is about a set, not a note")
	}

	wantInt(t, "held", app.Choices()[0].Held, 2)
	wantText(t, "tick", app.Choices()[0].Tick(3), "[ ]")
	for _, want := range []Mark{MarkAdd, MarkRemove, MarkLeave} {
		app.OnKey(Plain(CodeTab))
		if app.Choices()[0].Mark != want {
			t.Fatalf("mark %v, want %v", app.Choices()[0].Mark, want)
		}
	}

	// Over a marked set it queues rather than runs.
	app.OnKey(Plain(CodeTab))
	wantNoAction(t, app.OnKey(Plain(CodeEnter)))
	wantInt(t, "queue", len(app.Queue), 1)
	want := cmd.Change{Tags: []string{"+work"}, Touch: cmd.TouchStamp}
	if !slices.Equal(app.Queue[0].Change.Tags, want.Tags) || app.Queue[0].Change.Touch != want.Touch || app.Queue[0].Change.Remove {
		t.Fatalf("change %+v", app.Queue[0].Change)
	}
	wantInt(t, "keys", len(app.Queue[0].Keys), 3)
}

func TestATickOverASetMeansEveryOneOfThemCarriesIt(t *testing.T) {
	app := anApp()
	// The two notes that have `work`.
	mark(app, "aaaa1111", "bbbb2222")
	app.OnKey(Rune('#'))

	wantText(t, "tick", app.Choices()[0].Tick(2), "[x]")
	app.OnKey(Plain(CodeTab))
	if app.Choices()[0].Mark != MarkRemove {
		t.Fatal("want remove")
	}
}

func TestTheFilterNarrowsTheListAndLeavesTheChoicesAlone(t *testing.T) {
	app := anApp()
	app.OnKey(Rune('#'))
	app.OnKey(Plain(CodeTab))
	if app.Choices()[0].Mark != MarkRemove {
		t.Fatal("want remove")
	}

	typing(app, "q")
	if got := app.ShownTags(); !slices.Equal(got, []int{1}) {
		t.Fatalf("shown %v: only `q3` answers to that", got)
	}
	wantInt(t, "tags at (the cursor is on the list it can see)", app.TagsAt(), 0)
	// A choice filtered out of sight is kept.
	if app.Choices()[0].Mark != MarkRemove {
		t.Fatal("choice lost")
	}

	app.OnKey(Plain(CodeTab))
	if app.Choices()[1].Mark != MarkAdd {
		t.Fatal("want add")
	}
	wantAction(t, app.OnKey(Plain(CodeEnter)), tagAction("aaaa1111", cmd.TouchStamp, "-work", "+q3"))
}

func TestATagWithASpaceInItCanBeNamedHereLikeAnyOther(t *testing.T) {
	app := anApp()
	app.OnKey(Rune('#'))
	// As an import leaves behind. The space bar types a space.
	typing(app, "24.04 Dark patterns")
	wantText(t, "input", app.Input.Text(), "24.04 Dark patterns")
	wantProposal(t, app.Proposal(), Proposal{Tag: "24.04 Dark patterns"})

	app.OnKey(Plain(CodeTab))
	wantAction(t, app.OnKey(Plain(CodeEnter)), tagAction("aaaa1111", cmd.TouchStamp, "+24.04 Dark patterns"))
}

func TestOneKeystrokeApartIsTheFourShapesASlipTakes(t *testing.T) {
	for _, pair := range [][2]string{{"Work", "work"}, {"wor", "work"}, {"works", "work"}, {"wprk", "work"}, {"wrok", "work"}, {"q4", "q3"}} {
		if !oneEditApart(pair[0], pair[1]) {
			t.Errorf("%q should be one edit from %q", pair[0], pair[1])
		}
	}
	for _, pair := range [][2]string{{"wrko", "work"}, {"workshop", "work"}, {"q3", "budget"}} {
		if oneEditApart(pair[0], pair[1]) {
			t.Errorf("%q should not be one edit from %q", pair[0], pair[1])
		}
	}
}

func TestATagWithASpaceInItCanBeSearchedFor(t *testing.T) {
	app := NewApp("personal", "/notebook", aSession(aStatus(), []notebook.NoteFile{
		aNote("aaaa1111", "ubuntu-notes", "Ubuntu notes", []string{"12.34 foo bar"}, "body"),
		aNote("bbbb2222", "other-note", "Other note", []string{"work"}, "foo bar"),
	}))

	app.OnKey(Rune('/'))
	// Unquoted it is three terms ANDed, as a shell would split it.
	typing(app, "tag:12.34 foo bar")
	wantInt(t, "shown", app.Shown(), 0)

	app.OnKey(Plain(CodeEsc))
	app.OnKey(Rune('/'))
	typing(app, `tag:"12.34 foo bar"`)
	wantInt(t, "shown", app.Shown(), 1)
	wantSelected(t, app, "aaaa1111")
}

func TestQuotingLeavesAnOrdinaryQueryExactlyAsItWas(t *testing.T) {
	app := anApp()
	app.OnKey(Rune('/'))
	typing(app, "tag:work OR tag:q3")
	wantInt(t, "shown", app.Shown(), 2)
	if _, has := app.Error(); has {
		t.Fatal("error")
	}
}

func TestAQueueIsNotLeftBehindWithoutBeingAskedAbout(t *testing.T) {
	app := anApp()
	wantAction(t, app.OnKey(Rune('q')), Action{Kind: ActQuit})

	mark(app, "aaaa1111")
	tagWith(app, "archive")

	wantNoAction(t, app.OnKey(Rune('q')))
	wantMode(t, app, ModeConfirm)
	if app.Confirming != WhatQuit {
		t.Fatal("not a quit")
	}
	wantNoAction(t, app.OnKey(Rune('n')))
	wantMode(t, app, ModeBrowse)
	wantInt(t, "queue", len(app.Queue), 1)

	app.OnKey(Rune('q'))
	wantAction(t, app.OnKey(Rune('y')), Action{Kind: ActQuit})
}

func TestCtrlCStillMeansNow(t *testing.T) {
	app := anApp()
	mark(app, "aaaa1111")
	app.OnKey(Rune('#'))
	typing(app, "+archive")
	app.OnKey(Plain(CodeEnter))

	wantAction(t, app.OnKey(CtrlKey('c')), Action{Kind: ActQuit})
}

func TestTLeavesUpdatedAloneForAsLongAsItIsOn(t *testing.T) {
	app := anApp()
	if app.Touch != cmd.TouchStamp {
		t.Fatal("stamping is what a change means")
	}

	wantNoAction(t, app.OnKey(Rune('T')))
	if app.Touch != cmd.TouchKeep {
		t.Fatal("want keep")
	}

	// Every change made while it is on carries the command's `--no-touch`.
	wantAction(t, app.OnKey(Rune('e')), Action{Kind: ActEdit, Key: "aaaa1111", Touch: cmd.TouchKeep})
	app.OnKey(Rune('#'))
	typing(app, "urgent")
	app.OnKey(Plain(CodeTab))
	wantAction(t, app.OnKey(Plain(CodeEnter)), tagAction("aaaa1111", cmd.TouchKeep, "+urgent"))

	app.OnKey(Rune('T'))
	if app.Touch != cmd.TouchStamp {
		t.Fatal("want stamp")
	}
	wantAction(t, app.OnKey(Rune('e')), Action{Kind: ActEdit, Key: "aaaa1111", Touch: cmd.TouchStamp})
}

func TestTIsALetterWhileAQueryOrAPromptIsBeingTyped(t *testing.T) {
	app := anApp()
	app.OnKey(Rune('/'))
	typing(app, "T")
	wantText(t, "search", app.Search(), "T")
	if app.Touch != cmd.TouchStamp {
		t.Fatal("touch changed")
	}

	app.OnKey(Plain(CodeEsc))
	app.OnKey(Rune('m'))
	typing(app, "T")
	if !strings.HasSuffix(app.Input.Text(), "T") {
		t.Fatal("not typed")
	}
	if app.Touch != cmd.TouchStamp {
		t.Fatal("touch changed")
	}
}

func TestAnEmptiedPromptIsAWayOutRatherThanAnError(t *testing.T) {
	app := anApp()
	app.OnKey(Rune('m'))
	repeatKey(app, Plain(CodeBackspace), len("Budget review"))
	wantNoAction(t, app.OnKey(Plain(CodeEnter)))
	wantMode(t, app, ModeBrowse)

	app.OnKey(Rune('a'))
	typing(app, "A trip")
	wantNoAction(t, app.OnKey(Plain(CodeEsc)))
	wantMode(t, app, ModeBrowse)
	if !app.Input.IsEmpty() {
		t.Fatal("input left")
	}
}

func TestAChordDoesNotTypeItsOwnLetterIntoAPrompt(t *testing.T) {
	app := anApp()
	app.OnKey(Rune('a'))
	typing(app, "Trip")
	app.OnKey(CtrlKey('r'))
	app.OnKey(CtrlKey('o'))
	wantText(t, "input", app.Input.Text(), "Trip")
	wantMode(t, app, ModeAsk)
	if app.Asking != AskTitle {
		t.Fatal("not a title")
	}
}

func TestATitleBeingEditedIsEditedAndNotOnlyAddedTo(t *testing.T) {
	app := anApp()
	app.OnKey(Rune('m'))
	wantText(t, "input", app.Input.Text(), "Budget review")
	app.OnKey(CtrlKey('a'))
	app.OnKey(CtrlKey('d'))
	typing(app, "F")
	wantText(t, "input", app.Input.Text(), "Fudget review")
	wantAction(t, app.OnKey(Plain(CodeEnter)),
		Action{Kind: ActRetitle, Key: "aaaa1111", Title: "Fudget review", Touch: cmd.TouchStamp})
}

func TestThePromptTakesAWordBackAndPutsItBackAgain(t *testing.T) {
	app := anApp()
	app.OnKey(Rune('a'))
	typing(app, "Budget review")
	app.OnKey(CtrlKey('w'))
	wantText(t, "input", app.Input.Text(), "Budget ")
	app.OnKey(CtrlKey('y'))
	wantText(t, "input (and Ctrl-W is undoable)", app.Input.Text(), "Budget review")
}

func TestADeleteIsAskedAboutFirst(t *testing.T) {
	app := anApp()
	wantNoAction(t, app.OnKey(CtrlKey('d')))
	wantMode(t, app, ModeConfirm)
	if app.Confirming != WhatDelete {
		t.Fatal("not a delete")
	}

	// Anything but `y` keeps the note, `d` included.
	wantNoAction(t, app.OnKey(Rune('d')))
	wantMode(t, app, ModeBrowse)

	app.OnKey(CtrlKey('d'))
	wantAction(t, app.OnKey(Rune('y')), Action{Kind: ActRemove, Key: "aaaa1111"})
	wantMode(t, app, ModeBrowse)
}

func TestTheKeyThatUsedToDeleteSaysWhereTheDeletingWent(t *testing.T) {
	app := anApp()
	wantNoAction(t, app.OnKey(Rune('d')))
	wantMode(t, app, ModeBrowse)
	wantInt(t, "queue", len(app.Queue), 0)
	if app.Message == nil || app.Message.Line() != "delete is Ctrl-d" {
		t.Fatalf("message %+v", app.Message)
	}
}

func TestNothingUnderTheCursorIsNothingToChange(t *testing.T) {
	app := NewApp("personal", "/notebook", aSession(aStatus(), nil))
	for _, pressed := range "em#" {
		wantNoAction(t, app.OnKey(Rune(pressed)))
		if app.Mode != ModeBrowse {
			t.Fatalf("`%c` opened something", pressed)
		}
	}
	wantNoAction(t, app.OnKey(CtrlKey('d')))
	wantMode(t, app, ModeBrowse)

	// A note can still be made.
	app.OnKey(Rune('a'))
	wantMode(t, app, ModeAsk)
	if app.Asking != AskTitle {
		t.Fatal("not a title")
	}
}

func TestWhatACommandSaidIsShownAndThenLeftBehind(t *testing.T) {
	app := anApp()
	app.Report("aaaa1111  budget-review  [work, urgent]", nil)
	if app.Message == nil || !strings.Contains(app.Message.Text, "urgent") || app.Message.Failed {
		t.Fatalf("message %+v", app.Message)
	}
	wantMode(t, app, ModeBrowse)

	app.OnKey(Rune('j'))
	if app.Message != nil {
		t.Fatal("the next key moves on from it")
	}
}

func TestACommandThatColoursItsAnswerIsQuotedWithoutTheColour(t *testing.T) {
	app := anApp()
	// As `noda status` returns it, painted for the output layer to strip; a
	// card does not go through it.
	app.Report("notebook  personal  "+style.Muted.Paint("(main)")+"\nnotes     3", nil)
	if app.Message == nil {
		t.Fatal("an answer")
	}
	wantText(t, "text", app.Message.Text, "notebook  personal  (main)\nnotes     3")
}

func TestACommandThatRefusedPutsItsReasonOnACard(t *testing.T) {
	app := anApp()
	app.Report("", errors.New("/notebook/aaaa1111-budget-review.md: no frontmatter\nthe file was left as you saved it"))
	wantMode(t, app, ModeAlert)
	if app.Message == nil || !app.Message.Failed {
		t.Fatal("a reason")
	}
	// Both lines: the second says what to do about the first.
	if !strings.Contains(app.Message.Text, "was left as you saved it") {
		t.Fatal(app.Message.Text)
	}

	app.OnKey(Rune('x'))
	wantMode(t, app, ModeBrowse)
	if app.Message != nil {
		t.Fatal("message left")
	}
}

func TestADeletedNoteLeavesTheCursorWhereItWas(t *testing.T) {
	app := anApp()
	app.OnKey(Rune('j'))
	wantSelected(t, app, "bbbb2222")

	// As after a delete: the re-read notebook lacks the note under the cursor.
	app.Replace(aSession(aStatus(), []notebook.NoteFile{
		aNote("aaaa1111", "budget-review", "Budget review", []string{"work"}, "the q3 budget"),
		aNote("cccc3333", "reading-list", "Reading list", nil, "a book about budgets"),
	}))
	// The row is kept, so the next note is the one under the cursor.
	wantSelected(t, app, "cccc3333")
}

func TestAMarkAndAQueryDoNotTouchEachOther(t *testing.T) {
	app := anApp()
	mark(app, "aaaa1111")
	if !app.Marked("aaaa1111") {
		t.Fatal("not marked")
	}

	app.OnKey(Rune('/'))
	typing(app, "tag:q3")
	app.OnKey(Plain(CodeEnter))
	wantInt(t, "shown (the query answers to itself alone)", app.Shown(), 1)
	if !app.Marked("aaaa1111") {
		t.Fatal("a note the query is hiding is still marked")
	}

	// Marking under a query adds to the earlier marks.
	app.OnKey(Rune(' '))
	wantInt(t, "marks", len(app.Marks), 2)
}

func TestTheStarTakesWhatTheQueryShowsAndLeavesTheRest(t *testing.T) {
	app := anApp()
	mark(app, "cccc3333")

	app.OnKey(Rune('/'))
	typing(app, "tag:work")
	app.OnKey(Plain(CodeEnter))
	wantInt(t, "shown", app.Shown(), 2)

	app.OnKey(Rune('*'))
	wantInt(t, "marks (the two shown, and the one from before)", len(app.Marks), 3)

	app.OnKey(Rune('*'))
	wantInt(t, "marks", len(app.Marks), 1)
	if !app.Marked("cccc3333") {
		t.Fatal("lost the earlier mark")
	}
}

func TestEscapeDropsTheQueryFirstAndTheMarksAfter(t *testing.T) {
	app := anApp()
	mark(app, "aaaa1111")
	app.OnKey(Rune('/'))
	typing(app, "tag:q3")
	app.OnKey(Plain(CodeEnter))

	app.OnKey(Plain(CodeEsc))
	wantText(t, "search (the query goes first)", app.Search(), "")
	wantInt(t, "marks (still there)", len(app.Marks), 1)

	app.OnKey(Plain(CodeEsc))
	wantInt(t, "marks", len(app.Marks), 0)
}

func TestEscapeClosesTheNoteBeforeItTouchesTheQuery(t *testing.T) {
	app := anApp()
	app.OnKey(Rune('/'))
	typing(app, "tag:work")
	app.OnKey(Plain(CodeEnter))
	readIt(t, app, "the q3 budget\n")

	app.OnKey(Plain(CodeEsc))
	wantInt(t, "depth", app.Depth(), 1)
	wantText(t, "search", app.Search(), "tag:work")

	app.OnKey(Plain(CodeEsc))
	wantText(t, "search", app.Search(), "")
}

func TestWithNotesMarkedATagIsQueuedOverAllOfThem(t *testing.T) {
	app := anApp()
	mark(app, "aaaa1111", "bbbb2222")

	// Both carry `work`, so the first Tab removes it.
	app.OnKey(Rune('#'))
	wantText(t, "tick", app.Choices()[0].Tick(2), "[x]")
	app.OnKey(Plain(CodeTab))
	typing(app, "archive")
	app.OnKey(Plain(CodeTab))
	wantNoAction(t, app.OnKey(Plain(CodeEnter)))
	want := []cmd.Step{{
		Keys:   []string{"aaaa1111", "bbbb2222"},
		Change: cmd.Change{Tags: []string{"-work", "+archive"}, Touch: cmd.TouchStamp},
	}}
	if len(app.Queue) != 1 || !slices.Equal(app.Queue[0].Keys, want[0].Keys) ||
		!slices.Equal(app.Queue[0].Change.Tags, want[0].Change.Tags) || app.Queue[0].Change.Touch != cmd.TouchStamp {
		t.Fatalf("queue %+v", app.Queue)
	}
	// Queueing keeps the marks, for the next change to the same set.
	wantInt(t, "marks", len(app.Marks), 2)
}

func TestWithNothingMarkedTheSameKeyStillActsAtOnce(t *testing.T) {
	app := anApp()
	app.OnKey(Rune('#'))
	typing(app, "archive")
	app.OnKey(Plain(CodeTab))
	if action := app.OnKey(Plain(CodeEnter)); action == nil || action.Kind != ActTag {
		t.Fatalf("action %+v", action)
	}
	wantInt(t, "queue", len(app.Queue), 0)
}

func TestAQueuedDeleteAsksNothingUntilItIsSent(t *testing.T) {
	app := anApp()
	mark(app, "aaaa1111", "cccc3333")

	wantNoAction(t, app.OnKey(CtrlKey('d')))
	wantMode(t, app, ModeBrowse)
	wantInt(t, "queued deletions", app.QueuedDeletions(), 2)

	app.OnKey(Rune('Q'))
	wantNoAction(t, app.OnKey(Plain(CodeEnter)))
	wantMode(t, app, ModeConfirm)
	if app.Confirming != WhatSend {
		t.Fatal("not a send")
	}

	sent := app.OnKey(Rune('y'))
	if sent == nil || sent.Kind != ActSend || len(sent.Steps) != 1 {
		t.Fatalf("sent %+v", sent)
	}
}

func TestAQueueOfTagsGoesWithoutBeingAskedAbout(t *testing.T) {
	app := anApp()
	mark(app, "aaaa1111")
	tagWith(app, "archive")

	app.OnKey(Rune('Q'))
	if action := app.OnKey(Plain(CodeEnter)); action == nil || action.Kind != ActSend {
		t.Fatalf("action %+v", action)
	}
}

func TestAnEntryCanBeDroppedFromTheQueue(t *testing.T) {
	app := anApp()
	mark(app, "aaaa1111")
	for _, tag := range []string{"one", "two", "three"} {
		tagWith(app, tag)
	}
	wantInt(t, "queue", len(app.Queue), 3)

	app.OnKey(Rune('Q'))
	app.OnKey(Rune('j'))
	app.OnKey(Rune('d'))
	wantInt(t, "queue", len(app.Queue), 2)
	for _, step := range app.Queue {
		if step.Describe() == "tag: +two (1 note)" {
			t.Fatal("the wrong entry went")
		}
	}
	wantMode(t, app, ModeQueue)

	app.OnKey(Plain(CodeEsc))
	wantMode(t, app, ModeBrowse)
}

func TestATagThatCannotBeWrittenDownNeverReachesTheQueue(t *testing.T) {
	app := anApp()
	mark(app, "aaaa1111")
	app.OnKey(Rune('#'))
	// Refused where typed, before cmd.Check at the queue could see it.
	typing(app, "q3,urgent")
	proposal := app.Proposal()
	if proposal == nil || proposal.Refused == "" {
		t.Fatal("a comma is not a tag")
	}
	if !strings.Contains(proposal.Refused, "cannot contain") {
		t.Fatal(proposal.Refused)
	}

	app.OnKey(Plain(CodeTab))
	wantNoAction(t, app.OnKey(Plain(CodeEnter)))
	wantInt(t, "queue", len(app.Queue), 0)
	wantMode(t, app, ModeBrowse)
}

func TestSendingSpendsTheQueueAndTheMarks(t *testing.T) {
	app := anApp()
	mark(app, "aaaa1111")
	tagWith(app, "archive")

	app.Sent()
	wantInt(t, "queue", len(app.Queue), 0)
	wantInt(t, "marks", len(app.Marks), 0)
}

func TestAMarkOnANoteThatHasGoneGoesWithIt(t *testing.T) {
	app := anApp()
	mark(app, "aaaa1111", "bbbb2222")
	app.Replace(aSession(aStatus(), []notebook.NoteFile{
		aNote("bbbb2222", "meeting-notes", "Meeting notes", []string{"work"}, "agenda"),
	}))
	wantInt(t, "marks", len(app.Marks), 1)
	if !app.Marked("bbbb2222") {
		t.Fatal("lost the surviving mark")
	}
}

func TestANoteJustMadeCanBeFoundById(t *testing.T) {
	app := anApp()
	if slices.Contains(app.IDs(), "dddd4444") {
		t.Fatal("already there")
	}

	// By slug it lands away from the cursor.
	app.Replace(aSession(aStatus(), []notebook.NoteFile{
		aNote("aaaa1111", "budget-review", "Budget review", []string{"work"}, "the q3 budget"),
		aNote("bbbb2222", "meeting-notes", "Meeting notes", []string{"work"}, "x"),
		aNote("dddd4444", "trip-plan", "Trip plan", nil, "flights"),
	}))
	wantSelected(t, app, "aaaa1111")
	app.SelectID("dddd4444")
	wantSelected(t, app, "dddd4444")
}
