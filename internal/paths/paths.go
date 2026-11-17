// Package paths holds the XDG base directories, honoured on every platform
// including macOS. Per the spec a variable counts only when it holds an
// absolute path.
package paths

import (
	"errors"
	"fmt"
	"io/fs"
	"os"
	"path/filepath"
	"strings"
)

// Paths is the four XDG roots, each already suffixed with `noda`.
type Paths struct {
	config string
	data   string
	state  string
	cache  string
}

// New reads the roots from the environment through lookup, so nothing below
// cmd/noda touches the process environment.
func New(lookup func(key string) (string, bool)) (Paths, error) {
	home, _ := lookup("HOME")
	if home == "" {
		return Paths{}, errors.New("HOME is not set")
	}
	root := func(variable, fallback string) string {
		value, _ := lookup(variable)
		return resolve(value, home, fallback)
	}
	return Paths{
		config: root("XDG_CONFIG_HOME", ".config"),
		data:   root("XDG_DATA_HOME", ".local/share"),
		state:  root("XDG_STATE_HOME", ".local/state"),
		cache:  root("XDG_CACHE_HOME", ".cache"),
	}, nil
}

// Rooted puts all four roots under one directory, for tests.
func Rooted(root string) Paths {
	return Paths{
		config: filepath.Join(root, "config", "noda"),
		data:   filepath.Join(root, "data", "noda"),
		state:  filepath.Join(root, "state", "noda"),
		cache:  filepath.Join(root, "cache", "noda"),
	}
}

// resolve is the XDG rule: an absolute value is used, anything else (relative,
// empty, unset) gives the default under home.
func resolve(value, home, fallback string) string {
	if filepath.IsAbs(value) {
		return filepath.Join(value, "noda")
	}
	return filepath.Join(home, filepath.FromSlash(fallback), "noda")
}

func (p Paths) ConfigDir() string { return p.config }

func (p Paths) CacheDir() string { return p.cache }

func (p Paths) NotebooksDir() string { return filepath.Join(p.data, "notebooks") }

func (p Paths) NotebookDir(name string) string { return filepath.Join(p.NotebooksDir(), name) }

// ActiveFile is in state, not in the synced data.
func (p Paths) ActiveFile() string { return filepath.Join(p.state, "active") }

// CreateDirs makes every directory noda writes to.
func (p Paths) CreateDirs() error {
	for _, dir := range []string{p.config, p.state, p.cache, p.NotebooksDir()} {
		if err := os.MkdirAll(dir, 0o777); err != nil {
			return err
		}
	}
	return nil
}

const noActiveNotebook = "no active notebook — run `noda init` first"

// ActiveNotebook reads the pointer. An unreadable pointer is reported as such:
// `noda init` will not fix it.
func (p Paths) ActiveNotebook() (string, error) {
	file := p.ActiveFile()
	content, err := os.ReadFile(file)
	if errors.Is(err, fs.ErrNotExist) {
		return "", errors.New(noActiveNotebook)
	}
	if err != nil {
		// The path error already names the file; say it once.
		var pathErr *fs.PathError
		if errors.As(err, &pathErr) {
			err = pathErr.Err
		}
		return "", fmt.Errorf("%s: %w", file, err)
	}
	name := strings.TrimSpace(string(content))
	if name == "" {
		return "", errors.New(noActiveNotebook)
	}
	return name, nil
}

// SetActiveNotebook points at name.
func (p Paths) SetActiveNotebook(name string) error {
	if err := os.MkdirAll(p.state, 0o777); err != nil {
		return err
	}
	return os.WriteFile(p.ActiveFile(), []byte(name+"\n"), 0o666)
}
