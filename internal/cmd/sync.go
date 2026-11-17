package cmd

import (
	"errors"
	"fmt"
	"strings"

	"github.com/henry40408/noda/internal/clock"
	"github.com/henry40408/noda/internal/notebook"
	"github.com/henry40408/noda/internal/remote"
	"github.com/henry40408/noda/internal/style"
)

// RemoteSet points the active notebook at url, replacing any remote already
// set.
func RemoteSet(e Env, url string) (string, error) {
	url = strings.TrimSpace(url)
	if url == "" {
		return "", errors.New("a remote needs a URL")
	}
	active, err := e.openActive()
	if err != nil {
		return "", err
	}
	if err := active.SetRemote(url); err != nil {
		return "", err
	}
	// Redacted even though just typed: this line stays in the scrollback.
	return active.Name + "  " + remote.Redact(url), nil
}

// RemoteShow is the active notebook's remote.
func RemoteShow(e Env) (string, error) {
	active, err := e.openActive()
	if err != nil {
		return "", err
	}
	url, ok := active.RemoteURL()
	if !ok {
		return "", fmt.Errorf("notebook `%s` has no remote — set one with `noda remote set <url>`", active.Name)
	}
	return url, nil
}

// Pull fetches and integrates the remote branch.
func Pull(e Env) (string, error) {
	active, err := e.openActive()
	if err != nil {
		return "", err
	}
	return PullIn(active)
}

// PullIn is Pull in a notebook the caller already has open.
func PullIn(active *notebook.Notebook) (string, error) { return active.Pull() }

// Push sends the current branch to the remote.
func Push(e Env) (string, error) {
	active, err := e.openActive()
	if err != nil {
		return "", err
	}
	return PushIn(active)
}

// PushIn is Push in a notebook the caller already has open.
func PushIn(active *notebook.Notebook) (string, error) { return active.Push() }

// Sync is commit, pull, push, in that order: a pull before the commit merges
// into a tree missing local work, and a push before the pull is refused.
func Sync(e Env) (string, error) {
	active, err := e.openActive()
	if err != nil {
		return "", err
	}
	return SyncIn(active)
}

// SyncIn is Sync in a notebook the caller already has open.
func SyncIn(active *notebook.Notebook) (string, error) {
	var lines []string
	committed, err := active.CommitAll("sync: local changes")
	if err != nil {
		return "", err
	}
	if committed {
		lines = append(lines, "commit: local changes")
	}
	pulled, err := active.Pull()
	if err != nil {
		return "", err
	}
	pushed, err := active.Push()
	if err != nil {
		return "", err
	}
	return strings.Join(append(lines, pulled, pushed), "\n"), nil
}

// Snapshot marks the notebook as it stands, committing the working tree first
// so the snapshot includes what is on disk. The message defaults to the name.
func Snapshot(e Env, name, message string) (string, error) {
	active, err := e.openActive()
	if err != nil {
		return "", err
	}
	var lines []string
	committed, err := active.CommitAll("snapshot: " + name)
	if err != nil {
		return "", err
	}
	if committed {
		lines = append(lines, "commit: local changes")
	}
	if message == "" {
		message = name
	}
	target, err := active.Snapshot(name, message)
	if err != nil {
		return "", err
	}
	lines = append(lines, fmt.Sprintf("snapshot: %s -> %s", name, notebook.Short(target)))
	return strings.Join(lines, "\n"), nil
}

// SnapshotLs is name, time, commit, message: time before commit, as `deleted`
// has them.
func SnapshotLs(e Env) (string, error) {
	active, err := e.openActive()
	if err != nil {
		return "", err
	}
	snapshots, err := active.Snapshots()
	if err != nil {
		return "", err
	}
	if len(snapshots) == 0 {
		return style.Muted.Paint("no snapshots — take one with `noda snapshot <name>`"), nil
	}
	width := 0
	for _, snapshot := range snapshots {
		width = max(width, DisplayWidth(snapshot.Name))
	}
	var out strings.Builder
	for _, snapshot := range snapshots {
		out.WriteString(trimEnd(pad(snapshot.Name, width)+"  "+
			style.Muted.Paint(clock.FormatTime(snapshot.Seconds, snapshot.OffsetMinutes))+"  "+
			style.Commit.Paint(snapshot.ShortTarget())+"  "+snapshot.Message) + "\n")
	}
	return out.String(), nil
}

// Clone copies a remote notebook. The name defaults to the repository's own.
func Clone(e Env, url, name string) (string, error) {
	if name == "" {
		name = remote.NameFromURL(url)
	}
	if name == "" {
		return "", fmt.Errorf("cannot tell what to call the notebook from `%s` — pass a name", remote.Redact(url))
	}
	cloned, err := notebook.Clone(e.Git, e.Paths, notebook.CloneOptions{URL: url, Name: name, Interactive: e.Interactive})
	if err != nil {
		return "", err
	}
	notes, err := cloned.Notes()
	if err != nil {
		return "", err
	}
	return fmt.Sprintf("cloned `%s` (%d notes) to %s\nswitch to it with `noda use %s`", name, len(notes), cloned.Path, name), nil
}
