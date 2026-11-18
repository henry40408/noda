package sign

import (
	"context"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/henry40408/noda/internal/gitx"
	"github.com/henry40408/noda/internal/gitx/gittest"
)

// configWith is git's configuration with only these `section.name = value`
// lines in it, read the way noda reads it.
func configWith(t *testing.T, entries ...[2]string) gitx.ConfigValues {
	t.Helper()
	env := gittest.Isolate(t)
	var text strings.Builder
	for _, entry := range entries {
		section, name, _ := strings.Cut(entry[0], ".")
		if before, after, nested := strings.Cut(name, "."); nested {
			section, name = section+` "`+before+`"`, after
		}
		text.WriteString("[" + section + "]\n\t" + name + " = " + entry[1] + "\n")
	}
	gittest.WriteGlobalConfig(t, env, text.String(), false)
	git, err := gitx.New(gitx.Options{Env: env})
	if err != nil {
		t.Fatal(err)
	}
	values, err := gitx.LoadConfig(context.Background(), git, "")
	if err != nil {
		t.Fatal(err)
	}
	return values
}

func flag(value bool) *bool { return &value }

func TestNothingConfiguredAnywhereSignsNothing(t *testing.T) {
	signer, err := Resolve(nil, configWith(t))
	if err != nil || signer != nil {
		t.Fatalf("got %v, %v", signer, err)
	}
}

func TestGitAloneIsEnoughToTurnItOn(t *testing.T) {
	signer, err := Resolve(nil, configWith(t, [2]string{"commit.gpgsign", "true"}))
	if err != nil || signer == nil {
		t.Fatalf("got %v, %v", signer, err)
	}
}

func TestNodasOwnSettingOutranksGits(t *testing.T) {
	on := configWith(t, [2]string{"commit.gpgsign", "true"})
	if signer, err := Resolve(flag(false), on); err != nil || signer != nil {
		t.Fatalf("off over on: %v, %v", signer, err)
	}
	off := configWith(t, [2]string{"commit.gpgsign", "false"})
	if signer, err := Resolve(flag(true), off); err != nil || signer == nil {
		t.Fatalf("on over off: %v, %v", signer, err)
	}
}

func TestAFormatNodaCannotSignIsNamedRatherThanIgnored(t *testing.T) {
	values := configWith(t, [2]string{"commit.gpgsign", "true"}, [2]string{"gpg.format", "ssh"})
	_, err := Resolve(nil, values)
	if err == nil || !strings.Contains(err.Error(), "ssh") || !strings.Contains(err.Error(), "OpenPGP only") {
		t.Fatalf("got %v", err)
	}

	// Only checked when it would have signed.
	if signer, err := Resolve(flag(false), values); err != nil || signer != nil {
		t.Fatalf("got %v, %v", signer, err)
	}
}

func TestTheFormatSpecificProgramWinsAndTheKeyIsOptional(t *testing.T) {
	values := configWith(t,
		[2]string{"commit.gpgsign", "true"},
		[2]string{"gpg.program", "generic"},
		[2]string{"gpg.openpgp.program", "specific"},
		[2]string{"user.signingkey", "ABCD1234"},
	)
	signer, err := Resolve(nil, values)
	if err != nil || signer.Program != "specific" || signer.Key != "ABCD1234" {
		t.Fatalf("got %+v, %v", signer, err)
	}

	values = configWith(t, [2]string{"commit.gpgsign", "true"}, [2]string{"gpg.program", "generic"})
	signer, err = Resolve(nil, values)
	if err != nil || signer.Program != "generic" || signer.Key != "" {
		t.Fatalf("got %+v, %v", signer, err)
	}

	signer, err = Resolve(nil, configWith(t, [2]string{"commit.gpgsign", "true"}))
	if err != nil || signer.Program != "gpg" {
		t.Fatalf("got %+v, %v", signer, err)
	}
}

func TestAProgramThatIsNotThereSaysSoByName(t *testing.T) {
	_, err := (&Signer{Program: "noda-no-such-gpg"}).Sign([]byte("tree deadbeef\n"))
	if err == nil || !strings.Contains(err.Error(), "noda-no-such-gpg") || !strings.Contains(err.Error(), "not installed") {
		t.Fatalf("got %v", err)
	}
}

// script is a stand-in for gpg: a shell script printing what it is told to.
func script(t *testing.T, body string) string {
	t.Helper()
	path := filepath.Join(t.TempDir(), "fake-gpg")
	if err := os.WriteFile(path, []byte("#!/bin/sh\n"+body+"\n"), 0o755); err != nil {
		t.Fatal(err)
	}
	return path
}

func TestAProgramThatFailsPointsAtItsOwnOutput(t *testing.T) {
	_, err := (&Signer{Program: script(t, "exit 2")}).Sign([]byte("x"))
	if err == nil || !strings.Contains(err.Error(), "could not sign the commit — its output is above") {
		t.Fatalf("got %v", err)
	}
}

func TestAProgramThatPrintsNoSignatureIsNotGpg(t *testing.T) {
	_, err := (&Signer{Program: script(t, "echo hello")}).Sign([]byte("x"))
	if err == nil || !strings.Contains(err.Error(), "returned no OpenPGP signature — is it gpg?") {
		t.Fatalf("got %v", err)
	}
}

func TestTheSignatureComesBackAndTheKeyIsPassedOn(t *testing.T) {
	program := script(t, `echo "$@" >&2; echo "-----BEGIN PGP SIGNATURE-----"; echo; echo body; echo "-----END PGP SIGNATURE-----"`)
	var stderr strings.Builder
	signature, err := (&Signer{Program: program, Key: "K1", Stderr: &stderr}).Sign([]byte("x"))
	if err != nil {
		t.Fatal(err)
	}
	if !strings.HasPrefix(string(signature), "-----BEGIN PGP SIGNATURE-----\n") {
		t.Fatalf("got %q", signature)
	}
	if strings.TrimSpace(stderr.String()) != "-bsa -u K1" {
		t.Fatalf("gpg was run as %q", stderr.String())
	}
}
