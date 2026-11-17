package cmd

import (
	"fmt"
	"os"

	"github.com/henry40408/noda/internal/note"
	"github.com/henry40408/noda/internal/notebook"
)

// Restore brings a note's contents back from rev, as a new commit.
func Restore(e Env, key, rev string, touch Touch) (string, error) {
	active, err := e.openActive()
	if err != nil {
		return "", err
	}
	commit, err := active.Revision(rev)
	if err != nil {
		return "", err
	}

	// find: a broken note is about to be overwritten, not read. A missing note
	// is not an error here, which is how this undoes `noda rm`.
	current, findErr := find(active, key)
	id, err := restoredID(active, current, findErr == nil, commit, key, rev)
	if err != nil {
		return "", err
	}
	slugThen, text, held, err := active.NoteAt(commit, id)
	if err != nil {
		return "", err
	}
	if !held {
		return "", fmt.Errorf("`%s` did not exist at %s — `noda log %s` shows where it did", key, rev, key)
	}

	// Only contents travel back: a present note keeps today's name, a gone one
	// returns under its old one.
	slug := slugThen
	if findErr == nil {
		slug = current.slug
	}
	restored, err := note.Parse(text)
	if err != nil {
		return "", fmt.Errorf("the copy of `%s` at %s cannot be read: %w", key, rev, err)
	}
	if findErr == nil && sameAsDisk(current.path, text, touch) {
		return summary(id, slug, restored.Tags) + "  (no change)", nil
	}

	// The file changed now; `created` stays, it is the same note.
	if touch == TouchStamp {
		stamped, _ := note.SetField(text, "updated", note.Now())
		text = stamped
	}
	if err := os.WriteFile(active.NotePath(id, slug), []byte(text), 0o666); err != nil {
		return "", err
	}
	message := fmt.Sprintf("restore: %s to %s", slug, notebook.Short(commit))
	if err := active.Commit([]string{note.FileName(id, slug)}, message); err != nil {
		return "", err
	}
	return summary(id, slug, restored.Tags), nil
}

// restoredID is the id of the note present today, else the one history knew
// by that name.
func restoredID(active *notebook.Notebook, current found, present bool, commit, key, rev string) (string, error) {
	if present {
		return current.id, nil
	}
	id, ok, err := active.IDAt(commit, key)
	if err != nil {
		return "", err
	}
	if !ok {
		return "", fmt.Errorf("note not found at %s: %s", rev, key)
	}
	return id, nil
}

// sameAsDisk says whether restoring would change nothing. `updated` is ignored
// under Stamp, or restoring the same revision twice would never say "no
// change": the first restore stamped it.
func sameAsDisk(path, restored string, touch Touch) bool {
	onDisk, err := os.ReadFile(path)
	if err != nil {
		return false
	}
	if touch == TouchKeep {
		return string(onDisk) == restored
	}
	return withoutUpdated(string(onDisk)) == withoutUpdated(restored)
}

func withoutUpdated(text string) string {
	if blanked, ok := note.SetField(text, "updated", ""); ok {
		return blanked
	}
	return text
}
