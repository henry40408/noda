package tui

import (
	"errors"

	"github.com/gdamore/tcell/v2"

	"github.com/henry40408/noda/internal/cmd"
	"github.com/henry40408/noda/internal/tui/ui"
)

// RunOptions is what a session is run with.
type RunOptions struct {
	Env cmd.Env
	// Terminals is whether stdin and stdout are both terminals.
	Terminals bool
	// Screen is the terminal to draw on; nil opens the real one. A test passes
	// a simulated screen.
	Screen tcell.Screen
}

// ErrNeedsTerminal is refused before anything is drawn: a piped stdout would
// get escape sequences, and no keyboard means no way to quit.
var ErrNeedsTerminal = errors.New("noda tui needs a terminal at both ends; " +
	"`noda ls`, `noda search` and `noda show` are the ones to redirect")

// Browse runs the active notebook until the user quits. Everything is said on
// screen, so it prints nothing.
func Browse(options RunOptions) error {
	if options.Screen == nil && !options.Terminals {
		return ErrNeedsTerminal
	}
	// Before taking over the screen, so an error is printed at the prompt.
	app, err := Load(options.Env)
	if err != nil {
		return err
	}
	screen := options.Screen
	if screen == nil {
		screen, err = tcell.NewScreen()
		if err != nil {
			return err
		}
	}
	if err := screen.Init(); err != nil {
		return err
	}
	// Deferred, so a panic still gives the terminal back.
	defer screen.Fini()
	return loop(options.Env, screen, app)
}

func loop(env cmd.Env, screen tcell.Screen, app *App) error {
	foreground := foregroundOf(screen)
	for {
		Refresh(env, app)
		draw(screen, app)
		// Blocking rather than polling, which would keep a laptop awake. A
		// resize needs no arm of its own: the next draw uses the new size.
		event := screen.PollEvent()
		switch event := event.(type) {
		case nil:
			return nil
		case *tcell.EventResize:
			screen.Sync()
		case *tcell.EventKey:
			done, err := handle(env, foreground, screen, app, keyOf(event))
			if err != nil || done {
				return err
			}
		}
	}
}

// handle is one keystroke; done is true when it asked to quit.
func handle(env cmd.Env, foreground Foreground, screen tcell.Screen, app *App, key Key) (done bool, err error) {
	action := app.OnKey(key)
	if action == nil {
		return false, nil
	}
	switch action.Kind {
	case ActQuit:
		return true, nil
	case ActReload:
		return false, Reload(env, app)
	}
	// A slow action: show that it is working first.
	if said := action.Working(); said != "" {
		app.Working = said
		draw(screen, app)
		app.Working = ""
	}
	return false, Perform(env, foreground, app, *action)
}

func draw(screen tcell.Screen, app *App) {
	width, height := screen.Size()
	frame := ui.NewFrame(width, height)
	Draw(frame, app)
	ui.Flush(screen, frame)
	screen.Show()
}

// foregroundOf hands the terminal to $EDITOR and takes it back. Sync repaints
// every cell, since the editor left the screen as it pleased.
func foregroundOf(screen tcell.Screen) Foreground {
	return func(run func()) error {
		if err := screen.Suspend(); err != nil {
			return err
		}
		run()
		if err := screen.Resume(); err != nil {
			return err
		}
		screen.Sync()
		return nil
	}
}

// keyOf translates the terminal library's key into the TUI's own.
func keyOf(event *tcell.EventKey) Key {
	mods := event.Modifiers()
	key := Key{Ctrl: mods&tcell.ModCtrl != 0, Alt: mods&tcell.ModAlt != 0, Shift: mods&tcell.ModShift != 0}
	if code, ok := namedKeys[event.Key()]; ok {
		key.Code = code
		return key
	}
	switch k := event.Key(); {
	case k == tcell.KeyRune:
		key.Code, key.Char = CodeChar, event.Rune()
	case k >= tcell.KeyCtrlA && k <= tcell.KeyCtrlZ:
		key.Code, key.Char, key.Ctrl = CodeChar, rune('a'+(k-tcell.KeyCtrlA)), true
	}
	return key
}

var namedKeys = map[tcell.Key]Code{
	tcell.KeyEnter:     CodeEnter,
	tcell.KeyEscape:    CodeEsc,
	tcell.KeyTab:       CodeTab,
	tcell.KeyBacktab:   CodeBackTab,
	tcell.KeyUp:        CodeUp,
	tcell.KeyDown:      CodeDown,
	tcell.KeyLeft:      CodeLeft,
	tcell.KeyRight:     CodeRight,
	tcell.KeyPgUp:      CodePageUp,
	tcell.KeyPgDn:      CodePageDown,
	tcell.KeyHome:      CodeHome,
	tcell.KeyEnd:       CodeEnd,
	tcell.KeyBackspace: CodeBackspace,
	tcell.KeyDelete:    CodeDelete,
}
