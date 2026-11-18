package cmd

import (
	"os"
	"strings"

	"github.com/henry40408/noda/internal/style"
)

// Show prints a note verbatim, frontmatter included, because that is the file.
func Show(e Env, key string) (string, error) {
	active, err := e.openActive()
	if err != nil {
		return "", err
	}
	resolved, err := active.Resolve(key)
	if err != nil {
		return "", err
	}
	text, err := os.ReadFile(active.NotePath(resolved.ID, resolved.Slug))
	if err != nil {
		return "", err
	}
	return dimFrontmatter(string(text)), nil
}

// dimFrontmatter dims only the frontmatter; the body is the user's prose.
func dimFrontmatter(text string) string {
	rest, ok := strings.CutPrefix(text, "---\n")
	if !ok {
		return text
	}
	block, after, ok := strings.Cut(rest, "\n---\n")
	if !ok {
		return text
	}
	return style.Muted.Paint("---\n"+block+"\n---") + "\n" + after
}
