package cmd

import (
	"fmt"
	"os"
	"path/filepath"
	"slices"
	"strings"

	"github.com/henry40408/noda/internal/link"
	"github.com/henry40408/noda/internal/note"
	"github.com/henry40408/noda/internal/notebook"
)

// retarget is what a rename did to the links that named the thing renamed.
type retarget struct {
	// rewritten is the filenames of the notes whose bodies were rewritten.
	rewritten []string
	// stranded still name the old name: not asked for, or out of reach.
	stranded []string
}

// describe is empty when no note named the old name.
func (r retarget) describe(subject string, updateLinks bool) string {
	var out strings.Builder
	if len(r.rewritten) > 0 {
		fmt.Fprintf(&out, "updated  %d %s\n", len(r.rewritten), plural(len(r.rewritten), "note", "notes"))
	}
	if len(r.stranded) > 0 {
		stranded := slices.Clone(r.stranded)
		slices.Sort(stranded)
		stranded = slices.Compact(stranded)
		noun, verb := "notes", "link"
		if len(stranded) == 1 {
			noun, verb = "note", "links"
		}
		still := ""
		if updateLinks {
			still = "still "
		}
		fmt.Fprintf(&out, "%d %s %s%s to %s\n", len(stranded), noun, still, verb, subject)
		for _, name := range stranded {
			out.WriteString("  " + name + "\n")
		}
	}
	return out.String()
}

func plural(n int, one, many string) string {
	if n == 1 {
		return one
	}
	return many
}

// retargetLinks rewrites the links `names` accepts when updateLinks says so,
// and reports them either way. `file mv` matches the old name; `mv` matches the
// note's id, catching a link written two renames ago.
//
// A rewritten note is re-checked rather than assumed fixed: link.Rewrite cannot
// locate a destination written with backslash escapes.
func retargetLinks(active *notebook.Notebook, notes []notebook.NoteFile, names func(string) bool, replacement string, updateLinks bool) (retarget, error) {
	// Already reading as the replacement is correct, so neither rewritten nor
	// reported.
	outdated := func(body string) []string {
		var found []string
		for _, target := range link.Targets(body) {
			if target != replacement && names(target) {
				found = append(found, target)
			}
		}
		return found
	}
	var result retarget
	for _, file := range notes {
		name := note.FileName(file.ID, file.Slug)
		targets := outdated(file.Note.Body)
		if len(targets) == 0 {
			continue
		}
		if !updateLinks {
			result.stranded = append(result.stranded, name)
			continue
		}
		fixed, rewritten, err := rewriteNote(active, name, targets, replacement)
		if err != nil {
			return retarget{}, err
		}
		if !rewritten {
			result.stranded = append(result.stranded, name)
			continue
		}
		if len(outdated(fixed)) > 0 {
			result.stranded = append(result.stranded, name)
		}
		result.rewritten = append(result.rewritten, name)
	}
	return result, nil
}

// rewriteNote points each target in one note at replacement, carrying the
// frontmatter byte for byte, `updated` included. It returns the new body.
func rewriteNote(active *notebook.Notebook, name string, targets []string, replacement string) (string, bool, error) {
	path := filepath.Join(active.Path, name)
	text, err := os.ReadFile(path)
	if err != nil {
		return "", false, err
	}
	_, body, ok := note.SplitFrontmatter(string(text))
	if !ok {
		return "", false, nil
	}
	// A note can name another by two names it has had.
	fixed := body
	for _, target := range targets {
		if next, rewrote := link.Rewrite(fixed, target, replacement); rewrote {
			fixed = next
		}
	}
	if fixed == body {
		return "", false, nil
	}
	prefix := string(text[:len(text)-len(body)])
	if err := os.WriteFile(path, []byte(prefix+fixed), 0o666); err != nil {
		return "", false, err
	}
	return fixed, true, nil
}
