package tui

import (
	"errors"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"
	"unicode"

	"github.com/henry40408/noda/internal/cmd"
	"github.com/henry40408/noda/internal/gitx/gittest"
	"github.com/henry40408/noda/internal/notebook"
	"github.com/henry40408/noda/internal/paths"
	"github.com/henry40408/noda/internal/tui/ui"
)

// emptyNotebook is a throwaway root with signing off, then `init` run on it.
// Signing is turned off before anything commits: noda's setting outranks git's,
// and the developer's real config may sign with gpg. Nothing here reads the
// real home directory.
func emptyNotebook(t *testing.T) cmd.Env {
	t.Helper()
	env := cmd.Env{Paths: paths.Rooted(t.TempDir()), Git: gittest.Require(t)}
	if err := os.MkdirAll(env.Paths.ConfigDir(), 0o777); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(env.Paths.ConfigDir(), "config.toml"), []byte("sign = false\n"), 0o666); err != nil {
		t.Fatal(err)
	}
	if _, err := cmd.Init(env); err != nil {
		t.Fatal(err)
	}
	return env
}

func ptr(text string) *string { return &text }

// threeNotes is three notes, in listing order: `budget-review`,
// `meeting-notes`, `reading-list`.
func threeNotes(t *testing.T) cmd.Env {
	t.Helper()
	env := emptyNotebook(t)
	for _, note := range []struct {
		title, content string
		tags           []string
	}{
		{"Budget review", "the q3 budget is late", []string{"work"}},
		{"Meeting notes", "# Agenda\n\nthe budget, again", []string{"work", "q3"}},
		{"Reading list", "a book", nil},
	} {
		if _, err := cmd.Add(env, cmd.AddOptions{Title: ptr(note.title), Content: ptr(note.content), Tags: note.tags}); err != nil {
			t.Fatal(err)
		}
	}
	return env
}

func load(t *testing.T, env cmd.Env) *App {
	t.Helper()
	app, err := Load(env)
	if err != nil {
		t.Fatal(err)
	}
	return app
}

func notebookOf(t *testing.T, env cmd.Env) *notebook.Notebook {
	t.Helper()
	active, err := notebook.OpenActive(env.Git, env.Paths)
	if err != nil {
		t.Fatal(err)
	}
	return active
}

// drawAt is what the runtime does between a keystroke and a frame.
func drawAt(env cmd.Env, app *App, width, height int) *ui.Frame {
	Refresh(env, app)
	frame := ui.NewFrame(width, height)
	Draw(frame, app)
	return frame
}

// screenAt is the frame's rows, trailing blanks cut so assertions do not
// depend on terminal width.
func screenAt(env cmd.Env, app *App, width, height int) []string {
	buffer := drawAt(env, app, width, height).Buffer
	rows := make([]string, height)
	for y := range rows {
		rows[y] = strings.TrimRightFunc(buffer.Row(y), unicode.IsSpace)
	}
	return rows
}

// screen is tall enough for the full header, which collapses to one line on a
// shorter terminal.
func screen(env cmd.Env, app *App) []string { return screenAt(env, app, 90, 28) }

func hasLineWith(rows []string, needles ...string) bool {
	for _, row := range rows {
		all := true
		for _, needle := range needles {
			all = all && strings.Contains(row, needle)
		}
		if all {
			return true
		}
	}
	return false
}

// keyOfToken reads the golden files' spelling of a key.
func keyOfToken(t *testing.T, token string) Key {
	t.Helper()
	if rest, ok := strings.CutPrefix(token, "C-"); ok {
		return CtrlKey([]rune(rest)[0])
	}
	if rest, ok := strings.CutPrefix(token, "A-"); ok {
		return AltKey([]rune(rest)[0])
	}
	named := map[string]Key{
		"Enter": Plain(CodeEnter), "Esc": Plain(CodeEsc), "Tab": Plain(CodeTab),
		"BackTab": {Code: CodeBackTab, Shift: true}, "Up": Plain(CodeUp), "Down": Plain(CodeDown),
		"Left": Plain(CodeLeft), "Right": Plain(CodeRight), "PageUp": Plain(CodePageUp),
		"PageDown": Plain(CodePageDown), "Home": Plain(CodeHome), "End": Plain(CodeEnd),
		"Backspace": Plain(CodeBackspace), "Delete": Plain(CodeDelete), "Space": Rune(' '),
	}
	if key, ok := named[token]; ok {
		return key
	}
	if runes := []rune(token); len(runes) == 1 {
		return Rune(runes[0])
	}
	t.Fatalf("unknown key %s", token)
	return Key{}
}

// pressToken presses a golden key: `type:` types a run of characters.
func pressToken(t *testing.T, app *App, token string) {
	t.Helper()
	if typed, ok := strings.CutPrefix(token, "type:"); ok {
		typing(app, typed)
		return
	}
	app.OnKey(keyOfToken(t, token))
}

// noEditor is the foreground of a test: nothing here may open $EDITOR.
const headerHeight = 5

func noEditor(func()) error { return errors.New("this test must not open an editor") }

func perform(t *testing.T, env cmd.Env, app *App, action *Action) {
	t.Helper()
	if action == nil {
		t.Fatal("an action to perform")
	}
	if err := Perform(env, noEditor, app, *action); err != nil {
		t.Fatal(err)
	}
}

func commits(t *testing.T, env cmd.Env) int {
	t.Helper()
	out, err := cmd.Log(env, "", 0)
	if err != nil {
		t.Fatal(err)
	}
	return len(strings.Split(strings.TrimRight(out, "\n"), "\n"))
}

// opened presses Enter on the note under the cursor and draws; the session is
// left on the note.
func opened(env cmd.Env, app *App) []string {
	app.OnKey(Plain(CodeEnter))
	return screen(env, app)
}

func markAllShown(app *App) { app.OnKey(Rune('*')) }

// cursorAt is where the terminal cursor ended up, which no state-machine test
// can see.
func cursorAt(t *testing.T, env cmd.Env, app *App) (int, int) {
	t.Helper()
	cursor := drawAt(env, app, 90, 28).Cursor
	if cursor == nil {
		t.Fatal("the frame wants a cursor")
	}
	return cursor.X, cursor.Y
}

func key(code Code) Key { return Plain(code) }

// titlesTagged is read from the notebook, not the screen: columns give way at
// any width.
func titlesTagged(app *App, tag string) []string {
	var titles []string
	for _, file := range app.Rows() {
		if slices.Contains(file.Note.Tags, tag) {
			titles = append(titles, file.Note.Title)
		}
	}
	return titles
}

// addTagged adds a note with tags, for notebooks that need one more.
func addTagged(t *testing.T, env cmd.Env, title string, tags ...string) {
	t.Helper()
	if _, err := cmd.Add(env, cmd.AddOptions{Title: ptr(title), Content: ptr("body"), Tags: tags}); err != nil {
		t.Fatal(err)
	}
}

func commandLine(app *App, line string) *Action {
	app.OnKey(Rune(':'))
	typing(app, line)
	return app.OnKey(key(CodeEnter))
}

func notePath(t *testing.T, env cmd.Env, app *App) string {
	t.Helper()
	file, err := cmd.Path(env, app.Selected().ID)
	if err != nil {
		t.Fatal(err)
	}
	return strings.TrimSpace(file)
}

func updatedLine(t *testing.T, env cmd.Env, app *App) string {
	t.Helper()
	text, err := os.ReadFile(notePath(t, env, app))
	if err != nil {
		t.Fatal(err)
	}
	for _, line := range strings.Split(string(text), "\n") {
		if strings.HasPrefix(line, "updated:") {
			return line
		}
	}
	t.Fatal("an updated field")
	return ""
}

// backdate sets a date no clock will produce, so "it did not move" tests the
// flag, not the test's speed.
func backdate(t *testing.T, env cmd.Env, app *App) {
	t.Helper()
	file := notePath(t, env, app)
	text, err := os.ReadFile(file)
	if err != nil {
		t.Fatal(err)
	}
	lines := strings.Split(strings.TrimRight(string(text), "\n"), "\n")
	for i, line := range lines {
		if strings.HasPrefix(line, "updated:") {
			lines[i] = "updated: 2019-03-04T05:06:07Z"
		}
	}
	if err := os.WriteFile(file, []byte(strings.Join(lines, "\n")+"\n"), 0o666); err != nil {
		t.Fatal(err)
	}
}

// workedNotebook has something for every screen: a link by id, an overdue box,
// an unused attachment and a deletion in the history. The link names the
// budget's real id, which is what backlinks match on.
func workedNotebook(t *testing.T) cmd.Env {
	t.Helper()
	env := emptyNotebook(t)
	added, err := cmd.Add(env, cmd.AddOptions{Title: ptr("Budget review"), Content: ptr("the q3 budget is late"), Tags: []string{"work"}})
	if err != nil {
		t.Fatal(err)
	}
	id := strings.Fields(added)[0]
	meeting := "see [the budget](" + id + "-budget-review.md)\n\n- [ ] book a room due:2020-01-01\n"
	for _, note := range []cmd.AddOptions{
		{Title: ptr("Meeting notes"), Content: ptr(meeting), Tags: []string{"work", "q3"}},
		{Title: ptr("Reading list"), Content: ptr("a book")},
		{Title: ptr("Trip plan"), Content: ptr("flights"), Tags: []string{"travel"}},
	} {
		if _, err := cmd.Add(env, note); err != nil {
			t.Fatal(err)
		}
	}
	if _, err := cmd.Rm(env, "trip-plan"); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(notebookOf(t, env).Path, "diagram.png"), []byte("png"), 0o666); err != nil {
		t.Fatal(err)
	}
	return env
}

// goTo opens a screen from the prompt and lets the runtime load it.
func goTo(t *testing.T, env cmd.Env, app *App, line string) []string {
	t.Helper()
	if action := commandLine(app, line); action != nil {
		perform(t, env, app, action)
	}
	return screen(env, app)
}
