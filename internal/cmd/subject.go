package cmd

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"github.com/henry40408/noda/internal/note"
	"github.com/henry40408/noda/internal/notebook"
)

// subject is what a key names in a notebook: a note or one of its files.
type subject struct {
	// resolved is set for a note.
	resolved notebook.IDSlug
	isNote   bool
}

// resolveSubject reads one key as `path` and `backlinks` do: one key can name
// both a note and a file, which is refused.
func resolveSubject(active *notebook.Notebook, key string) (subject, error) {
	resolved, resolveErr := active.Resolve(key)
	asFile := filepath.Join(active.Path, key)
	isFile := !strings.ContainsAny(key, `/\`) && isRegularFile(asFile)
	asNote := resolveErr == nil

	switch {
	case asNote && isFile:
		return subject{}, fmt.Errorf("`%s` names both a note and a file — say which:\n  %s\n  %s",
			key, note.FileName(resolved.ID, resolved.Slug), key)
	case asNote:
		return subject{resolved: resolved, isNote: true}, nil
	case isFile:
		return subject{}, nil
	}
	// Only "no such note" needs widening; an ambiguous key's error stands.
	if strings.HasPrefix(resolveErr.Error(), notebook.NotFound) {
		return subject{}, fmt.Errorf("nothing called `%s` — the notebook holds no note and no file by that name", key)
	}
	return subject{}, resolveErr
}

func isRegularFile(path string) bool {
	info, err := os.Stat(path)
	return err == nil && info.Mode().IsRegular()
}
