package cmd

import (
	"fmt"
	"os"

	"github.com/henry40408/noda/internal/note"
	"github.com/henry40408/noda/internal/notebook"
)

// Edit opens a note in the editor and commits whatever was saved.
func Edit(e Env, key string, touch Touch) (string, error) {
	return EditWith(e, key, e.configuredEditor(), touch)
}

// EditWith names the editor, so tests can drive the command without changing
// the process environment.
func EditWith(e Env, key, editor string, touch Touch) (string, error) {
	active, err := e.openActive()
	if err != nil {
		return "", err
	}
	located, err := locate(active, key)
	if err != nil {
		return "", err
	}
	before, err := os.ReadFile(located.path)
	if err != nil {
		return "", err
	}
	if err := e.runEditor(editor, located.path); err != nil {
		return "", err
	}
	return settle(active, located, string(before), touch)
}

// RewriteIn is Edit with the new body in hand, for the browser. Only the body:
// the frontmatter is left as found, and title and tags have their own
// commands.
func RewriteIn(active *notebook.Notebook, key, body string, touch Touch) (string, error) {
	located, err := locate(active, key)
	if err != nil {
		return "", err
	}
	before, err := os.ReadFile(located.path)
	if err != nil {
		return "", err
	}
	after, ok := note.SetBody(string(before), body)
	if !ok {
		return "", fmt.Errorf("%s: not a note", located.path)
	}
	if err := os.WriteFile(located.path, []byte(after), 0o666); err != nil {
		return "", err
	}
	return settle(active, located, string(before), touch)
}

// settle reads the file back, refuses it if it is no longer a note, stamps it
// and commits it: shared by Edit and RewriteIn.
func settle(active *notebook.Notebook, located found, before string, touch Touch) (string, error) {
	afterBytes, err := os.ReadFile(located.path)
	if err != nil {
		return "", err
	}
	after := string(afterBytes)
	if after == before {
		return located.slug + "  (unchanged)", nil
	}

	// A rejected edit stays on disk, never silently discarded. The id needs no
	// guard: it lives in the filename.
	edited, err := note.Parse(after)
	if err != nil {
		return "", fmt.Errorf("%s: %w\nthe file was left as you saved it and was not committed", located.path, err)
	}

	// Only `updated` is set in place; the rest is committed as saved. Under
	// `--no-touch` nothing is written back, even an `updated` the user edited.
	if touch == TouchStamp {
		stamped, _ := note.SetField(after, "updated", note.Now())
		if stamped != after {
			if err := os.WriteFile(located.path, []byte(stamped), 0o666); err != nil {
				return "", err
			}
		}
	}

	file := note.FileName(located.id, located.slug)
	if err := active.Commit([]string{file}, "edit: "+located.slug); err != nil {
		return "", err
	}
	return summary(located.id, located.slug, edited.Tags), nil
}
