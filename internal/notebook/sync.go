package notebook

import (
	"errors"
	"fmt"
	"strings"

	"github.com/henry40408/noda/internal/gitx"
	"github.com/henry40408/noda/internal/remote"
)

func plural(n int, thing string) string {
	if n == 1 {
		return "1 " + thing
	}
	return fmt.Sprintf("%d %ss", n, thing)
}

// remoteURL is the configured remote as git uses it, or the error that points
// to `noda remote set`.
func (n *Notebook) remoteURL() (string, error) {
	url, ok := n.configuredRemote()
	if !ok {
		return "", fmt.Errorf("notebook `%s` has no remote — set one with `noda remote set <url>`", n.Name)
	}
	return url, nil
}

// network runs a git command that talks to the remote.
func (n *Notebook) network(url string, args ...string) (gitx.Result, error) {
	result, err := n.run(gitx.Cmd{Args: args, Network: true, Interactive: n.Interactive})
	return result, explain(err, url)
}

// explain adds the way out of a failed login to a git error. Anything it has
// nothing to say about passes through.
func explain(err error, url string) error {
	var failure *gitx.Error
	if !errors.As(err, &failure) {
		return err
	}
	if message, ok := remote.Explain(failure.Stderr, url); ok {
		return errors.New(message)
	}
	return err
}

// Fetch brings the remote's copy of the branch, and its tags so a snapshot
// taken elsewhere can be restored here, to the tracking ref. It returns that
// ref's commit, or false when the remote does not carry the branch yet, which
// is a normal first sync.
func (n *Notebook) Fetch() (string, bool, error) {
	branch, err := n.Branch()
	if err != nil {
		return "", false, err
	}
	url, err := n.remoteURL()
	if err != nil {
		return "", false, err
	}
	// Always an explicit refspec: `fetch.pruneTags` would delete local snapshots
	// otherwise.
	tracking := trackingRef(branch)
	_, err = n.run(gitx.Cmd{
		Args:    []string{"fetch", "--quiet", "--tags", "--no-recurse-submodules", remoteName, "+refs/heads/" + branch + ":" + tracking},
		Network: true, Interactive: n.Interactive,
	})
	if isMissingRemoteBranch(err) {
		return "", false, nil
	}
	if err != nil {
		return "", false, explain(err, url)
	}
	oid, err := n.read("rev-parse", "--verify", "--quiet", tracking)
	return oid, oid != "", err
}

func isMissingRemoteBranch(err error) bool {
	var failure *gitx.Error
	return errors.As(err, &failure) && strings.Contains(failure.Stderr, "couldn't find remote ref")
}

// Pull fast-forwards where it can and makes a merge commit where the histories
// diverged. A conflict, the same note edited on both sides, is refused with
// nothing touched: noda has no `--continue`.
func (n *Notebook) Pull() (string, error) {
	dirty, err := n.IsDirty()
	if err != nil {
		return "", err
	}
	if dirty {
		return "", fmt.Errorf("notebook `%s` has uncommitted changes — commit them, or use `noda sync`, which commits before pulling", n.Name)
	}
	branch, err := n.Branch()
	if err != nil {
		return "", err
	}
	incoming, found, err := n.Fetch()
	if err != nil {
		return "", err
	}
	if !found {
		return fmt.Sprintf("pull: the remote has no `%s` branch yet", branch), nil
	}
	drift, err := n.Drift(branch)
	if err != nil {
		return "", err
	}
	switch {
	case drift.Behind == 0:
		return "pull: already up to date", nil
	case drift.Ahead == 0:
		return n.fastForward(incoming, drift.Behind)
	}
	return n.merge(branch, incoming, drift.Behind)
}

func (n *Notebook) fastForward(incoming string, count int) (string, error) {
	if _, err := n.run(gitx.Cmd{Args: []string{"reset", "--hard", "--quiet", incoming}}); err != nil {
		return "", err
	}
	return fmt.Sprintf("pull: fast-forwarded %s to %s", plural(count, "commit"), Short(incoming)), nil
}

// merge works in git's object store first (`merge-tree` touches neither the
// index nor the working tree), so a conflict leaves nothing to roll back.
func (n *Notebook) merge(branch, incoming string, count int) (string, error) {
	// Renames are looked for, as libgit2 did; directory renames are not.
	merged, err := n.run(gitx.Cmd{
		Args: []string{
			"-c", "merge.directoryRenames=false", "-c", "merge.renames=true",
			"merge-tree", "--write-tree", "--name-only", "-z", "HEAD", incoming,
		},
		OKCodes: []int{0, 1},
	})
	if err != nil {
		return "", err
	}
	tokens := strings.Split(string(merged.Stdout), "\x00")
	if merged.Code == 1 {
		return "", fmt.Errorf("pull: `%s` conflicts with the remote in %s — the merge was rolled back; resolve it with git in %s",
			branch, strings.Join(conflictedFiles(tokens[1:]), ", "), n.Path)
	}
	parents, err := n.headParents()
	if err != nil {
		return "", err
	}
	_, err = n.commitTree(treeCommit{
		tree:    tokens[0],
		parents: append(parents, incoming),
		message: "merge: " + remoteName + "/" + branch,
	})
	if err != nil {
		return "", err
	}
	// HEAD has moved; the index and the working tree follow.
	if _, err := n.run(gitx.Cmd{Args: []string{"reset", "--hard", "--quiet", "HEAD"}}); err != nil {
		return "", err
	}
	return fmt.Sprintf("pull: merged %s from %s/%s", plural(count, "commit"), remoteName, branch), nil
}

// conflictedFiles are the names `merge-tree --name-only -z` lists after the
// tree, up to the empty token that ends them. A file in conflict is listed once
// per stage.
func conflictedFiles(tokens []string) []string {
	var files []string
	seen := map[string]bool{}
	for _, name := range tokens {
		if name == "" {
			break
		}
		if !seen[name] {
			seen[name] = true
			files = append(files, name)
		}
	}
	return files
}

// nonFastForward is libgit2's wording, kept for the refusal the Rust version
// printed most often.
const nonFastForward = "cannot push because a reference that you are trying to update on the remote " +
	"contains commits that are not present locally."

func rejected(reasons []string) error {
	return fmt.Errorf("push rejected — %s\nthe remote has commits you do not: run `noda pull` first", strings.Join(reasons, "; "))
}

// Push sends the branch and the snapshots the remote lacks. A rejection is
// reported as advice to pull.
func (n *Notebook) Push() (string, error) {
	branch, err := n.Branch()
	if err != nil {
		return "", err
	}
	// Before sending, while the tracking ref is still where the last sync left it.
	drift, err := n.Drift(branch)
	if err != nil {
		return "", err
	}
	url, err := n.remoteURL()
	if err != nil {
		return "", err
	}
	refspecs := []string{"refs/heads/" + branch + ":refs/heads/" + branch}
	tags, heldBack, err := n.tagsToSend(url)
	if err != nil {
		return "", err
	}
	for _, name := range tags {
		refspecs = append(refspecs, "refs/tags/"+name+":refs/tags/"+name)
	}

	if err := n.sendRefs(url, refspecs); err != nil {
		return "", err
	}
	return pushSummary(pushReport{
		branch: branch, url: remote.Redact(url), drift: drift,
		snapshots: len(tags), heldBack: heldBack,
	}), nil
}

// sendRefs pushes with the settings that would change what is sent forced off,
// and no hook: noda never runs one.
func (n *Notebook) sendRefs(url string, refspecs []string) error {
	args := []string{
		"-c", "push.followTags=false", "-c", "push.gpgSign=false", "-c", "push.recurseSubmodules=no",
		"push", "--porcelain", "--no-verify", remoteName,
	}
	result, err := n.run(gitx.Cmd{Args: append(args, refspecs...), Network: true, Interactive: n.Interactive, OKCodes: []int{0, 1}})
	if err != nil {
		return explain(err, url)
	}
	reasons := rejections(string(result.Stdout))
	if len(reasons) > 0 {
		return rejected(reasons)
	}
	if result.Code != 0 {
		return explain(&gitx.Error{Args: args, Code: result.Code, Stderr: string(result.Stderr)}, url)
	}
	return nil
}

// rejections reads `push --porcelain`: one line per ref, `!` for a refusal.
func rejections(output string) []string {
	var reasons []string
	for _, line := range strings.Split(output, "\n") {
		if !strings.HasPrefix(line, "!\t") {
			continue
		}
		fields := strings.Split(line, "\t")
		if len(fields) < 3 {
			continue
		}
		ref, _, _ := strings.Cut(fields[1], ":")
		summary := fields[2]
		if strings.Contains(summary, "(non-fast-forward)") || strings.Contains(summary, "(fetch first)") {
			reasons = append(reasons, nonFastForward)
			continue
		}
		reasons = append(reasons, ref+": "+strings.TrimSpace(summary))
	}
	return reasons
}

// tagsToSend splits the local snapshots into the ones the remote lacks and the
// ones it has at another commit. Two machines each made a `q3`: sending one
// would overwrite the other or fail the whole push, so it is held back.
func (n *Notebook) tagsToSend(url string) (send, heldBack []string, err error) {
	local, err := n.localTags()
	if err != nil || len(local) == 0 {
		return nil, nil, err
	}
	theirs, err := n.remoteTags(url)
	if err != nil {
		return nil, nil, err
	}
	for _, tag := range local {
		other, has := theirs[tag.name]
		switch {
		case !has:
			send = append(send, tag.name)
		case other != tag.oid:
			heldBack = append(heldBack, tag.name)
		}
	}
	return send, heldBack, nil
}

type localTag struct{ name, oid string }

// localTags are unpeeled, to compare against what the remote advertises.
func (n *Notebook) localTags() ([]localTag, error) {
	output, err := n.read("for-each-ref", "--format=%(objectname) %(refname)", "refs/tags")
	if err != nil {
		return nil, err
	}
	var tags []localTag
	for _, line := range strings.Split(output, "\n") {
		oid, ref, found := strings.Cut(line, " ")
		if found {
			tags = append(tags, localTag{name: strings.TrimPrefix(ref, "refs/tags/"), oid: oid})
		}
	}
	return tags, nil
}

// remoteTags is the remote's advertisement: one extra round trip, made only
// when there is a snapshot to send.
func (n *Notebook) remoteTags(url string) (map[string]string, error) {
	result, err := n.network(url, "ls-remote", "--tags", remoteName)
	if err != nil {
		return nil, err
	}
	found := map[string]string{}
	for _, line := range strings.Split(string(result.Stdout), "\n") {
		oid, ref, ok := strings.Cut(line, "\t")
		name := strings.TrimPrefix(ref, "refs/tags/")
		// `q3^{}` is the peeled form of an annotated tag.
		if ok && name != ref && !strings.HasSuffix(name, "^{}") {
			found[name] = oid
		}
	}
	return found, nil
}

type pushReport struct {
	branch, url string
	// drift is nil when the notebook has never synced.
	drift     *Drift
	snapshots int
	heldBack  []string
}

func pushSummary(r pushReport) string {
	var sent []string
	if r.drift != nil && r.drift.Ahead > 0 {
		sent = append(sent, plural(r.drift.Ahead, "commit"))
	}
	if r.snapshots > 0 {
		sent = append(sent, plural(r.snapshots, "snapshot"))
	}
	var out string
	switch {
	case len(sent) > 0:
		out = fmt.Sprintf("push: %s (%s) -> %s", r.branch, strings.Join(sent, ", "), r.url)
	case r.drift != nil:
		out = fmt.Sprintf("push: %s matches %s — nothing to send", r.branch, r.url)
	default:
		out = fmt.Sprintf("push: %s -> %s", r.branch, r.url)
	}
	for _, name := range r.heldBack {
		out += fmt.Sprintf("\nsnapshot `%s` was not sent — the remote already has that name for another commit; rename yours, or drop it with `git tag -d %s`", name, name)
	}
	return out
}
