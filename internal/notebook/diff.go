package notebook

import (
	"fmt"
	"strings"

	"github.com/henry40408/noda/internal/gitx"
	"github.com/henry40408/noda/internal/style"
)

// diffArgs fixes what a user's git settings would otherwise change in the
// output. `-M50%` is libgit2's default rename threshold, so `noda mv` reads as
// a rename and not as a deletion plus an unrelated new note.
var diffArgs = []string{
	"diff", "--no-color", "--no-ext-diff", "--no-textconv", "-M50%",
	"--unified=3", "--inter-hunk-context=0", "--no-indent-heuristic",
	"--diff-algorithm=myers", "--src-prefix=a/", "--dst-prefix=b/", "--abbrev=7",
}

// Diff is the uncommitted changes, or what the last commit changed when there
// are none (clean being the normal state). An untracked file is not in it,
// as it was not in libgit2's patch. file limits it to one path; empty means
// all.
func (n *Notebook) Diff(file string) (string, error) {
	dirty, err := n.IsDirty()
	if err != nil {
		return "", err
	}
	if dirty {
		return n.diffText("HEAD", "", file)
	}
	base, err := n.firstParentOrEmptyTree()
	if err != nil {
		return "", err
	}
	return n.diffText(base, "HEAD", file)
}

// firstParentOrEmptyTree is what the last commit is compared against: its
// parent, or nothing for the first commit.
func (n *Notebook) firstParentOrEmptyTree() (string, error) {
	result, err := n.run(gitx.Cmd{
		Args:     []string{"rev-parse", "--verify", "--quiet", "HEAD^"},
		ReadOnly: true,
		OKCodes:  []int{0, 1},
	})
	if err != nil {
		return "", err
	}
	if result.Code == 0 {
		return strings.TrimSpace(string(result.Stdout)), nil
	}
	empty, err := n.run(gitx.Cmd{Args: []string{"hash-object", "-t", "tree", "--stdin"}, Stdin: []byte{}})
	return strings.TrimSpace(string(empty.Stdout)), err
}

// DiffRemote is what a push would carry, offline: committed work from the
// merge base, `origin/<branch>...HEAD`. Two dots would show every line the
// remote added as removed.
func (n *Notebook) DiffRemote(branch, file string) (string, error) {
	tracking := trackingRef(branch)
	exists, err := n.hasRef(tracking)
	if err != nil {
		return "", err
	}
	if !exists {
		return "", fmt.Errorf("notebook `%s` has never synced, so there is nothing to compare against — run `noda sync` first", n.Name)
	}
	base, err := n.read("merge-base", "HEAD", tracking)
	if err != nil {
		return "", err
	}
	return n.diffText(base, "HEAD", file)
}

// diffText runs git diff between one or two revisions; with only one, it is
// compared against the working tree.
func (n *Notebook) diffText(from, to, file string) (string, error) {
	args := append([]string{}, diffArgs...)
	args = append(args, from)
	if to != "" {
		args = append(args, to)
	}
	args = append(args, "--")
	if file != "" {
		args = append(args, ":(literal)"+file)
	}
	return n.readRaw(args...)
}

// readRaw is read without trimming: a diff ends in a newline that belongs to it.
func (n *Notebook) readRaw(args ...string) (string, error) {
	result, err := n.run(gitx.Cmd{Args: args, ReadOnly: true})
	return string(result.Stdout), err
}

// ColorDiff paints a plain diff the way libgit2's printer classified it: a
// file header (every line up to the first hunk) is one bold block, a hunk
// header is cyan, additions are green, removals red. The newline belongs to
// the painted text, as it did there. git's own output is never coloured.
func ColorDiff(plain string) string {
	var out, header strings.Builder
	flushHeader := func() {
		if header.Len() > 0 {
			out.WriteString(style.Heading.Paint(header.String()))
			header.Reset()
		}
	}
	inHunk := false
	for _, line := range strings.SplitAfter(plain, "\n") {
		switch {
		case line == "":
		case strings.HasPrefix(line, "diff --git "):
			flushHeader()
			inHunk = false
			header.WriteString(line)
		case strings.HasPrefix(line, "@@"):
			flushHeader()
			inHunk = true
			out.WriteString(style.Hunk.Paint(line))
		case !inHunk:
			header.WriteString(line)
		case strings.HasPrefix(line, "+"):
			out.WriteString(style.Added.Paint(line))
		case strings.HasPrefix(line, "-"):
			out.WriteString(style.Removed.Paint(line))
		default:
			out.WriteString(line)
		}
	}
	flushHeader()
	return out.String()
}
