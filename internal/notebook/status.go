package notebook

import (
	"context"
	"strings"

	"github.com/henry40408/noda/internal/gitx"
)

// Drift is how far the branch has moved from the remote-tracking ref.
type Drift struct{ Ahead, Behind int }

// Status is where a notebook stands, as `noda status` reports it.
type Status struct {
	Branch string
	Notes  int
	Files  int
	// Uncommitted counts the files differing from HEAD, untracked ones
	// included.
	Uncommitted int
	Remote      string
	HasRemote   bool
	// Drift is nil (shown as `never synced`) when there is no remote-tracking
	// ref yet.
	Drift    *Drift
	Problems []ProblemGroup
}

// Status is measured against the remote-tracking ref, so it is as current as
// the last fetch: an orienting command should not need the network.
func (n *Notebook) Status() (Status, error) {
	branch, err := n.Branch()
	if err != nil {
		return Status{}, err
	}
	scan, err := n.Scan()
	if err != nil {
		return Status{}, err
	}
	uncommitted, err := n.uncommitted()
	if err != nil {
		return Status{}, err
	}
	drift, err := n.Drift(branch)
	if err != nil {
		return Status{}, err
	}
	url, hasRemote := n.RemoteURL()
	return Status{
		Branch:      branch,
		Notes:       len(scan.Notes),
		Files:       len(scan.Files),
		Uncommitted: uncommitted,
		Remote:      url,
		HasRemote:   hasRemote,
		Drift:       drift,
		Problems:    scan.Problems(),
	}, nil
}

func trackingRef(branch string) string {
	return "refs/remotes/" + remoteName + "/" + branch
}

// hasRef reports whether a ref exists.
func (n *Notebook) hasRef(ref string) (bool, error) {
	result, err := n.run(gitx.Cmd{
		Args:     []string{"rev-parse", "--verify", "--quiet", ref},
		ReadOnly: true,
		OKCodes:  []int{0, 1},
	})
	return result.Code == 0, err
}

// Drift is `(ahead, behind)` against the remote-tracking ref, offline. It is
// split from Status so a caller wanting only this skips its working-tree walks.
// It is nil when there is no such ref.
func (n *Notebook) Drift(branch string) (*Drift, error) {
	tracking := trackingRef(branch)
	exists, err := n.hasRef(tracking)
	if err != nil || !exists {
		return nil, err
	}
	output, err := n.run(gitx.Cmd{
		Args:     []string{"rev-list", "--left-right", "--count", "HEAD..." + tracking},
		ReadOnly: true,
	})
	if err != nil {
		return nil, err
	}
	ahead, behind, err := gitx.ParseLeftRight(output.Stdout)
	if err != nil {
		return nil, err
	}
	return &Drift{Ahead: ahead, Behind: behind}, nil
}

// Unpushed is Drift's ahead count as the commits themselves. It is a set,
// because after a merge the unpushed commits are interleaved with the remote's
// rather than a run from HEAD; `rev-list HEAD --not upstream` is what the ahead
// count counts, so the two agree. Empty when never synced.
func (n *Notebook) Unpushed(branch string) (map[string]struct{}, error) {
	tracking := trackingRef(branch)
	exists, err := n.hasRef(tracking)
	if err != nil || !exists {
		return map[string]struct{}{}, err
	}
	listed, err := n.read("rev-list", "HEAD", "--not", tracking)
	if err != nil {
		return nil, err
	}
	set := map[string]struct{}{}
	for _, oid := range strings.Fields(listed) {
		set[oid] = struct{}{}
	}
	return set, nil
}

// LastCommit is HEAD's committer time and offset: one commit read, cheap enough
// for a page listing every notebook.
func (n *Notebook) LastCommit() (seconds int64, offsetMinutes int, err error) {
	date, err := n.read("log", "-1", "--format=%cd", "--date=raw", "HEAD")
	if err != nil {
		return 0, 0, err
	}
	return gitx.ParseRawDate(date)
}

// Hooks are the hooks git would run here and noda will not.
func (n *Notebook) Hooks() ([]string, error) {
	return gitx.Hooks(context.Background(), n.git, n.Path)
}
