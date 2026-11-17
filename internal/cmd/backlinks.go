package cmd

import (
	"strings"

	"github.com/henry40408/noda/internal/note"
	"github.com/henry40408/noda/internal/notebook"
	"github.com/henry40408/noda/internal/style"
)

// Backlinks is the notes that link to a note or one of the notebook's files.
// It is its own command rather than an `ls` flag, because it parses every body.
func Backlinks(e Env, key string, format Format) (string, error) {
	active, err := e.openActive()
	if err != nil {
		return "", err
	}
	named, err := resolveSubject(active, key)
	if err != nil {
		return "", err
	}
	label := key
	var found []notebook.NoteFile
	if named.isNote {
		label = note.FileName(named.resolved.ID, named.resolved.Slug)
		found, err = active.BacklinksToNote(named.resolved.ID)
	} else {
		found, err = active.BacklinksToFile(key)
	}
	if err != nil {
		return "", err
	}

	switch format {
	// Before the empty check: a program asking for JSON gets a document.
	case FormatJSON:
		return backlinksJSON(active.Name, label, found), nil
	// No `--null`: an id has no spaces to protect.
	case FormatQuiet:
		var out strings.Builder
		for _, file := range found {
			out.WriteString(file.ID + "\n")
		}
		return out.String(), nil
	}
	if len(found) == 0 {
		return style.Muted.Paint("nothing links to " + label), nil
	}

	// `ls`'s row shape.
	idWidth := 0
	for _, file := range found {
		idWidth = max(idWidth, DisplayWidth(file.ID))
	}
	var out strings.Builder
	for _, file := range found {
		out.WriteString(trimEnd(pad(file.ID, idWidth)+"  "+file.Note.Title) + "\n")
	}
	return out.String(), nil
}

// backlinksJSON carries the resolved filename as `target`, so a script asking
// by slug learns what it resolved to.
func backlinksJSON(notebookName, target string, found []notebook.NoteFile) string {
	var out strings.Builder
	out.WriteString(`{"notebook":` + jsonString(notebookName) + `,"target":` + jsonString(target) + `,"backlinks":[`)
	for i, file := range found {
		if i > 0 {
			out.WriteByte(',')
		}
		out.WriteString(`{"id":` + jsonString(file.ID) + `,"slug":` + jsonString(file.Slug) +
			`,"file":` + jsonString(note.FileName(file.ID, file.Slug)) + `,"title":` + jsonString(file.Note.Title) + `}`)
	}
	out.WriteString("]}\n")
	return out.String()
}
