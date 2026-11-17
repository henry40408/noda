package tui

import (
	"os"
	"path/filepath"
	"strings"
	"testing"

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
