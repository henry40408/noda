package cmd

import (
	"errors"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
)

// editFile is the scratch file used when composing a note in the editor.
const editFile = "NOTE_EDITMSG.md"

// runEditor runs the editor on path, treating a non-zero exit as an aborted
// edit. It runs in the terminal the command line owns.
func (e Env) runEditor(editor, path string) error {
	parts := strings.Fields(editor)
	if len(parts) == 0 {
		return errors.New("$EDITOR is set but empty")
	}
	program := parts[0]
	command := exec.Command(program, append(parts[1:], path)...)
	command.Stdin, command.Stdout, command.Stderr = os.Stdin, os.Stderr, os.Stderr
	command.Env = e.Git.Environ()
	if err := command.Run(); err != nil {
		var exit *exec.ExitError
		if errors.As(err, &exit) {
			return fmt.Errorf("editor `%s` exited with %s", program, exit.ProcessState)
		}
		return fmt.Errorf("could not start editor `%s`: %w", program, err)
	}
	return nil
}

// composeInEditor opens the editor on a scratch file in the cache dir, never
// the notebook, so an abandoned edit leaves nothing in the repo, and returns
// what was written.
func (e Env) composeInEditor(title string, hasTitle bool) (string, error) {
	if err := os.MkdirAll(e.Paths.CacheDir(), 0o777); err != nil {
		return "", err
	}
	scratch := filepath.Join(e.Paths.CacheDir(), editFile)
	template := ""
	if hasTitle {
		template = "# " + title + "\n\n"
	}
	if err := os.WriteFile(scratch, []byte(template), 0o666); err != nil {
		return "", err
	}
	if err := e.runEditor(e.configuredEditor(), scratch); err != nil {
		return "", err
	}
	body, err := os.ReadFile(scratch)
	if err != nil {
		return "", err
	}
	_ = os.Remove(scratch)
	return string(body), nil
}
