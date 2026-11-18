package main

// The browser in a real terminal: the built binary on a pty, read back through
// a vt10x emulator.
//
// The in-process screen tests draw into a character buffer, which is blind to
// layout bugs: a cell skipped so later columns slide left, a card taller than
// twenty-four rows, a key dropped at eighty columns. The pty lives inside the
// test's own pseudo-terminal pair, so the developer's terminal is never touched.

import (
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
	"time"
	"unicode/utf8"

	"github.com/creack/pty"
	"github.com/hinshun/vt10x"
)

// patience is generous, because a test flaky on a loaded machine gets deleted.
const patience = 20 * time.Second

// aNotebook is three notes, in listing order.
func aNotebook(t *testing.T) sandbox {
	t.Helper()
	box := newSandbox(t)
	box.ok(t, "init")
	box.ok(t, "add", "Budget review", "--content", "the q3 budget is late", "--tag", "work")
	box.ok(t, "add", "Meeting notes", "--content", "# Agenda\n\nthe budget, again", "--tag", "work", "--tag", "q3")
	box.ok(t, "add", "Reading list", "--content", "a book")
	return box
}

// anEditorThatEdits appends a line and exits. It is a script because the
// editor command is split on whitespace, and it is set in config, which wins
// over the environment: setting only $EDITOR once opened the developer's vim.
func anEditorThatEdits(t *testing.T, box sandbox) {
	t.Helper()
	script := filepath.Join(t.TempDir(), "editor.sh")
	if err := os.WriteFile(script, []byte("#!/bin/sh\nprintf '\\nedited by the test\\n' >> \"$1\"\n"), 0o755); err != nil {
		t.Fatal(err)
	}
	config := filepath.Join(box.env["XDG_CONFIG_HOME"], "noda", "config.toml")
	file, err := os.OpenFile(config, os.O_APPEND|os.O_WRONLY, 0)
	if err != nil {
		t.Fatal(err)
	}
	defer file.Close()
	if _, err := fmt.Fprintf(file, "editor = %q\n", script); err != nil {
		t.Fatal(err)
	}
}

type browser struct {
	t       *testing.T
	command *exec.Cmd
	pty     *os.File
	screen  vt10x.Terminal
	exited  chan error
}

func openBrowser(t *testing.T, box sandbox, cols, rows int) *browser {
	t.Helper()
	return openBrowserWithStderr(t, box, cols, rows, nil)
}

// openBrowserWithStderr sends the browser's stderr to a file instead of the
// terminal, as `noda tui 2>log` does. A nil file leaves it on the terminal.
func openBrowserWithStderr(t *testing.T, box sandbox, cols, rows int, stderr *os.File) *browser {
	t.Helper()
	command := exec.Command(theBinary(t), "tui")
	if stderr != nil {
		command.Stderr = stderr
	}
	for name, value := range box.env {
		command.Env = append(command.Env, name+"="+value)
	}
	command.Env = withCoverage(append(command.Env, "TERM=xterm-256color"))
	master, err := pty.StartWithSize(command, &pty.Winsize{Cols: uint16(cols), Rows: uint16(rows)})
	if err != nil {
		t.Fatal(err)
	}
	b := &browser{t: t, command: command, pty: master, screen: vt10x.New(vt10x.WithSize(cols, rows)), exited: make(chan error, 1)}
	go feed(b.screen, master)
	go func() { b.exited <- command.Wait() }()
	// A failed assertion must not leave the child running.
	t.Cleanup(func() {
		_ = command.Process.Kill()
		_ = master.Close()
	})
	return b
}

// feed is io.Copy, except that a read ending inside a multi-byte character is
// not an error: vt10x takes the whole characters and reports the short write,
// so the rest is carried over to the next read.
func feed(screen io.Writer, from io.Reader) {
	buffer := make([]byte, 8192)
	var carried []byte
	for {
		n, err := from.Read(buffer)
		carried = append(carried, buffer[:n]...)
		if len(carried) > 0 {
			taken, _ := screen.Write(carried)
			carried = append([]byte(nil), carried[taken:]...)
		}
		if err != nil {
			return
		}
	}
}

func (b *browser) send(keys string) {
	b.t.Helper()
	if _, err := b.pty.WriteString(keys); err != nil {
		b.t.Fatal(err)
	}
}

// rows keeps leading blanks, so a column is measurable.
func (b *browser) rows() []string {
	b.screen.Lock()
	defer b.screen.Unlock()
	cols, height := b.screen.Size()
	rows := make([]string, height)
	for y := range rows {
		var row strings.Builder
		for x := 0; x < cols; x++ {
			row.WriteRune(b.screen.Cell(x, y).Char)
		}
		rows[y] = strings.TrimRight(row.String(), " ")
	}
	return rows
}

func (b *browser) now() string { return strings.Join(b.rows(), "\n") }

func (b *browser) waitUntil(what string, done func(screen string) bool) string {
	b.t.Helper()
	deadline := time.Now().Add(patience)
	for {
		screen := b.now()
		if done(screen) {
			return screen
		}
		if time.Now().After(deadline) {
			b.t.Fatalf("waited %v for %s; the screen was:\n%s", patience, what, screen)
		}
		time.Sleep(20 * time.Millisecond)
	}
}

func (b *browser) waitFor(needle string) string {
	b.t.Helper()
	return b.waitUntil(fmt.Sprintf("%q", needle), func(screen string) bool { return strings.Contains(screen, needle) })
}

func (b *browser) waitUntilGone(needle string) string {
	b.t.Helper()
	return b.waitUntil(fmt.Sprintf("%q to go", needle), func(screen string) bool { return !strings.Contains(screen, needle) })
}

// quit is `q`, then ctrl-c: an open card swallows the `q` that dismisses it.
func (b *browser) quit() {
	b.t.Helper()
	b.send("q")
	err, left := b.waitForExit(2 * time.Second)
	if !left {
		b.send("\x03")
		err, left = b.waitForExit(patience)
	}
	if !left {
		b.t.Fatal("the browser should have left by now")
	}
	if err != nil {
		b.t.Fatalf("the browser left with %v", err)
	}
}

func (b *browser) waitForExit(patience time.Duration) (error, bool) {
	select {
	case err := <-b.exited:
		return err, true
	case <-time.After(patience):
		return nil, false
	}
}

func waitForFile(t *testing.T, path, needle string) {
	t.Helper()
	deadline := time.Now().Add(patience)
	for {
		text, _ := os.ReadFile(path)
		if strings.Contains(string(text), needle) {
			return
		}
		if time.Now().After(deadline) {
			t.Fatalf("waited %v for %q in %s; it held:\n%s", patience, needle, path, text)
		}
		time.Sleep(20 * time.Millisecond)
	}
}

// columnOf is a position in characters of the row, which equals columns while
// the row holds no wide character.
func columnOf(t *testing.T, rows []string, needle string) int {
	t.Helper()
	for _, row := range rows {
		if at := strings.Index(row, needle); at >= 0 {
			return utf8.RuneCountInString(row[:at])
		}
	}
	t.Fatalf("%q is not on the screen:\n%s", needle, strings.Join(rows, "\n"))
	return 0
}

// countColumn is the terminal column of the count beside a tag. It is searched
// from the checkbox, because the listing shows beside the card and `Meeting
// notes` contains " note"; it counts runes because an index answers in bytes.
// vt10x has no wide characters: it stores one as a rune and a blank cell, so a
// rune count is already the terminal column.
func countColumn(row string) (int, bool) {
	boxed := strings.Index(row, "[x] ")
	if boxed < 0 {
		boxed = strings.Index(row, "[ ] ")
	}
	if boxed < 0 {
		return 0, false
	}
	at := strings.Index(row[boxed:], " note")
	if at < 0 {
		return 0, false
	}
	return utf8.RuneCountInString(row[:boxed+at]), true
}

// `專案管理` is four characters but eight columns; the in-process tests cannot tell.
func TestThePtyTagPickerLinesItsCountsUpInColumnsAndNotCharacters(t *testing.T) {
	box := aNotebook(t)
	box.ok(t, "add", "Ubuntu notes", "--content", "body", "--tag", "專案管理")
	b := openBrowser(t, box, 90, 28)
	b.waitFor("Budget review")

	// Wait on the card's footer; the tag is on the listing too.
	b.send("#")
	if screen := b.waitFor("tab  choose"); !strings.Contains(strings.ReplaceAll(screen, " ", ""), "專案管理") {
		t.Fatal(screen)
	}
	rows := b.rows()
	var counts []int
	for _, row := range rows {
		if column, ok := countColumn(row); ok {
			counts = append(counts, column)
		}
	}
	if len(counts) < 3 {
		t.Fatalf("three tags, three counts, and there were %d:\n%s", len(counts), strings.Join(rows, "\n"))
	}
	for _, column := range counts {
		if column != counts[0] {
			t.Fatalf("the counts are at %v:\n%s", counts, strings.Join(rows, "\n"))
		}
	}
	// The filter takes every letter, `q` included.
	b.send("\x1b")
	b.waitUntilGone("tab  choose")
	b.quit()
}

// Other layers hand the state machine a Tab key; only a real terminal shows
// that `\t` arrives as Tab and not as a filter character.
func TestThePtyTabThatChoosesArrivesAsTabAndNotAsACharacter(t *testing.T) {
	b := openBrowser(t, aNotebook(t), 90, 28)
	b.waitFor("Budget review")

	// The note carries `work`, so tab takes it off.
	b.send("#")
	if screen := b.waitFor("tab  choose"); !strings.Contains(screen, "[x] work") {
		t.Fatal(screen)
	}
	b.send("\t")
	if screen := b.waitFor("[-] work"); strings.Contains(screen, "[x] work") {
		t.Fatalf("the tab went into the filter:\n%s", screen)
	}
	b.send("\x1b")
	b.waitUntilGone("tab  choose")
	b.quit()
}

func TestThePtyListingArrivesThroughARealTerminal(t *testing.T) {
	b := openBrowser(t, aNotebook(t), 90, 28)

	screen := b.waitFor("Budget review")
	for _, want := range []string{"Meeting notes", "Reading list", "Notes"} {
		if !strings.Contains(screen, want) {
			t.Errorf("no %q:\n%s", want, screen)
		}
	}
	b.quit()
}

// The card has outgrown twenty-four rows three times.
func TestThePtyHelpCardFitsATwentyFourRowTerminal(t *testing.T) {
	b := openBrowser(t, aNotebook(t), 90, 24)
	b.waitFor("Budget review")

	// Not `keys`, which is also on the grid underneath.
	b.send("?")
	screen := b.waitFor("half a screen")
	if !strings.Contains(screen, "tag:work OR tag:q3 budget") {
		t.Fatalf("the search example was cut:\n%s", screen)
	}
	if !strings.Contains(screen, "readline: ctrl-a/e/w/u/k/y") {
		t.Fatalf("the card lost its last row:\n%s", screen)
	}
	b.quit()
}

// At eighty columns the grid drops keys from the right; `:` must survive.
func TestThePtyCommandKeySurvivesEightyColumns(t *testing.T) {
	b := openBrowser(t, aNotebook(t), 80, 28)
	screen := b.waitFor("Budget review")

	// `<:>`, not `command`: `ctrl-a  commands` would pass without it.
	if !strings.Contains(screen, "<:>") || !strings.Contains(screen, "<?>") {
		t.Fatalf("the prompt or help key went:\n%s", screen)
	}
	b.quit()
}

// A skipped blank cell slides every later cell left. The note screen has
// blanks mid-grid.
func TestThePtyBlankCellHoldsItsColumnOpen(t *testing.T) {
	// At ninety columns the note screen's fourth column is gone.
	b := openBrowser(t, aNotebook(t), 110, 28)
	b.waitFor("Budget review")

	// `scroll` is the only grid word the listing lacks.
	b.send("\r")
	b.waitFor("scroll")
	rows := b.rows()

	// One column; three of these sit on rows with a blank cell to their left.
	todo := columnOf(t, rows, "<t>")
	for _, key := range []string{"<l>", "<b>", "<B>"} {
		if got := columnOf(t, rows, key); got != todo {
			t.Fatalf("%s slid out of its column:\n%s", key, strings.Join(rows, "\n"))
		}
	}
	b.quit()
}

func TestThePtyTypingAQueryNarrowsTheListing(t *testing.T) {
	b := openBrowser(t, aNotebook(t), 90, 28)
	b.waitFor("Reading list")

	b.send("/budget")
	if screen := b.waitUntilGone("Reading list"); !strings.Contains(screen, "Budget review") {
		t.Fatal(screen)
	}
	b.quit()
}

// Handing the terminal back once waited forever for a cursor-position reply,
// which no in-process test could see.
func TestThePtyComingBackFromTheEditorRedrawsTheScreen(t *testing.T) {
	box := aNotebook(t)
	anEditorThatEdits(t, box)
	b := openBrowser(t, box, 90, 28)
	b.waitFor("Budget review")

	// Wait for the file and for the browser to be back; a key sent between them
	// races the editor for the terminal.
	b.send("e")
	waitForFile(t, strings.TrimSpace(box.ok(t, "path", "budget-review")), "edited by the test")
	b.waitFor("budget-review")

	b.send("\r")
	b.waitFor("edited by the test")
	b.quit()
}

// With stderr redirected, an editor that draws on stdout must still reach the
// terminal; it used to be wired to stderr and drew into the log file instead.
func TestThePtyEditorScreenGoesToTheTerminalWhenStderrIsRedirected(t *testing.T) {
	box := aNotebook(t)
	script := filepath.Join(t.TempDir(), "editor.sh")
	body := "#!/bin/sh\necho EDITOR-DREW-ON-STDOUT\nprintf '\\nedited by the test\\n' >> \"$1\"\n"
	if err := os.WriteFile(script, []byte(body), 0o755); err != nil {
		t.Fatal(err)
	}
	config := filepath.Join(box.env["XDG_CONFIG_HOME"], "noda", "config.toml")
	file, err := os.OpenFile(config, os.O_APPEND|os.O_WRONLY, 0)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := fmt.Fprintf(file, "editor = %q\n", script); err != nil {
		t.Fatal(err)
	}
	file.Close()

	logPath := filepath.Join(t.TempDir(), "stderr.log")
	logFile, err := os.Create(logPath)
	if err != nil {
		t.Fatal(err)
	}
	defer logFile.Close()
	b := openBrowserWithStderr(t, box, 90, 28, logFile)
	b.waitFor("Budget review")
	b.send("e")
	waitForFile(t, strings.TrimSpace(box.ok(t, "path", "budget-review")), "edited by the test")
	b.waitFor("budget-review")
	b.quit()

	logged, err := os.ReadFile(logPath)
	if err != nil {
		t.Fatal(err)
	}
	if strings.Contains(string(logged), "EDITOR-DREW-ON-STDOUT") {
		t.Fatalf("the editor's screen went into the stderr file: %q", logged)
	}
}
