package notebook

import (
	"fmt"
	"slices"
	"strings"

	"github.com/henry40408/noda/internal/note"
)

// Resolve turns a key into one note's id and slug: exact slug first, then a
// folded id prefix, as git abbreviates object ids. An ambiguous key is an error
// naming the candidates. It reads no file.
func (n *Notebook) Resolve(key string) (IDSlug, error) {
	if key == "" || strings.ContainsAny(key, `/\`) || strings.Contains(key, "..") {
		return IDSlug{}, fmt.Errorf("invalid note reference: %s", key)
	}
	named, err := n.NamedFiles()
	if err != nil {
		return IDSlug{}, err
	}
	wanted := note.NormalizeID(key)

	var bySlug, byID []IDSlug
	for _, found := range named {
		if found.Slug == key {
			bySlug = append(bySlug, found)
		} else if strings.HasPrefix(note.NormalizeID(found.ID), wanted) {
			byID = append(byID, found)
		}
	}
	matched := byID
	if len(bySlug) > 0 {
		matched = bySlug
	}
	slices.SortFunc(matched, compareIDSlug)

	switch len(matched) {
	case 1:
		return matched[0], nil
	case 0:
		return IDSlug{}, fmt.Errorf("%s: %s", NotFound, key)
	}
	lines := make([]string, len(matched))
	for i, candidate := range matched {
		lines[i] = fmt.Sprintf("  %s  %s", candidate.ID, candidate.Slug)
	}
	return IDSlug{}, fmt.Errorf("`%s` matches %d notes — say which:\n%s", key, len(matched), strings.Join(lines, "\n"))
}
