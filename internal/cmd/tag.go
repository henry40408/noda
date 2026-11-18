package cmd

import (
	"fmt"
	"os"
	"slices"
	"strings"

	"github.com/henry40408/noda/internal/note"
	"github.com/henry40408/noda/internal/notebook"
)

// Tag adds or removes tags. Adding a tag a note carries, or removing one it
// lacks, is a no-op, not an error.
func Tag(e Env, key string, changes []string, touch Touch) (string, error) {
	active, err := e.openActive()
	if err != nil {
		return "", err
	}
	return TagIn(active, key, changes, touch)
}

// TagIn is Tag in a notebook the caller already has open.
func TagIn(active *notebook.Notebook, key string, changes []string, touch Touch) (string, error) {
	edits, err := parseTags(changes, key)
	if err != nil {
		return "", err
	}
	done, err := applyTags(active, key, edits, touch)
	if err != nil {
		return "", err
	}
	if done.changed {
		if err := active.Commit(done.files, "tag: "+done.slug); err != nil {
			return "", err
		}
	}
	return done.summary, nil
}

// tagEdit is parsed before any note is opened, so a bad change is refused
// before the first write rather than halfway through a set.
type tagEdit struct {
	name   string
	remove bool
}

// parseTags reads `+tag` and `-tag`. A key of "" leaves the hint about where a
// flag goes out of the message.
func parseTags(changes []string, key string) ([]tagEdit, error) {
	edits := make([]tagEdit, 0, len(changes))
	for _, change := range changes {
		edit, err := parseTag(change, key)
		if err != nil {
			return nil, err
		}
		edits = append(edits, edit)
	}
	return edits, nil
}

func parseTag(change, key string) (tagEdit, error) {
	// The list takes hyphen values, so a flag after it arrives here:
	// `--no-touch` would silently remove a tag `-no-touch` nobody has.
	if strings.HasPrefix(change, "--") {
		hint := ""
		if key != "" {
			hint = fmt.Sprintf(" — `noda tag %s %s +tag`", key, change)
		}
		return tagEdit{}, fmt.Errorf("`%s` is being read as a tag: the tags take everything after them, so a flag has to come before them%s", change, hint)
	}
	if name, ok := strings.CutPrefix(change, "+"); ok {
		name = strings.TrimSpace(name)
		if name == "" {
			return tagEdit{}, fmt.Errorf("`+` needs a tag name after it")
		}
		if err := note.ValidateTag(name); err != nil {
			return tagEdit{}, err
		}
		return tagEdit{name: name}, nil
	}
	if name, ok := strings.CutPrefix(change, "-"); ok {
		name = strings.TrimSpace(name)
		if name == "" {
			return tagEdit{}, fmt.Errorf("`-` needs a tag name after it")
		}
		return tagEdit{name: name, remove: true}, nil
	}
	return tagEdit{}, fmt.Errorf("tags must be given as `+%s` to add or `-%s` to remove", change, change)
}

// applied is what a change did to one note, uncommitted: one command is one
// commit, but a browser queue is one commit for the lot, and both share this
// code.
type applied struct {
	id, slug string
	// files are relative to the notebook.
	files   []string
	summary string
	// changed is false when the note already said what it was asked to say.
	changed bool
}

func applyTags(active *notebook.Notebook, key string, edits []tagEdit, touch Touch) (applied, error) {
	located, err := locate(active, key)
	if err != nil {
		return applied{}, err
	}
	changed := located.note
	before := slices.Clone(changed.Tags)
	for _, edit := range edits {
		changed.Tags = withTag(changed.Tags, edit)
	}

	file := note.FileName(located.id, located.slug)
	result := applied{id: located.id, slug: located.slug, files: []string{file}}
	if slices.Equal(changed.Tags, before) {
		result.summary = summary(located.id, located.slug, changed.Tags) + "  (no change)"
		return result, nil
	}
	if err := writeNote(located.path, changed, touch); err != nil {
		return applied{}, err
	}
	result.summary = summary(located.id, located.slug, changed.Tags)
	result.changed = true
	return result, nil
}

func withTag(tags []string, edit tagEdit) []string {
	if edit.remove {
		return slices.DeleteFunc(tags, func(tag string) bool { return tag == edit.name })
	}
	if slices.Contains(tags, edit.name) {
		return tags
	}
	return append(tags, edit.name)
}

// writeNote saves a note, stamping `updated` unless the caller keeps it.
func writeNote(path string, changed note.Note, touch Touch) error {
	if touch == TouchStamp {
		now := note.Now()
		changed.Updated = &now
	}
	return os.WriteFile(path, []byte(changed.Render()), 0o666)
}
