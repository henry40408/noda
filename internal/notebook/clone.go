package notebook

import (
	"context"
	"errors"
	"fmt"
	"os"
	"sort"
	"strings"

	"github.com/henry40408/noda/internal/gitx"
	"github.com/henry40408/noda/internal/paths"
	"github.com/henry40408/noda/internal/remote"
)

// CloneOptions say what to clone and whether ssh may prompt.
type CloneOptions struct {
	URL, Name   string
	Interactive bool
}

// Clone copies a remote notebook. Whatever it leaves behind on failure is
// removed.
func Clone(g *gitx.Git, p paths.Paths, opts CloneOptions) (*Notebook, error) {
	if err := ValidateName(opts.Name); err != nil {
		return nil, err
	}
	path := p.NotebookDir(opts.Name)
	if _, err := os.Stat(path); err == nil {
		return nil, fmt.Errorf("notebook already exists: %s", opts.Name)
	}
	if err := os.MkdirAll(p.NotebooksDir(), 0o777); err != nil {
		return nil, err
	}
	_, err := g.Run(context.Background(), gitx.Cmd{
		Dir:         p.NotebooksDir(),
		Args:        []string{"clone", "--quiet", "--", opts.URL, path},
		Network:     true,
		Interactive: opts.Interactive,
	})
	if err != nil {
		os.RemoveAll(path)
		return nil, explain(err, opts.URL)
	}
	notebook := newNotebook(g, p, opts.Name)
	notebook.Interactive = opts.Interactive
	if err := notebook.adoptRemoteBranch(); err != nil {
		os.RemoveAll(path)
		return nil, err
	}
	return notebook, nil
}

// adoptRemoteBranch handles a HEAD that names a branch the remote lacks
// (machines disagreeing on `init.defaultBranch`), which checks out nothing. A
// sole remote branch is adopted; otherwise it fails naming them.
func (n *Notebook) adoptRemoteBranch() error {
	hasHead, err := n.hasRef("HEAD")
	if err != nil || hasHead {
		return err
	}
	branches, err := n.remoteBranches()
	if err != nil {
		return err
	}
	switch len(branches) {
	case 0:
		return errors.New("the remote has no commits yet")
	case 1:
		return n.adopt(branches[0])
	}
	return fmt.Errorf("the remote's default branch is missing, and it has more than one to choose from: %s", strings.Join(branches, ", "))
}

func (n *Notebook) adopt(branch string) error {
	refname := "refs/heads/" + branch
	oid, err := n.read("rev-parse", "--verify", trackingRef(branch))
	if err != nil {
		return err
	}
	steps := [][]string{
		{"update-ref", "-m", "noda clone: adopt the remote's branch", refname, oid},
		{"symbolic-ref", "HEAD", refname},
		{"reset", "--hard", "--quiet"},
	}
	for _, args := range steps {
		if _, err := n.run(gitx.Cmd{Args: args}); err != nil {
			return err
		}
	}
	return nil
}

// remoteBranches are the branches the clone has tracking refs for.
func (n *Notebook) remoteBranches() ([]string, error) {
	prefix := "refs/remotes/" + remoteName + "/"
	output, err := n.read("for-each-ref", "--format=%(refname)", prefix)
	if err != nil {
		return nil, err
	}
	var branches []string
	for _, ref := range strings.Fields(output) {
		if branch := strings.TrimPrefix(ref, prefix); branch != "HEAD" {
			branches = append(branches, branch)
		}
	}
	sort.Strings(branches)
	return branches, nil
}

// NameFromURL is the name a clone gets when none is given.
func NameFromURL(url string) string { return remote.NameFromURL(url) }
