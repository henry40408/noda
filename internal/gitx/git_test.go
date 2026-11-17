package gitx_test

import (
	"context"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/henry40408/noda/internal/gitx"
	"github.com/henry40408/noda/internal/gitx/gittest"
)

func run(t *testing.T, git *gitx.Git, dir string, args ...string) string {
	t.Helper()
	result, err := git.Run(context.Background(), gitx.Cmd{Dir: dir, Args: args})
	if err != nil {
		t.Fatalf("git %v: %v", args, err)
	}
	return string(result.Stdout)
}

func newRepo(t *testing.T, git *gitx.Git) string {
	t.Helper()
	dir := t.TempDir()
	run(t, git, dir, "init", "--quiet")
	return dir
}

func commitFile(t *testing.T, git *gitx.Git, dir, name, content string) {
	t.Helper()
	if err := os.WriteFile(filepath.Join(dir, name), []byte(content), 0o644); err != nil {
		t.Fatal(err)
	}
	run(t, git, dir, "add", "--all")
	run(t, git, dir, "commit", "--quiet", "-m", "add "+name)
}

func TestRunActsOnDirWhateverTheParentEnvironmentSays(t *testing.T) {
	env := gittest.Isolate(t)
	env["GIT_DIR"] = "/elsewhere"
	env["GIT_WORK_TREE"] = "/elsewhere"
	env["GIT_CONFIG_PARAMETERS"] = "'core.bare=true'"
	git, err := gitx.New(gitx.Options{Env: env})
	if err != nil {
		t.Fatal(err)
	}
	dir := newRepo(t, git)
	want, _ := filepath.EvalSymlinks(dir)
	got := strings.TrimSpace(run(t, git, dir, "rev-parse", "--show-toplevel"))
	if resolved, _ := filepath.EvalSymlinks(got); resolved != want {
		t.Fatalf("ran in %s, want %s", got, want)
	}
}

func TestUserSettingsDoNotChangeParsedOutput(t *testing.T) {
	env := gittest.Isolate(t)
	settings := "[diff]\n\tnoprefix = true\n\tmnemonicPrefix = true\n\texternal = /nonexistent/driver\n[color]\n\tui = always\n" +
		"[log]\n\tshowSignature = true\n\tdecorate = full\n[core]\n\tquotePath = true\n"
	f, err := os.OpenFile(env["GIT_CONFIG_GLOBAL"], os.O_APPEND|os.O_WRONLY, 0)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := f.WriteString(settings); err != nil {
		t.Fatal(err)
	}
	f.Close()
	git, err := gitx.New(gitx.Options{Env: env})
	if err != nil {
		t.Fatal(err)
	}
	dir := newRepo(t, git)
	commitFile(t, git, dir, "é.md", "one\n")
	if err := os.WriteFile(filepath.Join(dir, "é.md"), []byte("two\n"), 0o644); err != nil {
		t.Fatal(err)
	}

	diff := run(t, git, dir, "diff", "--no-ext-diff", "HEAD")
	if !strings.Contains(diff, "diff --git a/é.md b/é.md") || strings.Contains(diff, "\x1b[") {
		t.Errorf("diff:\n%q", diff)
	}
	log := run(t, git, dir, "log", "--format=%H %s")
	if strings.Contains(log, "gpg") || strings.Contains(log, "(") {
		t.Errorf("log:\n%q", log)
	}
	if names := run(t, git, dir, "ls-files"); strings.TrimSpace(names) != "é.md" {
		t.Errorf("ls-files: %q", names)
	}
}

func TestRunReportsExitCodesAsErrorsUnlessAllowed(t *testing.T) {
	git := gittest.Require(t)
	dir := newRepo(t, git)

	_, err := git.Run(context.Background(), gitx.Cmd{Dir: dir, Args: []string{"rev-parse", "--verify", "--quiet", "nothing"}})
	if !gitx.IsExit(err, 1) {
		t.Fatalf("exit 1 should surface as an error: %v", err)
	}
	result, err := git.Run(context.Background(), gitx.Cmd{
		Dir: dir, Args: []string{"rev-parse", "--verify", "--quiet", "nothing"}, OKCodes: []int{0, 1}, ReadOnly: true,
	})
	if err != nil || result.Code != 1 {
		t.Fatalf("an allowed code is a result: %v %+v", err, result)
	}
	_, err = git.Run(context.Background(), gitx.Cmd{Dir: dir, Args: []string{"checkout", "no-such-branch"}})
	if err == nil || !strings.Contains(err.Error(), "no-such-branch") {
		t.Fatalf("the reason is git's last line: %v", err)
	}
}

func TestRunFeedsStdinAndCopiesStderrOnRequest(t *testing.T) {
	var passed strings.Builder
	env := gittest.Isolate(t)
	git, err := gitx.New(gitx.Options{Env: env, Stderr: &passed})
	if err != nil {
		t.Fatal(err)
	}
	dir := newRepo(t, git)
	result, err := git.Run(context.Background(), gitx.Cmd{Dir: dir, Args: []string{"hash-object", "--stdin"}, Stdin: []byte("hello\n")})
	if err != nil || strings.TrimSpace(string(result.Stdout)) != "ce013625030ba8dba906f756967f9e9ca394464a" {
		t.Fatalf("%v %q", err, result.Stdout)
	}
	_, _ = git.Run(context.Background(), gitx.Cmd{Dir: dir, Args: []string{"checkout", "nope"}, PassStderr: true})
	if !strings.Contains(passed.String(), "nope") {
		t.Errorf("stderr was not passed on: %q", passed.String())
	}
	passed.Reset()
	_, _ = git.Run(context.Background(), gitx.Cmd{Dir: dir, Args: []string{"checkout", "nope"}})
	if passed.Len() != 0 {
		t.Errorf("stderr leaked without PassStderr: %q", passed.String())
	}
}

func TestExtraEnvOutranksEverythingElse(t *testing.T) {
	git := gittest.Require(t)
	dir := newRepo(t, git)
	run(t, git, dir, "commit", "--quiet", "--allow-empty", "-m", "empty")
	result, err := git.Run(context.Background(), gitx.Cmd{
		Dir: dir, Args: []string{"commit", "--quiet", "--allow-empty", "-m", "dated"},
		ExtraEnv: map[string]string{"GIT_AUTHOR_DATE": "1700000000 +0000"},
	})
	if err != nil {
		t.Fatal(err, result)
	}
	if got := strings.TrimSpace(run(t, git, dir, "log", "-1", "--format=%at")); got != "1700000000" {
		t.Fatal(got)
	}
}

func TestBatchReadsObjectsAndMissesCleanly(t *testing.T) {
	git := gittest.Require(t)
	dir := newRepo(t, git)
	commitFile(t, git, dir, "a.md", "alpha\nline two\n")
	commitFile(t, git, dir, "b.md", "")

	batch, err := git.OpenBatch(context.Background(), dir)
	if err != nil {
		t.Fatal(err)
	}
	defer func() {
		if err := batch.Close(); err != nil {
			t.Error(err)
		}
	}()

	kind, data, ok, err := batch.Read("HEAD:a.md")
	if err != nil || !ok || kind != "blob" || string(data) != "alpha\nline two\n" {
		t.Fatalf("%q %q %v %v", kind, data, ok, err)
	}
	kind, data, ok, err = batch.Read("HEAD:b.md")
	if err != nil || !ok || kind != "blob" || len(data) != 0 {
		t.Fatalf("an empty blob: %q %q %v %v", kind, data, ok, err)
	}
	if _, _, ok, err = batch.Read("HEAD:missing.md"); err != nil || ok {
		t.Fatalf("a missing path: %v %v", ok, err)
	}
	kind, _, ok, err = batch.Read("HEAD")
	if err != nil || !ok || kind != "commit" {
		t.Fatalf("a commit: %q %v %v", kind, ok, err)
	}
	if _, _, _, err = batch.Read("a\nb"); err == nil {
		t.Fatal("a line break would split one request in two")
	}
	if _, _, ok, err = batch.Read(fmt.Sprintf("%040d", 7)); err != nil || ok {
		t.Fatalf("an unknown oid: %v %v", ok, err)
	}
}
