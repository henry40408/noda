package gitx

import (
	"context"
	"os"
	"path/filepath"
	"sort"
	"strings"
)

// Hooks are the hooks git would run in dir and noda does not: it commits and
// pushes without running any, as libgit2 did, so `doctor` has to say so. Found
// as git finds them: the directory is git's own answer (`core.hooksPath`
// included), a hook needs the executable bit, and `*.sample` is not one. An
// unreadable directory is not a finding.
func Hooks(ctx context.Context, g *Git, dir string) ([]string, error) {
	result, err := g.Run(ctx, Cmd{Dir: dir, Args: []string{"rev-parse", "--git-path", "hooks"}, ReadOnly: true, showHooksPath: true})
	if err != nil {
		return nil, err
	}
	hooksDir := strings.TrimRight(string(result.Stdout), "\n")
	if !filepath.IsAbs(hooksDir) {
		// Relative to where git ran.
		hooksDir = filepath.Join(dir, hooksDir)
	}
	entries, err := os.ReadDir(hooksDir)
	if err != nil {
		return nil, nil
	}

	var found []string
	for _, entry := range entries {
		if hookRuns(hooksDir, entry) {
			found = append(found, entry.Name())
		}
	}
	sort.Strings(found)
	return found, nil
}

func hookRuns(dir string, entry os.DirEntry) bool {
	if entry.IsDir() || strings.HasSuffix(entry.Name(), ".sample") {
		return false
	}
	// os.Stat follows a symlinked hook; the entry's own type does not.
	info, err := os.Stat(filepath.Join(dir, entry.Name()))
	return err == nil && info.Mode().IsRegular() && info.Mode().Perm()&0o111 != 0
}
