package cmd

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"github.com/henry40408/noda/internal/note"
	"github.com/henry40408/noda/internal/notebook"
)

// Path is for the tools noda does not wrap: `pandoc "$(noda path meeting-notes)"`.
// Without a key it is the notebook's directory.
func Path(e Env, key string) (string, error) {
	active, err := e.openActive()
	if err != nil {
		return "", err
	}
	if key == "" {
		return active.Path + "\n", nil
	}

	// One key can name both.
	resolved, resolveErr := active.Resolve(key)
	asFile := filepath.Join(active.Path, key)
	isFile := !strings.ContainsAny(key, `/\`) && isRegularFile(asFile)
	asNote := resolveErr == nil

	switch {
	case asNote && isFile:
		return "", fmt.Errorf("`%s` names both a note and a file — say which:\n  %s\n  %s",
			key, note.FileName(resolved.ID, resolved.Slug), key)
	case asNote:
		return active.NotePath(resolved.ID, resolved.Slug) + "\n", nil
	case isFile:
		return asFile + "\n", nil
	}
	// Only "no such note" needs widening; an ambiguous key's error stands.
	if strings.HasPrefix(resolveErr.Error(), notebook.NotFound) {
		return "", fmt.Errorf("nothing called `%s` — the notebook holds no note and no file by that name", key)
	}
	return "", resolveErr
}

func isRegularFile(path string) bool {
	info, err := os.Stat(path)
	return err == nil && info.Mode().IsRegular()
}
