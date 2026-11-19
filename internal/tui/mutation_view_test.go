package tui

// Boundary tests for the drawing: each asserts an exact width, count or
// column, so a mutated comparison or sum moves something visible.

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"unicode"

	"github.com/henry40408/noda/internal/cmd"
	"github.com/henry40408/noda/internal/notebook"
	"github.com/henry40408/noda/internal/tui/ui"
)

func viewSpansText(spans []ui.Span) string {
	var text strings.Builder
	for _, span := range spans {
		text.WriteString(span.Content)
	}
	return text.String()
}

func viewLinesText(line ui.Line) string { return viewSpansText(line.Spans) }

func viewRowsOf(frame *ui.Frame) []string {
	rows := make([]string, frame.Area().Height)
	for y := range rows {
		rows[y] = strings.TrimRightFunc(frame.Buffer.Row(y), unicode.IsSpace)
	}
	return rows
}

// viewCardRows is what a card drew, one string per row, without the margin.
func viewCardRows(rows []string) []string {
	var card []string
	for _, row := range rows {
		if trimmed := strings.TrimSpace(row); trimmed != "" {
			card = append(card, trimmed)
		}
	}
	return card
}

func viewCellOf(t *testing.T, frame *ui.Frame, text string) *ui.Cell {
	t.Helper()
	for y, row := range viewRowsOf(frame) {
		if at := strings.Index(row, text); at >= 0 {
			return frame.Buffer.At(len([]rune(row[:at])), y)
		}
	}
	t.Fatalf("%q is not on the screen", text)
	return nil
}

func TestAFieldStepsOverACharacterWhoseDecodingIsTheErrorRune(t *testing.T) {
	field := typed("a\ufffd")
	field.Key(Plain(CodeEnd))
	field.Key(Plain(CodeLeft))
	expectBefore(t, field, "a")
	field.Key(Plain(CodeHome))
	field.Key(Plain(CodeRight))
	field.Key(Plain(CodeRight))
	expectBefore(t, field, "a\ufffd")
}

func TestAWordForwardAtTheEndOfTheTextStaysThere(t *testing.T) {
	field := typed("budget")
	expectEdit(t, field.Key(AltKey('f')), Moved)
	expectBefore(t, field, "budget")
	expectEdit(t, field.Key(AltKey('d')), NoEdit)
	expectText(t, field, "budget")
}

func TestALogOfOneNoteOffersToRestoreItAndTheNotebooksLogDoesNot(t *testing.T) {
	one := keysFor(LogView("aaaa1111"))
	if one[0][0].key != "enter" || one[0][0].what != "restore it…" {
		t.Fatalf("a note's log opens with %+v", one[0][0])
	}
	all := keysFor(LogView(""))
	if all[0][0].key != "j/k" {
		t.Fatalf("the notebook's log opens with %+v", all[0][0])
	}
}

func TestTheHeaderShowsDriftOnlyWhileTheBranchHasMoved(t *testing.T) {
	for _, c := range []struct {
		drift *notebook.Drift
		want  string
	}{
		{nil, ""},
		{&notebook.Drift{}, ""},
		{&notebook.Drift{Ahead: 1}, "  ↑1 ↓0"},
		{&notebook.Drift{Behind: 1}, "  ↑0 ↓1"},
		{&notebook.Drift{Ahead: 2, Behind: 3}, "  ↑2 ↓3"},
	} {
		app := &App{}
		app.Status.Branch = "main"
		app.Status.Drift = c.drift
		got := viewLinesText(info(app)[1])
		if want := padRight("Branch:", labelWidth) + "main" + c.want; got != want {
			t.Fatalf("drift %+v drew %q, want %q", c.drift, got, want)
		}
	}
}

func TestTheMenuKeepsAColumnThatFitsTheRoomExactly(t *testing.T) {
	app := load(t, threeNotes(t))
	wanted := 0
	for _, pair := range keysFor(app.View())[0] {
		if pair.key != "" {
			wanted = max(wanted, runeCount(pair.key)+2+keyGap+runeCount(pair.what))
		}
	}
	wanted += menuGap
	if got := viewLinesText(menu(app, wanted)[0]); !strings.Contains(got, "<") {
		t.Fatalf("a column %d wide in %d: %q", wanted, wanted, got)
	}
	if got := viewLinesText(menu(app, wanted-1)[0]); got != "" {
		t.Fatalf("a column %d wide in %d: %q", wanted, wanted-1, got)
	}
}

func TestTheTitleOfAScreenWithNoCountHasNoNumber(t *testing.T) {
	env := workedNotebook(t)
	app := load(t, env)
	file := filepath.Join(notebookOf(t, env).Path, app.Selected().ID+"-budget-review.md")
	was, err := os.ReadFile(file)
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(file, append(was, "a line nobody committed\n"...), 0o666); err != nil {
		t.Fatal(err)
	}
	goTo(t, env, app, "diff")
	if got := viewSpansText(titleSpans(app)); got != "Diff" {
		t.Fatalf("the diff screen is titled %q", got)
	}
	app = load(t, env)
	app.OnKey(Rune('B'))
	id := app.View().Name
	want := "Blame" + gap + id + gap + app.NoteOf(id).Note.Title
	if got := viewSpansText(titleSpans(app)); got != want {
		t.Fatalf("the blame screen is titled %q, want %q", got, want)
	}
}

func TestTheTitleBandRulesOnlyTheRoomThatIsLeftAndKeepsTheSessionWhole(t *testing.T) {
	app := load(t, threeNotes(t))
	app.OnKey(Rune(' '))
	used := ui.LineOf(titleSpans(app)...).Width()
	held := viewSpansText(session(app))
	exact := used + len(held) + 2
	for _, c := range []struct {
		width int
		want  string
	}{
		{exact, viewSpansText(titleSpans(app)) + " " + held},
		{exact + 1, viewSpansText(titleSpans(app)) + " ─ " + held},
		{exact - 1, viewSpansText(titleSpans(app)) + " " + held},
	} {
		frame := ui.NewFrame(c.width, 1)
		drawTitle(frame, frame.Area(), app)
		if got := viewRowsOf(frame)[0]; got != c.want {
			t.Fatalf("width %d drew %q, want %q", c.width, got, c.want)
		}
	}
}

func TestACardIsAsWideAsItsWidestLineOrItsTitle(t *testing.T) {
	for _, c := range []struct {
		name  string
		title string
		width int
	}{
		{"line", "", 12},
		{"title", " a long title here ", 21},
	} {
		frame := ui.NewFrame(40, 10)
		card{title: c.title, lines: []ui.Line{ui.LineRaw("0123456789")}}.draw(frame, frame.Area())
		got := viewCardRows(viewRowsOf(frame))
		if len(got) != 3 {
			t.Fatalf("%s: %q", c.name, got)
		}
		for i, row := range got {
			if width := runeCount(row); width != c.width && (i != 1 || width != c.width) {
				t.Fatalf("%s: row %d is %d wide, want %d: %q", c.name, i, width, c.width, row)
			}
		}
	}
}

func TestACardWrapsALineThatIsTooWideOntoTheRowsItNeeds(t *testing.T) {
	for _, c := range []struct {
		text string
		rows int
	}{
		{"abcdefghijklmnopqrst", 2},
		{"abcdefghijklmnopqrstuvwx", 3},
		{"abcdefghij", 1},
	} {
		frame := ui.NewFrame(12, 12)
		card{lines: []ui.Line{ui.LineRaw(c.text)}}.draw(frame, frame.Area())
		got := viewCardRows(viewRowsOf(frame))
		if len(got) != c.rows+2 || !strings.HasPrefix(got[len(got)-1], "╰") {
			t.Fatalf("%q drew %q, want %d rows inside the border", c.text, got, c.rows)
		}
	}
}

func TestPerformSelectsTheNoteThatAddMade(t *testing.T) {
	env := threeNotes(t)
	config := filepath.Join(env.Paths.ConfigDir(), "config.toml")
	if err := os.WriteFile(config, []byte("sign = false\neditor = \"true\"\n"), 0o666); err != nil {
		t.Fatal(err)
	}
	app := load(t, env)
	handOver := func(run func()) error { run(); return nil }
	for _, title := range []string{"Zebra", "Aardvark"} {
		known := app.IDs()
		if err := Perform(env, handOver, app, Action{Kind: ActAdd, Title: title, HasTitle: true}); err != nil {
			t.Fatal(err)
		}
		selected := app.Selected()
		if selected == nil || selected.Note.Title != title {
			t.Fatalf("the cursor is on %+v after adding %q (had %v)", selected, title, known)
		}
		app.OnKey(Rune('G'))
	}
}

func TestPerformEndsTheSessionWhenTheTerminalCannotBeHandedOver(t *testing.T) {
	env := threeNotes(t)
	app := load(t, env)
	broken := func(func()) error { return errors.New("no tty") }
	err := Perform(env, broken, app, Action{Kind: ActEdit, Key: app.Selected().ID, Touch: cmd.TouchStamp})
	if !errors.Is(err, errTerminal) {
		t.Fatalf("Perform returned %v", err)
	}
}

func TestDrawTellsTheStateHowManyRowsAScreenOfARowListHolds(t *testing.T) {
	env := threeNotes(t)
	app := load(t, env)
	drawAt(env, app, 90, 28)
	// The header, title and status take 7 rows, the crumbs one, the heading one.
	if want := 28 - InfoRows - 1 - 1 - 1 - headingRows; app.page != want {
		t.Fatalf("page %d, want %d", app.page, want)
	}
}

func TestAListingTooNarrowForItsFloorStillDrawsWithoutLongColumns(t *testing.T) {
	env := threeNotes(t)
	app := load(t, env)
	rows := screenAt(env, app, 20, 24)
	if !hasLineWith(rows, "ID") {
		t.Fatalf("%q", rows)
	}
}

func TestALongListingKeepsAColumnWhenTheRowFitsItExactly(t *testing.T) {
	env := threeNotes(t)
	app := load(t, env)
	app.Long = true
	idWidth, slugWidth, stampWidth := 0, 0, 0
	for _, file := range app.Rows() {
		idWidth = max(idWidth, runeCount(file.ID))
		slugWidth = max(slugWidth, runeCount(file.Slug))
		stampWidth = max(stampWidth, runeCount(stamp(file.Note.Created)), runeCount(stamp(file.Note.Updated)))
	}
	idWidth += runeCount(markSymbol)
	spent := 5*columnGap + idWidth + titleFloor + slugWidth + 2*stampWidth
	width := spent + gutter + barColumn
	if rows := screenAt(env, app, width, 24); !hasLineWith(rows, "UPDATED") {
		t.Fatalf("width %d: %q", width, rows)
	}
	if rows := screenAt(env, app, width-1, 24); hasLineWith(rows, "UPDATED") {
		t.Fatalf("width %d: %q", width-1, rows)
	}
}

func TestOnlyTheFirstNineTagsGetADigitKey(t *testing.T) {
	env := threeNotes(t)
	var tags []string
	for i := range 11 {
		tags = append(tags, fmt.Sprintf("tag%02d", i))
	}
	addTagged(t, env, "Many tags", tags...)
	app := load(t, env)
	goTo(t, env, app, "tags")
	rows := tagRows(app).rows
	for at, row := range rows {
		want := ""
		if at < ScopeKeys {
			want = fmt.Sprint(at + 1)
		}
		if got := viewLinesText(row.Cells[0]); got != want {
			t.Fatalf("tag %d has key %q, want %q", at, got, want)
		}
	}
	if len(rows) <= ScopeKeys {
		t.Fatalf("%d tags", len(rows))
	}
}

func TestBlameSaysNotCommittedOnlyForLinesNobodyCommitted(t *testing.T) {
	env := threeNotes(t)
	app := load(t, env)
	file := filepath.Join(notebookOf(t, env).Path, app.Selected().ID+"-budget-review.md")
	was, err := os.ReadFile(file)
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(file, append(was, "a line nobody committed\n"...), 0o666); err != nil {
		t.Fatal(err)
	}
	app.OnKey(Rune('B'))
	rows := screen(env, app)
	for _, row := range rows {
		isNew := strings.Contains(row, "a line nobody committed")
		isOld := strings.Contains(row, "the q3 budget is late")
		if isNew && !strings.Contains(row, "not committed") {
			t.Fatalf("an uncommitted line is dated: %q", row)
		}
		if isOld && strings.Contains(row, "not committed") {
			t.Fatalf("a committed line is undated: %q", row)
		}
	}
	if !hasLineWith(rows, "not committed", "a line nobody committed") || !hasLineWith(rows, "the q3 budget is late") {
		t.Fatalf("%q", rows)
	}
}

func TestTheCommandsCardScrollsToKeepTheCursorOnTheLastRowOfAScreen(t *testing.T) {
	app := &App{}
	shown := MatchingCommands("")
	app.commandsAt = 14
	frame := ui.NewFrame(80, 14)
	drawCommands(frame, frame.Area(), app)
	room := 14 - 4
	first := 14 - (room - 1)
	rows := viewRowsOf(frame)
	for at, spec := range shown {
		got := viewHasCommandRow(rows, spec.Usage())
		if want := at >= first && at < first+room; got != want {
			t.Fatalf("command %d (%s) shown %v, want %v\n%s", at, spec.Usage(), got, want, strings.Join(rows, "\n"))
		}
	}
}

func viewHasCommandRow(rows []string, usage string) bool {
	for _, row := range rows {
		if strings.Contains(row, "│"+usage+" ") {
			return true
		}
	}
	return false
}

func TestTheCommandsCardCutsEachDescriptionToExactlyWhatTheRowHolds(t *testing.T) {
	app := &App{}
	const width = 56
	frame := ui.NewFrame(width, 40)
	drawCommands(frame, frame.Area(), app)
	shown := MatchingCommands("")
	usageWidth := 0
	for _, spec := range shown {
		usageWidth = max(usageWidth, runeCount(spec.Usage()))
	}
	rows := viewCardRows(viewRowsOf(frame))
	room := width - 2 - usageWidth - 2
	cutOne := false
	for i, spec := range shown {
		want := "│" + padRight(spec.Usage(), usageWidth) + "  " + cut(spec.What, room)
		cutOne = cutOne || strings.HasSuffix(want, "…")
		if got := strings.TrimRight(strings.TrimSuffix(rows[i+1], "│"), " "); got != strings.TrimRight(want, " ") {
			t.Fatalf("row %d is %q, want %q", i, got, want)
		}
	}
	if !cutOne || !strings.Contains(strings.Join(rows, "\n"), "esc  back") {
		t.Fatalf("a description was not cut, or the footer was pushed off:\n%s", strings.Join(rows, "\n"))
	}
}

func TestTheQueueReversesOnlyTheStepUnderTheCursor(t *testing.T) {
	app := &App{}
	app.Queue = []cmd.Step{{Keys: []string{"a"}}, {Keys: []string{"b", "c"}}}
	app.queueAt = 1
	frame := ui.NewFrame(60, 12)
	drawQueue(frame, frame.Area(), app)
	if viewCellOf(t, frame, "(1 note)").Modifier&ui.Reversed != 0 {
		t.Fatal("the first step is reversed")
	}
	if viewCellOf(t, frame, "(2 notes)").Modifier&ui.Reversed == 0 {
		t.Fatal("the step under the cursor is not reversed")
	}
}

func viewPicker(tags int, tagsAt int) *App {
	app := &App{}
	for i := 1; i <= tags; i++ {
		app.choices = append(app.choices, Choice{Tag: fmt.Sprintf("t%02d", i), Notes: 1})
	}
	app.aimedAt = []string{"a", "b"}
	app.tagsAt = tagsAt
	return app
}

func TestTheTagPickerScrollsToKeepTheCursorOnTheLastRowOfAScreen(t *testing.T) {
	app := viewPicker(12, 10)
	frame := ui.NewFrame(60, 12)
	drawTagging(frame, frame.Area(), app)
	rows := strings.Join(viewRowsOf(frame), "\n")
	// Eight rows fit: the cursor on the 11th puts the 4th at the top.
	for _, shown := range []string{"t04", "t11"} {
		if !strings.Contains(rows, shown) {
			t.Fatalf("%s is missing:\n%s", shown, rows)
		}
	}
	for _, hidden := range []string{"t03", "t12"} {
		if strings.Contains(rows, hidden) {
			t.Fatalf("%s is shown:\n%s", hidden, rows)
		}
	}
	if !strings.Contains(rows, "1 more") {
		t.Fatalf("the footer does not count the row left over:\n%s", rows)
	}
}

func TestCutKeepsWholeCharactersAndALineThatJustFits(t *testing.T) {
	for _, c := range []struct {
		text string
		room int
		want string
	}{
		{"abc", 3, "abc"},
		{"abcd", 3, "ab…"},
		{"abc", 2, "a…"},
		{"ab  cd", 4, "ab…"},
		{"預算會議", 3, "預算…"},
		{"a", 0, "…"},
	} {
		if got := cut(c.text, c.room); got != c.want {
			t.Fatalf("cut(%q, %d) = %q, want %q", c.text, c.room, got, c.want)
		}
	}
}

func TestFrontmatterThatHoldsNothingIsStillDimmed(t *testing.T) {
	front, body := splitFrontmatter("---\n\n---\nbody")
	if front != "---\n\n---\n" || body != "body" {
		t.Fatalf("front %q body %q", front, body)
	}
	front, body = splitFrontmatter("---\ntitle: x\n---\nbody")
	if front != "---\ntitle: x\n---\n" || body != "body" {
		t.Fatalf("front %q body %q", front, body)
	}
	if front, body = splitFrontmatter("---\nnever closed"); front != "" || body != "---\nnever closed" {
		t.Fatalf("front %q body %q", front, body)
	}
}

func TestMarkedMakesOneSpanPerRunAndNoneThatIsEmpty(t *testing.T) {
	for _, c := range []struct {
		text  string
		terms []string
		want  []string
	}{
		{"budget", []string{"budget"}, []string{"budget"}},
		{"the budget", []string{"budget"}, []string{"the ", "budget"}},
		{"budget is", []string{"budget"}, []string{"budget", " is"}},
		{"a budget b", []string{"budget"}, []string{"a ", "budget", " b"}},
		{"", nil, []string{""}},
		{"plain", nil, []string{"plain"}},
	} {
		line := marked(c.text, c.terms, ui.Style{})
		var got []string
		for _, span := range line.Spans {
			got = append(got, span.Content)
		}
		if strings.Join(got, "|") != strings.Join(c.want, "|") || len(got) != len(c.want) {
			t.Fatalf("marked(%q, %q) = %q, want %q", c.text, c.terms, got, c.want)
		}
	}
}

func TestTheEarliestMatchWinsAndTheFirstTermWinsATie(t *testing.T) {
	start, end, found := nextMatch("the budget is late", []string{"late", "budget"})
	if !found || start != 4 || end != 10 {
		t.Fatalf("(%d, %d, %v), want the budget at 4..10", start, end, found)
	}
	start, end, found = nextMatch("budget", []string{"bud", "budget"})
	if !found || start != 0 || end != 3 {
		t.Fatalf("(%d, %d, %v), want the first term at 0..3", start, end, found)
	}
	if _, _, found = nextMatch("budget", []string{"", "zzz"}); found {
		t.Fatal("an empty term matched")
	}
}
