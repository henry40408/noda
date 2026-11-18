package cmd

import (
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"

	"github.com/henry40408/noda/internal/note"
	"github.com/henry40408/noda/internal/notebook"
)

// FileAdd copies files into the active notebook and commits them. It takes no
// note: which note uses a file is said in that note's prose, not twice. Rename
// is `--as` and applies to a single source.
func FileAdd(e Env, sources []string, rename string) (string, error) {
	if rename != "" && len(sources) > 1 {
		return "", fmt.Errorf("`--as` renames one file, so it cannot be given with several")
	}
	active, err := e.openActive()
	if err != nil {
		return "", err
	}

	// All checked before any is copied, or a half-done copy gets committed.
	names := make([]string, len(sources))
	for i, source := range sources {
		names[i], err = plannedFileName(active, source, rename)
		if err != nil {
			return "", err
		}
	}
	for i, source := range sources {
		if err := copyFile(source, filepath.Join(active.Path, names[i])); err != nil {
			return "", err
		}
	}

	message := fmt.Sprintf("file: add %d files", len(names))
	if len(names) == 1 {
		message = "file: add " + names[0]
	}
	if err := active.Commit(names, message); err != nil {
		return "", err
	}
	var out strings.Builder
	for _, name := range names {
		out.WriteString("added  " + name + "\n")
	}
	return out.String(), nil
}

func plannedFileName(active *notebook.Notebook, source, rename string) (string, error) {
	if !isRegularFile(source) {
		return "", fmt.Errorf("not a file: %s — a notebook holds files, not directories", source)
	}
	name := rename
	if name == "" {
		name = filepath.Base(source)
	}
	if err := validateFileName(name); err != nil {
		return "", err
	}
	if err := refuseANotesName(name); err != nil {
		return "", err
	}
	if _, err := os.Stat(filepath.Join(active.Path, name)); err == nil {
		return "", fmt.Errorf("the notebook already holds %s — copy it in under another name with `--as`", name)
	}
	return name, nil
}

func copyFile(source, destination string) (err error) {
	in, err := os.Open(source)
	if err != nil {
		return err
	}
	defer in.Close()
	out, err := os.OpenFile(destination, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0o666)
	if err != nil {
		return err
	}
	defer func() {
		if closeErr := out.Close(); err == nil {
			err = closeErr
		}
	}()
	_, err = io.Copy(out, in)
	return err
}

// FileRm is a commit, so `git revert` brings it back. A note is refused:
// `noda rm` is for those.
func FileRm(e Env, name string) (string, error) {
	active, err := e.openActive()
	if err != nil {
		return "", err
	}
	if err := validateFileName(name); err != nil {
		return "", err
	}
	notes, files, err := active.Inventory()
	if err != nil {
		return "", err
	}
	if !containsName(files, name) {
		return "", missingFile(notes, name, "remove it with `noda rm`")
	}
	if err := os.Remove(filepath.Join(active.Path, name)); err != nil {
		return "", err
	}
	if err := active.Commit([]string{name}, "file: rm "+name); err != nil {
		return "", err
	}
	return "removed  " + name, nil
}

// FileMv renames one of the notebook's files and always reports the links it
// broke: a full walk, paid because a rename is rare and the damage otherwise
// silent. updateLinks rewrites them instead (see retargetLinks).
func FileMv(e Env, old, renamed string, updateLinks bool) (string, error) {
	active, err := e.openActive()
	if err != nil {
		return "", err
	}
	if err := validateFileName(old); err != nil {
		return "", err
	}
	if err := validateFileName(renamed); err != nil {
		return "", err
	}
	if err := refuseANotesName(renamed); err != nil {
		return "", err
	}
	if old == renamed {
		return "", fmt.Errorf("%s already is its name", old)
	}
	notes, files, err := active.Inventory()
	if err != nil {
		return "", err
	}
	if !containsName(files, old) {
		return "", missingFile(notes, old, "retitle it with `noda mv`")
	}
	if _, err := os.Stat(filepath.Join(active.Path, renamed)); err == nil {
		return "", fmt.Errorf("the notebook already holds %s — pick a name it does not", renamed)
	}
	if err := os.Rename(filepath.Join(active.Path, old), filepath.Join(active.Path, renamed)); err != nil {
		return "", err
	}

	result, err := retargetLinks(active, notes, func(target string) bool { return target == old }, renamed, updateLinks)
	if err != nil {
		return "", err
	}
	changed := append([]string{old, renamed}, result.rewritten...)
	if err := active.Commit(changed, fmt.Sprintf("file: mv %s -> %s", old, renamed)); err != nil {
		return "", err
	}
	return fmt.Sprintf("renamed  %s -> %s\n", old, renamed) + result.describe(old, updateLinks), nil
}

func containsName(names []string, name string) bool {
	for _, candidate := range names {
		if candidate == name {
			return true
		}
	}
	return false
}

// missingFile says so, and when the name is a note's, names the right command.
func missingFile(notes []notebook.NoteFile, name, instead string) error {
	for _, file := range notes {
		if note.FileName(file.ID, file.Slug) == name {
			return fmt.Errorf("%s is a note — %s", name, instead)
		}
	}
	return fmt.Errorf("the notebook holds no file called %s", name)
}

// refuseANotesName: such a name reads as a note without frontmatter, which
// `doctor` reports broken.
func refuseANotesName(name string) error {
	if note.NamesANote(name) {
		return fmt.Errorf("%s claims a note's id — a file noda would then report as a broken note", name)
	}
	return nil
}

// validateFileName holds a file to one flat directory, so a name is never a
// path. A leading `.` is refused because the walk skips dotfiles: nothing would
// ever list it.
func validateFileName(name string) error {
	switch {
	case name == "":
		return fmt.Errorf("a file needs a name")
	case strings.ContainsAny(name, `/\`) || name == "." || name == "..":
		return fmt.Errorf("a file's name cannot be a path: %s", name)
	case strings.HasPrefix(name, "."):
		return fmt.Errorf("noda does not list dotfiles, so it will not add one: %s", name)
	// Refused rather than cut to fit like a slug: links point at this name.
	case len(name) > note.MaxFileNameLen:
		return fmt.Errorf("a filename has to fit in %d bytes, and this one is %d: %s", note.MaxFileNameLen, len(name), name)
	}
	return nil
}
