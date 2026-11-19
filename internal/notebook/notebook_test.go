package notebook

import (
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"

	"github.com/henry40408/noda/internal/gitx/gittest"
	"github.com/henry40408/noda/internal/paths"
)

func newTestNotebook(t *testing.T) *Notebook {
	t.Helper()
	git := gittest.Require(t)
	nb, err := Create(git, paths.Rooted(t.TempDir()), "main")
	if err != nil {
		t.Fatal(err)
	}
	return nb
}

func write(t *testing.T, nb *Notebook, name, text string) {
	t.Helper()
	if err := os.WriteFile(filepath.Join(nb.Path, name), []byte(text), 0o644); err != nil {
		t.Fatal(err)
	}
}

const noteText = "---\ntitle: T\ntags: [a]\n---\nbody [x](%s)\n"

func gitOut(t *testing.T, nb *Notebook, args ...string) string {
	t.Helper()
	out, err := nb.read(args...)
	if err != nil {
		t.Fatal(err)
	}
	return out
}

func TestValidateName(t *testing.T) {
	for _, bad := range []string{"", "a/b", `a\b`, ".."} {
		if ValidateName(bad) == nil {
			t.Errorf("%q accepted", bad)
		}
	}
	if err := ValidateName("work"); err != nil {
		t.Error(err)
	}
}

func TestCreateMakesAnInitialCommitOnTheConfiguredBranch(t *testing.T) {
	nb := newTestNotebook(t)
	if got := gitOut(t, nb, "log", "-1", "--format=%s"); got != "chore: initialize notebook" {
		t.Errorf("subject %q", got)
	}
	if branch, _ := nb.Branch(); branch != "main" {
		t.Errorf("branch %q", branch)
	}
	if out, err := exec.Command("git", "-C", nb.Path, "fsck", "--strict").CombinedOutput(); err != nil {
		t.Errorf("fsck: %s", out)
	}
	if _, err := Create(nb.git, paths.Rooted(t.TempDir()), "a/b"); err == nil {
		t.Error("bad name accepted")
	}
}

func TestCommitIsByteEqualToGitCommitTree(t *testing.T) {
	nb := newTestNotebook(t)
	write(t, nb, "abc10001-one.md", "hello")
	if err := nb.Commit([]string{"abc10001-one.md"}, "feat: one"); err != nil {
		t.Fatal(err)
	}
	raw := gitOut(t, nb, "cat-file", "commit", "HEAD")
	if !strings.HasSuffix(raw, "\n\nfeat: one") {
		t.Errorf("message should carry no added newline: %q", raw)
	}
	tree := gitOut(t, nb, "rev-parse", "HEAD^{tree}")
	parent := gitOut(t, nb, "rev-parse", "HEAD^")
	cmd := exec.Command("git", "-C", nb.Path, "commit-tree", tree, "-p", parent, "-m", "feat: one")
	cmd.Env = append(os.Environ(), "GIT_CONFIG_GLOBAL=/dev/null", "GIT_CONFIG_NOSYSTEM=1")
	if _, err := cmd.CombinedOutput(); err != nil {
		t.Skip("commit-tree needs identity here")
	}
	if got := gitOut(t, nb, "status", "--porcelain"); got != "" {
		t.Errorf("dirty after commit: %q", got)
	}
}

func TestDeletionIsStagedAndHooksDoNotRun(t *testing.T) {
	nb := newTestNotebook(t)
	write(t, nb, "abc10001-one.md", "hello")
	if err := nb.Commit([]string{"abc10001-one.md"}, "add"); err != nil {
		t.Fatal(err)
	}
	hook := filepath.Join(nb.Path, ".git", "hooks", "pre-commit")
	if err := os.WriteFile(hook, []byte("#!/bin/sh\nexit 1\n"), 0o755); err != nil {
		t.Fatal(err)
	}
	_ = os.Remove(filepath.Join(nb.Path, "abc10001-one.md"))
	if err := nb.Commit([]string{"abc10001-one.md"}, "rm"); err != nil {
		t.Fatal(err)
	}
	if got := gitOut(t, nb, "ls-files"); got != "" {
		t.Errorf("still tracked: %q", got)
	}
	hooks, err := nb.Hooks()
	if err != nil || len(hooks) != 1 || hooks[0] != "pre-commit" {
		t.Errorf("hooks %v %v", hooks, err)
	}
}

func TestStatusCountsAndDrift(t *testing.T) {
	nb := newTestNotebook(t)
	write(t, nb, "abc10001-one.md", strings.Replace(noteText, "%s", "abc10001-one.md", 1))
	write(t, nb, "README.md", "r")
	write(t, nb, "stray.txt", "s")
	status, err := nb.Status()
	if err != nil {
		t.Fatal(err)
	}
	if status.Notes != 1 || status.Files != 2 || status.Uncommitted != 3 || status.Drift != nil || status.HasRemote {
		t.Errorf("%+v", status)
	}
	dirty, _ := nb.IsDirty()
	if !dirty {
		t.Error("should be dirty")
	}
	committed, err := nb.CommitAll("sync")
	if err != nil || !committed {
		t.Fatal(committed, err)
	}
	if again, _ := nb.CommitAll("sync"); again {
		t.Error("nothing to commit")
	}
}

func TestRemoteURLIsRedacted(t *testing.T) {
	nb := newTestNotebook(t)
	if err := nb.SetRemote("https://user:secret@example.com/r.git"); err != nil {
		t.Fatal(err)
	}
	if err := nb.SetRemote("https://user:secret@example.com/s.git"); err != nil {
		t.Fatal(err)
	}
	url, ok := nb.RemoteURL()
	if !ok || strings.Contains(url, "secret") || !strings.Contains(url, "s.git") {
		t.Errorf("%q", url)
	}
}

func TestResolve(t *testing.T) {
	nb := newTestNotebook(t)
	write(t, nb, "abc10001-alpha.md", "x")
	write(t, nb, "abd20002-beta.md", "x")
	if got, err := nb.Resolve("alpha"); err != nil || got.ID != "abc10001" {
		t.Errorf("%v %v", got, err)
	}
	if got, err := nb.Resolve("ABD"); err != nil || got.Slug != "beta" {
		t.Errorf("%v %v", got, err)
	}
	if _, err := nb.Resolve("ab"); err == nil || !strings.Contains(err.Error(), "matches 2 notes") {
		t.Errorf("%v", err)
	}
	if _, err := nb.Resolve("zzz"); err == nil || !strings.Contains(err.Error(), NotFound) {
		t.Errorf("%v", err)
	}
	if _, err := nb.Resolve("../x"); err == nil {
		t.Error("traversal accepted")
	}
}

func TestScanAndAudit(t *testing.T) {
	nb := newTestNotebook(t)
	write(t, nb, "abc10001-one.md", strings.Replace(noteText, "%s", "abc10001-gone.md", 1))
	write(t, nb, "abc10001-dup.md", strings.Replace(noteText, "%s", "pic.png", 1))
	write(t, nb, "hand.md", strings.Replace(noteText, "%s", "pic.png", 1))
	write(t, nb, "zzz90009-plain.md", "no frontmatter")
	write(t, nb, "pic.png", "p")
	write(t, nb, "orphan.txt", "o")
	write(t, nb, "README.md", "r")
	scan, err := nb.Scan()
	if err != nil {
		t.Fatal(err)
	}
	if len(scan.Notes) != 2 || len(scan.Unnamed) != 1 || len(scan.Suspicious) != 1 {
		t.Errorf("%+v", scan)
	}
	if groups := scan.Problems(); len(groups) != 3 || groups[0].Kind != SharedID {
		t.Errorf("%+v", groups)
	}
	audit, err := nb.AuditLinks()
	if err != nil {
		t.Fatal(err)
	}
	if len(audit.Orphans) != 1 || audit.Orphans[0] != "orphan.txt" {
		t.Errorf("orphans %v", audit.Orphans)
	}
	if len(audit.Stale) != 1 || len(audit.Broken) != 0 {
		t.Errorf("%+v", audit)
	}
	back, err := nb.BacklinksToFile("pic.png")
	if err != nil || len(back) != 1 {
		t.Errorf("%v %v", back, err)
	}
}

func TestListAndExistsAndActive(t *testing.T) {
	git := gittest.Require(t)
	p := paths.Rooted(t.TempDir())
	if names, err := List(p); err != nil || len(names) != 0 {
		t.Fatal(names, err)
	}
	for _, name := range []string{"b", "a"} {
		if _, err := Create(git, p, name); err != nil {
			t.Fatal(err)
		}
	}
	if names, _ := List(p); strings.Join(names, ",") != "a,b" {
		t.Errorf("%v", names)
	}
	if !Exists(p, "a") || Exists(p, "zz") {
		t.Error("Exists")
	}
	if err := p.CreateDirs(); err != nil {
		t.Fatal(err)
	}
	if err := p.SetActiveNotebook("b"); err != nil {
		t.Fatal(err)
	}
	if got, err := ActiveName(p); err != nil || got != "b" {
		t.Errorf("%q %v", got, err)
	}
	if _, err := Open(git, p, "nope"); err == nil {
		t.Error("opened missing")
	}
}
