package cmd

import (
	"fmt"

	"github.com/henry40408/noda/internal/notebook"
)

// Use sets the active notebook.
func Use(e Env, name string) (string, error) {
	if err := notebook.ValidateName(name); err != nil {
		return "", err
	}
	if !notebook.Exists(e.Paths, name) {
		return "", fmt.Errorf("notebook not found: %s — create it with `noda notebook add %s`", name, name)
	}
	if err := e.Paths.SetActiveNotebook(name); err != nil {
		return "", err
	}
	return "active notebook: " + name, nil
}
