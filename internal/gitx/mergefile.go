package gitx

import (
	"context"
	"os"
	"path/filepath"
)

// MergeInput is a three-way merge of text. Ours is what the reader wrote,
// Theirs what was saved since, Base the version both began from.
type MergeInput struct {
	Base, Ours, Theirs string
	OursLabel          string
	TheirsLabel        string
}

// MergeResult is the merged text, carrying conflict markers when not Clean.
type MergeResult struct {
	Text  string
	Clean bool
}

// MergeFile merges with `git merge-file`, which needs files: they are written
// to a private directory that is removed afterwards.
func MergeFile(ctx context.Context, g *Git, in MergeInput) (MergeResult, error) {
	dir, err := os.MkdirTemp("", "noda-merge-")
	if err != nil {
		return MergeResult{}, err
	}
	defer os.RemoveAll(dir)

	texts := map[string]string{"base": in.Base, "ours": in.Ours, "theirs": in.Theirs}
	for name, text := range texts {
		if err := os.WriteFile(filepath.Join(dir, name), []byte(text), 0o600); err != nil {
			return MergeResult{}, err
		}
	}
	// The base label is only printed in diff3 style, which fixedConfig rules out.
	result, err := g.Run(ctx, Cmd{
		Dir: dir,
		Args: []string{
			"merge-file", "-p", "-L", in.OursLabel, "-L", "base", "-L", in.TheirsLabel,
			"ours", "base", "theirs",
		},
		OKCodes: conflictCodes(),
	})
	if err != nil {
		return MergeResult{}, err
	}
	return MergeResult{Text: string(result.Stdout), Clean: result.Code == 0}, nil
}

// conflictCodes are the exit codes of a finished merge: the number of
// conflicts. 128 and above is a failure.
func conflictCodes() []int {
	codes := make([]int, 128)
	for i := range codes {
		codes[i] = i
	}
	return codes
}
