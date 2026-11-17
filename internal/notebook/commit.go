package notebook

import (
	"context"
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"github.com/henry40408/noda/internal/gitx"
	"github.com/henry40408/noda/internal/sign"
)

// fallbackAuthor is who commits when neither noda nor git says.
var fallbackAuthor = Author{Name: "noda", Email: "noda@localhost"}

// Commit stages files and commits them. A path that no longer exists is staged
// as a deletion, so a rename is one commit.
func (n *Notebook) Commit(files []string, message string) error {
	if err := n.stage(files); err != nil {
		return err
	}
	return n.commitIndex(message)
}

// stage is `git add` without pathspec magic or `.gitignore`: a file noda was
// told to commit is committed.
func (n *Notebook) stage(files []string) error {
	if len(files) == 0 {
		return nil
	}
	_, err := n.run(gitx.Cmd{
		Args:  []string{"update-index", "--add", "--remove", "-z", "--stdin"},
		Stdin: []byte(strings.Join(files, "\x00") + "\x00"),
	})
	return err
}

// CommitAll commits everything in the working tree, for `noda sync`. It
// reports false when there was nothing to commit.
func (n *Notebook) CommitAll(message string) (bool, error) {
	dirty, err := n.IsDirty()
	if err != nil || !dirty {
		return false, err
	}
	if _, err := n.run(gitx.Cmd{Args: []string{"add", "--all"}}); err != nil {
		return false, err
	}
	return true, n.commitIndex(message)
}

// IsDirty is whether anything differs from HEAD, untracked files included.
func (n *Notebook) IsDirty() (bool, error) {
	count, err := n.uncommitted()
	return count > 0, err
}

// uncommitted counts the paths that differ from HEAD.
func (n *Notebook) uncommitted() (int, error) {
	// Renames off: a moved note is two paths, as the Rust version counted it.
	result, err := n.run(gitx.Cmd{
		Args:     []string{"status", "--porcelain=v2", "-z", "--no-renames", "--untracked-files=normal"},
		ReadOnly: true,
	})
	if err != nil {
		return 0, err
	}
	return gitx.CountStatus(result.Stdout), nil
}

func (n *Notebook) commitIndex(message string) error {
	tree, err := n.read("write-tree")
	if err != nil {
		return err
	}
	parents, err := n.headParents()
	if err != nil {
		return err
	}
	_, err = n.commitTree(treeCommit{tree: tree, parents: parents, message: message})
	return err
}

// headParents is HEAD, or nothing when the branch has no commit yet.
func (n *Notebook) headParents() ([]string, error) {
	result, err := n.run(gitx.Cmd{
		Args:     []string{"rev-parse", "--verify", "--quiet", "HEAD^{commit}"},
		ReadOnly: true,
		OKCodes:  []int{0, 1},
	})
	if err != nil || result.Code == 1 {
		return nil, err
	}
	return []string{strings.TrimSpace(string(result.Stdout))}, nil
}

// treeCommit is what makes a commit: the tree, the parents (the first is what
// HEAD is moved from) and the message.
type treeCommit struct {
	tree    string
	parents []string
	message string
}

// commitTree writes the commit, signing when configured, and moves HEAD onto
// it. It does the work in steps, signing needs the commit's text before it is
// an object, and `git commit` would run hooks, which noda never does.
func (n *Notebook) commitTree(c treeCommit) (string, error) {
	values, err := gitx.LoadConfig(context.Background(), n.git, n.Path)
	if err != nil {
		return "", err
	}
	who := n.signature(values)
	signer, err := sign.Resolve(n.sign, values)
	if err != nil {
		return "", err
	}

	encoded := gitx.CommitObject{Tree: c.tree, Parents: c.parents, Author: who, Committer: who, Message: c.message}.Encode()
	if signer != nil {
		signer.Env, signer.Stderr = n.git.Environ(), n.git.Stderr()
		armored, err := signer.Sign(encoded)
		if err != nil {
			return "", err
		}
		encoded = gitx.WithSignature(encoded, armored)
	}

	stored, err := n.run(gitx.Cmd{Args: []string{"hash-object", "-t", "commit", "-w", "--stdin"}, Stdin: encoded})
	if err != nil {
		return "", err
	}
	oid := strings.TrimSpace(string(stored.Stdout))
	return oid, n.moveHead(oid, c, who, signer != nil)
}

// moveHead points the branch HEAD names, or a detached HEAD, at oid. The old
// value is checked, so a commit made meanwhile is not overwritten.
func (n *Notebook) moveHead(oid string, c treeCommit, who gitx.Ident, signed bool) error {
	previous := strings.Repeat("0", len(oid))
	if len(c.parents) > 0 {
		previous = c.parents[0]
	}
	_, err := n.run(gitx.Cmd{
		Args:     []string{"update-ref", "-m", reflogMessage(c, signed), "HEAD", oid, previous},
		ExtraEnv: committerEnv(who),
	})
	return err
}

// reflogMessage is libgit2's wording, which a signed commit never had: the Rust
// version moved HEAD itself with a plain `commit:`.
func reflogMessage(c treeCommit, signed bool) string {
	subject, _, _ := strings.Cut(c.message, "\n")
	kind := "commit"
	if !signed {
		switch len(c.parents) {
		case 0:
			kind = "commit (initial)"
		case 1:
		default:
			kind = "commit (merge)"
		}
	}
	return kind + ": " + subject
}

func committerEnv(who gitx.Ident) map[string]string {
	return map[string]string{
		"GIT_COMMITTER_NAME":  who.Name,
		"GIT_COMMITTER_EMAIL": who.Email,
		"GIT_COMMITTER_DATE":  who.RawDate(),
	}
}

// signature is config.toml first, then git, then a neutral identity.
func (n *Notebook) signature(values gitx.ConfigValues) gitx.Ident {
	who := fallbackAuthor
	if n.author != nil {
		who = *n.author
	} else if gitAuthor, ok := authorFrom(values); ok {
		who = gitAuthor
	}
	return gitx.Ident{Name: who.Name, Email: who.Email, When: n.now()}
}

func authorFrom(values gitx.ConfigValues) (Author, bool) {
	name, hasName := values.Get("user.name")
	email, hasEmail := values.Get("user.email")
	if !hasName || !hasEmail || name == "" || email == "" {
		return Author{}, false
	}
	return Author{Name: name, Email: email}, true
}

// GitAuthor is the identity git itself would use here, as `Name <email>`.
func (n *Notebook) GitAuthor() (string, bool) {
	values, err := gitx.LoadConfig(context.Background(), n.git, n.Path)
	if err != nil {
		return "", false
	}
	who, ok := authorFrom(values)
	if !ok {
		return "", false
	}
	return fmt.Sprintf("%s <%s>", who.Name, who.Email), true
}

// fileExists reports whether path names something on disk, as a link in a note
// is checked against the filesystem.
func (n *Notebook) fileExists(name string) bool {
	_, err := os.Stat(filepath.Join(n.Path, name))
	return err == nil
}
