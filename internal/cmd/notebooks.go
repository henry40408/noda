package cmd

import (
	"errors"
	"fmt"
	"os"
	"strings"

	"github.com/henry40408/noda/internal/notebook"
	"github.com/henry40408/noda/internal/style"
)

// NotebookAdd creates a notebook (a new git repo), optionally pointed at a
// remote.
func NotebookAdd(e Env, name, remoteURL string) (string, error) {
	if err := os.MkdirAll(e.Paths.NotebooksDir(), 0o777); err != nil {
		return "", err
	}
	created, err := notebook.Create(e.Git, e.Paths, name)
	if err != nil {
		return "", err
	}
	if remoteURL != "" {
		if err := created.SetRemote(remoteURL); err != nil {
			return "", err
		}
	}
	return fmt.Sprintf("created notebook `%s` at %s", name, created.Path), nil
}

// notebookRow is one `notebook ls` line. A remote never synced pads its column.
type notebookRow struct {
	name      string
	remote    string
	hasRemote bool
	drift     *notebook.Drift
}

// NotebookLs lists the notebooks, the active one marked `*`, each with its
// drift from its remote. Drift rather than Status keeps it cheap per row (two
// refs compared, no working-tree walk), and nothing touches the network.
func NotebookLs(e Env) (string, error) {
	names, err := notebook.List(e.Paths)
	if err != nil {
		return "", err
	}
	if len(names) == 0 {
		return "", nil
	}
	active, _ := notebook.ActiveName(e.Paths)

	rows := make([]notebookRow, len(names))
	nameWidth, remoteWidth := 0, 0
	for i, name := range names {
		rows[i] = readNotebookRow(e, name)
		nameWidth = max(nameWidth, DisplayWidth(name))
		remoteWidth = max(remoteWidth, DisplayWidth(rows[i].remote))
	}

	var out strings.Builder
	for _, row := range rows {
		marker := " "
		if row.name == active {
			marker = "*"
		}
		// Muted unless there is something to push or pull.
		words := Standing(row.hasRemote, row.drift)
		if !row.hasRemote || row.drift == nil || (row.drift.Ahead == 0 && row.drift.Behind == 0) {
			words = style.Muted.Paint(words)
		}
		line := marker + " " + pad(row.name, nameWidth) + "  "
		if row.hasRemote {
			line += pad(row.remote, remoteWidth) + "  "
		}
		out.WriteString(trimEnd(line+words) + "\n")
	}
	return out.String(), nil
}

func readNotebookRow(e Env, name string) notebookRow {
	row := notebookRow{name: name}
	opened, err := e.open(name)
	if err != nil {
		return row
	}
	row.remote, row.hasRemote = opened.RemoteURL()
	if branch, err := opened.Branch(); err == nil {
		row.drift, _ = opened.Drift(branch)
	}
	return row
}

// NotebookRm is not a commit and cannot be undone, so the active notebook is
// refused and any other is confirmed first.
func NotebookRm(e Env, name string, force bool) (string, error) {
	if err := notebook.ValidateName(name); err != nil {
		return "", err
	}
	if !notebook.Exists(e.Paths, name) {
		return "", fmt.Errorf("notebook not found: %s", name)
	}
	if active, err := notebook.ActiveName(e.Paths); err == nil && active == name {
		return "", fmt.Errorf("`%s` is the active notebook — switch with `noda use <name>` first", name)
	}

	if !force {
		confirmed, err := e.confirmRemoval(name)
		if err != nil {
			return "", err
		}
		if !confirmed {
			return fmt.Sprintf("kept notebook `%s`", name), nil
		}
	}
	dir := e.Paths.NotebookDir(name)
	if err := os.RemoveAll(dir); err != nil {
		return "", err
	}
	return fmt.Sprintf("removed notebook `%s` and its history at %s", name, dir), nil
}

func (e Env) confirmRemoval(name string) (bool, error) {
	if e.Confirm == nil {
		return false, errors.New("there is no terminal to confirm at — pass `--force` if you mean it")
	}
	count := 0
	if opened, err := e.open(name); err == nil {
		if notes, err := opened.Notes(); err == nil {
			count = len(notes)
		}
	}
	return e.Confirm(fmt.Sprintf("delete notebook `%s` — %d %s and their whole history? this is not a commit and cannot be undone [y/N] ",
		name, count, plural(count, "note", "notes")))
}

// NotebookRename renames a notebook, carrying the active pointer with it.
func NotebookRename(e Env, old, renamed string) (string, error) {
	if err := notebook.ValidateName(old); err != nil {
		return "", err
	}
	if err := notebook.ValidateName(renamed); err != nil {
		return "", err
	}
	if !notebook.Exists(e.Paths, old) {
		return "", fmt.Errorf("notebook not found: %s", old)
	}
	if _, err := os.Stat(e.Paths.NotebookDir(renamed)); err == nil {
		return "", fmt.Errorf("notebook already exists: %s", renamed)
	}
	if err := os.Rename(e.Paths.NotebookDir(old), e.Paths.NotebookDir(renamed)); err != nil {
		return "", err
	}
	if active, err := notebook.ActiveName(e.Paths); err == nil && active == old {
		if err := e.Paths.SetActiveNotebook(renamed); err != nil {
			return "", err
		}
	}
	return fmt.Sprintf("renamed notebook `%s` to `%s`", old, renamed), nil
}

// NotebookCurrent is the active notebook's name.
func NotebookCurrent(e Env) (string, error) {
	return notebook.ActiveName(e.Paths)
}
