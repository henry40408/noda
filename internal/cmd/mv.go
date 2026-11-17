package cmd

import (
	"errors"
	"os"
	"path/filepath"
	"slices"
	"strings"

	"github.com/henry40408/noda/internal/note"
	"github.com/henry40408/noda/internal/notebook"
)

// Rename is what `mv` changes.
type Rename struct {
	Key         string
	NewTitle    string
	UpdateLinks bool
	Touch       Touch
}

// Mv retitles a note. The slug follows the title; the id never moves, so links
// to it go stale rather than broken (`backlinks` still finds them by id). A
// retitle reports them; UpdateLinks rewrites them, opt-in because it edits
// notes the command was not pointed at.
func Mv(e Env, rename Rename) (string, error) {
	active, err := e.openActive()
	if err != nil {
		return "", err
	}
	return MvIn(active, rename)
}

// MvIn is Mv in a notebook the caller already has open.
func MvIn(active *notebook.Notebook, rename Rename) (string, error) {
	located, err := locate(active, rename.Key)
	if err != nil {
		return "", err
	}
	title := strings.TrimSpace(rename.NewTitle)
	if title == "" {
		return "", errors.New("a note needs a title")
	}
	if err := note.ValidateTitle(title); err != nil {
		return "", err
	}

	slug := note.Slugify(title)
	moved := located.note
	moved.Title = title
	file := note.FileName(located.id, slug)
	if err := writeNote(filepath.Join(active.Path, file), moved, rename.Touch); err != nil {
		return "", err
	}
	changed := []string{file}
	slugMoved := slug != located.slug
	if slugMoved {
		if err := os.Remove(located.path); err != nil {
			return "", err
		}
		changed = append(changed, note.FileName(located.id, located.slug))
	}

	var found *retarget
	// Reads every note, so only when the slug moved or the flag asks: the
	// latter repairs links left stale by an earlier rename.
	if slugMoved || rename.UpdateLinks {
		result, err := retargetNote(active, located.id, file, rename.UpdateLinks)
		if err != nil {
			return "", err
		}
		found = &result
		changed = append(changed, result.rewritten...)
	}

	// A self-linking note is here twice: renamed, and rewritten.
	slices.Sort(changed)
	changed = slices.Compact(changed)
	if err := active.Commit(changed, "mv: "+located.slug+" -> "+slug); err != nil {
		return "", err
	}

	out := summary(located.id, slug, moved.Tags)
	if found == nil {
		return out, nil
	}
	// By id, because a link can be two renames behind.
	lines := found.describe(located.id+" by an older name", rename.UpdateLinks)
	if lines == "" {
		return out, nil
	}
	return out + "\n" + strings.TrimRight(lines, " \t\r\n"), nil
}

// retargetNote reads the notebook after the rename, so a self-link is read
// under its new name.
func retargetNote(active *notebook.Notebook, id, file string, updateLinks bool) (retarget, error) {
	notes, _, err := active.Inventory()
	if err != nil {
		return retarget{}, err
	}
	folded := note.NormalizeID(id)
	names := func(target string) bool { return notebook.LinkedNoteID(target) == folded }
	return retargetLinks(active, notes, names, file, updateLinks)
}
