package cmd

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"github.com/henry40408/noda/internal/note"
	"github.com/henry40408/noda/internal/notebook"
)

// AddOptions is what `add` is given. A nil Content opens the editor.
type AddOptions struct {
	Title   *string
	Content *string
	Tags    []string
}

// Add creates a note and commits it.
func Add(e Env, opts AddOptions) (string, error) {
	active, err := e.openActive()
	if err != nil {
		return "", err
	}

	// Before the editor opens, so a bad title is not found after composing.
	// AddIn checks again, being reachable on its own.
	if opts.Title != nil {
		if err := note.ValidateTitle(*opts.Title); err != nil {
			return "", err
		}
	}
	if _, err := cleanTags(opts.Tags); err != nil {
		return "", err
	}

	body := ""
	if opts.Content != nil {
		body = *opts.Content
	} else {
		title := ""
		if opts.Title != nil {
			title = *opts.Title
		}
		body, err = e.composeInEditor(title, opts.Title != nil)
		if err != nil {
			return "", err
		}
	}
	return AddIn(active, AddOptions{Title: opts.Title, Content: &body, Tags: opts.Tags})
}

// AddIn is Add in an already-open notebook with the body written: it never
// opens an editor, so `noda web` can call it.
func AddIn(active *notebook.Notebook, opts AddOptions) (string, error) {
	if opts.Title != nil {
		if err := note.ValidateTitle(*opts.Title); err != nil {
			return "", err
		}
	}
	tags, err := cleanTags(opts.Tags)
	if err != nil {
		return "", err
	}
	body := strings.ReplaceAll(*opts.Content, "\r\n", "\n")
	title, err := chooseTitle(opts.Title, body)
	if err != nil {
		return "", err
	}

	slug := note.Slugify(title)
	taken, err := active.TakenIDs()
	if err != nil {
		return "", err
	}
	id := note.MintID(taken)
	// Both stamps, so no reader has to infer `updated` from `created`.
	now := note.Now()
	written := note.Note{
		Title:   title,
		Tags:    tags,
		Created: &now,
		Updated: &now,
		Body:    strings.TrimLeft(body, "\n"),
	}

	file := note.FileName(id, slug)
	if err := os.WriteFile(filepath.Join(active.Path, file), []byte(written.Render()), 0o666); err != nil {
		return "", err
	}
	if err := active.Commit([]string{file}, "add: "+slug); err != nil {
		return "", err
	}
	return summary(id, slug, written.Tags), nil
}

func chooseTitle(given *string, body string) (string, error) {
	if given != nil && strings.TrimSpace(*given) != "" {
		return strings.TrimSpace(*given), nil
	}
	if derived, ok := deriveTitle(body); ok {
		return derived, nil
	}
	return "", errors.New("aborted: the note is empty, so it has no title")
}

// deriveTitle is the first non-empty line, with any Markdown heading marker
// stripped.
func deriveTitle(body string) (string, bool) {
	for _, line := range strings.Split(body, "\n") {
		if line = strings.TrimSpace(strings.TrimLeft(line, "#")); line != "" {
			return line, true
		}
	}
	return "", false
}

// cleanTags trims them, as the frontmatter reads back, and refuses any that
// cannot round-trip.
func cleanTags(tags []string) ([]string, error) {
	cleaned := make([]string, 0, len(tags))
	for _, tag := range tags {
		tag = strings.TrimSpace(tag)
		if err := note.ValidateTag(tag); err != nil {
			return nil, err
		}
		cleaned = append(cleaned, tag)
	}
	return cleaned, nil
}

// summary is the one-line acknowledgement every mutating command prints.
func summary(id, slug string, tags []string) string {
	if len(tags) == 0 {
		return fmt.Sprintf("%s  %s", id, slug)
	}
	return fmt.Sprintf("%s  %s  [%s]", id, slug, strings.Join(tags, ", "))
}
