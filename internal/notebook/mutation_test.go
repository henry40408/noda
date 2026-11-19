package notebook

import (
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"testing"
	"time"

	"github.com/henry40408/noda/internal/gitx"
	"github.com/henry40408/noda/internal/gitx/gittest"
	"github.com/henry40408/noda/internal/note"
	"github.com/henry40408/noda/internal/paths"
)

const plainNote = "---\ntitle: T\n---\nbody\n"

func writeNotes(t *testing.T, nb *Notebook, names ...string) {
	t.Helper()
	for _, name := range names {
		write(t, nb, name, plainNote)
	}
}

func TestScanSortsNotesByIDThenSlug(t *testing.T) {
	nb := newTestNotebook(t)
	writeNotes(t, nb, "abd20002-mid.md", "abc10001-zeta.md", "abc10001-alpha.md", "abe30003-first.md")
	scan, err := nb.Scan()
	if err != nil {
		t.Fatal(err)
	}
	want := []IDSlug{{"abc10001", "alpha"}, {"abc10001", "zeta"}, {"abd20002", "mid"}, {"abe30003", "first"}}
	if !reflect.DeepEqual(scan.Notes, want) {
		t.Errorf("got %v", scan.Notes)
	}
}

func TestNamedFilesSortByIDThenSlug(t *testing.T) {
	nb := newTestNotebook(t)
	writeNotes(t, nb, "abd20002-mid.md", "abc10001-zeta.md", "abc10001-alpha.md")
	got, err := nb.NamedFiles()
	if err != nil {
		t.Fatal(err)
	}
	want := []IDSlug{{"abc10001", "alpha"}, {"abc10001", "zeta"}, {"abd20002", "mid"}}
	if !reflect.DeepEqual(got, want) {
		t.Errorf("got %v", got)
	}
}

func TestNotesSortBySlugAndKeepFilenameOrderOnATie(t *testing.T) {
	nb := newTestNotebook(t)
	writeNotes(t, nb, "aaa10001-zeta.md", "bbb20002-alpha.md", "ccc30003-alpha.md")
	notes, err := nb.Notes()
	if err != nil {
		t.Fatal(err)
	}
	var got []string
	for _, found := range notes {
		got = append(got, found.ID+"-"+found.Slug)
	}
	want := []string{"bbb20002-alpha", "ccc30003-alpha", "aaa10001-zeta"}
	if !reflect.DeepEqual(got, want) {
		t.Errorf("got %v", got)
	}
}

func TestResolveListsCandidatesInOrder(t *testing.T) {
	nb := newTestNotebook(t)
	writeNotes(t, nb, "abc10001-zeta.md", "abc10001-alpha.md", "abd20002-mid.md", "abe30003-alpha.md")
	_, err := nb.Resolve("ab")
	want := "  abc10001  alpha\n  abc10001  zeta\n  abd20002  mid\n  abe30003  alpha"
	if err == nil || !strings.Contains(err.Error(), "matches 4 notes") || !strings.HasSuffix(err.Error(), want) {
		t.Errorf("%v", err)
	}
	_, err = nb.Resolve("alpha")
	want = "  abc10001  alpha\n  abe30003  alpha"
	if err == nil || !strings.Contains(err.Error(), "matches 2 notes") || !strings.HasSuffix(err.Error(), want) {
		t.Errorf("%v", err)
	}
}

func TestOneNoteIsNotAProblem(t *testing.T) {
	nb := newTestNotebook(t)
	writeNotes(t, nb, "abc10001-alpha.md", "abd20002-beta.md")
	scan, err := nb.Scan()
	if err != nil {
		t.Fatal(err)
	}
	if groups := scan.Problems(); len(groups) != 0 {
		t.Errorf("%+v", groups)
	}
}

func TestLinksToFileMatchesOnlyTheNamedFile(t *testing.T) {
	file := note.Note{Body: "see [a](a.png) and [b](b.png)\n"}
	if !LinksToFile(file, "b.png") || LinksToFile(file, "c.png") {
		t.Error("LinksToFile")
	}
	if LinksToFile(note.Note{Body: "no links\n"}, "a.png") {
		t.Error("a note without links")
	}
}

func TestUpdateRefArgsCheckTheOldHead(t *testing.T) {
	oid := strings.Repeat("a", 40)
	if got := updateRefArgs("m", oid, nil); got[4] != oid || got[5] != strings.Repeat("0", 40) {
		t.Errorf("unborn: %v", got)
	}
	parent := strings.Repeat("b", 40)
	if got := updateRefArgs("m", oid, []string{parent}); got[5] != parent {
		t.Errorf("born: %v", got)
	}
}

func TestColorDiffWithoutAFileHeaderPaintsNoEmptyHeader(t *testing.T) {
	got := ColorDiff("@@ -1 +1 @@\n-a\n+b\n")
	want := "\x1b[36m@@ -1 +1 @@\n\x1b[0m\x1b[31m-a\n\x1b[0m\x1b[32m+b\n\x1b[0m"
	if got != want {
		t.Errorf("got %q", got)
	}
}

func TestCommitSettingsReadsTheConfigAndIgnoresABrokenOne(t *testing.T) {
	p := paths.Rooted(t.TempDir())
	if err := os.MkdirAll(p.ConfigDir(), 0o755); err != nil {
		t.Fatal(err)
	}
	file := filepath.Join(p.ConfigDir(), "config.toml")
	if err := os.WriteFile(file, []byte("author = \"Ann <ann@example.com>\"\nsign = false\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	author, sign := commitSettings(p)
	if author == nil || author.Name != "Ann" || author.Email != "ann@example.com" || sign == nil || *sign {
		t.Errorf("%+v %v", author, sign)
	}
	if err := os.WriteFile(file, []byte("author = = broken\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	if author, sign := commitSettings(p); author != nil || sign != nil {
		t.Errorf("a broken config gave %+v %v", author, sign)
	}
}

func TestAuthorFromNeedsBothNameAndEmail(t *testing.T) {
	cases := []struct {
		config string
		want   bool
	}{
		{"[user]\n\tname = Ann\n\temail = ann@example.com\n", true},
		{"[user]\n\tname = Ann\n", false},
		{"[user]\n\temail = ann@example.com\n", false},
		{"[user]\n\tname = \"\"\n\temail = ann@example.com\n", false},
		{"[user]\n\tname = Ann\n\temail = \"\"\n", false},
	}
	for _, c := range cases {
		env := gittest.Isolate(t)
		gittest.WriteGlobalConfig(t, env, c.config, false)
		git, err := gitx.New(gitx.Options{Env: env})
		if err != nil {
			t.Fatal(err)
		}
		values, err := gitx.LoadConfig(t.Context(), git, "")
		if err != nil {
			t.Fatal(err)
		}
		author, ok := authorFrom(values)
		if ok != c.want || (ok && author != (Author{Name: "Ann", Email: "ann@example.com"})) {
			t.Errorf("%q: %+v %v", c.config, author, ok)
		}
	}
}

func TestWriteCommitReportsWhetherItSigned(t *testing.T) {
	env := gittest.Isolate(t)
	program := filepath.Join(t.TempDir(), "fakegpg")
	script := "#!/bin/sh\ncat >/dev/null\nprintf -- '-----BEGIN PGP SIGNATURE-----\\n\\nfake\\n-----END PGP SIGNATURE-----\\n'\n"
	if err := os.WriteFile(program, []byte(script), 0o755); err != nil {
		t.Fatal(err)
	}
	gittest.WriteGlobalConfig(t, env, "[gpg]\n\tprogram = "+program+"\n", true)
	git, err := gitx.New(gitx.Options{Env: env})
	if err != nil {
		t.Fatal(err)
	}
	nb, err := Create(git, paths.Rooted(t.TempDir()), "main")
	if err != nil {
		t.Fatal(err)
	}
	commit := treeCommit{
		tree:    gitOut(t, nb, "rev-parse", "HEAD^{tree}"),
		parents: []string{gitOut(t, nb, "rev-parse", "HEAD")},
		message: "m",
	}
	for _, wantSigned := range []bool{true, false} {
		nb.sign = &wantSigned
		written, err := nb.writeCommit(commit)
		if err != nil {
			t.Fatal(err)
		}
		hasSignature := strings.Contains(gitOut(t, nb, "cat-file", "commit", written.oid), "gpgsig ")
		if written.signed != wantSigned || hasSignature != wantSigned {
			t.Errorf("want signed=%v: got %v, header %v", wantSigned, written.signed, hasSignature)
		}
	}
}

func TestIDsAtAndRootBlobsFailOnAnUnknownCommit(t *testing.T) {
	nb := newTestNotebook(t)
	write(t, nb, "abc10001-alpha.md", plainNote)
	if err := nb.Commit([]string{"abc10001-alpha.md"}, "add"); err != nil {
		t.Fatal(err)
	}
	ids, err := nb.idsAt("HEAD")
	if err != nil || len(ids) != 1 {
		t.Errorf("%v %v", ids, err)
	}
	if _, err := nb.idsAt(strings.Repeat("0", 40)); err == nil {
		t.Error("idsAt accepted an unknown commit")
	}
}

func TestDeletedIgnoresARenameWhoseNewNameIsGoneFromDisk(t *testing.T) {
	nb := newTestNotebook(t)
	write(t, nb, "abc10001-first.md", plainNote)
	if err := nb.Commit([]string{"abc10001-first.md"}, "add"); err != nil {
		t.Fatal(err)
	}
	if err := os.Rename(filepath.Join(nb.Path, "abc10001-first.md"), filepath.Join(nb.Path, "abc10001-second.md")); err != nil {
		t.Fatal(err)
	}
	if err := nb.Commit([]string{"abc10001-first.md", "abc10001-second.md"}, "rename"); err != nil {
		t.Fatal(err)
	}
	// Removed without a commit: the history holds the id in its last commit.
	if err := os.Remove(filepath.Join(nb.Path, "abc10001-second.md")); err != nil {
		t.Fatal(err)
	}
	gone, err := nb.Deleted()
	if err != nil || len(gone) != 0 {
		t.Errorf("%+v %v", gone, err)
	}
}

func TestSnapshotsAreNewestFirstThenByName(t *testing.T) {
	nb := newTestNotebook(t)
	commitAt := func(name string, when int64) {
		t.Helper()
		nb.now = func() time.Time { return time.Unix(when, 0).UTC() }
		write(t, nb, name, plainNote)
		if err := nb.Commit([]string{name}, "add "+name); err != nil {
			t.Fatal(err)
		}
	}
	snapshot := func(name string) {
		t.Helper()
		if _, err := nb.Snapshot(name, name); err != nil {
			t.Fatal(err)
		}
	}
	commitAt("abc10001-one.md", 1_700_000_000)
	snapshot("m")
	snapshot("b")
	commitAt("abc20002-two.md", 1_700_001_000)
	snapshot("z")
	snapshot("a")
	commitAt("abc30003-three.md", 1_700_002_000)
	snapshot("n")
	snapshots, err := nb.Snapshots()
	if err != nil {
		t.Fatal(err)
	}
	var got []string
	for _, found := range snapshots {
		got = append(got, found.Name)
	}
	if want := []string{"n", "a", "z", "b", "m"}; !reflect.DeepEqual(got, want) {
		t.Errorf("got %v", got)
	}
}

func TestSnapshotOfATakenNameIsRefusedByNoda(t *testing.T) {
	nb := newTestNotebook(t)
	if _, err := nb.Snapshot("v1", "first"); err != nil {
		t.Fatal(err)
	}
	// git's own refusal also says "already exists"; noda's names the fix.
	_, err := nb.Snapshot("v1", "again")
	if err == nil || !strings.Contains(err.Error(), "pick another name") {
		t.Errorf("%v", err)
	}
}
