package tui

import (
	"fmt"
	"slices"
	"testing"

	"github.com/henry40408/noda/internal/cmd"
	"github.com/henry40408/noda/internal/notebook"
)

// These tests pin the boundaries of app.go, app_keys.go and app_prompt.go: the
// first and last row, the empty list, the clamp of an index one past the end.

func notesNamed(count int) []notebook.NoteFile {
	notes := make([]notebook.NoteFile, count)
	for i := range notes {
		id := fmt.Sprintf("id%02d", i)
		notes[i] = aNote(id, "slug-"+id, "Title "+id, nil, "body")
	}
	return notes
}

func anEmptyApp() *App { return NewApp("personal", "/notebook", Session{}) }

// putContent is what the runtime does on a screen that needs a fetch, without
// the runtime.
func putContent(app *App, content Content) {
	app.loaded, app.loadedView = &content, app.top().View
}

func TestAChoiceOffersOnlyTheMarksThatWouldChangeSomething(t *testing.T) {
	cases := []struct {
		held, total int
		want        []Mark
	}{
		{held: 0, total: 3, want: []Mark{MarkLeave, MarkAdd}},
		{held: 3, total: 3, want: []Mark{MarkLeave, MarkRemove}},
		{held: 1, total: 3, want: []Mark{MarkLeave, MarkAdd, MarkRemove}},
	}
	for _, c := range cases {
		got := Choice{Held: c.held}.states(c.total)
		if !slices.Equal(got, c.want) {
			t.Errorf("held %d of %d: states %v, want %v", c.held, c.total, got, c.want)
		}
	}
}

func TestATypoIsRecognisedWhenOneTagIsTheEndOfTheOther(t *testing.T) {
	// A whole tag matching the end of the other leaves nothing to compare; the
	// scan must stop there, not read before the start of the shorter one.
	for _, pair := range [][2]string{{"b", "ab"}, {"ab", "b"}, {"ab", "abc"}, {"abc", "ab"}} {
		if !oneEditApart(pair[0], pair[1]) {
			t.Errorf("%q and %q are one keystroke apart", pair[0], pair[1])
		}
	}
	if oneEditApart("abcd", "axyd") {
		t.Error("two changed characters in the middle are not one keystroke")
	}
}

func TestAbsDropsTheSign(t *testing.T) {
	for in, want := range map[int]int{-3: 3, -1: 1, 0: 0, 1: 1, 3: 3} {
		wantInt(t, fmt.Sprintf("abs(%d)", in), abs(in), want)
	}
}

func TestAFreshSessionIsNotWalkingTheHistory(t *testing.T) {
	wantInt(t, "historyAt", anApp().historyAt, -1)
}

func TestAReloadFollowsTheNoteToTheTopOfTheListing(t *testing.T) {
	app := anApp()
	app.OnKey(Rune('j'))
	wantSelected(t, app, "bbbb2222")
	notes := aSession(aStatus(), []notebook.NoteFile{
		aNote("bbbb2222", "meeting-notes", "Meeting notes", nil, "agenda"),
		aNote("cccc3333", "reading-list", "Reading list", nil, "a book"),
	})
	app.Replace(notes)
	wantSelected(t, app, "bbbb2222")
}

func TestAScreenAboutTheNotebooksLogSurvivesAReload(t *testing.T) {
	app := anApp()
	cases := []struct {
		name string
		view View
		want bool
	}{
		{"the notebook's own log", LogView(""), true},
		{"the log of a note that is there", LogView("aaaa1111"), true},
		{"the log of a note that has gone", LogView("gone0000"), false},
	}
	for _, c := range cases {
		if got := app.screenSurvives(c.view); got != c.want {
			t.Errorf("%s survives = %v, want %v", c.name, got, c.want)
		}
	}
}

func TestACursorPastTheLastNoteSelectsNothing(t *testing.T) {
	app := anApp()
	app.TableState().Selected = 3
	if file := app.Selected(); file != nil {
		t.Fatalf("selected %q past the end", file.ID)
	}
}

func TestACursorPastTheLastTaskSelectsNoNote(t *testing.T) {
	app := anApp()
	app.open(View{Kind: ViewTodo})
	putContent(app, Content{Kind: ContentTodo, Tasks: []Task{{Note: 1}}})
	app.TableState().Selected = 0
	if file := app.Selected(); file == nil || file.ID != "bbbb2222" {
		t.Fatalf("the first task's note = %v", file)
	}
	app.TableState().Selected = 1
	if file := app.Selected(); file != nil {
		t.Fatalf("selected %q past the last task", file.ID)
	}
}

func TestACursorPastTheLastBacklinkSelectsNoNote(t *testing.T) {
	app := anApp()
	app.open(BacklinksView(Subject{Name: "aaaa1111"}))
	putContent(app, Content{Kind: ContentBacklinks, Linking: []int{2}})
	app.TableState().Selected = 0
	if file := app.Selected(); file == nil || file.ID != "cccc3333" {
		t.Fatalf("the first backlink's note = %v", file)
	}
	app.TableState().Selected = 1
	if file := app.Selected(); file != nil {
		t.Fatalf("selected %q past the last backlink", file.ID)
	}
}

func TestNoteAtIsNilOneOutsideTheNotes(t *testing.T) {
	app := anApp()
	if app.NoteAt(0) == nil || app.NoteAt(2) == nil {
		t.Fatal("the first and last notes are there")
	}
	if app.NoteAt(3) != nil || app.NoteAt(-1) != nil {
		t.Fatal("one outside the notes is nothing")
	}
}

func TestAnEmptyListingStillHasRows(t *testing.T) {
	if !anEmptyApp().HasRows() {
		t.Fatal("a listing with no notes is a list, not a block of text")
	}
}

func TestSelectIDReachesTheFirstNote(t *testing.T) {
	app := anApp()
	app.OnKey(Rune('G'))
	app.SelectID("aaaa1111")
	wantSelected(t, app, "aaaa1111")
}

func TestAnEmptyListAndAnEmptyScreenHaveNoCursor(t *testing.T) {
	app := anEmptyApp()
	wantInt(t, "empty listing cursor", app.TableState().Selected, -1)
	app.open(View{Kind: ViewTodo})
	wantInt(t, "empty todo cursor", app.TableState().Selected, -1)
}

func TestTheCursorIsClampedToTheLastRow(t *testing.T) {
	app := anApp()
	app.cursorTo(10)
	wantInt(t, "cursorTo", app.TableState().Selected, 2)
	app.selectRow(0)
	app.selectRow(10)
	wantInt(t, "selectRow", app.listing().Table.Selected, 2)
}

func TestScrollingAnEmptyListLeavesTheOffsetAlone(t *testing.T) {
	app := anEmptyApp()
	app.top().Scroll = 3
	app.step(1)
	wantInt(t, "scroll after step", app.Scroll(), 3)
	app.jump(edgeFirst)
	wantInt(t, "scroll after jump", app.Scroll(), 3)
}

func TestCtrlFMovesByHalfAPage(t *testing.T) {
	app := NewApp("personal", "/notebook", aSession(aStatus(), notesNamed(30)))
	app.SetPage(10)
	app.OnKey(CtrlKey('f'))
	wantInt(t, "cursor after Ctrl-f", app.TableState().Selected, 5)
	app.OnKey(CtrlKey('b'))
	wantInt(t, "cursor after Ctrl-b", app.TableState().Selected, 0)
}

func TestNineTagsMakeKeyNineScopeTheNinthTag(t *testing.T) {
	tags := []string{"t1", "t2", "t3", "t4", "t5", "t6", "t7", "t8", "t9"}
	app := NewApp("personal", "/notebook", aSession(aStatus(), []notebook.NoteFile{
		aNote("aaaa1111", "all", "All", tags, "body"),
	}))
	app.OnKey(Rune('9'))
	if app.Search() == "" {
		t.Fatal("key 9 scopes the listing by the ninth tag")
	}
}

func TestEnterOnARowPastTheEndOpensNothing(t *testing.T) {
	cases := []struct {
		name  string
		setup func(app *App)
	}{
		{"tags", func(app *App) {
			app.open(View{Kind: ViewTags})
		}},
		{"files", func(app *App) {
			app.files = []string{"a.png"}
			app.open(View{Kind: ViewFiles})
		}},
		{"notebooks", func(app *App) {
			app.open(View{Kind: ViewNotebooks})
		}},
		{"deleted", func(app *App) {
			app.open(View{Kind: ViewDeleted})
			putContent(app, Content{Kind: ContentDeleted, Gone: []notebook.Deleted{{ID: "gone0000"}}})
		}},
		{"log", func(app *App) {
			app.open(LogView("aaaa1111"))
			putContent(app, Content{Kind: ContentLog, Entries: []notebook.Entry{{ID: "abcdef123"}}})
		}},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			app := anApp()
			c.setup(app)
			depth := app.Depth()
			app.TableState().Selected = app.rowsHere()
			wantNoAction(t, app.OnKey(Plain(CodeEnter)))
			wantMode(t, app, ModeBrowse)
			wantInt(t, "depth", app.Depth(), depth)
		})
	}
}

func TestComposingAPromptLineStopsWalkingTheHistory(t *testing.T) {
	app := anApp()
	app.historyAt = 0
	app.compose("restore x y")
	wantInt(t, "historyAt", app.historyAt, -1)
}

func TestQueueKeysStayInsideTheQueue(t *testing.T) {
	steps := func() []cmd.Step {
		return []cmd.Step{{Keys: []string{"first"}}, {Keys: []string{"second"}}}
	}
	t.Run("opening clamps a cursor left past the end", func(t *testing.T) {
		app := anApp()
		app.Queue, app.queueAt = steps(), 5
		app.OnKey(Rune('Q'))
		wantInt(t, "queueAt", app.queueAt, 1)
	})
	t.Run("down stops on the last entry", func(t *testing.T) {
		app := anApp()
		app.Queue, app.queueAt, app.Mode = steps(), 1, ModeQueue
		app.OnKey(Rune('j'))
		wantInt(t, "queueAt", app.queueAt, 1)
	})
	t.Run("dropping from an empty queue does nothing", func(t *testing.T) {
		app := anApp()
		app.Mode = ModeQueue
		app.OnKey(Rune('d'))
		wantInt(t, "queue length", len(app.Queue), 0)
	})
	t.Run("dropping the last entry moves the cursor up", func(t *testing.T) {
		app := anApp()
		app.Queue, app.queueAt, app.Mode = steps(), 1, ModeQueue
		app.OnKey(Rune('d'))
		wantInt(t, "queue length", len(app.Queue), 1)
		wantInt(t, "queueAt", app.queueAt, 0)
	})
}

func TestTheCommandPaletteHandlesAnEmptyMatch(t *testing.T) {
	app := anApp()
	app.OnKey(CtrlKey('a'))
	typing(app, "zzzz")
	wantNoAction(t, app.OnKey(Plain(CodeEnter)))
	wantMode(t, app, ModeBrowse)
}

func TestChoosingFromThePaletteStopsWalkingTheHistory(t *testing.T) {
	app := anApp()
	app.OnKey(CtrlKey('a'))
	app.historyAt = 2
	app.OnKey(Plain(CodeEnter))
	wantMode(t, app, ModeCommand)
	wantInt(t, "historyAt", app.historyAt, -1)
}

func TestRunningAPromptLineStopsWalkingTheHistory(t *testing.T) {
	app := anApp()
	app.OnKey(Rune(':'))
	app.historyAt = 2
	app.OnKey(Plain(CodeEnter))
	wantInt(t, "historyAt", app.historyAt, -1)
}

func TestLogWithNoKeyOpensTheLogItself(t *testing.T) {
	app := anApp()
	wantNoAction(t, commandLine(app, "log"))
	if app.View().Kind != ViewLog {
		t.Fatalf("view %v, want the log", app.View())
	}
}

func TestTagWithNothingAfterItIsRefused(t *testing.T) {
	app := anApp()
	wantNoAction(t, commandLine(app, "tag"))
	if app.Message == nil || !app.Message.Failed {
		t.Fatal("a tag command with no change is refused")
	}
}

func openPicker(app *App) {
	app.OnKey(Rune('#'))
}

func TestThePickerCursorStopsOnTheLastRow(t *testing.T) {
	app := anApp()
	openPicker(app)
	wantInt(t, "picker rows", app.PickerRows(), 2)
	repeatKey(app, Plain(CodeDown), 5)
	wantInt(t, "cursor", app.TagsAt(), 1)
}

func TestOnlyTypingOrErasingSendsThePickerCursorHome(t *testing.T) {
	app := anApp()
	openPicker(app)
	app.OnKey(Plain(CodeDown))
	app.OnKey(Plain(CodeLeft))
	wantInt(t, "cursor after a key that edits nothing", app.TagsAt(), 1)
	app.OnKey(Rune('w'))
	wantInt(t, "cursor after typing", app.TagsAt(), 0)
}

func TestTheNearestTagIsTheCommonestAndNeverAnUnusedOne(t *testing.T) {
	app := anApp()
	app.choices = []Choice{{Tag: "cat", Notes: 2}, {Tag: "cut", Notes: 2}, {Tag: "cot", Notes: 0}}
	tag, notes := app.nearest("cit")
	wantText(t, "nearest tag", tag, "cut")
	wantInt(t, "its notes", notes, 2)
	app.choices = []Choice{{Tag: "cat", Notes: 0}}
	tag, _ = app.nearest("cit")
	wantText(t, "nearest of an unused tag", tag, "")
}

func TestBlameWithNoKeyOpensBlameOfTheNoteUnderTheCursor(t *testing.T) {
	app := anApp()
	wantNoAction(t, commandLine(app, "blame"))
	if app.View() != BlameView("aaaa1111") {
		t.Fatalf("view %v, want the blame of the first note", app.View())
	}
}
