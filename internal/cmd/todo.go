package cmd

import (
	"os"
	"slices"
	"strings"

	"github.com/henry40408/noda/internal/clock"
	"github.com/henry40408/noda/internal/note"
	"github.com/henry40408/noda/internal/style"
	"github.com/henry40408/noda/internal/todo"
)

// Today is the local date, `YYYY-MM-DD`. Public so the browser agrees with
// `noda todo` on what is overdue.
func Today() string {
	tz, set := os.LookupEnv("TZ")
	return clock.LocalNow(tz, set).Format("2006-01-02")
}

// todoRow is one item with the note it came from.
type todoRow struct {
	id, slug string
	item     todo.Item
}

// Todo is every unticked checkbox in the notebook, soonest due first.
//
// There is no `noda done`: an item has no stable address (line numbers move,
// text collides), and ids would make the file noda-only. `noda edit` it.
func Todo(e Env, asJSON bool) (string, error) {
	return TodoOn(e, asJSON, Today())
}

// TodoOn is Todo with today given, so a test need not freeze the clock.
func TodoOn(e Env, asJSON bool, today string) (string, error) {
	active, err := e.openActive()
	if err != nil {
		return "", err
	}
	notes, err := active.Notes()
	if err != nil {
		return "", err
	}
	var rows []todoRow
	for _, file := range notes {
		for _, item := range todo.Items(file.Note.Body) {
			rows = append(rows, todoRow{id: file.ID, slug: file.Slug, item: item})
		}
	}
	slices.SortStableFunc(rows, func(left, right todoRow) int {
		return todo.Compare(left.slug, left.item, right.slug, right.item)
	})

	// Before the empty check: JSON always gets a document.
	if asJSON {
		return todoJSON(active.Name, rows), nil
	}
	if len(rows) == 0 {
		return style.Muted.Paint("nothing to do"), nil
	}
	return todoTable(rows, today), nil
}

func todoTable(rows []todoRow, today string) string {
	idWidth, slugWidth := 0, 0
	for _, row := range rows {
		idWidth = max(idWidth, DisplayWidth(row.id))
		slugWidth = max(slugWidth, DisplayWidth(row.slug))
	}
	var out strings.Builder
	for _, row := range rows {
		// Never truncated: a cut-off item would have to be opened to read.
		due := strings.Repeat(" ", clock.DateWidth)
		if row.item.Due != nil && row.item.Overdue(today) {
			due = style.Overdue.Paint(*row.item.Due)
		} else if row.item.Due != nil {
			due = style.Muted.Paint(*row.item.Due)
		}
		out.WriteString(trimEnd(pad(row.id, idWidth)+"  "+pad(row.slug, slugWidth)+"  "+due+"  "+row.item.Text) + "\n")
	}
	return out.String()
}

// todoJSON has `due` but not `overdue`: a program has its own clock and time
// zone.
func todoJSON(notebookName string, rows []todoRow) string {
	var out strings.Builder
	out.WriteString(`{"notebook":` + jsonString(notebookName) + `,"todo":[`)
	for i, row := range rows {
		if i > 0 {
			out.WriteByte(',')
		}
		out.WriteString(`{"id":` + jsonString(row.id) + `,"slug":` + jsonString(row.slug) +
			`,"file":` + jsonString(note.FileName(row.id, row.slug)) + `,"text":` + jsonString(row.item.Text) +
			`,"due":` + jsonStamp(row.item.Due) + `}`)
	}
	out.WriteString("]}\n")
	return out.String()
}
