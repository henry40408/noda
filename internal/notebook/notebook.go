// Package notebook is a git repository of Markdown files; every mutation is a
// commit. A note's identity is its filename, `<id>-<slug>.md`, and no
// bookkeeping file is committed alongside, so there is nothing to conflict on.
package notebook

import (
	"context"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"time"

	"github.com/henry40408/noda/internal/clock"
	"github.com/henry40408/noda/internal/config"
	"github.com/henry40408/noda/internal/gitx"
	"github.com/henry40408/noda/internal/note"
	"github.com/henry40408/noda/internal/paths"
	"github.com/henry40408/noda/internal/remote"
)

const remoteName = "origin"

// NotFound is matched by `path` and `backlinks`, which widen only this failure
// because they were asked about a file too.
const NotFound = "note not found"

// ReadmeFile is spelled exactly, so no other file is excused from being an
// orphan.
const ReadmeFile = "README.md"

// Author is who commits, as `config.toml` names them.
type Author struct{ Name, Email string }

// Notebook is one notebook on disk.
type Notebook struct {
	Name string
	Path string
	// Interactive lets ssh ask for a passphrase or a host key at the terminal.
	// It is off by default: a web errand would hang there, and a full-screen
	// interface would be drawn over.
	Interactive bool

	git    *gitx.Git
	author *Author
	// sign is nil when git's `commit.gpgsign` decides. It is resolved at the
	// commit, so a misconfigured `gpg.format` stops `noda add` rather than
	// `noda ls`.
	sign *bool
	// now is replaced by tests that need fixed commit times.
	now func() time.Time
}

// ValidateName refuses what would escape the data directory: notebook names
// become directory names.
func ValidateName(name string) error {
	if name == "" || strings.ContainsAny(name, `/\`) || strings.Contains(name, "..") {
		return fmt.Errorf("invalid notebook name: %s", name)
	}
	return nil
}

// Create makes a notebook with an empty first commit, because HEAD has to name
// something before a branch can be pushed or compared against a remote.
func Create(g *gitx.Git, p paths.Paths, name string) (*Notebook, error) {
	if err := ValidateName(name); err != nil {
		return nil, err
	}
	path := p.NotebookDir(name)
	if _, err := os.Stat(path); err == nil {
		return nil, fmt.Errorf("notebook already exists: %s", name)
	}
	if err := os.MkdirAll(path, 0o777); err != nil {
		return nil, err
	}
	values, err := gitx.LoadConfig(context.Background(), g, "")
	if err != nil {
		return nil, err
	}
	notebook := newNotebook(g, p, name)
	_, err = notebook.run(gitx.Cmd{Args: []string{"init", "--quiet", "--initial-branch=" + initialBranch(values)}})
	if err != nil {
		return nil, err
	}
	if err := notebook.Commit(nil, "chore: initialize notebook"); err != nil {
		return nil, err
	}
	return notebook, nil
}

// initialBranch is `init.defaultBranch`, else what `git init` falls back to.
func initialBranch(values gitx.ConfigValues) string {
	configured, _ := values.Get("init.defaultBranch")
	if configured = strings.TrimSpace(configured); configured != "" {
		return configured
	}
	return "master"
}

// Open finds an existing notebook.
func Open(g *gitx.Git, p paths.Paths, name string) (*Notebook, error) {
	if err := ValidateName(name); err != nil {
		return nil, err
	}
	if _, err := os.Stat(filepath.Join(p.NotebookDir(name), ".git")); err != nil {
		return nil, fmt.Errorf("notebook not found: %s — run `noda init` or `noda notebook add %s`", name, name)
	}
	return newNotebook(g, p, name), nil
}

// OpenActive opens the notebook the commands act on.
func OpenActive(g *gitx.Git, p paths.Paths) (*Notebook, error) {
	name, err := ActiveName(p)
	if err != nil {
		return nil, err
	}
	return Open(g, p, name)
}

func newNotebook(g *gitx.Git, p paths.Paths, name string) *Notebook {
	author, sign := commitSettings(p)
	tz, set := g.Getenv("TZ")
	return &Notebook{
		Name:   name,
		Path:   p.NotebookDir(name),
		git:    g,
		author: author,
		sign:   sign,
		now:    func() time.Time { return clock.LocalNow(tz, set) },
	}
}

// commitSettings is who to commit as and whether to sign, from one config
// read. A malformed author is ignored here; `noda config` reports it.
func commitSettings(p paths.Paths) (*Author, *bool) {
	settings, err := config.Load(p)
	if err != nil {
		return nil, nil
	}
	var author *Author
	if text, ok := settings.Get("author"); ok {
		if name, email, ok := config.AuthorParts(text); ok {
			author = &Author{Name: name, Email: email}
		}
	}
	if value, set := settings.Sign(); set {
		return author, &value
	}
	return author, nil
}

// ActiveName is the notebook the commands act on: the state pointer, else the
// configured default if it exists.
func ActiveName(p paths.Paths) (string, error) {
	name, missing := p.ActiveNotebook()
	if missing == nil {
		return name, nil
	}
	fallback := config.DefaultNotebook
	if settings, err := config.Load(p); err == nil {
		if configured, ok := settings.Get("notebook"); ok {
			fallback = configured
		}
	}
	if !Exists(p, fallback) {
		return "", missing
	}
	return fallback, nil
}

// Exists reports whether name is a notebook.
func Exists(p paths.Paths, name string) bool {
	if ValidateName(name) != nil {
		return false
	}
	_, err := os.Stat(filepath.Join(p.NotebookDir(name), ".git"))
	return err == nil
}

// List is the notebooks, sorted. A directory that is not a git repo is skipped,
// not reported.
func List(p paths.Paths) ([]string, error) {
	entries, err := os.ReadDir(p.NotebooksDir())
	if errors.Is(err, os.ErrNotExist) {
		return nil, nil
	}
	if err != nil {
		return nil, err
	}
	var names []string
	for _, entry := range entries {
		if _, err := os.Stat(filepath.Join(p.NotebooksDir(), entry.Name(), ".git")); err == nil {
			names = append(names, entry.Name())
		}
	}
	sort.Strings(names)
	return names, nil
}

// Branch is the current branch, or `HEAD` when detached.
func (n *Notebook) Branch() (string, error) {
	result, err := n.run(gitx.Cmd{Args: []string{"symbolic-ref", "--quiet", "--short", "HEAD"}, ReadOnly: true, OKCodes: []int{0, 1}})
	if err != nil {
		return "", err
	}
	if result.Code == 1 {
		return "HEAD", nil
	}
	return strings.TrimSpace(string(result.Stdout)), nil
}

// SetRemote points the notebook at url, replacing any remote already
// configured.
func (n *Notebook) SetRemote(url string) error {
	if _, ok := n.configuredRemote(); ok {
		_, err := n.run(gitx.Cmd{Args: []string{"remote", "set-url", remoteName, "--", url}})
		return err
	}
	_, err := n.run(gitx.Cmd{Args: []string{"remote", "add", remoteName, "--", url}})
	return err
}

// configuredRemote is the URL of the remote exactly as configured: `git remote
// get-url` would apply `url.*.insteadOf`, which is not what a person set.
func (n *Notebook) configuredRemote() (string, bool) {
	result, err := n.run(gitx.Cmd{Args: []string{"config", "--get", "remote." + remoteName + ".url"}, ReadOnly: true, OKCodes: []int{0, 1}})
	if err != nil || result.Code != 0 {
		return "", false
	}
	return strings.TrimRight(string(result.Stdout), "\n"), true
}

// RemoteURL is the configured remote with its credentials taken out, here
// rather than per screen so a new screen cannot leak them. Fetch and push use
// the URL as configured.
func (n *Notebook) RemoteURL() (string, bool) {
	url, ok := n.configuredRemote()
	if !ok {
		return "", false
	}
	return remote.Redact(url), true
}

// NotePath is where a note's file is.
func (n *Notebook) NotePath(id, slug string) string {
	return filepath.Join(n.Path, note.FileName(id, slug))
}

// run executes git in the notebook's directory.
func (n *Notebook) run(cmd gitx.Cmd) (gitx.Result, error) {
	if cmd.Dir == "" {
		cmd.Dir = n.Path
	}
	return n.git.Run(context.Background(), cmd)
}

// read runs a read-only command and returns its output, trimmed of the
// newline at the end.
func (n *Notebook) read(args ...string) (string, error) {
	result, err := n.run(gitx.Cmd{Args: args, ReadOnly: true})
	if err != nil {
		return "", err
	}
	return strings.TrimSuffix(string(result.Stdout), "\n"), nil
}
