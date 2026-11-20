package gitx_test

import (
	"context"
	"testing"

	"github.com/henry40408/noda/internal/gitx"
	"github.com/henry40408/noda/internal/gitx/gittest"
)

const (
	conflictOpen  = "<<<<<<< what you wrote\n"
	conflictSplit = "=======\n"
	conflictClose = ">>>>>>> saved since\n"
)

// TestMergeFile runs with a user's diff3 setting that must not leak into the
// result: a conflict never carries a base section.
func TestMergeFile(t *testing.T) {
	cases := []struct {
		name               string
		base, ours, theirs string
		wantText           string
		wantClean          bool
	}{
		{name: "nothing changed", base: "a\nb\nc\n", ours: "a\nb\nc\n", theirs: "a\nb\nc\n",
			wantText: "a\nb\nc\n", wantClean: true},
		{name: "only ours changed", base: "a\nb\nc\n", ours: "a\nB\nc\n", theirs: "a\nb\nc\n",
			wantText: "a\nB\nc\n", wantClean: true},
		{name: "only theirs changed", base: "a\nb\nc\n", ours: "a\nb\nc\n", theirs: "a\nb\nC\n",
			wantText: "a\nb\nC\n", wantClean: true},
		{name: "edits far apart both apply", base: "a\nb\nc\n", ours: "A\nb\nc\n", theirs: "a\nb\nC\n",
			wantText: "A\nb\nC\n", wantClean: true},
		{name: "same edit on both sides is not a conflict", base: "a\nb\nc\n", ours: "a\nX\nc\n", theirs: "a\nX\nc\n",
			wantText: "a\nX\nc\n", wantClean: true},
		{name: "different edits to one line conflict", base: "a\nb\nc\n", ours: "a\nX\nc\n", theirs: "a\nY\nc\n",
			wantText: "a\n" + conflictOpen + "X\n" + conflictSplit + "Y\n" + conflictClose + "c\n"},
		{name: "ours deletes a line theirs left alone", base: "a\nb\nc\n", ours: "a\nc\n", theirs: "a\nb\nc\n",
			wantText: "a\nc\n", wantClean: true},
		{name: "ours deletes a line theirs edited", base: "a\nb\nc\n", ours: "a\nc\n", theirs: "a\nB\nc\n",
			wantText: "a\n" + conflictOpen + conflictSplit + "B\n" + conflictClose + "c\n"},
		{name: "theirs deletes a line ours left alone", base: "a\nb\nc\n", ours: "a\nb\nc\n", theirs: "a\nc\n",
			wantText: "a\nc\n", wantClean: true},
		{name: "additions at opposite ends both apply", base: "a\nb\nc\n", ours: "a\nb\nc\nd\n", theirs: "z\na\nb\nc\n",
			wantText: "z\na\nb\nc\nd\n", wantClean: true},
		{name: "different appends conflict", base: "a\nb\nc\n", ours: "a\nb\nc\nd\n", theirs: "a\nb\nc\ne\n",
			wantText: "a\nb\nc\n" + conflictOpen + "d\n" + conflictSplit + "e\n" + conflictClose},
		{name: "different appends to an empty base conflict", base: "", ours: "x\n", theirs: "y\n",
			wantText: conflictOpen + "x\n" + conflictSplit + "y\n" + conflictClose},
		{name: "empty base, only ours wrote", base: "", ours: "x\n", theirs: "",
			wantText: "x\n", wantClean: true},
		{name: "empty base, only theirs wrote", base: "", ours: "", theirs: "y\n",
			wantText: "y\n", wantClean: true},
		{name: "ours drops the final newline", base: "a\n", ours: "a", theirs: "a\n",
			wantText: "a", wantClean: true},
		{name: "no final newline anywhere conflicts on the last line", base: "a\nb", ours: "a\nb\nc", theirs: "A\nb",
			wantText: conflictOpen + "a\nb\nc\n" + conflictSplit + "A\nb\n" + conflictClose},
		{name: "ours adds a trailing blank line", base: "a\nb\n", ours: "a\nb\n\n", theirs: "a\nb\n",
			wantText: "a\nb\n\n", wantClean: true},
		{name: "edits two lines apart both apply", base: "one\ntwo\nthree\nfour\nfive\n", ours: "one\n2\nthree\nfour\nfive\n", theirs: "one\ntwo\nthree\n4\nfive\n",
			wantText: "one\n2\nthree\n4\nfive\n", wantClean: true},
		{name: "edits on adjacent lines conflict", base: "one\ntwo\nthree\nfour\nfive\n", ours: "one\n2\nthree\nfour\nfive\n", theirs: "one\ntwo\n3\nfour\nfive\n",
			wantText: "one\n" + conflictOpen + "2\nthree\n" + conflictSplit + "two\n3\n" + conflictClose + "four\nfive\n"},
		{name: "unicode edits to different lines conflict as one hunk", base: "日本語\n二行目\n", ours: "日本語!\n二行目\n", theirs: "日本語\n二行目!\n",
			wantText: conflictOpen + "日本語!\n二行目\n" + conflictSplit + "日本語\n二行目!\n" + conflictClose},
		{name: "unicode edits to one line conflict", base: "日本語\n二行目\n", ours: "日本語!\n二行目\n", theirs: "日本語?\n二行目\n",
			wantText: conflictOpen + "日本語!\n" + conflictSplit + "日本語?\n" + conflictClose + "二行目\n"},
		{name: "CRLF conflict keeps the line ending on markers", base: "a\r\nb\r\n", ours: "a\r\nB\r\n", theirs: "A\r\nb\r\n",
			wantText: "<<<<<<< what you wrote\r\na\r\nB\r\n=======\r\nA\r\nb\r\n>>>>>>> saved since\r\n"},
		{name: "ours converts CRLF to LF", base: "a\r\nb\r\n", ours: "a\nB\n", theirs: "a\r\nb\r\n",
			wantText: "a\nB\n", wantClean: true},
		{name: "append after shared line conflicts", base: "x\n", ours: "x\nours\n", theirs: "x\ntheirs\n",
			wantText: "x\n" + conflictOpen + "ours\n" + conflictSplit + "theirs\n" + conflictClose},
		{name: "edits seven lines apart in one file conflict as one hunk", base: "a\nb\nc\nd\ne\nf\ng\n", ours: "a\nB\nc\nd\ne\nF\ng\n", theirs: "A\nb\nc\nd\ne\nf\nG\n",
			wantText: conflictOpen + "a\nB\nc\nd\ne\nF\ng\n" + conflictSplit + "A\nb\nc\nd\ne\nf\nG\n" + conflictClose},
		{name: "conflict keeps surrounding lines outside the hunk", base: "a\nb\nc\nd\ne\nf\ng\n", ours: "a\nb\nc\nX\ne\nf\ng\n", theirs: "a\nb\nc\nY\ne\nf\ng\n",
			wantText: "a\nb\nc\n" + conflictOpen + "X\n" + conflictSplit + "Y\n" + conflictClose + "e\nf\ng\n"},
		{name: "duplicated line on ours only", base: "shared\n", ours: "shared\nshared\n", theirs: "shared\n",
			wantText: "shared\nshared\n", wantClean: true},
		{name: "only theirs changed a one-line file", base: "x\n", ours: "x\n", theirs: "y\n",
			wantText: "y\n", wantClean: true},
		{name: "only ours changed a one-line file", base: "x\n", ours: "y\n", theirs: "x\n",
			wantText: "y\n", wantClean: true},
		{name: "heading edit and body edit both apply", base: "# Title\n\ntext\n", ours: "# Mine\n\ntext\n", theirs: "# Title\n\ntext changed\n",
			wantText: "# Mine\n\ntext changed\n", wantClean: true},
	}

	env := gittest.Isolate(t)
	git, err := gitx.New(gitx.Options{Env: env})
	if err != nil {
		t.Fatal(err)
	}
	appendGlobalConfig(t, git, "[merge]\n\tconflictStyle = diff3\n")
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			got, err := gitx.MergeFile(context.Background(), git, gitx.MergeInput{
				Base: c.base, Ours: c.ours, Theirs: c.theirs,
				OursLabel: "what you wrote", TheirsLabel: "saved since",
			})
			if err != nil {
				t.Fatal(err)
			}
			if got.Clean != c.wantClean || got.Text != c.wantText {
				t.Errorf("got clean=%v %q, want clean=%v %q", got.Clean, got.Text, c.wantClean, c.wantText)
			}
		})
	}
}
