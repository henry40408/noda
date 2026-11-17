package cmd

import (
	"context"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"testing"

	"github.com/henry40408/noda/internal/gitx"
	"github.com/henry40408/noda/internal/notebook"
)

// eGit runs the isolated git in dir and returns its trimmed stdout.
func eGit(t *testing.T, e Env, dir string, args ...string) string {
	t.Helper()
	result, err := e.Git.Run(context.Background(), gitx.Cmd{Dir: dir, Args: args})
	if err != nil {
		t.Fatal(err)
	}
	return strings.TrimSpace(string(result.Stdout))
}

func eCommitCount(t *testing.T, e Env, dir string) int {
	t.Helper()
	count, err := strconv.Atoi(eGit(t, e, dir, "rev-list", "--count", "HEAD"))
	if err != nil {
		t.Fatal(err)
	}
	return count
}

// eHeadSignature is the signature stored in HEAD's commit object, if any.
func eHeadSignature(t *testing.T, e Env, dir string) (string, bool) {
	t.Helper()
	commit := eGit(t, e, dir, "cat-file", "commit", "HEAD")
	if !strings.Contains(commit, "\ngpgsig ") {
		return "", false
	}
	return commit, true
}

// eStubGpg writes an executable /bin/sh script that reads the commit on stdin
// and runs script. A real key would test gpg instead, on a keyring that may be
// locked.
func eStubGpg(t *testing.T, name, script string) string {
	t.Helper()
	path := filepath.Join(t.TempDir(), name)
	if err := os.WriteFile(path, []byte("#!/bin/sh\n"+script), 0o755); err != nil {
		t.Fatal(err)
	}
	return path
}

// eSigns is a stub that signs; eFails refuses.
const (
	eSigns = "cat > /dev/null\n" +
		"printf -- '-----BEGIN PGP SIGNATURE-----\\n\\nc3R1Yg==\\n-----END PGP SIGNATURE-----\\n'\n"
	eFails = "cat > /dev/null\nexit 1\n"
)

// eSignWith pins gpg.format and gpg.openpgp.program in the notebook's own
// repository config: they are consulted first, so without pinning them a
// developer's own setting would sign with their real key, and the test would
// pass without testing the stub.
func eSignWith(t *testing.T, e Env, name, program string) {
	t.Helper()
	dir := e.Paths.NotebookDir(name)
	eGit(t, e, dir, "config", "gpg.format", "openpgp")
	eGit(t, e, dir, "config", "gpg.openpgp.program", program)
	if _, err := ConfigSet(e, "sign", "true"); err != nil {
		t.Fatal(err)
	}
}

func eAdd(e Env, title, content string) (string, error) {
	return Add(e, AddOptions{Title: &title, Content: &content})
}

func eNoteFile(summary string) string {
	fields := strings.Split(summary, "  ")
	return fields[0] + "-" + fields[1] + ".md"
}

func TestSigningAttachesTheSignatureAndStillMovesTheBranch(t *testing.T) {
	e, opened := aNotebook(t)
	dir := opened.Path
	if _, signed := eHeadSignature(t, e, dir); signed {
		t.Fatal("unsigned by default")
	}

	before := eCommitCount(t, e, dir)
	eSignWith(t, e, "main", eStubGpg(t, "gpg-ok", eSigns))
	out, err := eAdd(e, "Alpha", "a\n")
	if err != nil {
		t.Fatal(err)
	}

	signature, signed := eHeadSignature(t, e, dir)
	if !signed || !strings.Contains(signature, "BEGIN PGP SIGNATURE") {
		t.Fatalf("signed: %v\n%s", signed, signature)
	}

	// The branch has to move with the commit object and leave a clean working
	// tree.
	if got := eCommitCount(t, e, dir); got != before+1 {
		t.Fatalf("commits %d, before %d", got, before)
	}
	listed, err := Ls(e, List{})
	if err != nil || !strings.Contains(listed, "Alpha") {
		t.Fatalf("ls: %q, %v", listed, err)
	}
	if !fileExists(filepath.Join(dir, eNoteFile(out))) {
		t.Fatalf("the note file is missing for %q", out)
	}
	if status := eGit(t, e, dir, "status", "--porcelain"); status != "" {
		t.Fatalf("nothing uncommitted, got:\n%s", status)
	}

	// The next commit builds on it.
	if _, err := eAdd(e, "Beta", "b\n"); err != nil {
		t.Fatal(err)
	}
	if parents := strings.Fields(eGit(t, e, dir, "rev-list", "--parents", "-n", "1", "HEAD")); len(parents) != 2 {
		t.Fatalf("one parent expected: %v", parents)
	}
	if got := eCommitCount(t, e, dir); got != before+2 {
		t.Fatalf("commits %d, before %d", got, before)
	}
}

func TestAGpgThatFailsTakesTheCommitWithIt(t *testing.T) {
	e, opened := aNotebook(t)
	eSignWith(t, e, "main", eStubGpg(t, "gpg-no", eFails))
	before := eCommitCount(t, e, opened.Path)

	_, err := eAdd(e, "Alpha", "a\n")
	if err == nil || !strings.Contains(err.Error(), "could not sign") {
		t.Fatalf("got %v", err)
	}
	if got := eCommitCount(t, e, opened.Path); got != before {
		t.Fatalf("an unsigned commit is not the fallback: %d, was %d", got, before)
	}
}

func TestAGpgThatIsNotGpgIsNotTakenAtItsWord(t *testing.T) {
	e, opened := aNotebook(t)
	// Exits 0 and prints nothing, as a wrong `gpg.program` would.
	eSignWith(t, e, "main", eStubGpg(t, "gpg-quiet", "cat > /dev/null\n"))
	before := eCommitCount(t, e, opened.Path)

	_, err := eAdd(e, "Alpha", "a\n")
	if err == nil || !strings.Contains(err.Error(), "no OpenPGP signature") {
		t.Fatalf("got %v", err)
	}
	if got := eCommitCount(t, e, opened.Path); got != before {
		t.Fatalf("commits %d, was %d", got, before)
	}
}

func TestAFormatNodaCannotSignIsRefusedAtTheCommit(t *testing.T) {
	e, opened := aNotebook(t)
	eSignWith(t, e, "main", eStubGpg(t, "gpg-ok2", eSigns))
	eGit(t, e, opened.Path, "config", "gpg.format", "ssh")

	_, err := eAdd(e, "Alpha", "a\n")
	if err == nil || !strings.Contains(err.Error(), "OpenPGP only") {
		t.Fatalf("got %v", err)
	}

	// Reading still works: the notebook is unsignable, not broken.
	if _, err := Ls(e, List{}); err != nil {
		t.Fatalf("ls: %v", err)
	}
}

// eBareRemote makes an empty bare repository whose HEAD names branch.
func eBareRemote(t *testing.T, e Env, branch string) string {
	t.Helper()
	path := filepath.Join(t.TempDir(), "origin.git")
	eGit(t, e, t.TempDir(), "init", "--quiet", "--bare", "--initial-branch="+branch, path)
	return path
}

func TestTheMergeAPullMakesIsSignedToo(t *testing.T) {
	e, _ := aNotebook(t)
	url := eBareRemote(t, e, eBranchOf(t, e, "main"))
	if _, err := RemoteSet(e, url); err != nil {
		t.Fatal(err)
	}
	if _, err := eAdd(e, "Alpha", "a\n"); err != nil {
		t.Fatal(err)
	}
	if _, err := Sync(e); err != nil {
		t.Fatal(err)
	}
	if _, err := Clone(e, url, "mirror"); err != nil {
		t.Fatal(err)
	}

	if _, err := eAdd(e, "Laptop", "l\n"); err != nil {
		t.Fatal(err)
	}
	if _, err := Sync(e); err != nil {
		t.Fatal(err)
	}

	// The merge is a commit like any other, so it is signed like any other.
	if _, err := Use(e, "mirror"); err != nil {
		t.Fatal(err)
	}
	eSignWith(t, e, "mirror", eStubGpg(t, "gpg-merge", eSigns))
	if _, err := eAdd(e, "Desktop", "d\n"); err != nil {
		t.Fatal(err)
	}
	out, err := Sync(e)
	if err != nil || !strings.Contains(out, "merged") {
		t.Fatalf("sync: %q, %v", out, err)
	}

	mirror := e.Paths.NotebookDir("mirror")
	if parents := strings.Fields(eGit(t, e, mirror, "rev-list", "--parents", "-n", "1", "HEAD")); len(parents) != 3 {
		t.Fatalf("the merge is what HEAD points at: %v", parents)
	}
	signature, signed := eHeadSignature(t, e, mirror)
	if !signed || !strings.Contains(signature, "BEGIN PGP SIGNATURE") {
		t.Fatalf("signed merge: %v\n%s", signed, signature)
	}
	if fileExists(filepath.Join(mirror, ".git", "MERGE_HEAD")) {
		t.Fatal("the repository is still mid-merge")
	}
}

// eBranchOf is `main` or `master` per `init.defaultBranch`, so no test may
// assume either.
func eBranchOf(t *testing.T, e Env, name string) string {
	t.Helper()
	opened, err := e.open(name)
	if err != nil {
		t.Fatal(err)
	}
	branch, err := opened.Branch()
	if err != nil {
		t.Fatal(err)
	}
	return branch
}

func TestSignIsASettingLikeAnyOther(t *testing.T) {
	e, _ := aNotebook(t)

	if got, err := ConfigGet(e, "sign"); err != nil || got != "false" {
		t.Fatalf("default: %q, %v", got, err)
	}
	if _, err := ConfigSet(e, "sign", "true"); err != nil {
		t.Fatal(err)
	}
	if got, _ := ConfigGet(e, "sign"); got != "true" {
		t.Fatalf("got %q", got)
	}

	// Written as a boolean, not a string.
	text, err := os.ReadFile(filepath.Join(e.Paths.ConfigDir(), "config.toml"))
	if err != nil || !strings.Contains(string(text), "sign = true") {
		t.Fatalf("config.toml: %q, %v", text, err)
	}

	shown := plain(ConfigShow(e))
	if !strings.Contains(shown, "sign      true") || !strings.Contains(shown, "(config.toml)") {
		t.Fatalf("show:\n%s", shown)
	}

	// git's spellings (`yes`) are refused rather than read as false.
	if _, err := ConfigSet(e, "sign", "yes"); err == nil || !strings.Contains(err.Error(), "`true` or `false`") {
		t.Fatalf("got %v", err)
	}
	if got, _ := ConfigGet(e, "sign"); got != "true" {
		t.Fatalf("a refused value changed the setting: %q", got)
	}

	// Unset hands the decision back to git.
	out, err := ConfigUnset(e, "sign")
	if err != nil || !strings.Contains(out, "now from") {
		t.Fatalf("unset: %q, %v", out, err)
	}
}

func TestANotebookSaysWhereItStandsInGitsOwnWords(t *testing.T) {
	cases := []struct {
		name      string
		hasRemote bool
		drift     *notebook.Drift
		want      string
	}{
		{"no remote", false, nil, "no remote"},
		{"never synced", true, nil, "never synced"},
		{"in sync", true, &notebook.Drift{}, "in sync"},
		{"ahead", true, &notebook.Drift{Ahead: 2}, "2 to push"},
		{"behind", true, &notebook.Drift{Behind: 1}, "1 to pull"},
		{"both", true, &notebook.Drift{Ahead: 2, Behind: 1}, "2 to push, 1 to pull"},
	}
	for _, c := range cases {
		if got := Standing(c.hasRemote, c.drift); got != c.want {
			t.Errorf("%s: got %q, want %q", c.name, got, c.want)
		}
	}
}

func TestANotebookWithNoRemoteIsNotMerelyUnsynced(t *testing.T) {
	if got := Drifted(nil); got != "never synced" {
		t.Fatalf("got %q", got)
	}
	if got := Standing(false, nil); got != "no remote" {
		t.Fatalf("got %q", got)
	}
	// No remote outranks the drift.
	if got := Standing(false, &notebook.Drift{Ahead: 2}); got != "no remote" {
		t.Fatalf("got %q", got)
	}
}
