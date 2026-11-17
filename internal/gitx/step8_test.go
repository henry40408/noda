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

func TestParseBool(t *testing.T) {
	for text, want := range map[string]bool{"true": true, "Yes": true, "on": true, "2": true, "false": false, "no": false, "off": false, "": false, "0": false} {
		got, ok := gitx.ParseBool(text)
		if !ok || got != want {
			t.Errorf("%q: %v %v", text, got, ok)
		}
	}
	if _, ok := gitx.ParseBool("maybe"); ok {
		t.Error("maybe is not a boolean")
	}
}

func TestCountStatusSkipsTheRenameOrigin(t *testing.T) {
	raw := "1 .M N... 100644 100644 100644 a b f\x002 R. N... 100644 100644 100644 a b R100 new\x00old\x00? u\x00"
	if got := gitx.CountStatus([]byte(raw)); got != 3 {
		t.Errorf("got %d", got)
	}
}

func TestParseRawDateAndLeftRight(t *testing.T) {
	seconds, offset, err := gitx.ParseRawDate("1700000000 -0130")
	if err != nil || seconds != 1700000000 || offset != -90 {
		t.Errorf("%d %d %v", seconds, offset, err)
	}
	if ahead, behind, err := gitx.ParseLeftRight([]byte("3\t4\n")); err != nil || ahead != 3 || behind != 4 {
		t.Errorf("%d %d %v", ahead, behind, err)
	}
}

func TestRawDateIsSigned(t *testing.T) {
	when := time.Unix(1700000000, 0).In(time.FixedZone("x", -(5*3600 + 30*60)))
	if got := (gitx.Ident{When: when}).RawDate(); got != "1700000000 -0530" {
		t.Error(got)
	}
}

func TestEncodeMatchesGitCommitTree(t *testing.T) {
	env := gittest.Isolate(t)
	git, err := gitx.New(gitx.Options{Env: env})
	if err != nil {
		t.Fatal(err)
	}
	dir := t.TempDir()
	run := func(extra map[string]string, stdin string, args ...string) string {
		t.Helper()
		result, err := git.Run(context.Background(), gitx.Cmd{Dir: dir, Args: args, Stdin: []byte(stdin), ExtraEnv: extra})
		if err != nil {
			t.Fatal(err)
		}
		return strings.TrimSpace(string(result.Stdout))
	}
	run(nil, "", "init", "--quiet")
	tree := run(nil, "", "write-tree")
	who := gitx.Ident{Name: "A B", Email: "a@b.c", When: time.Unix(1700000000, 0).In(time.FixedZone("x", 3600))}
	ours := gitx.CommitObject{Tree: tree, Author: who, Committer: who, Message: "msg\n"}.Encode()
	theirsID := run(map[string]string{
		"GIT_AUTHOR_NAME": "A B", "GIT_AUTHOR_EMAIL": "a@b.c", "GIT_AUTHOR_DATE": who.RawDate(),
		"GIT_COMMITTER_NAME": "A B", "GIT_COMMITTER_EMAIL": "a@b.c", "GIT_COMMITTER_DATE": who.RawDate(),
	}, "", "commit-tree", tree, "-m", "msg")
	ourID := run(nil, string(ours), "hash-object", "-t", "commit", "--stdin")
	if ourID != theirsID {
		t.Errorf("object ids differ: %s vs %s", ourID, theirsID)
	}
	signed := gitx.WithSignature(ours, []byte("-----BEGIN PGP SIGNATURE-----\n\nabc\n-----END PGP SIGNATURE-----\n"))
	if !strings.Contains(string(signed), "gpgsig -----BEGIN PGP SIGNATURE-----\n \n abc\n -----END PGP SIGNATURE-----\n \n\nmsg") {
		t.Errorf("%q", signed)
	}
}

func TestConfigLastValueWinsAndIncludes(t *testing.T) {
	env := gittest.Isolate(t)
	dir := filepath.Dir(env["GIT_CONFIG_GLOBAL"])
	extra := filepath.Join(dir, "extra")
	if err := os.WriteFile(extra, []byte("[commit]\n\tgpgsign = true\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	text := "[user]\n\tname = One\n\tname = Two\n\tflag\n[include]\n\tpath = " + extra + "\n"
	if err := os.WriteFile(env["GIT_CONFIG_GLOBAL"], []byte(text), 0o644); err != nil {
		t.Fatal(err)
	}
	git, err := gitx.New(gitx.Options{Env: env})
	if err != nil {
		t.Fatal(err)
	}
	values, err := gitx.LoadConfig(context.Background(), git, "")
	if err != nil {
		t.Fatal(err)
	}
	if name, _ := values.Get("USER.name"); name != "Two" {
		t.Errorf("name %q", name)
	}
	if on, set := values.Bool("commit.gpgsign"); !on || !set {
		t.Error("include not followed")
	}
	if on, set := values.Bool("user.flag"); !on || !set {
		t.Error("bare key is true")
	}
}
