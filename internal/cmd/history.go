package cmd

import (
	"fmt"
	"strings"
	"time"

	"github.com/henry40408/noda/internal/clock"
	"github.com/henry40408/noda/internal/note"
	"github.com/henry40408/noda/internal/notebook"
	"github.com/henry40408/noda/internal/style"
)

// Unpushed marks an unpushed commit in `noda log` and the TUI's log screen: the
// arrow the TUI header uses for commits ahead. A margin, not a column.
const Unpushed = "↑"

// Log is the notebook's history, or one note's, with unpushed commits marked.
func Log(e Env, key string, limit int) (string, error) {
	active, err := e.openActive()
	if err != nil {
		return "", err
	}
	// find, not locate: a note with broken frontmatter is the one whose past you
	// want.
	id := ""
	if key != "" {
		target, err := find(active, key)
		if err != nil {
			return "", err
		}
		id = target.id
	}

	branch, err := active.Branch()
	if err != nil {
		return "", err
	}
	// Empty, at no cost, when there is no remote.
	unpushed, err := active.Unpushed(branch)
	if err != nil {
		return "", err
	}
	entries, err := active.Log(id, limit)
	if err != nil {
		return "", err
	}

	shown := 0
	var out strings.Builder
	for _, entry := range entries {
		mark := " "
		if _, ahead := unpushed[entry.ID]; ahead {
			shown++
			mark = style.Muted.Paint(Unpushed)
		}
		out.WriteString(trimEnd(fmt.Sprintf("%s %s  %s  %s", mark, style.Commit.Paint(entry.ShortID()),
			style.Muted.Paint(clock.FormatTime(entry.Seconds, entry.OffsetMinutes)), entry.Summary)) + "\n")
	}

	// `-n` can hide unpushed commits, so say how many. Whole-notebook only:
	// against one note's log the subtraction would mean nothing.
	if hidden := len(unpushed) - shown; id == "" && hidden > 0 {
		out.WriteString(style.Muted.Paint(fmt.Sprintf("%d more to push, below what `-n` shows", hidden)) + "\n")
	}
	return out.String(), nil
}

// Blame is `log`'s columns in `log`'s order, body only (see Notebook.Blame).
// No line numbers: in prose one looks for a paragraph, not a row.
func Blame(e Env, key string) (string, error) {
	active, err := e.openActive()
	if err != nil {
		return "", err
	}
	target, err := find(active, key)
	if err != nil {
		return "", err
	}
	lines, err := active.Blame(target.id, target.slug)
	if err != nil {
		return "", err
	}
	var out strings.Builder
	for _, line := range lines {
		when := style.Muted.Paint(pad("not committed", clock.TimeWidth))
		if line.Commit != "" {
			when = style.Muted.Paint(clock.FormatTime(line.Seconds, line.OffsetMinutes))
		}
		out.WriteString(trimEnd(style.Commit.Paint(line.ShortCommit())+"  "+when+"  "+line.Text) + "\n")
	}
	return out.String(), nil
}

// Deleted is the notes in history but no longer in the notebook, newest loss
// first. It is its own command rather than an `ls` flag because it walks all
// of history. The revision printed is the deleting commit's parent, which is
// what `restore` needs.
func Deleted(e Env, notebookName string, asJSON bool) (string, error) {
	name := notebookName
	if name == "" {
		active, err := notebook.ActiveName(e.Paths)
		if err != nil {
			return "", err
		}
		name = active
	}
	opened, err := e.open(name)
	if err != nil {
		return "", err
	}
	gone, err := opened.Deleted()
	if err != nil {
		return "", err
	}

	// Before the empty check: JSON always gets a document.
	if asJSON {
		return deletedJSON(name, gone), nil
	}
	if len(gone) == 0 {
		return "", nil
	}

	idWidth, slugWidth := 0, 0
	for _, entry := range gone {
		idWidth = max(idWidth, DisplayWidth(entry.ID))
		slugWidth = max(slugWidth, DisplayWidth(entry.Slug))
	}
	var out strings.Builder
	for _, entry := range gone {
		out.WriteString(trimEnd(pad(entry.ID, idWidth)+"  "+pad(entry.Slug, slugWidth)+"  "+
			style.Muted.Paint(clock.FormatTime(entry.RemovedAt, entry.OffsetMinutes))+"  "+
			style.Commit.Paint(entry.RestoreFromShort())+"  "+entry.Title) + "\n")
	}
	out.WriteString(style.Muted.Paint("`noda restore <note> <commit>` with the commit above brings one back"))
	return out.String(), nil
}

// deletedJSON is hand-written, as ls's is. Commit ids are in full, since an
// abbreviation can stop being unique; times are in UTC.
func deletedJSON(notebookName string, gone []notebook.Deleted) string {
	var out strings.Builder
	out.WriteString(`{"notebook":` + jsonString(notebookName) + `,"deleted":[`)
	for i, entry := range gone {
		if i > 0 {
			out.WriteByte(',')
		}
		out.WriteString(`{"id":` + jsonString(entry.ID) + `,"slug":` + jsonString(entry.Slug) +
			`,"file":` + jsonString(note.FileName(entry.ID, entry.Slug)) + `,"title":` + jsonString(entry.Title) +
			`,"removed_at":` + jsonString(rfc3339(entry.RemovedAt)) +
			`,"removed_in":` + jsonString(entry.RemovedIn) +
			`,"restore_from":` + jsonString(entry.RestoreFrom) + `}`)
	}
	out.WriteString("]}\n")
	return out.String()
}

// rfc3339 is the spelling noda writes everywhere.
func rfc3339(seconds int64) string {
	return time.Unix(seconds, 0).UTC().Format("2006-01-02T15:04:05Z")
}

// Diff is the uncommitted changes, or what the last commit changed, as a unified
// diff `git apply` will take. With remote it shows what a push would carry
// instead.
func Diff(e Env, key string, remote bool) (string, error) {
	active, err := e.openActive()
	if err != nil {
		return "", err
	}
	// find: the diff is how you find out why a note will not parse.
	file := ""
	if key != "" {
		target, err := find(active, key)
		if err != nil {
			return "", err
		}
		file = note.FileName(target.id, target.slug)
	}

	var plain string
	if remote {
		branch, err := active.Branch()
		if err != nil {
			return "", err
		}
		plain, err = active.DiffRemote(branch, file)
		if err != nil {
			return "", err
		}
	} else {
		plain, err = active.Diff(file)
		if err != nil {
			return "", err
		}
	}
	return notebook.ColorDiff(plain), nil
}
