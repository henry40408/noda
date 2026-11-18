package cmd

import (
	"path/filepath"
)

// Path is for the tools noda does not wrap: `pandoc "$(noda path meeting-notes)"`.
// Without a key it is the notebook's directory.
func Path(e Env, key string) (string, error) {
	active, err := e.openActive()
	if err != nil {
		return "", err
	}
	if key == "" {
		return active.Path + "\n", nil
	}
	named, err := resolveSubject(active, key)
	if err != nil {
		return "", err
	}
	if named.isNote {
		return active.NotePath(named.resolved.ID, named.resolved.Slug) + "\n", nil
	}
	return filepath.Join(active.Path, key) + "\n", nil
}
