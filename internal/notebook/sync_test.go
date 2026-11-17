package notebook

import (
	"context"
	"errors"
	"net/http"
	"net/http/cgi"
	"net/http/httptest"
	"os"
	"path/filepath"
	"regexp"
	"strings"
	"testing"

	"github.com/henry40408/noda/internal/gitx"
	"github.com/henry40408/noda/internal/gitx/gittest"
	"github.com/henry40408/noda/internal/paths"
)

// rootOf is the fixture directory, which the goldens write as <ROOT>.
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
	want := golden(t, "diverged-clean", "sync", "noda sync")
	pulled, err := nb.Pull()
	if got := mustSync(t, pulled, err); got != want[0] {
		t.Fatalf("pull = %q, want %q", got, want[0])
	}
	pushed, err := nb.Push()
	if got := masked(nb, mustSync(t, pushed, err)); got != want[1] {
		t.Fatalf("push = %q, want %q", got, want[1])
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
	// The Rust golden's stderr, after its `noda: ` prefix.
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
	want := golden(t, "behind", "sync", "noda sync")
	if got := masked(nb, mustSync(t, pushed, err)); got != want[1] {
		t.Fatalf("push = %q, want %q", got, want[1])
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
	want := golden(t, "ahead", "push", "noda push")
	if got := masked(nb, mustSync(t, pushed, err)); got != want[0] {
		t.Fatalf("push = %q, want %q", got, want[0])
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
	want := golden(t, "tags", "push", "noda push")
	got := strings.Split(masked(nb, mustSync(t, pushed, err)), "\n")
	same(t, "push", got, want)
}

func TestPushRejectedAsksForAPull(t *testing.T) {
	nb := fixture(t, "diverged-clean")
	_, err := nb.Push()
	// The same text the Rust golden has, which libgit2 wrote and git does not.
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
	nb.Fetch()
	if !strings.Contains(sshArgs(t, record), "BatchMode=yes") {
		t.Fatalf("ssh was started with %q", sshArgs(t, record))
	}
	nb.Interactive = true
	nb.Fetch()
	if strings.Contains(sshArgs(t, record), "BatchMode") {
		t.Fatalf("ssh was started with %q", sshArgs(t, record))
	}
}

func TestAUsersOwnSSHCommandIsLeftAlone(t *testing.T) {
	nb := fixture(t, "ahead")
	record := fakeSSH(t, nb)
	gitOut(t, nb, "config", "core.sshCommand", "ssh -o Mine=1")
	nb.Fetch()
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

func TestALoginInTheURLWorksAndNeverReachesGitsArguments(t *testing.T) {
	nb := fixture(t, "behind")
	url := httpRemote(t, nb, "u", "p@ss")
	withLogin := strings.Replace(url, "http://", "http://u:p%40ss@", 1)
	if err := nb.SetRemote(withLogin); err != nil {
		t.Fatal(err)
	}
	cmd := nb.networkCmd(withLogin, gitx.Cmd{Args: []string{"fetch", remoteName}})
	if strings.Contains(strings.Join(cmd.Args, " "), "p%40ss") || strings.Contains(strings.Join(cmd.Args, " "), "p@ss") {
		t.Fatalf("the secret is in the arguments: %v", cmd.Args)
	}
	pulled, err := nb.Pull()
	if got := mustSync(t, pulled, err); !strings.HasPrefix(got, "pull: fast-forwarded 2 commits") {
		t.Fatalf("pull = %q", got)
	}
	if configured, _ := nb.configuredRemote(); configured != withLogin {
		t.Fatalf("the remote was changed to %q", configured)
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
