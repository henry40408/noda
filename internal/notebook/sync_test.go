package notebook

import (
	"context"
	"errors"
	"net/http"
	"net/http/cgi"
	"net/http/httptest"
	"os"
	"os/exec"
	"path/filepath"
	"regexp"
	"strings"
	"testing"

	"github.com/henry40408/noda/internal/gitx"
	"github.com/henry40408/noda/internal/gitx/gittest"
	"github.com/henry40408/noda/internal/paths"
)

// rootOf is the fixture directory, which the expectations write as <ROOT>.
func rootOf(nb *Notebook) string {
	return strings.TrimSuffix(nb.Path, "/data/noda/notebooks/default")
}

func masked(nb *Notebook, text string) string {
	return strings.ReplaceAll(text, rootOf(nb), "<ROOT>")
}

func mustSync(t *testing.T, text string, err error) string {
	t.Helper()
	if err != nil {
		t.Fatal(err)
	}
	return text
}

func TestPullMergesDivergedHistoryAndPushSendsTheMerge(t *testing.T) {
	nb := fixture(t, "diverged-clean")
	pulled, err := nb.Pull()
	if got, want := mustSync(t, pulled, err), "pull: merged 1 commit from origin/main"; got != want {
		t.Fatalf("pull = %q, want %q", got, want)
	}
	pushed, err := nb.Push()
	const wantPush = "push: main (2 commits) -> file://<ROOT>/remote.git"
	if got := masked(nb, mustSync(t, pushed, err)); got != wantPush {
		t.Fatalf("push = %q, want %q", got, wantPush)
	}
	parents := strings.Fields(gitOut(t, nb, "log", "-1", "--format=%P"))
	if len(parents) != 2 {
		t.Fatalf("merge commit has parents %v", parents)
	}
	if subject := gitOut(t, nb, "log", "-1", "--format=%s"); subject != "merge: origin/main" {
		t.Fatalf("subject = %q", subject)
	}
	if dirty, _ := nb.IsDirty(); dirty {
		t.Fatal("the working tree differs from the merge commit")
	}
}

func TestPullConflictLeavesTheNotebookUntouched(t *testing.T) {
	nb := fixture(t, "diverged-conflict")
	head := gitOut(t, nb, "rev-parse", "HEAD")
	_, err := nb.Pull()
	if err == nil {
		t.Fatal("expected a conflict")
	}
	// The text the Rust version printed, after its `noda: ` prefix.
	const expected = "pull: `main` conflicts with the remote in 01abc234-budget-review.md — the merge was rolled back; resolve it with git in <ROOT>/data/noda/notebooks/default"
	if text := masked(nb, err.Error()); text != expected {
		t.Fatalf("error = %q", text)
	}
	if gitOut(t, nb, "rev-parse", "HEAD") != head {
		t.Fatal("HEAD moved")
	}
	if dirty, _ := nb.IsDirty(); dirty {
		t.Fatal("the working tree changed")
	}
	if _, err := os.Stat(filepath.Join(nb.Path, ".git", "MERGE_HEAD")); err == nil {
		t.Fatal("a merge was left in progress")
	}
}

func TestPullFastForwardsAndPushThenMatches(t *testing.T) {
	nb := fixture(t, "behind")
	pulled, err := nb.Pull()
	if got := mustSync(t, pulled, err); !regexp.MustCompile(`^pull: fast-forwarded 2 commits to [0-9a-f]{7}$`).MatchString(got) {
		t.Fatalf("pull = %q", got)
	}
	again, err := nb.Pull()
	if got := mustSync(t, again, err); got != "pull: already up to date" {
		t.Fatalf("second pull = %q", got)
	}
	pushed, err := nb.Push()
	const want = "push: main matches file://<ROOT>/remote.git — nothing to send"
	if got := masked(nb, mustSync(t, pushed, err)); got != want {
		t.Fatalf("push = %q, want %q", got, want)
	}
}

func TestPullRefusesWithUncommittedChanges(t *testing.T) {
	nb := fixture(t, "behind")
	write(t, nb, "scratch.md", "x")
	if _, err := nb.Pull(); err == nil || !strings.Contains(err.Error(), "has uncommitted changes") {
		t.Fatalf("err = %v", err)
	}
}

func TestPushCountsCommitsAndSnapshotsAndKeepsTheTrackingRef(t *testing.T) {
	nb := fixture(t, "ahead")
	pushed, err := nb.Push()
	const want = "push: main (2 commits, 1 snapshot) -> file://<ROOT>/remote.git"
	if got := masked(nb, mustSync(t, pushed, err)); got != want {
		t.Fatalf("push = %q, want %q", got, want)
	}
	drift, err := nb.Drift("main")
	if err != nil || drift.Ahead != 0 {
		t.Fatalf("drift = %+v, %v", drift, err)
	}
	again, err := nb.Push()
	if got := masked(nb, mustSync(t, again, err)); !strings.Contains(got, "matches file://<ROOT>/remote.git — nothing to send") {
		t.Fatalf("second push = %q", got)
	}
}

func TestPushHoldsBackASnapshotTheRemoteHasElsewhere(t *testing.T) {
	nb := fixture(t, "tags")
	pushed, err := nb.Push()
	got := strings.Split(masked(nb, mustSync(t, pushed, err)), "\n")
	same(t, "push", got, []string{
		"push: main (1 commit, 1 snapshot) -> file://<ROOT>/remote.git",
		"snapshot `clash` was not sent — the remote already has that name for another commit; rename yours, or drop it with `git tag -d clash`",
	})
}

func TestPushRejectedAsksForAPull(t *testing.T) {
	nb := fixture(t, "diverged-clean")
	_, err := nb.Push()
	// The same text the Rust version had, which libgit2 wrote and git does not.
	const expected = "push rejected — " + nonFastForward + "\nthe remote has commits you do not: run `noda pull` first"
	if err == nil || err.Error() != expected {
		t.Fatalf("err = %v", err)
	}
}

func TestPushRunsNoHook(t *testing.T) {
	nb := fixture(t, "ahead")
	marker := filepath.Join(rootOf(nb), "hook-ran")
	hook := "#!/bin/sh\ntouch " + marker + "\nexit 1\n"
	if err := os.WriteFile(filepath.Join(nb.Path, ".git", "hooks", "pre-push"), []byte(hook), 0o755); err != nil {
		t.Fatal(err)
	}
	if _, err := nb.Push(); err != nil {
		t.Fatal(err)
	}
	if _, err := os.Stat(marker); err == nil {
		t.Fatal("pre-push ran")
	}
}

func TestSyncWithoutARemoteSaysHowToSetOne(t *testing.T) {
	nb := newTestNotebook(t)
	for name, call := range map[string]func() error{
		"pull":  func() error { _, err := nb.Pull(); return err },
		"push":  func() error { _, err := nb.Push(); return err },
		"fetch": func() error { _, _, err := nb.Fetch(); return err },
	} {
		if err := call(); err == nil || !strings.Contains(err.Error(), "has no remote — set one with `noda remote set <url>`") {
			t.Fatalf("%s: %v", name, err)
		}
	}
}

func TestFetchOfAnEmptyRemoteIsNotAnError(t *testing.T) {
	nb := fixture(t, "ahead")
	empty := filepath.Join(rootOf(nb), "empty.git")
	gitOut(t, nb, "init", "--quiet", "--bare", empty)
	if err := nb.SetRemote("file://" + empty); err != nil {
		t.Fatal(err)
	}
	pulled, err := nb.Pull()
	if got := mustSync(t, pulled, err); got != "pull: the remote has no `main` branch yet" {
		t.Fatalf("pull = %q", got)
	}
}

func TestFetchKeepsLocalSnapshotsWhenPruneTagsIsOn(t *testing.T) {
	nb := fixture(t, "ahead")
	gitOut(t, nb, "config", "fetch.pruneTags", "true")
	if _, _, err := nb.Fetch(); err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(gitOut(t, nb, "tag", "-l"), "before-books") {
		t.Fatal("a local snapshot was pruned")
	}
}

func TestCloneAdoptsTheOnlyBranchWhenHeadNamesAMissingOne(t *testing.T) {
	nb := fixture(t, "linear")
	remoteDir := filepath.Join(rootOf(nb), "origin.git")
	gitOut(t, nb, "clone", "--quiet", "--bare", nb.Path, remoteDir)
	gitOut(t, nb, "-C", remoteDir, "branch", "-m", "main", "trunk")
	gitOut(t, nb, "-C", remoteDir, "symbolic-ref", "HEAD", "refs/heads/missing")

	other := paths.Rooted(t.TempDir())
	cloned, err := Clone(nb.git, other, CloneOptions{URL: "file://" + remoteDir, Name: "copy"})
	if err != nil {
		t.Fatal(err)
	}
	if branch, _ := cloned.Branch(); branch != "trunk" {
		t.Fatalf("branch = %q", branch)
	}
	if dirty, _ := cloned.IsDirty(); dirty {
		t.Fatal("the checkout is not clean")
	}
	if _, err := Clone(nb.git, other, CloneOptions{URL: "file://" + remoteDir, Name: "copy"}); err == nil || !strings.Contains(err.Error(), "notebook already exists: copy") {
		t.Fatalf("second clone: %v", err)
	}
}

func TestCloneFailureLeavesNothingBehind(t *testing.T) {
	nb := fixture(t, "linear")
	other := paths.Rooted(t.TempDir())
	_, err := Clone(nb.git, other, CloneOptions{URL: "file://" + filepath.Join(rootOf(nb), "nowhere.git"), Name: "x"})
	if err == nil {
		t.Fatal("expected a failure")
	}
	if Exists(other, "x") {
		t.Fatal("a failed clone left its directory")
	}
	empty := filepath.Join(rootOf(nb), "empty.git")
	gitOut(t, nb, "init", "--quiet", "--bare", empty)
	_, err = Clone(nb.git, other, CloneOptions{URL: "file://" + empty, Name: "e"})
	if err == nil || err.Error() != "the remote has no commits yet" || Exists(other, "e") {
		t.Fatalf("empty remote: %v", err)
	}
}

func TestCloneNameFromURL(t *testing.T) {
	for url, want := range map[string]string{
		"https://example.com/team/notes.git": "notes",
		"git@example.com:team/notes.git/":    "notes",
		"git@example.com:notes":              "notes",
		"/srv/notes.git.git":                 "notes",
		"https://example.com/":               "example.com",
		"":                                   "",
	} {
		if got := NameFromURL(url); got != want {
			t.Errorf("NameFromURL(%q) = %q, want %q", url, got, want)
		}
	}
}

// withEnv replaces the notebook's git with one that sees extra variables.
func withEnv(t *testing.T, nb *Notebook, extra map[string]string) {
	t.Helper()
	env := gittest.Isolate(t)
	env["TZ"] = "UTC"
	for name, value := range extra {
		env[name] = value
	}
	git, err := gitx.New(gitx.Options{Env: env})
	if err != nil {
		t.Fatal(err)
	}
	nb.git = git
}

func TestSSHFailureIsExplained(t *testing.T) {
	nb := fixture(t, "ahead")
	script := filepath.Join(t.TempDir(), "ssh")
	body := "#!/bin/sh\necho 'git@example.com: Permission denied (publickey).' >&2\nexit 255\n"
	if err := os.WriteFile(script, []byte(body), 0o755); err != nil {
		t.Fatal(err)
	}
	withEnv(t, nb, map[string]string{"GIT_SSH_COMMAND": script})
	if err := nb.SetRemote("ssh://git@example.com/team/notes.git"); err != nil {
		t.Fatal(err)
	}
	_, _, err := nb.Fetch()
	if err == nil || !strings.Contains(err.Error(), "Permission denied (publickey)") || !strings.Contains(err.Error(), "ssh-add -l") {
		t.Fatalf("err = %v", err)
	}
}

// fakeSSH puts an `ssh` first on PATH that records its arguments.
func fakeSSH(t *testing.T, nb *Notebook) (record string) {
	t.Helper()
	dir := t.TempDir()
	record = filepath.Join(dir, "args")
	body := "#!/bin/sh\necho \"$*\" > " + record + "\nexit 255\n"
	if err := os.WriteFile(filepath.Join(dir, "ssh"), []byte(body), 0o755); err != nil {
		t.Fatal(err)
	}
	path, _ := nb.git.Getenv("PATH")
	withEnv(t, nb, map[string]string{"PATH": dir + string(os.PathListSeparator) + path})
	if err := nb.SetRemote("ssh://git@example.com/x.git"); err != nil {
		t.Fatal(err)
	}
	return record
}

func sshArgs(t *testing.T, record string) string {
	t.Helper()
	raw, err := os.ReadFile(record)
	if err != nil {
		t.Fatal(err)
	}
	return string(raw)
}

func TestSSHIsKeptFromPromptingUnlessTheNotebookIsInteractive(t *testing.T) {
	nb := fixture(t, "ahead")
	record := fakeSSH(t, nb)
	_, _, _ = nb.Fetch()
	if !strings.Contains(sshArgs(t, record), "BatchMode=yes") {
		t.Fatalf("ssh was started with %q", sshArgs(t, record))
	}
	nb.Interactive = true
	_, _, _ = nb.Fetch()
	if strings.Contains(sshArgs(t, record), "BatchMode") {
		t.Fatalf("ssh was started with %q", sshArgs(t, record))
	}
}

func TestAUsersOwnSSHCommandIsLeftAlone(t *testing.T) {
	nb := fixture(t, "ahead")
	record := fakeSSH(t, nb)
	gitOut(t, nb, "config", "core.sshCommand", "ssh -o Mine=1")
	_, _, _ = nb.Fetch()
	args := sshArgs(t, record)
	if !strings.Contains(args, "Mine=1") || strings.Contains(args, "BatchMode") {
		t.Fatalf("ssh was started with %q", args)
	}
}

// httpRemote serves the fixture's bare remote over HTTP with Basic Auth.
func httpRemote(t *testing.T, nb *Notebook, user, password string) string {
	t.Helper()
	remoteDir := filepath.Join(rootOf(nb), "remote.git")
	gitOut(t, nb, "-C", remoteDir, "config", "http.receivepack", "true")
	execPath := gitOut(t, nb, "--exec-path")
	backend := &cgi.Handler{
		Path: filepath.Join(execPath, "git-http-backend"),
		Env:  []string{"GIT_PROJECT_ROOT=" + rootOf(nb), "GIT_HTTP_EXPORT_ALL=1"},
	}
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		u, p, ok := r.BasicAuth()
		if !ok || u != user || p != password {
			w.Header().Set("WWW-Authenticate", `Basic realm="noda"`)
			http.Error(w, "unauthorized", http.StatusUnauthorized)
			return
		}
		backend.ServeHTTP(w, r)
	}))
	t.Cleanup(server.Close)
	return server.URL + "/remote.git"
}

func TestHTTPSUsesTheCredentialHelper(t *testing.T) {
	nb := fixture(t, "behind")
	url := httpRemote(t, nb, "u", "p")
	if err := nb.SetRemote(url); err != nil {
		t.Fatal(err)
	}
	if _, err := nb.Pull(); err == nil || !strings.Contains(err.Error(), "git credential approve") {
		t.Fatalf("without a helper: %v", err)
	}
	gitOut(t, nb, "config", "credential.helper", "!f() { echo username=u; echo password=p; }; f")
	pulled, err := nb.Pull()
	if got := mustSync(t, pulled, err); !strings.HasPrefix(got, "pull: fast-forwarded 2 commits") {
		t.Fatalf("pull = %q", got)
	}
}

func TestHTTPSErrorsNeverShowTheTokenInTheURL(t *testing.T) {
	nb := fixture(t, "behind")
	url := httpRemote(t, nb, "right", "right")
	secret := "s3cr3t"
	if err := nb.SetRemote(strings.Replace(url, "http://", "http://"+secret+"@", 1)); err != nil {
		t.Fatal(err)
	}
	_, err := nb.Pull()
	if err == nil {
		t.Fatal("expected a failure")
	}
	if strings.Contains(err.Error(), secret) {
		t.Fatalf("the token reached the error: %v", err)
	}
}

// tracedGit puts a `git` first on PATH that logs its arguments and runs the
// real one with GIT_TRACE on, so the log holds what git itself spawns (the
// remote helper, with its URL) and not only what noda asked for. gitx drops
// GIT_TRACE from the environment, hence the wrapper.
func tracedGit(t *testing.T, nb *Notebook) (log string) {
	t.Helper()
	real, err := exec.LookPath("git")
	if err != nil {
		t.Fatal(err)
	}
	dir := t.TempDir()
	log = filepath.Join(dir, "trace")
	body := "#!/bin/sh\necho \"git $*\" >> " + log + "\nGIT_TRACE=" + log + " exec " + real + " \"$@\"\n"
	if err := os.WriteFile(filepath.Join(dir, "git"), []byte(body), 0o755); err != nil {
		t.Fatal(err)
	}
	path, _ := nb.git.Getenv("PATH")
	withEnv(t, nb, map[string]string{"PATH": dir + string(os.PathListSeparator) + path})
	return log
}

func TestALoginInTheURLWorksAndNeverReachesGitsArguments(t *testing.T) {
	nb := fixture(t, "behind")
	url := httpRemote(t, nb, "u", "SECRETpw")
	withLogin := strings.Replace(url, "http://", "http://u:SECRETpw@", 1)
	if err := nb.SetRemote(withLogin); err != nil {
		t.Fatal(err)
	}
	log := tracedGit(t, nb)
	pulled, err := nb.Pull()
	if got := mustSync(t, pulled, err); !strings.HasPrefix(got, "pull: fast-forwarded 2 commits") {
		t.Fatalf("pull = %q", got)
	}
	trace, err := os.ReadFile(log)
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(string(trace), "remote-http") {
		t.Fatalf("the trace shows no remote helper:\n%s", trace)
	}
	if strings.Contains(string(trace), "SECRETpw") {
		t.Fatalf("the secret is in a command line git ran:\n%s", trace)
	}
	if configured, _ := nb.configuredRemote(); configured != withLogin {
		t.Fatalf("the remote was changed to %q", configured)
	}
}

func TestPushWithALoginInTheURLSendsToOneRemoteOnly(t *testing.T) {
	nb := fixture(t, "ahead")
	url := httpRemote(t, nb, "u", "SECRETpw")
	other := filepath.Join(t.TempDir(), "other.git")
	gitOut(t, nb, "init", "--bare", "--quiet", other)
	// A second URL on the remote: git pushes to all of them, so it receives the
	// branch unless noda names the one repository itself.
	gitOut(t, nb, "config", "remote.origin.url", other)
	withLogin := strings.Replace(url, "http://", "http://u:SECRETpw@", 1)
	gitOut(t, nb, "config", "--add", "remote.origin.url", withLogin)
	log := tracedGit(t, nb)
	if _, err := nb.Push(); err != nil {
		t.Fatal(err)
	}
	remoteDir := filepath.Join(rootOf(nb), "remote.git")
	if got, want := gitOut(t, nb, "-C", remoteDir, "rev-parse", "main"), gitOut(t, nb, "rev-parse", "main"); got != want {
		t.Fatalf("the remote has %q, want %q", got, want)
	}
	if refs := gitOut(t, nb, "-C", other, "for-each-ref"); refs != "" {
		t.Fatalf("the other URL received a push:\n%s", refs)
	}
	trace, _ := os.ReadFile(log)
	if strings.Contains(string(trace), "SECRETpw") {
		t.Fatalf("the secret is in a command line git ran:\n%s", trace)
	}
}

// git moves the tracking ref only for a push made under the remote's name, and a
// login in the URL makes noda push to the bare URL instead.
func TestPushWithALoginInTheURLMovesTheTrackingRef(t *testing.T) {
	nb := fixture(t, "ahead")
	url := httpRemote(t, nb, "u", "SECRETpw")
	if err := nb.SetRemote(strings.Replace(url, "http://", "http://u:SECRETpw@", 1)); err != nil {
		t.Fatal(err)
	}
	if _, err := nb.Push(); err != nil {
		t.Fatal(err)
	}
	drift, err := nb.Drift("main")
	if err != nil || drift == nil || drift.Ahead != 0 || drift.Behind != 0 {
		t.Fatalf("drift after the push = %+v, %v", drift, err)
	}
	if got, want := gitOut(t, nb, "rev-parse", trackingRef("main")), gitOut(t, nb, "rev-parse", "main"); got != want {
		t.Fatalf("tracking ref = %q, want %q", got, want)
	}
}

func TestPushOfABranchWithNoCommitIsRefused(t *testing.T) {
	nb := fixture(t, "ahead")
	gitOut(t, nb, "update-ref", "-d", "refs/heads/main")
	gitOut(t, nb, "update-ref", "-d", trackingRef("main"))
	remoteDir := filepath.Join(rootOf(nb), "remote.git")
	before := gitOut(t, nb, "-C", remoteDir, "rev-parse", "main")
	if _, err := nb.Push(); err == nil {
		t.Fatal("expected a refusal")
	}
	if after := gitOut(t, nb, "-C", remoteDir, "rev-parse", "main"); after != before {
		t.Fatalf("the remote's main moved from %q to %q", before, after)
	}
}

func TestPushSaysSoWhenItCannotMoveTheTrackingRef(t *testing.T) {
	nb := fixture(t, "ahead")
	url := httpRemote(t, nb, "u", "SECRETpw")
	if err := nb.SetRemote(strings.Replace(url, "http://", "http://u:SECRETpw@", 1)); err != nil {
		t.Fatal(err)
	}
	lock := filepath.Join(nb.Path, ".git", trackingRef("main")+".lock")
	if err := os.MkdirAll(filepath.Dir(lock), 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(lock, nil, 0o644); err != nil {
		t.Fatal(err)
	}
	if _, err := nb.Push(); err == nil || !strings.Contains(err.Error(), "main.lock") {
		t.Fatalf("err = %v, want the lock named", err)
	}
}

func TestASnapshotNameTakenOnBothSidesDoesNotFailThePull(t *testing.T) {
	nb := fixture(t, "tags")
	before := gitOut(t, nb, "for-each-ref", "refs/tags")
	if _, err := nb.Pull(); err != nil {
		t.Fatalf("pull: %v", err)
	}
	if after := gitOut(t, nb, "for-each-ref", "refs/tags"); !strings.Contains(after, before) {
		t.Fatalf("a local tag changed:\n%s\nbecame\n%s", before, after)
	}
	if left := gitOut(t, nb, "for-each-ref", remoteTagsRef); left != "" {
		t.Fatalf("the private copies stayed: %s", left)
	}
}

func TestPullBringsInATagTheNotebookLacks(t *testing.T) {
	nb := fixture(t, "behind")
	remoteDir := filepath.Join(rootOf(nb), "remote.git")
	gitOut(t, nb, "-C", remoteDir, "tag", "from-elsewhere", "main")
	if _, err := nb.Pull(); err != nil {
		t.Fatal(err)
	}
	remoteTip := gitOut(t, nb, "-C", remoteDir, "rev-parse", "from-elsewhere")
	if got := gitOut(t, nb, "rev-parse", "refs/tags/from-elsewhere"); got != remoteTip {
		t.Fatalf("tag = %q, want %q", got, remoteTip)
	}
}

func TestAContextStopsTheGitRunsOfANotebookCopy(t *testing.T) {
	nb := fixture(t, "behind")
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if _, _, err := nb.WithContext(ctx).Fetch(); !errors.Is(err, context.Canceled) {
		t.Fatalf("err = %v", err)
	}
	if _, _, err := nb.Fetch(); err != nil {
		t.Fatalf("the original is affected: %v", err)
	}
}

func TestPullMergesHistoriesThatShareNoCommit(t *testing.T) {
	nb := fixture(t, "ahead")
	remoteDir := filepath.Join(rootOf(nb), "remote.git")
	emptyTree := gitOut(t, nb, "-C", remoteDir, "hash-object", "-t", "tree", "-w", "/dev/null")
	root := gitOut(t, nb, "-C", remoteDir, "commit-tree", emptyTree, "-m", "another machine's first commit")
	gitOut(t, nb, "-C", remoteDir, "update-ref", "refs/heads/main", root)
	pulled, err := nb.Pull()
	if got := mustSync(t, pulled, err); !strings.HasPrefix(got, "pull: merged") {
		t.Fatalf("pull = %q", got)
	}
	if dirty, _ := nb.IsDirty(); dirty {
		t.Fatal("the working tree differs from the merge commit")
	}
}
