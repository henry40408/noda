// Package sign signs a commit with GPG.
//
// noda builds the commit object itself, has gpg sign it and stores it with the
// signature, rather than leaving it to `git commit -S`: git would pass the
// committer's identity to gpg when no `user.signingkey` is set (noda leaves the
// choice to gpg), would wrap gpg's failure in its own message, would sign with
// ssh or x509 when `gpg.format` says so (noda refuses), and would run hooks.
//
// OpenPGP only: `ssh` or `x509` is an error rather than a silently unsigned
// commit, which nothing downstream could tell apart.
package sign

import (
	"bytes"
	"errors"
	"fmt"
	"io"
	"io/fs"
	"os/exec"
	"strings"

	"github.com/henry40408/noda/internal/gitx"
)

// armorHeader is checked for because a `gpg.program` that is not gpg may exit
// 0 too.
const armorHeader = "-----BEGIN PGP SIGNATURE-----"

// Signer is how to reach gpg. A nil Signer means unsigned.
type Signer struct {
	// Program is `gpg.openpgp.program`, `gpg.program`, then `gpg`: git's order.
	Program string
	// Key is `user.signingkey`; empty lets gpg pick its default, as git does.
	Key string
	// Env is what gpg runs under: GNUPGHOME, GPG_TTY and PATH are in it.
	Env []string
	// Stderr is gpg's stderr, passed through and not captured, or a pinentry
	// prompt becomes a silent hang.
	Stderr io.Writer
}

// Resolve decides whether to sign. configured is noda's own `sign` setting,
// which outranks `commit.gpgsign` as `author` outranks `user.name`.
func Resolve(configured *bool, values gitx.ConfigValues) (*Signer, error) {
	wanted := false
	if configured != nil {
		wanted = *configured
	} else if on, ok := values.Bool("commit.gpgsign"); ok {
		wanted = on
	}
	if !wanted {
		return nil, nil
	}

	// Unset means git's default, `openpgp`.
	format, found := values.Get("gpg.format")
	if !found {
		format = "openpgp"
	}
	if format != "openpgp" {
		return nil, fmt.Errorf("signing is on, but `gpg.format` is `%s` — noda signs with OpenPGP only.\n"+
			"Set `gpg.format` to openpgp, or turn signing off with `noda config sign false`", format)
	}

	signer := &Signer{Program: "gpg"}
	if program, ok := values.Get("gpg.openpgp.program"); ok {
		signer.Program = program
	} else if program, ok := values.Get("gpg.program"); ok {
		signer.Program = program
	}
	signer.Key, _ = values.Get("user.signingkey")
	return signer, nil
}

// Sign returns the armored detached signature of content.
func (s *Signer) Sign(content []byte) ([]byte, error) {
	args := []string{"-bsa"} // Detached, sign, armored: what git passes.
	if s.Key != "" {
		args = append(args, "-u", s.Key)
	}
	command := exec.Command(s.Program, args...)
	command.Env = s.Env
	command.Stdin = bytes.NewReader(content)
	var stdout bytes.Buffer
	command.Stdout = &stdout
	command.Stderr = s.Stderr
	if command.Stderr == nil {
		command.Stderr = io.Discard
	}

	if err := command.Run(); err != nil {
		return nil, s.explain(err)
	}
	signature := stdout.Bytes()
	if !strings.HasPrefix(strings.TrimLeft(string(signature), " \t\r\n"), armorHeader) {
		return nil, fmt.Errorf("`%s` returned no OpenPGP signature — is it gpg?", s.Program)
	}
	return signature, nil
}

func (s *Signer) explain(err error) error {
	var exit *exec.ExitError
	if errors.As(err, &exit) {
		return fmt.Errorf("`%s` could not sign the commit — its output is above", s.Program)
	}
	if errors.Is(err, exec.ErrNotFound) || errors.Is(err, fs.ErrNotExist) {
		return fmt.Errorf("signing is on, but `%s` is not installed — install it, point "+
			"`gpg.program` at it, or turn signing off with `noda config sign false`", s.Program)
	}
	return fmt.Errorf("could not run `%s`: %w", s.Program, err)
}
