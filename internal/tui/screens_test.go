package tui

// The TUI end to end: a real notebook, real keystrokes, and the frame drawn
// into a buffer. The state machine's own tests are in app_*_test.go.

import (
	"context"
	"errors"
	"path/filepath"
	"strings"
	"testing"

	"github.com/henry40408/noda/internal/cmd"
	"github.com/henry40408/noda/internal/config"
	"github.com/henry40408/noda/internal/gitx"
)

// barRune is matched at the start of the line, which is otherwise the body's
// left padding.
const barRune = "▌"

func TestTheListingNamesANoteTheWayEveryOtherListingDoes(t *testing.T) {
	env := threeNotes(t)
	app := load(t, env)
	rows := screen(env, app)

	// The id is minted, so a literal cannot stand in for it.
	id := app.Selected().ID
	if !hasLineWith(rows, id, "Budget review", "[work]") ||
		!hasLineWith(rows, "Meeting notes", "[work, q3]") ||
		!hasLineWith(rows, "Reading list") ||
		hasLineWith(rows, "Reading list", "[]") {
		t.Fatalf("%q", rows)
	}
}

func TestTheHeaderSaysWhereTheNotebookStands(t *testing.T) {
	env := threeNotes(t)
	app := load(t, env)
	head := screen(env, app)[:headerHeight]

	// A fresh notebook's branch follows init.defaultBranch, so it is read back
	// rather than named.
	for _, want := range [][]string{
		{"Notebook:", config.DefaultNotebook}, {"Branch:", app.Status.Branch}, {"Remote:", "none"},
		{"Notes:", "3 notes"}, {"Changes:", "none"}, {"<enter>", "read"}, {"<ctrl-d>", "delete"},
	} {
		if !hasLineWith(head, want...) {
			t.Errorf("no header line with %q in %q", want, head)
		}
	}
}

func TestTheHeaderHoldsStillWhileTheSessionChangesUnderneathIt(t *testing.T) {
	env := threeNotes(t)
	app := load(t, env)
	before := screen(env, app)[:headerHeight]

	// Marks and queue go on the title band: in the header they widened it and
	// dropped the rightmost column, hiding the marking keys.
	app.OnKey(Rune('*'))
	app.OnKey(Rune('T'))
	tagWith(app, "archive")

	after := screen(env, app)
	if strings.Join(after[:headerHeight], "\n") != strings.Join(before, "\n") {
		t.Fatalf("the header moved:\n%q\n%q", before, after[:headerHeight])
	}
	if !hasLineWith(after, "3 marks", "1 queued") || !hasLineWith(after[:headerHeight], "<space>", "mark") {
		t.Fatalf("%q", after)
	}
}

func TestTheWayToEverythingElseIsOnTheHeaderOfEveryScreen(t *testing.T) {
	env := threeNotes(t)
	app := load(t, env)

	// 80 columns is where the grid starts dropping its rightmost column.
	listing := screenAt(env, app, 80, 28)[:headerHeight]
	if !hasLineWith(listing, "<:>", "command") || !hasLineWith(listing, "<ctrl-a>", "commands") {
		t.Fatalf("%q", listing)
	}
	app.OnKey(key(CodeEnter))
	note := screenAt(env, app, 80, 28)[:headerHeight]
	for _, want := range [][]string{{"<:>", "command"}, {"<ctrl-a>", "commands"}, {"<esc>", "back"}, {"<q>", "quit"}} {
		if !hasLineWith(note, want...) {
			t.Errorf("no %q in %q", want, note)
		}
	}
}

func TestEveryScreenNamesItsColumnsAlongTheTop(t *testing.T) {
	env := threeNotes(t)
	app := load(t, env)

	if listing := screen(env, app); !hasLineWith(listing, "ID", "TITLE", "TAGS") {
		t.Fatalf("%q", listing)
	}
	// created and updated are the same width, so which is which must be labelled.
	app.OnKey(CtrlKey('w'))
	if wide := screen(env, app); !hasLineWith(wide, "SLUG", "CREATED", "UPDATED") {
		t.Fatalf("%q", wide)
	}
}

func barred(rows []string) []string {
	var out []string
	for _, row := range rows {
		if strings.HasPrefix(row, barRune) {
			out = append(out, row)
		}
	}
	return out
}

func TestTheRowUnderTheCursorIsTheOnlyOneWearingTheBar(t *testing.T) {
	env := threeNotes(t)
	app := load(t, env)

	listing := screen(env, app)
	if got := barred(listing); len(got) != 1 || !strings.Contains(got[0], "Budget review") {
		t.Fatalf("%q", listing)
	}
	app.OnKey(Rune('j'))
	moved := screen(env, app)
	got := barred(moved)
	if len(got) != 1 || !strings.Contains(got[0], "Meeting notes") || strings.Contains(got[0], "TITLE") {
		t.Fatalf("%q", moved)
	}
}

func TestAListSaysHowMuchOfItIsOffTheScreenAndOnlyWhenSomeIs(t *testing.T) {
	env := threeNotes(t)
	app := load(t, env)

	// Room for all three: no scroll indicator.
	if roomy := screen(env, app); strings.Contains(strings.Join(roomy, "\n"), "█") {
		t.Fatalf("%q", roomy)
	}
	cramped := screenAt(env, app, 90, 7)
	if !strings.Contains(strings.Join(cramped, "\n"), "█") {
		t.Fatalf("%q", cramped)
	}
	// The indicator's column comes off the chrome, not off the row.
	if !hasLineWith(cramped, "Budget review", "[work]") {
		t.Fatalf("%q", cramped)
	}
}

func TestTheTitleBandSaysWhatTheScreenIsOf(t *testing.T) {
	env := threeNotes(t)
	app := load(t, env)
	if !hasLineWith(screen(env, app), "Notes  all  3") {
		t.Fatal("the listing's title band")
	}
	id := app.Selected().ID
	if note := opened(env, app); !hasLineWith(note, "Note  "+id, "Budget review") {
		t.Fatalf("%q", note)
	}
}

func TestANoteOpensOnAScreenOfItsOwnAndEscapeComesBack(t *testing.T) {
	env := threeNotes(t)
	app := load(t, env)

	if listing := screen(env, app); hasLineWith(listing, "the q3 budget is late") {
		t.Fatalf("%q", listing)
	}
	id := app.Selected().ID
	first := opened(env, app)
	if !hasLineWith(first, "the q3 budget is late") || !hasLineWith(first, "title: Budget review") || !hasLineWith(first, "notes", id) {
		t.Fatalf("%q", first)
	}
	app.OnKey(key(CodeEsc))
	back := screen(env, app)
	if !hasLineWith(back, "Meeting notes") || hasLineWith(back, "the q3 budget is late") {
		t.Fatalf("the note's body went with its screen: %q", back)
	}
	app.OnKey(Rune('j'))
	moved := opened(env, app)
	if !hasLineWith(moved, "# Agenda") || hasLineWith(moved, "the q3 budget is late") {
		t.Fatalf("%q", moved)
	}
}

func TestAQueryNarrowsTheListingAsItIsTyped(t *testing.T) {
	env := threeNotes(t)
	app := load(t, env)

	app.OnKey(Rune('/'))
	typing(app, "tag:q3")
	rows := screen(env, app)
	if !hasLineWith(rows, "Meeting notes") || hasLineWith(rows, "Budget review") || hasLineWith(rows, "Reading list") ||
		!hasLineWith(rows, "/tag:q3") || !hasLineWith(rows, "Notes  tag:q3  1") {
		t.Fatalf("%q", rows)
	}
}

func TestTheCursorIsDrawnWhereTheNextCharacterWouldGo(t *testing.T) {
	env := threeNotes(t)
	app := load(t, env)

	app.OnKey(Rune('/'))
	typing(app, "tag:q3")
	// Six characters plus the `/` in front of them.
	x, y := cursorAt(t, env, app)
	if x != 7 {
		t.Fatalf("x = %d", x)
	}
	app.OnKey(CtrlKey('a'))
	if x, gotY := cursorAt(t, env, app); x != 1 || gotY != y {
		t.Fatalf("ctrl-a: %d,%d", x, gotY)
	}
	app.OnKey(CtrlKey('f'))
	if x, gotY := cursorAt(t, env, app); x != 2 || gotY != y {
		t.Fatalf("ctrl-f: %d,%d", x, gotY)
	}
	if !hasLineWith(screen(env, app), "/tag:q3") {
		t.Fatal("the query is still shown")
	}
}

// CJK characters are two columns wide; a cursor that counted characters would
// sit inside the previous one.
func TestTheCursorCountsColumnsAndNotCharacters(t *testing.T) {
	env := threeNotes(t)
	app := load(t, env)

	app.OnKey(Rune('m'))
	for range "Budget review" {
		app.OnKey(key(CodeBackspace))
	}
	typing(app, "預算")
	wide, _ := cursorAt(t, env, app)
	app.OnKey(key(CodeLeft))
	back, _ := cursorAt(t, env, app)
	if wide-back != 2 {
		t.Fatalf("one character back is two columns back, got %d", wide-back)
	}
}

func TestAnUnfinishedQuerySaysWhyInsteadOfEmptyingTheScreen(t *testing.T) {
	env := threeNotes(t)
	app := load(t, env)

	app.OnKey(Rune('/'))
	// A bare `O` is a text: term, and one title has one.
	typing(app, "tag:work O")
	if good := screen(env, app); !hasLineWith(good, "Meeting notes") {
		t.Fatalf("%q", good)
	}
	typing(app, "R")
	unfinished := screen(env, app)
	if !hasLineWith(unfinished, "needs a term on both sides") || !hasLineWith(unfinished, "Meeting notes") {
		t.Fatalf("the last good result is still on screen: %q", unfinished)
	}
}

func TestAQueryThatMatchesNothingSaysSo(t *testing.T) {
	env := threeNotes(t)
	app := load(t, env)

	app.OnKey(Rune('/'))
	typing(app, "tag:archived")
	rows := screen(env, app)
	if !hasLineWith(rows, "nothing matches") || !hasLineWith(rows, "Notes  tag:archived  0") {
		t.Fatalf("%q", rows)
	}
}

func TestTheHelpCardListsTheKeysAndGoesAwayAgain(t *testing.T) {
	env := threeNotes(t)
	app := load(t, env)

	app.OnKey(Rune('?'))
	help := screen(env, app)
	// The filter example is the one entry that says what its key cannot.
	if !hasLineWith(help, "keys") || !hasLineWith(help, "quit") || !hasLineWith(help, "filter: tag:work OR tag:q3 budget") {
		t.Fatalf("%q", help)
	}
	app.OnKey(key(CodeEsc))
	if without := screen(env, app); hasLineWith(without, "first / last") {
		t.Fatalf("%q", without)
	}
}

func TestAReloadPicksUpANoteWrittenFromSomewhereElse(t *testing.T) {
	env := threeNotes(t)
	app := load(t, env)
	if app.Total() != 3 {
		t.Fatalf("total %d", app.Total())
	}
	if _, err := cmd.Add(env, cmd.AddOptions{Title: ptr("Trip plan"), Content: ptr("flights")}); err != nil {
		t.Fatal(err)
	}
	// The state machine returns the action; the test performs it as the runtime would.
	if action := app.OnKey(Rune('r')); action == nil || action.Kind != ActReload {
		t.Fatalf("%+v", action)
	}
	if err := Reload(env, app); err != nil {
		t.Fatal(err)
	}
	if rows := screen(env, app); !hasLineWith(rows, "Trip plan") {
		t.Fatalf("%q", rows)
	}
}

func TestATagLongEnoughToFillTheListingDoesNotTakeTheTitleWithIt(t *testing.T) {
	env := threeNotes(t)
	// The shape an import leaves: long enough to swallow a narrow listing.
	addTagged(t, env, "Ubuntu notes", "24.04 Dark patterns")
	app := load(t, env)
	// At 80 columns the title is no longer starved; the cap matters when narrower.
	rows := screenAt(env, app, 46, 28)

	// Uncapped, the tags take the row. The title keeps its floor.
	for _, title := range []string{"Budget rev", "Meeting no", "Reading li"} {
		if !hasLineWith(rows, title) {
			t.Errorf("no %q in %q", title, rows)
		}
	}
	if !hasLineWith(rows, "[24.04 Dark") {
		t.Fatalf("the long tag list still says it is there: %q", rows)
	}
}

func TestATagWithASpaceCanBeFilteredForFromTheScreenItIsOn(t *testing.T) {
	env := threeNotes(t)
	addTagged(t, env, "Ubuntu notes", "24.04 Dark patterns")
	app := load(t, env)

	app.OnKey(Rune('/'))
	// Quoted as a shell would, and kept in one piece.
	typing(app, `tag:"24.04 Dark patterns"`)
	rows := screen(env, app)
	if !hasLineWith(rows, `tag:"24.04 Dark patterns"`, "  1") {
		t.Fatalf("%q", rows)
	}
	if got := app.Selected(); got == nil || got.Note.Title != "Ubuntu notes" {
		t.Fatalf("%+v", got)
	}
}

func TestLeavingWithAQueueInHandIsAskedAbout(t *testing.T) {
	env := threeNotes(t)
	app := load(t, env)

	app.OnKey(Rune('*'))
	tagWith(app, "archive")

	if action := app.OnKey(Rune('q')); action != nil {
		t.Fatalf("not yet: %+v", action)
	}
	asked := screen(env, app)
	if !hasLineWith(asked, "leave the queue behind?") || !hasLineWith(asked, "1 change over 3 notes") || !hasLineWith(asked, "written down anywhere") {
		t.Fatalf("%q", asked)
	}
	app.OnKey(key(CodeEsc))
	if len(app.Queue) != 1 {
		t.Fatalf("%d queued", len(app.Queue))
	}
	app.OnKey(Rune('Q'))
	if action := app.OnKey(key(CodeEnter)); action == nil || action.Kind != ActSend {
		t.Fatalf("%+v", action)
	}
}

func TestAQueuedDeleteTakesEveryNoteItWasAimedAt(t *testing.T) {
	env := threeNotes(t)
	app := load(t, env)
	before := commits(t, env)

	app.OnKey(Rune('/'))
	typing(app, "tag:work")
	app.OnKey(key(CodeEnter))
	markAllShown(app)
	app.OnKey(CtrlKey('d'))

	app.OnKey(Rune('Q'))
	if queued := screen(env, app); !hasLineWith(queued, "rm: 2 notes") {
		t.Fatalf("%q", queued)
	}
	// Only the delete makes the send ask first.
	if action := app.OnKey(key(CodeEnter)); action != nil {
		t.Fatalf("%+v", action)
	}
	if asked := screen(env, app); !hasLineWith(asked, "send the queue?") || !hasLineWith(asked, "2 notes to be deleted") {
		t.Fatalf("%q", asked)
	}
	perform(t, env, app, app.OnKey(Rune('y')))

	app.OnKey(key(CodeEsc))
	after := screen(env, app)
	if app.Total() != 1 || !hasLineWith(after, "Reading list") || hasLineWith(after, "Budget review") {
		t.Fatalf("%d notes: %q", app.Total(), after)
	}
	if got := commits(t, env) - before; got != 1 {
		t.Fatalf("%d commits", got)
	}
}

func TestATagChosenOnTheCardIsOnTheNoteAfterwards(t *testing.T) {
	env := threeNotes(t)
	app := load(t, env)

	app.OnKey(Rune('#'))
	card := screen(env, app)
	if !hasLineWith(card, "tags: budget-review") || !hasLineWith(card, "[x] work") || !hasLineWith(card, "[ ] q3") {
		t.Fatalf("%q", card)
	}
	// A tag the notebook lacks is the row after the last one.
	typing(app, "urgent")
	if rows := screen(env, app); !hasLineWith(rows, "[ ] urgent", "new") {
		t.Fatalf("%q", rows)
	}
	app.OnKey(key(CodeTab))
	// `new`: a count of 0 would read as a tag that lost its notes.
	if rows := screen(env, app); !hasLineWith(rows, "[+] urgent", "new") {
		t.Fatalf("%q", rows)
	}
	perform(t, env, app, app.OnKey(key(CodeEnter)))

	// The status line is `noda tag`'s own answer. Read first: the next key clears it.
	after := screen(env, app)
	if id := app.Selected().ID; !hasLineWith(after, id, "budget-review", "urgent") {
		t.Fatalf("%q", after)
	}
	if file := opened(env, app); !hasLineWith(file, "tags: [work, urgent]") {
		t.Fatalf("%q", file)
	}
}

func TestATagWithASpaceInItCanBeRemovedFromTheScreenItIsOn(t *testing.T) {
	env := threeNotes(t)
	// An imported tag may contain a space.
	addTagged(t, env, "Ubuntu notes", "24.04 Dark patterns", "work")
	app := load(t, env)
	app.OnKey(Rune('G'))
	if got := app.Selected(); got == nil || got.Note.Title != "Ubuntu notes" {
		t.Fatalf("%+v", got)
	}
	// No quoting: `dark` filters the card down to the one row.
	app.OnKey(Rune('#'))
	typing(app, "dark")
	card := screen(env, app)
	if !hasLineWith(card, "[x] 24.04 Dark patterns") || hasLineWith(card, "[x] work") {
		t.Fatalf("and only that row: %q", card)
	}
	app.OnKey(key(CodeTab))
	if rows := screen(env, app); !hasLineWith(rows, "[-] 24.04 Dark patterns") {
		t.Fatalf("%q", rows)
	}
	perform(t, env, app, app.OnKey(key(CodeEnter)))

	after := opened(env, app)
	if !hasLineWith(after, "tags: [work]") || hasLineWith(after, "Dark patterns") {
		t.Fatalf("the spaced tag is gone and the other one is not: %q", after)
	}
}

func TestARetitleRenamesTheNoteAndKeepsTheCursorOnIt(t *testing.T) {
	env := threeNotes(t)
	app := load(t, env)
	id := app.Selected().ID

	app.OnKey(Rune('m'))
	for range "Budget review" {
		app.OnKey(key(CodeBackspace))
	}
	typing(app, "Quarterly plan")
	perform(t, env, app, app.OnKey(key(CodeEnter)))

	after := screen(env, app)
	if !hasLineWith(after, "Quarterly plan") || hasLineWith(after, "Budget review") {
		t.Fatalf("%q", after)
	}
	// The slug moved it; the cursor followed the id.
	if app.Selected().ID != id {
		t.Fatalf("the cursor left %s", id)
	}
	if file := opened(env, app); !hasLineWith(file, "title: Quarterly plan") {
		t.Fatalf("it is read from the file it is now in: %q", file)
	}
}

func TestADeleteIsAskedAboutAndThenCarriedOut(t *testing.T) {
	env := threeNotes(t)
	app := load(t, env)

	// Deleting is behind a modifier; the plain key only says where it went.
	app.OnKey(Rune('d'))
	if rows := screen(env, app); !hasLineWith(rows, "delete is Ctrl-d") || app.Total() != 3 {
		t.Fatalf("%q", rows)
	}
	app.OnKey(CtrlKey('d'))
	asked := screen(env, app)
	if !hasLineWith(asked, "delete this note?") || !hasLineWith(asked, "Budget review") || !hasLineWith(asked, "git revert brings it back") {
		t.Fatalf("%q", asked)
	}
	perform(t, env, app, app.OnKey(Rune('y')))

	after := screen(env, app)
	if hasLineWith(after, "Budget review") || app.Total() != 2 {
		t.Fatalf("%q", after)
	}
	// The cursor keeps the row, landing on the note that took its place.
	if got := app.Selected(); got == nil || got.Note.Title != "Meeting notes" {
		t.Fatalf("%+v", got)
	}
}

const backdated = "updated: 2019-03-04T05:06:07Z"

func TestTHoldsANotesOwnUpdatedThroughAChange(t *testing.T) {
	env := threeNotes(t)
	app := load(t, env)
	backdate(t, env, app)
	if got := updatedLine(t, env, app); got != backdated {
		t.Fatal(got)
	}
	if hasLineWith(screen(env, app), "keeping updated") {
		t.Fatal("the flag is off")
	}
	app.OnKey(Rune('T'))
	if !hasLineWith(screen(env, app), "keeping updated") {
		t.Fatal("a setting you cannot see is one you forget you left on")
	}
	app.OnKey(Rune('#'))
	typing(app, "urgent")
	app.OnKey(key(CodeTab))
	perform(t, env, app, app.OnKey(key(CodeEnter)))

	if after := opened(env, app); !hasLineWith(after, "tags: [work, urgent]") {
		t.Fatalf("the change went in: %q", after)
	}
	if got := updatedLine(t, env, app); got != backdated {
		t.Fatalf("the date the note came with is still the date it carries: %s", got)
	}
	app.OnKey(key(CodeEsc))

	app.OnKey(Rune('T'))
	if hasLineWith(screen(env, app), "keeping updated") {
		t.Fatal("the flag is off again")
	}
	// The note carries `urgent` now, so one Tab is the `-`.
	app.OnKey(Rune('#'))
	typing(app, "urgent")
	app.OnKey(key(CodeTab))
	perform(t, env, app, app.OnKey(key(CodeEnter)))
	if got := updatedLine(t, env, app); got == backdated {
		t.Fatal("updated did not move")
	}
}

func TestAChangeTheCommandRefusesIsReportedInItsOwnWords(t *testing.T) {
	env := threeNotes(t)
	app := load(t, env)

	// The card will not offer a tag the frontmatter cannot hold; the prompt can name one.
	perform(t, env, app, commandLine(app, "tag +q3,urgent"))

	after := screen(env, app)
	if !hasLineWith(after, "a tag cannot contain `,`") || !hasLineWith(after, "Budget review", "[work]") {
		t.Fatalf("the note is as it was: %q", after)
	}
}

func TestAQueueArrivesInTheHistoryAsOneCommit(t *testing.T) {
	env := threeNotes(t)
	app := load(t, env)
	before := commits(t, env)

	markAllShown(app)
	if !hasLineWith(screen(env, app), "3 marks") {
		t.Fatal("three marks")
	}
	// `work` is on two of the three, so Tab cycles to removing it from all.
	tagWith(app, "archive")
	app.OnKey(Rune('#'))
	typing(app, "work")
	app.OnKey(key(CodeTab))
	app.OnKey(key(CodeTab))
	if action := app.OnKey(key(CodeEnter)); action != nil {
		t.Fatalf("a queued change is not run: %+v", action)
	}
	if len(app.Queue) != 2 || !hasLineWith(screen(env, app), "2 queued") {
		t.Fatalf("%d queued", len(app.Queue))
	}
	app.OnKey(Rune('Q'))
	// The queue uses the commit message's wording.
	if queued := screen(env, app); !hasLineWith(queued, "tag: +archive (3 notes)") || !hasLineWith(queued, "tag: -work (3 notes)") {
		t.Fatalf("%q", queued)
	}
	perform(t, env, app, app.OnKey(key(CodeEnter)))

	if got := commits(t, env) - before; got != 1 {
		t.Fatalf("two changes over three notes, and one thing was done: %d commits", got)
	}
	after := screen(env, app)
	if !hasLineWith(after, "2 changes over 3 notes") || !hasLineWith(after, "[q3, archive]") {
		t.Fatalf("and the listing says so: %q", after)
	}
	if file := opened(env, app); !hasLineWith(file, "tags: [archive]") {
		t.Fatalf("%q", file)
	}
	app.OnKey(key(CodeEsc))
	if len(app.Queue) != 0 || len(app.Marks) != 0 {
		t.Fatalf("%d queued, %d marked", len(app.Queue), len(app.Marks))
	}
}

func TestMarksMadeUnderOneQuerySurviveTheNext(t *testing.T) {
	env := threeNotes(t)
	app := load(t, env)

	// Marked under a query the next search will not show.
	app.OnKey(Rune('/'))
	typing(app, "tag:q3")
	app.OnKey(key(CodeEnter))
	markAllShown(app)
	first := app.Selected().ID

	app.OnKey(key(CodeEsc))
	app.OnKey(Rune('/'))
	typing(app, "book")
	app.OnKey(key(CodeEnter))
	if app.Shown() != 1 || app.Selected().ID == first {
		t.Fatalf("the first note is not in this result: %d shown", app.Shown())
	}
	markAllShown(app)
	if len(app.Marks) != 2 {
		t.Fatalf("%d marks", len(app.Marks))
	}
	tagWith(app, "seen")
	app.OnKey(Rune('Q'))
	perform(t, env, app, app.OnKey(key(CodeEnter)))
	app.OnKey(key(CodeEsc))

	// A mark is a note picked out, not a query re-run when sending.
	if got := strings.Join(titlesTagged(app, "seen"), ","); got != "Meeting notes,Reading list" {
		t.Fatal(got)
	}
}

func TestACommandReachesWhatNoKeyDoes(t *testing.T) {
	env := threeNotes(t)
	app := load(t, env)

	// `noda status` has no key; this is why the prompt exists.
	perform(t, env, app, commandLine(app, "status"))

	// More than a line, so it goes on a card.
	after := screen(env, app)
	if !hasLineWith(after, "3 notes") || !(hasLineWith(after, "branch") || hasLineWith(after, "clean")) {
		t.Fatalf("%q", after)
	}
}

func TestANoteCanBeOpenedByNameFromThePrompt(t *testing.T) {
	env := threeNotes(t)
	app := load(t, env)

	perform(t, env, app, commandLine(app, "open reading-list"))

	rows := screen(env, app)
	if app.Depth() != 2 || !hasLineWith(rows, "Note  ", "Reading list") || !hasLineWith(rows, "a book") {
		t.Fatalf("depth %d: %q", app.Depth(), rows)
	}
}

func TestANameThatNamesNothingIsRefusedInTheNotebooksOwnWords(t *testing.T) {
	env := threeNotes(t)
	app := load(t, env)

	perform(t, env, app, commandLine(app, "open nowhere"))

	// The answer `noda show nowhere` gives, on a card because it is a refusal.
	after := screen(env, app)
	if !hasLineWith(after, "nowhere") || app.Depth() != 1 {
		t.Fatalf("nothing opened, depth %d: %q", app.Depth(), after)
	}
}

func TestATagCanBeChangedFromTheCommandLineToo(t *testing.T) {
	env := threeNotes(t)
	app := load(t, env)

	// Naming a note, which the `#` key cannot do.
	perform(t, env, app, commandLine(app, "tag reading-list +urgent"))

	app.OnKey(Rune('G'))
	if after := screen(env, app); !hasLineWith(after, "Reading list", "[urgent]") {
		t.Fatalf("%q", after)
	}
}

func TestTheCommandListIsNarrowedByWhatACommandDoes(t *testing.T) {
	env := threeNotes(t)
	app := load(t, env)

	app.OnKey(CtrlKey('a'))
	listed := screen(env, app)
	if !hasLineWith(listed, "commands") || !hasLineWith(listed, "open <note>") {
		t.Fatalf("%q", listed)
	}
	// Matches on descriptions, not only names.
	typing(app, "remote")
	narrowed := screen(env, app)
	if !hasLineWith(narrowed, "push") || !hasLineWith(narrowed, "pull") || hasLineWith(narrowed, "open <note>") {
		t.Fatalf("%q", narrowed)
	}
}

func TestALineThatIsNotACommandStaysOnTheLine(t *testing.T) {
	env := threeNotes(t)
	app := load(t, env)

	if action := commandLine(app, "frobnicate"); action != nil {
		t.Fatalf("%+v", action)
	}
	after := screen(env, app)
	if !hasLineWith(after, "frobnicate") || !hasLineWith(after, "Budget review") || app.Total() != 3 {
		t.Fatalf("%q", after)
	}
}

func TestItRefusesToRunWhereThereIsNoTerminal(t *testing.T) {
	env := threeNotes(t)
	if err := Browse(RunOptions{Env: env}); !errors.Is(err, ErrNeedsTerminal) || !strings.Contains(err.Error(), "needs a terminal") {
		t.Fatalf("a pipe is not a terminal: %v", err)
	}
}

func TestTheTodoScreenListsTheBoxesWithTheDatesThatHaveBeenMissed(t *testing.T) {
	env := workedNotebook(t)
	app := load(t, env)

	todo := goTo(t, env, app, "todo")
	if !hasLineWith(todo, "Todo", "  1") || !hasLineWith(todo, "meeting-notes", "2020-01-01", "book a room") {
		t.Fatalf("%q", todo)
	}
	app.OnKey(key(CodeEsc))
	app.OnKey(Rune('t'))
	if !hasLineWith(screen(env, app), "book a room") {
		t.Fatal("t opens the same screen")
	}
}

func TestTheTagsScreenCountsThemAndEnterNarrowsTheListing(t *testing.T) {
	env := workedNotebook(t)
	app := load(t, env)

	if tags := goTo(t, env, app, "tags"); !hasLineWith(tags, "work", "2 notes") {
		t.Fatalf("%q", tags)
	}
	// A tag narrows the listing rather than opening a screen of its own.
	app.OnKey(key(CodeEnter))
	narrowed := screen(env, app)
	if !hasLineWith(narrowed, "Notes  tag:work  2") || strings.Join(app.Crumbs(), ",") != "notes" {
		t.Fatalf("%v: %q", app.Crumbs(), narrowed)
	}
}

func TestTheBacklinksScreenFindsTheNoteThatPointsHere(t *testing.T) {
	env := workedNotebook(t)
	app := load(t, env)

	// The meeting notes link to the budget, which the cursor opens on.
	found := goTo(t, env, app, "backlinks")
	if !hasLineWith(found, "Backlinks  ") || !hasLineWith(found, "Meeting notes") {
		t.Fatalf("%q", found)
	}
	app.OnKey(key(CodeEnter))
	if note := screen(env, app); !hasLineWith(note, "Note  ", "Meeting notes") {
		t.Fatalf("%q", note)
	}
}

func TestTheFilesScreenLeadsToWhatUsesAnAttachment(t *testing.T) {
	env := workedNotebook(t)
	app := load(t, env)

	if files := goTo(t, env, app, "files"); !hasLineWith(files, "diagram.png") {
		t.Fatalf("%q", files)
	}
	// Nothing uses it: the orphan `doctor --links` reports, not an empty screen.
	app.OnKey(key(CodeEnter))
	if orphan := screen(env, app); !hasLineWith(orphan, "nothing links here") {
		t.Fatalf("%q", orphan)
	}
}

func TestTheLogScreenShowsTheNotebooksCommitsAndThenOneNotesOwn(t *testing.T) {
	env := workedNotebook(t)
	app := load(t, env)

	whole := goTo(t, env, app, "log")
	all := commits(t, env)
	if !hasLineWith(whole, "Log  "+config.DefaultNotebook) || len(app.Entries()) != all {
		t.Fatalf("%d of %d: %q", len(app.Entries()), all, whole)
	}
	// On a note, the log is only that note's.
	app.OnKey(key(CodeEsc))
	app.OnKey(key(CodeEnter))
	app.OnKey(Rune('l'))
	one := screen(env, app)
	if !hasLineWith(one, "Log  ", "Budget review") || len(app.Entries()) >= all {
		t.Fatalf("%q", one)
	}
}

// The chrome says `↑1`; this says which commit, with the same arrow and margin
// as `noda log`.
func TestTheLogScreenMarksWhatTheRemoteHasNotSeen(t *testing.T) {
	env := workedNotebook(t)
	branch, err := notebookOf(t, env).Branch()
	if err != nil {
		t.Fatal(err)
	}
	// `main` or `master` depending on the machine.
	parent := t.TempDir()
	bare := filepath.Join(parent, "origin.git")
	for _, step := range []struct {
		dir  string
		args []string
	}{
		{parent, []string{"init", "--bare", bare}},
		{bare, []string{"symbolic-ref", "HEAD", "refs/heads/" + branch}},
	} {
		if _, err := env.Git.Run(context.Background(), gitx.Cmd{Dir: step.dir, Args: step.args}); err != nil {
			t.Fatal(err)
		}
	}
	if _, err := cmd.RemoteSet(env, bare); err != nil {
		t.Fatal(err)
	}
	if _, err := cmd.Push(env); err != nil {
		t.Fatal(err)
	}
	addTagged(t, env, "Not sent yet")

	app := load(t, env)
	log := goTo(t, env, app, "log")
	if !hasLineWith(log, "↑", "not-sent-yet") {
		t.Fatalf("%q", log)
	}
	// Commits the remote has get a blank margin. The chrome's `↑1` has no space
	// after the arrow, so the count below does not match it.
	for _, row := range log {
		if strings.Contains(row, "initialize notebook") && strings.Contains(row, "↑") {
			t.Fatalf("a commit the remote has was marked: %q", log)
		}
	}
	marked := 0
	for _, row := range log {
		if strings.Contains(row, "↑ ") {
			marked++
		}
	}
	if marked != 1 {
		t.Fatalf("%d marked: %q", marked, log)
	}
	// `↑` is East Asian Ambiguous: a one-cell column clips a hex digit off the
	// row carrying it.
	for _, entry := range app.Entries() {
		if id := entry.ShortID(); !hasLineWith(log, id) {
			t.Fatalf("`%s` was clipped: %q", id, log)
		}
	}
}

// `p` is a toggle, so it reads the row before deciding.
func TestPPinsTheNoteUnderTheCursorAndTheSecondPressLetsItDown(t *testing.T) {
	env := threeNotes(t)
	app := load(t, env)

	app.OnKey(Rune('j'))
	id := app.Selected().ID
	action := app.OnKey(Rune('p'))
	if action == nil || action.Kind != ActPin || action.Key != id || !action.Pinned || action.Touch != cmd.TouchStamp {
		t.Fatalf("p pins: %+v", action)
	}
	perform(t, env, app, action)

	if app.Rows()[0].ID != id {
		t.Fatal("the pinned note leads")
	}
	if rows := screen(env, app); !hasLineWith(rows, id, "pinned") {
		t.Fatalf("%q", rows)
	}
	app.OnKey(Rune('g'))
	action = app.OnKey(Rune('p'))
	if action == nil || action.Kind != ActPin || action.Key != id || action.Pinned || action.Touch != cmd.TouchStamp {
		t.Fatalf("p unpins: %+v", action)
	}
	perform(t, env, app, action)
	if app.NoteOf(id).Note.IsPinned() {
		t.Fatal("the second press left it pinned")
	}
}
