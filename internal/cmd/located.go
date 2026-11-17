package cmd

import (
	"fmt"
	"os"

	"github.com/henry40408/noda/internal/note"
	"github.com/henry40408/noda/internal/notebook"
)

// found is a note reference resolved to a file, with the parse result kept
// rather than thrown: a command that only needs which note must not fail on a
// broken one.
type found struct {
	// id and slug come from the filename.
	id, slug string
	path     string
	note     note.Note
	parseErr error
}

func find(opened *notebook.Notebook, key string) (found, error) {
	resolved, err := opened.Resolve(key)
	if err != nil {
		return found{}, err
	}
	path := opened.NotePath(resolved.ID, resolved.Slug)
	text, err := os.ReadFile(path)
	if err != nil {
		return found{}, err
	}
	result := found{id: resolved.ID, slug: resolved.Slug, path: path}
	parsed, err := note.Parse(string(text))
	if err != nil {
		result.parseErr = fmt.Errorf("%s: %w", path, err)
		return result, nil
	}
	result.note = parsed
	return result, nil
}

// locate is find with the note parsed.
func locate(opened *notebook.Notebook, key string) (found, error) {
	result, err := find(opened, key)
	if err != nil {
		return found{}, err
	}
	return result, result.parseErr
}
