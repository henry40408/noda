package gitx_test

import (
	"context"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/henry40408/noda/internal/gitx"
	"github.com/henry40408/noda/internal/gitx/gittest"
)

func freshRepo(t *testing.T, extraEnv map[string]string) (*gitx.Git, string) {
	t.Helper()
	env := gittest.Isolate(t)
	for name, value := range extraEnv {
		env[name] = value
	}
	git, err := gitx.New(gitx.Options{Env: env})
	if err != nil {
		t.Fatal(err)
	}
	dir := t.TempDir()
	if _, err := git.Run(context.Background(), gitx.Cmd{Dir: dir, Args: []string{"init", "--quiet"}}); err != nil {
		t.Fatal(err)
	}
	return git, dir
}

// appendGlobalConfig adds to the isolated global config gittest.Isolate made.
func appendGlobalConfig(t *testing.T, git *gitx.Git, text string) {
	t.Helper()
	env := map[string]string{}
	for _, name := range []string{"HOME", "GIT_CONFIG_GLOBAL"} {
		env[name], _ = git.Getenv(name)
	}
	gittest.WriteGlobalConfig(t, env, text, true)
}

func TestARunNeedsADirectory(t *testing.T) {
	git, _ := freshRepo(t, nil)
	if _, err := git.Run(context.Background(), gitx.Cmd{Args: []string{"status"}}); err == nil {
		t.Fatal("an empty directory means noda's own working directory")
	}
}

func TestADamagedGitDirectoryDoesNotFallThroughToTheRepositoryAbove(t *testing.T) {
	git, outer := freshRepo(t, nil)
	inner := filepath.Join(outer, "notebook")
	if err := os.MkdirAll(filepath.Join(inner, ".git"), 0o755); err != nil {
		t.Fatal(err)
	}
	result, err := git.Run(context.Background(), gitx.Cmd{Dir: inner, Args: []string{"rev-parse", "--show-toplevel"}, OKCodes: []int{0, 128}})
	if err != nil {
		t.Fatal(err)
	}
	if result.Code == 0 {
		t.Fatalf("git worked on %s", strings.TrimSpace(string(result.Stdout)))
	}
}

func TestHooksDoNotRunButAreStillFound(t *testing.T) {
	git, dir := freshRepo(t, nil)
	marker := filepath.Join(dir, "ran")
	hook := "#!/bin/sh\ntouch " + marker + "\n"
	if err := os.WriteFile(filepath.Join(dir, ".git", "hooks", "reference-transaction"), []byte(hook), 0o755); err != nil {
		t.Fatal(err)
	}
	ctx := context.Background()
	if _, err := git.Run(ctx, gitx.Cmd{Dir: dir, Args: []string{"update-ref", "refs/heads/x", "HEAD"}, OKCodes: []int{0, 128}}); err != nil {
		t.Fatal(err)
	}
	if _, err := git.Run(ctx, gitx.Cmd{Dir: dir, Args: []string{"symbolic-ref", "HEAD", "refs/heads/y"}}); err != nil {
		t.Fatal(err)
	}
	if _, err := os.Stat(marker); err == nil {
		t.Fatal("a hook ran")
	}
	found, err := gitx.Hooks(ctx, git, dir)
	if err != nil || len(found) != 1 || found[0] != "reference-transaction" {
		t.Fatalf("hooks = %v, %v", found, err)
	}
}

func TestCancellingStopsGitPromptly(t *testing.T) {
	git, dir := freshRepo(t, nil)
	ctx, cancel := context.WithTimeout(context.Background(), 300*time.Millisecond)
	defer cancel()
	started := time.Now()
	// `exec` makes sleep git's direct child: a plain `sleep 5` can stay behind a
	// shell that holds git's pipes open, and Run then waits out WaitDelay.
	_, err := git.Run(ctx, gitx.Cmd{Dir: dir, Args: []string{"-c", "alias.nap=!exec sleep 5", "nap"}})
	if err == nil || ctx.Err() == nil {
		t.Fatalf("err = %v", err)
	}
	if time.Since(started) > 3*time.Second {
		t.Fatalf("took %v to stop", time.Since(started))
	}
}

func TestErrorNamesTheReasonNotTheAdviceAfterIt(t *testing.T) {
	err := &gitx.Error{Args: []string{"push"}, Code: 1, Stderr: "error: failed to push some refs\nhint: Updates were rejected\nhint: See the note\n"}
	if err.Error() != "failed to push some refs" {
		t.Fatalf("%q", err.Error())
	}
	err = &gitx.Error{Args: []string{"-c", "a=b", "fetch"}, Code: 7}
	if err.Error() != "git fetch exited with code 7" {
		t.Fatalf("%q", err.Error())
	}
}

func TestBatchTellsAMissingPathWithSpacesFromAnError(t *testing.T) {
	git, dir := freshRepo(t, nil)
	batch, err := git.OpenBatch(context.Background(), dir)
	if err != nil {
		t.Fatal(err)
	}
	defer batch.Close()
	for _, spec := range []string{"HEAD:my file.md", "HEAD:a b c.md"} {
		_, _, ok, err := batch.Read(spec)
		if err != nil || ok {
			t.Fatalf("%q: ok=%v err=%v", spec, ok, err)
		}
	}
}

func TestBatchOutsideARepositoryExplainsAndStaysBroken(t *testing.T) {
	git, _ := freshRepo(t, nil)
	batch, err := git.OpenBatch(context.Background(), t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	defer batch.Close()
	_, _, _, first := batch.Read("HEAD:a.md")
	if first == nil || !strings.Contains(first.Error(), "not a git repository") {
		t.Fatalf("first read: %v", first)
	}
	_, _, _, second := batch.Read("HEAD:b.md")
	if second == nil || !strings.Contains(second.Error(), "no longer usable") {
		t.Fatalf("second read: %v", second)
	}
}

func TestARelativePathEntryDoesNotChooseGit(t *testing.T) {
	env := gittest.Isolate(t)
	trap := t.TempDir()
	if err := os.WriteFile(filepath.Join(trap, "git"), []byte("#!/bin/sh\nexit 1\n"), 0o755); err != nil {
		t.Fatal(err)
	}
	t.Chdir(trap)
	env["PATH"] = ".:" + env["PATH"]
	git, err := gitx.New(gitx.Options{Env: env})
	if err != nil {
		t.Fatal(err)
	}
	if _, err := git.Run(context.Background(), gitx.Cmd{Dir: trap, Args: []string{"version"}}); err != nil {
		t.Fatal(err)
	}
}

func TestDiffSettingsOfTheUserDoNotChangeAPatch(t *testing.T) {
	git, dir := freshRepo(t, map[string]string{"GIT_DIFF_OPTS": "--unified=0"})
	ctx := context.Background()
	text := "a\n\nb\nc\nd\ne\nf\ng\n"
	if err := os.WriteFile(filepath.Join(dir, "f"), []byte(text), 0o644); err != nil {
		t.Fatal(err)
	}
	for _, args := range [][]string{{"add", "f"}, {"-c", "user.name=n", "-c", "user.email=e@x", "commit", "--quiet", "-m", "m"}} {
		if _, err := git.Run(ctx, gitx.Cmd{Dir: dir, Args: args}); err != nil {
			t.Fatal(err)
		}
	}
	if err := os.WriteFile(filepath.Join(dir, "f"), []byte(strings.Replace(text, "e\n", "E\n", 1)), 0o644); err != nil {
		t.Fatal(err)
	}
	config := "[diff]\n\tsuppressBlankEmpty = true\n\torderFile = /nonexistent\n[color \"diff\"]\n\tnew = red\n[color]\n\tdiff = always\n"
	appendGlobalConfig(t, git, config)
	result, err := git.Run(ctx, gitx.Cmd{Dir: dir, Args: []string{"diff", "--unified=3"}})
	if err != nil {
		t.Fatal(err)
	}
	if strings.Contains(string(result.Stdout), "\x1b[") || !strings.Contains(string(result.Stdout), "\n c\n") {
		t.Fatalf("the patch followed a user's setting:\n%q", result.Stdout)
	}
}
