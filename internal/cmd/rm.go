package cmd

import (
	"os"

	"github.com/henry40408/noda/internal/note"
	"github.com/henry40408/noda/internal/notebook"
)

// Rm deletes a note. It is a commit, so `git revert` brings the note back with
// its id intact.
func Rm(e Env, key string) (string, error) {
	active, err := e.openActive()
	if err != nil {
		return "", err
	}
	return RmIn(active, key)
}

// RmIn is Rm in a notebook the caller already has open.
func RmIn(active *notebook.Notebook, key string) (string, error) {
	done, err := applyRemove(active, key)
	if err != nil {
		return "", err
	}
	if err := active.Commit(done.files, "rm: "+done.slug); err != nil {
		return "", err
	}
	return done.summary, nil
}

func applyRemove(active *notebook.Notebook, key string) (applied, error) {
	// find, not locate: a note that no longer parses must still be removable.
	target, err := find(active, key)
	if err != nil {
		return applied{}, err
	}
	if err := os.Remove(target.path); err != nil {
		return applied{}, err
	}
	var tags []string
	if target.parseErr == nil {
		tags = target.note.Tags
	}
	return applied{
		id: target.id, slug: target.slug, changed: true,
		files:   []string{note.FileName(target.id, target.slug)},
		summary: "removed  " + summary(target.id, target.slug, tags),
	}, nil
}
