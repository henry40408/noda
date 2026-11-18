package cmd

import (
	"fmt"

	"github.com/henry40408/noda/internal/note"
	"github.com/henry40408/noda/internal/notebook"
)

// Pin floats a note to the top of every listing, or lets it back down.
func Pin(e Env, key string, pinned bool, touch Touch) (string, error) {
	active, err := e.openActive()
	if err != nil {
		return "", err
	}
	return PinIn(active, key, pinned, touch)
}

// PinIn is Pin in a notebook the caller already has open.
func PinIn(active *notebook.Notebook, key string, pinned bool, touch Touch) (string, error) {
	done, err := applyPin(active, key, pinned, touch)
	if err != nil {
		return "", err
	}
	if done.changed {
		verb := "unpin"
		if pinned {
			verb = "pin"
		}
		if err := active.Commit(done.files, verb+": "+done.slug); err != nil {
			return "", err
		}
	}
	return done.summary, nil
}

func applyPin(active *notebook.Notebook, key string, pinned bool, touch Touch) (applied, error) {
	located, err := locate(active, key)
	if err != nil {
		return applied{}, err
	}
	changed := located.note
	before := changed.Pinned

	// Unpinning drops the line rather than writing `false`, so pin + unpin
	// round-trips to the original file.
	changed.Pinned = nil
	if pinned {
		value := note.PinnedValue
		changed.Pinned = &value
	}

	mark := "unpinned"
	if pinned {
		mark = "pinned"
	}
	line := fmt.Sprintf("%s  %s", summary(located.id, located.slug, changed.Tags), mark)
	result := applied{id: located.id, slug: located.slug, files: []string{note.FileName(located.id, located.slug)}}
	if equalText(changed.Pinned, before) {
		result.summary = line + "  (no change)"
		return result, nil
	}
	if err := writeNote(located.path, changed, touch); err != nil {
		return applied{}, err
	}
	result.summary = line
	result.changed = true
	return result, nil
}

func equalText(a, b *string) bool {
	if a == nil || b == nil {
		return a == b
	}
	return *a == *b
}
