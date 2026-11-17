package tui

import (
	"slices"
	"strings"
	"testing"

	"github.com/henry40408/noda/internal/cmd"
	"github.com/henry40408/noda/internal/notebook"
)

// command types a line at the `:` prompt and presses Enter.
func command(t *testing.T, app *App, line string) *Action {
	t.Helper()
	app.OnKey(Rune(':'))
	if app.Mode != ModeCommand {
		t.Fatal("`:` did not open the prompt")
	}
	typing(app, line)
	return app.OnKey(Plain(CodeEnter))
}

func wantMessage(t *testing.T, app *App, failed bool, contains string) {
	t.Helper()
	if app.Message == nil {
		t.Fatal("expected a message")
	}
	if app.Message.Failed != failed || !strings.Contains(app.Message.Text, contains) {
		t.Fatalf("message %+v, want failed=%v containing %q", app.Message, failed, contains)
	}
}

func wantCrumbs(t *testing.T, app *App, want ...string) {
	t.Helper()
	if got := app.Crumbs(); !slices.Equal(got, want) {
		t.Fatalf("crumbs %v, want %v", got, want)
	}
}

func wantView(t *testing.T, app *App, want View) {
	t.Helper()
	if app.View() != want {
		t.Fatalf("view %+v, want %+v", app.View(), want)
	}
}

func TestACommandAimsAtTheNoteOnScreenWhenItNamesNone(t *testing.T) {
	app := anApp()
	app.OnKey(Rune('j'))
	wantAction(t, command(t, app, "edit"), Action{Kind: ActEdit, Key: "bbbb2222", Touch: cmd.TouchStamp})
	wantMode(t, app, ModeBrowse)
}

func TestACommandCanNameTheNoteItIsAimedAt(t *testing.T) {
	app := anApp()
	wantAction(t, command(t, app, "edit cccc3333"), Action{Kind: ActEdit, Key: "cccc3333", Touch: cmd.TouchStamp})
}

func TestATagChangeIsToldFromANoteByTheSignInFrontOfIt(t *testing.T) {
	app := anApp()
	wantAction(t, command(t, app, "tag +urgent"), tagAction("aaaa1111", cmd.TouchStamp, "+urgent"))
	// A key cannot begin with `+` or `-`, so here the first is a note.
	wantAction(t, command(t, app, "tag cccc3333 -work +q3"), tagAction("cccc3333", cmd.TouchStamp, "-work", "+q3"))
	// Quoting works here too, with no shell in front of the field.
	wantAction(t, command(t, app, `tag -"24.04 Dark patterns"`), tagAction("aaaa1111", cmd.TouchStamp, "-24.04 Dark patterns"))
}

func TestAQueryTypedAtTheCommandLineKeepsItsQuotes(t *testing.T) {
	app := NewApp("personal", "/notebook", aSession(aStatus(), []notebook.NoteFile{
		aNote("aaaa1111", "ubuntu-notes", "Ubuntu notes", []string{"12.34 foo bar"}, "body"),
		aNote("bbbb2222", "other-note", "Other note", []string{"work"}, "foo bar"),
	}))
	wantNoAction(t, command(t, app, `notes tag:"12.34 foo bar"`))
	wantInt(t, "shown", app.Shown(), 1)
	wantText(t, "search", app.Search(), `tag:"12.34 foo bar"`)
}

func TestNotesComesBackUpTheStackAndNarrowsOnTheWay(t *testing.T) {
	app := anApp()
	readIt(t, app, "the q3 budget\n")
	wantInt(t, "depth", app.Depth(), 2)

	command(t, app, "notes tag:q3")
	wantInt(t, "depth", app.Depth(), 1)
	wantInt(t, "shown", app.Shown(), 1)
}

func TestACommandThatIsNotOneSaysSoOnTheLineAndDoesNothing(t *testing.T) {
	app := anApp()
	wantNoAction(t, command(t, app, "frobnicate the notebook"))
	// A line, not a card: nothing ran.
	wantMode(t, app, ModeBrowse)
	wantMessage(t, app, true, "frobnicate")
}

func TestACommandThatNeedsSomethingSaysWhatItTakes(t *testing.T) {
	app := anApp()
	wantNoAction(t, command(t, app, "open"))
	wantMessage(t, app, true, "open <note>")

	wantNoAction(t, command(t, app, "doctor --fix"))
	if app.Message == nil || !app.Message.Failed {
		t.Fatal("expected a failure")
	}
}

func TestOpeningByNameIsLeftToTheNotebookToAnswer(t *testing.T) {
	app := anApp()
	wantAction(t, command(t, app, "open meeting-notes"), Action{Kind: ActOpen, Key: "meeting-notes"})
	wantInt(t, "depth (nothing opens until the notebook answers)", app.Depth(), 1)
}

func TestACommandOnANoteIsAimedAtTheNoteItIsOn(t *testing.T) {
	app := anApp()
	readIt(t, app, "the q3 budget\n")
	wantAction(t, command(t, app, "mv Budget revision"),
		Action{Kind: ActRetitle, Key: "aaaa1111", Title: "Budget revision", Touch: cmd.TouchStamp})
}

func TestTheDeleteCommandAsksTheQuestionTheKeyAsks(t *testing.T) {
	app := anApp()
	wantNoAction(t, command(t, app, "rm"))
	wantMode(t, app, ModeConfirm)
	if app.Confirming != WhatDelete {
		t.Fatal("not a delete")
	}
	wantAction(t, app.OnKey(Rune('y')), Action{Kind: ActRemove, Key: "aaaa1111"})
}

func TestQuittingByNameAsksAboutTheQueueToo(t *testing.T) {
	app := anApp()
	mark(app, "aaaa1111")
	tagWith(app, "archive")

	wantNoAction(t, command(t, app, "quit"))
	wantMode(t, app, ModeConfirm)
	if app.Confirming != WhatQuit {
		t.Fatal("not a quit")
	}
}

func TestDoctorFromHereReportsAndChangesNothing(t *testing.T) {
	app := anApp()
	wantAction(t, command(t, app, "doctor"), Action{Kind: ActRun, Run: Run{Kind: RunDoctor}})
	wantAction(t, command(t, app, "doctor --links --times"),
		Action{Kind: ActRun, Run: Run{Kind: RunDoctor, Links: true, Times: true}})
}

func TestTheNetworkCommandsSayWhatIsBeingWaitedFor(t *testing.T) {
	app := anApp()
	sync := command(t, app, "sync")
	wantAction(t, sync, Action{Kind: ActRun, Run: Run{Kind: RunSync}})
	wantText(t, "working", sync.Working(), "syncing…")
	wantText(t, "working", Action{Kind: ActRun, Run: Run{Kind: RunStatus}}.Working(), "")
}

func TestThePromptRemembersWhatHasBeenTypedIntoIt(t *testing.T) {
	app := anApp()
	command(t, app, "status")
	command(t, app, "push")

	app.OnKey(Rune(':'))
	app.OnKey(Plain(CodeUp))
	wantText(t, "input", app.Input.Text(), "push")
	app.OnKey(Plain(CodeUp))
	wantText(t, "input", app.Input.Text(), "status")
	app.OnKey(Plain(CodeDown))
	wantText(t, "input", app.Input.Text(), "push")
	// Forward past the newest gives back an empty line.
	app.OnKey(Plain(CodeDown))
	if !app.Input.IsEmpty() {
		t.Fatal("input not empty")
	}
}

func TestAChordDoesNotTypeItsOwnLetterIntoTheCommandLine(t *testing.T) {
	app := anApp()
	app.OnKey(Rune(':'))
	typing(app, "stat")
	app.OnKey(CtrlKey('d'))
	app.OnKey(CtrlKey('a'))
	wantText(t, "input", app.Input.Text(), "stat")
	wantMode(t, app, ModeCommand)
}

func TestTheCommandListPutsWhatYouPickOnThePrompt(t *testing.T) {
	app := anApp()
	app.OnKey(CtrlKey('a'))
	wantMode(t, app, ModeCommands)

	typing(app, "snap")
	app.OnKey(Plain(CodeEnter))
	// Onto the prompt, not run, with a space as it takes an argument.
	wantMode(t, app, ModeCommand)
	wantText(t, "input", app.Input.Text(), "snapshot ")
}

func TestTheCommandListLeavesNothingBehindWhenItIsEscaped(t *testing.T) {
	app := anApp()
	app.OnKey(CtrlKey('a'))
	typing(app, "push")
	app.OnKey(Plain(CodeEsc))
	wantMode(t, app, ModeBrowse)
	if !app.Input.IsEmpty() {
		t.Fatal("input left")
	}
}

func TestTheCrumbsSayHowFarDownYouAre(t *testing.T) {
	app := anApp()
	wantCrumbs(t, app, "notes")

	readIt(t, app, "the q3 budget\n")
	wantCrumbs(t, app, "notes", "aaaa1111")

	app.OnKey(Plain(CodeEsc))
	wantCrumbs(t, app, "notes")
}

// aWorkingApp is a notebook whose notes carry checkboxes and links.
func aWorkingApp() *App {
	return NewApp("personal", "/notebook", aSession(aStatus(), []notebook.NoteFile{
		aNote("aaaa1111", "budget-review", "Budget review", []string{"work"},
			"- [ ] chase finance due:2026-08-01\n- [x] done already\n"),
		aNote("bbbb2222", "meeting-notes", "Meeting notes", []string{"work", "q3"},
			"see [the budget](aaaa1111-budget-review.md)\n\n- [ ] book a room\n"),
		aNote("cccc3333", "reading-list", "Reading list", []string{"q3"}, "nothing links from here"),
	}))
}

const (
	oidOnes   = "1111111111111111111111111111111111111111"
	oidTwos   = "2222222222222222222222222222222222222222"
	oidThrees = "3333333333333333333333333333333333333333"
	oidFours  = "4444444444444444444444444444444444444444"
)

func aCommit(hex string, seconds int64, summary string) notebook.Entry {
	return notebook.Entry{ID: hex, Seconds: seconds, Summary: summary}
}

func aBlameLine(text string) notebook.BlameLine {
	return notebook.BlameLine{Commit: oidOnes, Seconds: 1_770_000_000, Text: text}
}

func TestTListsEveryUntickedBoxSoonestFirst(t *testing.T) {
	app := aWorkingApp()
	app.OnKey(Rune('t'))
	wantView(t, app, View{Kind: ViewTodo})

	// The ticked one is not listed, as in `noda todo`.
	var said []string
	for _, task := range app.Tasks() {
		said = append(said, task.Item.Text)
	}
	if !slices.Equal(said, []string{"chase finance", "book a room"}) {
		t.Fatalf("tasks %v", said)
	}
	// Dated before undated, by todo.Compare.
	if due := app.Tasks()[0].Item.Due; due == nil || *due != "2026-08-01" {
		t.Fatalf("due %v", due)
	}
	if app.Tasks()[1].Item.Due != nil {
		t.Fatal("second is dated")
	}
}

func TestARowThatNamesANoteIsTheNoteTheKeysAimAt(t *testing.T) {
	app := aWorkingApp()
	app.OnKey(Rune('t'))
	wantSelected(t, app, "aaaa1111")
	app.OnKey(Rune('j'))
	wantSelected(t, app, "bbbb2222")

	wantAction(t, app.OnKey(Rune('e')), Action{Kind: ActEdit, Key: "bbbb2222", Touch: cmd.TouchStamp})

	app.OnKey(Plain(CodeEnter))
	wantView(t, app, NoteView("bbbb2222"))
	wantCrumbs(t, app, "notes", "todo", "bbbb2222")
}

func TestAScreenAboutTheNotebookHasNoNoteForTheKeysThatNeedOne(t *testing.T) {
	app := aWorkingApp()
	app.OnKey(Rune(':'))
	typing(app, "tags")
	app.OnKey(Plain(CodeEnter))
	wantView(t, app, View{Kind: ViewTags})
	if app.Selected() != nil {
		t.Fatal("selected")
	}
	// Not reaching past the screen for the listing's note.
	wantNoAction(t, app.OnKey(Rune('e')))
	wantNoAction(t, app.OnKey(CtrlKey('d')))
	wantMode(t, app, ModeBrowse)
}

func TestTheTagsAreCountedCommonestFirstAndEnterNarrowsTheListing(t *testing.T) {
	app := aWorkingApp()
	app.OnKey(Rune(':'))
	typing(app, "tags")
	app.OnKey(Plain(CodeEnter))

	want := []Tally{{"q3", 2}, {"work", 2}}
	if !slices.Equal(app.Tallies(), want) {
		t.Fatalf("tallies %v", app.Tallies())
	}

	app.OnKey(Plain(CodeEnter))
	wantView(t, app, View{Kind: ViewNotes})
	wantInt(t, "depth", app.Depth(), 1)
	wantText(t, "search", app.Search(), "tag:q3")
	wantInt(t, "shown", app.Shown(), 2)
}

func TestATagWithASpaceInItIsQuotedOnItsWayToTheQuery(t *testing.T) {
	app := NewApp("personal", "/notebook", aSession(aStatus(), []notebook.NoteFile{
		aNote("aaaa1111", "budget-review", "Budget review", []string{"24.04 Dark patterns"}, ""),
		aNote("bbbb2222", "meeting-notes", "Meeting notes", nil, ""),
	}))
	app.OnKey(Rune(':'))
	typing(app, "tags")
	app.OnKey(Plain(CodeEnter))
	app.OnKey(Plain(CodeEnter))

	// Unquoted this would be three terms and find nothing.
	wantText(t, "search", app.Search(), `tag:"24.04 Dark patterns"`)
	wantInt(t, "shown", app.Shown(), 1)
}

func linkingIDs(app *App) []string {
	var found []string
	for _, at := range app.Linking() {
		if file := app.NoteAt(at); file != nil {
			found = append(found, file.ID)
		}
	}
	return found
}

func TestBShowsWhatLinksToTheNoteInFrontOfYou(t *testing.T) {
	app := aWorkingApp()
	app.OnKey(Rune('b'))
	wantView(t, app, BacklinksView(Subject{Name: "aaaa1111"}))
	if got := linkingIDs(app); !slices.Equal(got, []string{"bbbb2222"}) {
		t.Fatalf("found %v", got)
	}

	// A note nothing links to opens on an empty list.
	app.OnKey(Plain(CodeEsc))
	app.OnKey(Rune('G'))
	app.OnKey(Rune('b'))
	wantInt(t, "linking", len(app.Linking()), 0)
	if app.Selected() != nil {
		t.Fatal("no row, so nothing to aim at")
	}
}

func TestLFollowsTheScreenTheWayLogItselfDoes(t *testing.T) {
	app := aWorkingApp()
	app.OnKey(Rune('l'))
	wantView(t, app, LogView(""))
	wantNeed(t, app.Wanted(), Need{Kind: NeedLog})

	app.OnKey(Plain(CodeEsc))
	readIt(t, app, "the q3 budget\n")
	app.OnKey(Rune('l'))
	wantView(t, app, LogView("aaaa1111"))
	wantNeed(t, app.Wanted(), Need{Kind: NeedLog, ID: "aaaa1111"})
}

func TestACommitInOneNotesHistoryWritesTheRestoreRatherThanRunningIt(t *testing.T) {
	app := aWorkingApp()
	readIt(t, app, "the q3 budget\n")
	app.OnKey(Rune('l'))
	supplied(t, app, Content{Kind: ContentLog, Entries: []notebook.Entry{
		aCommit(oidOnes, 1_770_000_000, "edit"),
		aCommit(oidTwos, 1_769_000_000, "add"),
	}})

	app.OnKey(Rune('j'))
	wantNoAction(t, app.OnKey(Plain(CodeEnter)))
	// On the prompt, waiting for a second Enter.
	wantMode(t, app, ModeCommand)
	wantText(t, "input", app.Input.Text(), "restore aaaa1111 2222222")

	wantAction(t, app.OnKey(Plain(CodeEnter)),
		Action{Kind: ActRestore, Key: "aaaa1111", Rev: "2222222", Touch: cmd.TouchStamp})
}

func TestTheNotebooksOwnLogHasNoNoteToRestore(t *testing.T) {
	app := aWorkingApp()
	app.OnKey(Rune('l'))
	supplied(t, app, Content{Kind: ContentLog, Entries: []notebook.Entry{aCommit(oidOnes, 1_770_000_000, "edit")}})
	app.OnKey(Plain(CodeEnter))
	wantMode(t, app, ModeBrowse)
	if !app.Input.IsEmpty() {
		t.Fatal("input not empty")
	}
}

func TestADeletedNoteOffersTheRevisionRestoreHasToBeGiven(t *testing.T) {
	app := aWorkingApp()
	app.OnKey(Rune(':'))
	typing(app, "deleted")
	app.OnKey(Plain(CodeEnter))
	wantNeed(t, app.Wanted(), Need{Kind: NeedDeleted})

	supplied(t, app, Content{Kind: ContentDeleted, Gone: []notebook.Deleted{{
		ID: "dddd4444", Slug: "trip-plan", Title: "Trip plan",
		RemovedIn: oidThrees, RestoreFrom: oidFours, RemovedAt: 1_770_000_000,
	}}})
	app.OnKey(Plain(CodeEnter))
	// The commit before the deletion, which is what `restore` wants.
	wantText(t, "input", app.Input.Text(), "restore dddd4444 4444444")
}

func TestAFileLeadsToWhatUsesIt(t *testing.T) {
	session := aSession(aStatus(), []notebook.NoteFile{
		aNote("aaaa1111", "budget-review", "Budget review", nil, "![the shape of it](diagram.png)\n"),
	})
	session.Files = []string{"diagram.png"}
	app := NewApp("personal", "/notebook", session)

	app.OnKey(Rune(':'))
	typing(app, "files")
	app.OnKey(Plain(CodeEnter))
	if !slices.Equal(app.Files(), []string{"diagram.png"}) {
		t.Fatalf("files %v", app.Files())
	}

	app.OnKey(Plain(CodeEnter))
	wantView(t, app, BacklinksView(Subject{File: true, Name: "diagram.png"}))
	if got := linkingIDs(app); !slices.Equal(got, []string{"aaaa1111"}) {
		t.Fatalf("an image counts as a use: %v", got)
	}
}

func TestMovingToAnotherNotebookWaitsForTheQueue(t *testing.T) {
	session := aSession(aStatus(), []notebook.NoteFile{aNote("aaaa1111", "a", "A", []string{"work"}, "")})
	session.Notebooks = []string{"personal", "work"}
	app := NewApp("personal", "/notebook", session)

	app.OnKey(Rune(':'))
	typing(app, "notebooks")
	app.OnKey(Plain(CodeEnter))
	// Not the one you are in.
	wantNoAction(t, app.OnKey(Plain(CodeEnter)))
	app.OnKey(Rune('j'))
	wantAction(t, app.OnKey(Plain(CodeEnter)), Action{Kind: ActUse, Key: "work"})

	app.Queue = append(app.Queue, cmd.Step{Keys: []string{"aaaa1111"}, Change: cmd.Change{Remove: true}})
	wantNoAction(t, app.OnKey(Plain(CodeEnter)))
	wantMessage(t, app, true, "personal")
}

func TestGoingBackToAScreenLandsWhereItWasLeft(t *testing.T) {
	app := aWorkingApp()
	app.OnKey(Rune('t'))
	app.OnKey(Rune('j'))
	wantSelected(t, app, "bbbb2222")

	// The todo list is derived again on the way back; the cursor survives.
	app.OnKey(Plain(CodeEnter))
	wantView(t, app, NoteView("bbbb2222"))
	app.OnKey(Plain(CodeEsc))
	wantView(t, app, View{Kind: ViewTodo})
	if row, ok := app.Row(); !ok || row != 1 {
		t.Fatalf("row %d %v", row, ok)
	}
	wantSelected(t, app, "bbbb2222")
}

func TestAScreenAboutANoteThatHasGoneIsClosedLikeTheNoteItself(t *testing.T) {
	app := aWorkingApp()
	readIt(t, app, "the q3 budget\n")
	app.OnKey(Rune('B'))
	wantView(t, app, BlameView("aaaa1111"))
	wantInt(t, "depth", app.Depth(), 3)

	app.Replace(aSession(aStatus(), []notebook.NoteFile{aNote("cccc3333", "reading-list", "Reading list", nil, "")}))
	wantInt(t, "depth", app.Depth(), 1)
	wantView(t, app, View{Kind: ViewNotes})
}

func TestAReloadWorksTheScreenOutAgainRatherThanLeavingItStale(t *testing.T) {
	app := aWorkingApp()
	app.OnKey(Rune('t'))
	wantInt(t, "tasks", len(app.Tasks()), 2)

	app.Replace(aSession(aStatus(), []notebook.NoteFile{
		aNote("aaaa1111", "budget-review", "Budget review", []string{"work"},
			"- [ ] chase finance\n- [ ] and the other thing\n"),
	}))
	wantView(t, app, View{Kind: ViewTodo})
	wantInt(t, "tasks (the new notebook's boxes, not the old)", len(app.Tasks()), 2)
	wantText(t, "task", app.Tasks()[1].Item.Text, "and the other thing")
}

func TestAnAnswerThatArrivesAfterTheReaderHasMovedOnIsDropped(t *testing.T) {
	app := aWorkingApp()
	app.OnKey(Rune('l'))
	asked := app.View()
	// Escape while the walk is still going.
	app.OnKey(Plain(CodeEsc))

	app.Supply(asked, Content{Kind: ContentLog, Entries: []notebook.Entry{aCommit(oidOnes, 1_770_000_000, "edit")}})
	wantView(t, app, View{Kind: ViewNotes})
	wantInt(t, "entries (it landed on the wrong screen)", len(app.Entries()), 0)
}

func TestANamedNoteIsResolvedByTheNotebookAndNotHere(t *testing.T) {
	app := aWorkingApp()
	app.OnKey(Rune(':'))
	typing(app, "blame meeting-notes")
	wantAction(t, app.OnKey(Plain(CodeEnter)), Action{Kind: ActShow, Key: "meeting-notes", Look: LookBlame})
	app.LookAt(LookBlame, "bbbb2222")
	wantView(t, app, BlameView("bbbb2222"))
}

func TestRestoreTakesANoteAndARevisionAndSaysSoWhenItHasNeither(t *testing.T) {
	app := aWorkingApp()
	app.OnKey(Rune(':'))
	typing(app, "restore aaaa1111")
	wantNoAction(t, app.OnKey(Plain(CodeEnter)))
	wantMessage(t, app, true, "<note> <rev>")
}

func TestAPageOfTextScrollsAndAListOfRowsDoesNot(t *testing.T) {
	app := aWorkingApp()
	app.OnKey(Rune('B'))
	supplied(t, app, Content{Kind: ContentBlame, Blame: []notebook.BlameLine{
		aBlameLine("one"), aBlameLine("two"), aBlameLine("three"),
	}})
	if app.HasRows() {
		t.Fatal("a blame has rows")
	}
	app.OnKey(Rune('j'))
	wantInt(t, "scroll", app.Scroll(), 1)
	app.OnKey(Rune('G'))
	wantInt(t, "scroll (its last line, and no further)", app.Scroll(), 2)

	app.OnKey(Plain(CodeEsc))
	app.OnKey(Rune('t'))
	if !app.HasRows() {
		t.Fatal("a todo has no rows")
	}
	app.OnKey(Rune('j'))
	wantInt(t, "scroll (a list moves a cursor, not a page)", app.Scroll(), 0)
	if row, ok := app.Row(); !ok || row != 1 {
		t.Fatalf("row %d", row)
	}
}

// dated is the note with created and updated set.
func dated(file notebook.NoteFile, created, updated string) notebook.NoteFile {
	file.Note.Created = &created
	file.Note.Updated = &updated
	return file
}

// aDatedNotes are three notes whose four orders all differ, so a test can tell
// which is in force.
func aDatedNotes() []notebook.NoteFile {
	return []notebook.NoteFile{
		dated(aNote("aaaa1111", "budget-review", "Zebra", []string{"work"}, ""), "2026-01-01T00:00:00Z", "2026-03-01T00:00:00Z"),
		dated(aNote("bbbb2222", "meeting-notes", "Apple", []string{"work", "q3"}, ""), "2026-02-01T00:00:00Z", "2026-01-01T00:00:00Z"),
		dated(aNote("cccc3333", "reading-list", "Mango", []string{"q3"}, ""), "2026-03-01T00:00:00Z", "2026-02-01T00:00:00Z"),
	}
}

func aDatedApp() *App {
	return NewApp("personal", "/notebook", aSession(aStatus(), aDatedNotes()))
}

func listed(app *App) []string {
	var ids []string
	for _, file := range app.Rows() {
		ids = append(ids, file.ID)
	}
	return ids
}

func wantListed(t *testing.T, app *App, want ...string) {
	t.Helper()
	if got := listed(app); !slices.Equal(got, want) {
		t.Fatalf("listed %v, want %v", got, want)
	}
}

func TestSWalksTheOrdersThatSortNames(t *testing.T) {
	app := aDatedApp()
	if app.Sort != cmd.SortSlug {
		t.Fatal("not slug")
	}
	wantListed(t, app, "aaaa1111", "bbbb2222", "cccc3333")

	// Newest first, as `--sort created`.
	app.OnKey(Rune('S'))
	if app.Sort != cmd.SortCreated {
		t.Fatal("not created")
	}
	wantListed(t, app, "cccc3333", "bbbb2222", "aaaa1111")

	app.OnKey(Rune('S'))
	if app.Sort != cmd.SortUpdated {
		t.Fatal("not updated")
	}
	wantListed(t, app, "aaaa1111", "cccc3333", "bbbb2222")

	app.OnKey(Rune('S'))
	if app.Sort != cmd.SortTitle {
		t.Fatal("not title")
	}
	wantListed(t, app, "bbbb2222", "cccc3333", "aaaa1111")

	app.OnKey(Rune('S'))
	if app.Sort != cmd.SortSlug {
		t.Fatal("round to where it started")
	}
}

func TestRTurnsWhicheverOrderIsInForce(t *testing.T) {
	app := aDatedApp()
	app.OnKey(Rune('R'))
	wantListed(t, app, "cccc3333", "bbbb2222", "aaaa1111")

	// Kept across a change of order, as `ls -r`.
	app.OnKey(Rune('S'))
	wantListed(t, app, "aaaa1111", "bbbb2222", "cccc3333")
}

func TestReorderingKeepsTheCursorOnTheNoteItWasOn(t *testing.T) {
	app := aDatedApp()
	app.OnKey(Rune('G'))
	wantSelected(t, app, "cccc3333")

	app.OnKey(Rune('S'))
	wantSelected(t, app, "cccc3333")
	if row, ok := app.Row(); !ok || row != 0 {
		t.Fatalf("row %d: it is the newest, so it is first now", row)
	}
}

func TestTheOrderSurvivesReadingTheNotebookAgain(t *testing.T) {
	app := aDatedApp()
	app.OnKey(Rune('S'))
	app.OnKey(Rune('R'))
	wantListed(t, app, "aaaa1111", "bbbb2222", "cccc3333")

	// A read comes back in walk order.
	app.Replace(aSession(aStatus(), aDatedNotes()))
	if app.Sort != cmd.SortCreated || !app.Reverse {
		t.Fatal("order lost")
	}
	wantListed(t, app, "aaaa1111", "bbbb2222", "cccc3333")
}

func TestTheQueryNarrowsWhateverOrderIsInForce(t *testing.T) {
	app := aDatedApp()
	app.OnKey(Rune('S'))
	app.OnKey(Rune('/'))
	typing(app, "tag:work")
	wantListed(t, app, "bbbb2222", "aaaa1111") // still newest first
}

func TestCtrlWIsTheListingsOwnDensityAndNoOtherScreens(t *testing.T) {
	app := aDatedApp()
	if app.Long {
		t.Fatal("long")
	}
	app.OnKey(CtrlKey('w'))
	if !app.Long {
		t.Fatal("not long")
	}
	app.OnKey(CtrlKey('w'))
	if app.Long {
		t.Fatal("long")
	}

	app.OnKey(Rune('t'))
	app.OnKey(CtrlKey('w'))
	if app.Long {
		t.Fatalf("%v has no wide row", app.View())
	}
}

func TestADigitNarrowsToOneOfTheCommonestTagsAndZeroLetsGo(t *testing.T) {
	app := aDatedApp()
	// Commonest first, ties alphabetical: `q3` and `work` both have two.
	app.OnKey(Rune('1'))
	wantText(t, "search", app.Search(), "tag:q3")
	wantInt(t, "shown", app.Shown(), 2)

	app.OnKey(Rune('2'))
	wantText(t, "search", app.Search(), "tag:work")

	// Past the end of the list: nothing.
	app.OnKey(Rune('9'))
	wantText(t, "search", app.Search(), "tag:work")

	app.OnKey(Rune('0'))
	wantText(t, "search", app.Search(), "")
	wantInt(t, "shown", app.Shown(), 3)
}

func TestADigitComesBackDownToTheListingFromWhereverItIsPressed(t *testing.T) {
	app := aDatedApp()
	readIt(t, app, "the q3 budget\n")
	app.OnKey(Rune('B'))
	wantInt(t, "depth", app.Depth(), 3)

	app.OnKey(Rune('1'))
	wantView(t, app, View{Kind: ViewNotes})
	wantInt(t, "depth", app.Depth(), 1)
	wantText(t, "search", app.Search(), "tag:q3")
}

func TestTheDigitsAreTheNumbersTheTagsScreenPutsBesideThem(t *testing.T) {
	app := aDatedApp()
	app.OnKey(Rune(':'))
	typing(app, "tags")
	app.OnKey(Plain(CodeEnter))

	second := app.Tallies()[1].Tag
	app.OnKey(Rune('2'))
	wantText(t, "search", app.Search(), "tag:"+second)
}

func TestCtrlGGivesTheCrumbRowBack(t *testing.T) {
	app := aDatedApp()
	if !app.CrumbsShown {
		t.Fatal("hidden")
	}
	app.OnKey(CtrlKey('g'))
	if app.CrumbsShown {
		t.Fatal("shown")
	}
	app.OnKey(CtrlKey('g'))
	if !app.CrumbsShown {
		t.Fatal("hidden")
	}
}
