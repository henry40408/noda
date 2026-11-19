package gitx

import (
	"os/exec"
	"slices"
	"testing"
	"time"

	"github.com/henry40408/noda/internal/remote"
)

func TestRawDateOfUTCIsPlusZero(t *testing.T) {
	when := time.Unix(1700000000, 0).In(time.UTC)
	if got := (Ident{When: when}).RawDate(); got != "1700000000 +0000" {
		t.Error(got)
	}
}

func TestWithSignatureInsertsTheHeaderAfterTheOthers(t *testing.T) {
	got := string(WithSignature([]byte("tree x\n\nmsg\n"), []byte("SIG\nLINE\n")))
	if want := "tree x\ngpgsig SIG\n LINE\n \n\nmsg\n"; got != want {
		t.Errorf("got %q want %q", got, want)
	}
}

func TestWithSignatureHandlesAnEmptyHeaderBlockAndNoBlankLine(t *testing.T) {
	got := string(WithSignature([]byte("\n\nmsg"), []byte("S\n")))
	if want := "\ngpgsig S\n \n\nmsg"; got != want {
		t.Errorf("got %q want %q", got, want)
	}
	if got := string(WithSignature([]byte("tree x\n"), []byte("S\n"))); got != "tree x\n" {
		t.Errorf("no blank line must leave the object alone: %q", got)
	}
}

func TestConfigValuesFindTheFirstEntry(t *testing.T) {
	values := ConfigValues{entries: []configEntry{{key: "user.name", value: "Only"}, {key: "core.flag", value: "true"}}}
	if got, ok := values.Get("user.name"); !ok || got != "Only" {
		t.Errorf("Get: %q %v", got, ok)
	}
	single := ConfigValues{entries: []configEntry{{key: "core.flag", value: "true"}}}
	if on, ok := single.Bool("core.flag"); !on || !ok {
		t.Errorf("Bool: %v %v", on, ok)
	}
	if _, ok := single.Get("core.other"); ok {
		t.Error("an unset key was found")
	}
	if _, ok := single.Bool("core.other"); ok {
		t.Error("an unset key was found")
	}
}

func TestCanonicalKeyFoldsSectionAndNameButNotSubsection(t *testing.T) {
	for key, want := range map[string]string{
		"NoDots":            "nodots",
		"A.B":               "a.b",
		"Remote.Origin.URL": "remote.Origin.url",
		".A.B":              ".A.b",
	} {
		if got := canonicalKey(key); got != want {
			t.Errorf("%q: got %q want %q", key, got, want)
		}
	}
}

func TestCredentialArgsAreEmptyWithoutALogin(t *testing.T) {
	if got := credentialArgs(nil); got != nil {
		t.Errorf("got %v", got)
	}
	got := credentialArgs(&remote.Credentials{Username: "u", Password: "p"})
	if len(got) != 4 || got[1] != "credential.helper=" || got[3] != "credential.helper="+credentialHelper {
		t.Errorf("got %v", got)
	}
}

func TestSubcommandSkipsOptionsAndToleratesNone(t *testing.T) {
	cases := []struct {
		args []string
		want string
	}{
		{nil, ""},
		{[]string{"-c", "a=b"}, ""},
		{[]string{"-C", "dir", "--no-pager"}, ""},
		{[]string{"-c", "a=b", "log", "-1"}, "log"},
		{[]string{"diff"}, "diff"},
	}
	for _, c := range cases {
		if got := subcommand(c.args); got != c.want {
			t.Errorf("%v: got %q want %q", c.args, got, c.want)
		}
	}
}

func TestStopGentlyWaitsFiveSeconds(t *testing.T) {
	command := exec.Command("true")
	stopGently(command)
	if command.WaitDelay != 5*time.Second || command.Cancel == nil {
		t.Errorf("%v", command.WaitDelay)
	}
}

func TestWithoutExternalDiffTouchesOnlyPatchCommands(t *testing.T) {
	cases := []struct {
		args, want []string
	}{
		{[]string{"diff", "x"}, []string{"diff", "--no-ext-diff", "--no-textconv", "x"}},
		{[]string{"-c", "a=b", "log"}, []string{"-c", "a=b", "log", "--no-ext-diff", "--no-textconv"}},
		{[]string{"status"}, []string{"status"}},
		{[]string{"-c", "a=b"}, []string{"-c", "a=b"}},
		{nil, nil},
	}
	for _, c := range cases {
		if got := withoutExternalDiff(c.args); !slices.Equal(got, c.want) {
			t.Errorf("%v: got %v want %v", c.args, got, c.want)
		}
	}
}

func TestParseRawDateChecksTheSignAndBothNumbers(t *testing.T) {
	for text, want := range map[string]int{"5 +0130": 90, "5 -0130": -90, "5 +0000": 0} {
		seconds, offset, err := ParseRawDate(text)
		if err != nil || seconds != 5 || offset != want {
			t.Errorf("%q: %d %d %v", text, seconds, offset, err)
		}
	}
	for _, bad := range []string{"5 *0130", "5 x0130", "5 +xx30", "5 +01xx", "x +0130"} {
		if _, _, err := ParseRawDate(bad); err == nil {
			t.Errorf("%q accepted", bad)
		}
	}
}
