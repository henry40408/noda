// Package cmd is what each noda command does. A command takes an Env, or for
// its `In` half an open notebook, and returns the text to print: only
// cmd/noda writes to the terminal.
package cmd

import (
	"os"

	"github.com/henry40408/noda/internal/config"
	"github.com/henry40408/noda/internal/gitx"
	"github.com/henry40408/noda/internal/notebook"
	"github.com/henry40408/noda/internal/paths"
)

// Env is what every command needs from the outside world. Tests build one over
// a throwaway root; nothing here reads the process environment itself.
type Env struct {
	Paths paths.Paths
	Git   *gitx.Git
	// Interactive lets ssh and gpg ask at the terminal: set by the command
	// line, never by the web server or the full-screen interface.
	Interactive bool
	// Confirm asks a yes/no question where the user is, for the commands that
	// cannot be undone. Nil means there is nowhere to ask.
	Confirm func(question string) (bool, error)
}

func (e Env) openActive() (*notebook.Notebook, error) {
	opened, err := notebook.OpenActive(e.Git, e.Paths)
	if err != nil {
		return nil, err
	}
	opened.Interactive = e.Interactive
	return opened, nil
}

func (e Env) open(name string) (*notebook.Notebook, error) {
	opened, err := notebook.Open(e.Git, e.Paths, name)
	if err != nil {
		return nil, err
	}
	opened.Interactive = e.Interactive
	return opened, nil
}

// configuredEditor is the editor command, falling back as `config.Editor` does.
// A config that cannot be read is skipped here: the command that opens an
// editor is not the one to report it.
func (e Env) configuredEditor() string {
	configured := ""
	if settings, err := config.Load(e.Paths); err == nil {
		configured, _ = settings.Get("editor")
	}
	visual, _ := e.Git.Getenv("VISUAL")
	editor, _ := e.Git.Getenv("EDITOR")
	command, _ := config.Editor(configured, visual, editor)
	return command
}

func fileExists(path string) bool {
	_, err := os.Stat(path)
	return err == nil
}
