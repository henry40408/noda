// Package gitx runs the system `git`. noda reads and writes notebooks through
// the git command line, so every run goes through one place that fixes the
// environment and the settings its output depends on.
package gitx

import (
	"bytes"
	"context"
	"errors"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"regexp"
	"slices"
	"sort"
	"strconv"
	"syscall"
	"time"

	"github.com/henry40408/noda/internal/remote"
)

// Version is a git release.
type Version struct{ Major, Minor, Patch int }

// MinVersion is the oldest git noda supports: `git config --includes` and the
// `-z` formats it parses are all there.
var MinVersion = Version{2, 39, 0}

func (v Version) String() string {
	return fmt.Sprintf("%d.%d.%d", v.Major, v.Minor, v.Patch)
}

// Less orders versions.
func (v Version) Less(other Version) bool {
	if v.Major != other.Major {
		return v.Major < other.Major
	}
	if v.Minor != other.Minor {
		return v.Minor < other.Minor
	}
	return v.Patch < other.Patch
}

var versionPattern = regexp.MustCompile(`^git version (\d+)\.(\d+)(?:\.(\d+))?`)

// parseVersion reads `git version 2.39.5`, with or without a vendor suffix
// (`2.39.5 (Apple Git-154)`, `2.47.1.windows.1`, `2.50.0.rc1`).
func parseVersion(output string) (Version, error) {
	match := versionPattern.FindStringSubmatch(output)
	if match == nil {
		return Version{}, fmt.Errorf("cannot read a version from %q", output)
	}
	var numbers [3]int
	for i := range numbers {
		if match[i+1] != "" {
			numbers[i], _ = strconv.Atoi(match[i+1])
		}
	}
	return Version{numbers[0], numbers[1], numbers[2]}, nil
}

// Options configure New.
type Options struct {
	// Env is noda's environment. gitx never reads the process's own, so the
	// caller decides what git sees.
	Env map[string]string
	// Stderr receives git's stderr for a Cmd that asks for PassStderr (gpg's
	// pinentry prompts and ssh's passphrase prompts come out there).
	Stderr io.Writer
}

// Git runs one git executable.
type Git struct {
	path    string
	env     map[string]string
	stderr  io.Writer
	Version Version
}

// New finds git on the PATH of opts.Env and checks it is new enough.
func New(opts Options) (*Git, error) {
	path, err := lookPath(opts.Env["PATH"])
	if err != nil {
		return nil, fmt.Errorf("noda needs git %s or later on PATH: %w", shortVersion(MinVersion), err)
	}
	git := &Git{path: path, env: opts.Env, stderr: opts.Stderr}
	if git.stderr == nil {
		git.stderr = io.Discard
	}
	output, err := git.versionOutput()
	if err != nil {
		return nil, fmt.Errorf("noda needs git %s or later on PATH: %w", shortVersion(MinVersion), err)
	}
	git.Version, err = parseVersion(output)
	if err != nil {
		return nil, fmt.Errorf("noda needs git %s or later on PATH: %w", shortVersion(MinVersion), err)
	}
	if git.Version.Less(MinVersion) {
		return nil, fmt.Errorf("noda needs git %s or later, found %s", shortVersion(MinVersion), git.Version)
	}
	return git, nil
}

func shortVersion(v Version) string {
	return fmt.Sprintf("%d.%d", v.Major, v.Minor)
}

func lookPath(pathVariable string) (string, error) {
	for _, dir := range filepath.SplitList(pathVariable) {
		// A relative entry would run a `git` out of whatever directory noda is
		// in, which may be an untrusted clone.
		if !filepath.IsAbs(dir) {
			continue
		}
		candidate := filepath.Join(dir, "git")
		info, err := os.Stat(candidate)
		if err == nil && info.Mode().IsRegular() && info.Mode().Perm()&0o111 != 0 {
			return candidate, nil
		}
	}
	return "", errors.New("git was not found")
}

func (g *Git) versionOutput() (string, error) {
	command := exec.Command(g.path, "version")
	command.Env = sanitizedEnv(g.env, true, nil)
	output, err := command.Output()
	if err != nil {
		return "", fmt.Errorf("running `git version`: %w", err)
	}
	return string(output), nil
}

// Getenv is a variable of the environment git was given.
func (g *Git) Getenv(name string) (string, bool) {
	value, found := g.env[name]
	return value, found
}

// Environ is that environment as `NAME=value` pairs, for the programs noda
// starts itself (gpg) so they see what git sees.
func (g *Git) Environ() []string {
	pairs := make([]string, 0, len(g.env))
	for name, value := range g.env {
		pairs = append(pairs, name+"="+value)
	}
	sort.Strings(pairs)
	return pairs
}

// Stderr is where a Cmd with PassStderr, and anything else needing the
// terminal, writes.
func (g *Git) Stderr() io.Writer {
	return g.stderr
}

// Cmd is one git run.
type Cmd struct {
	// Dir is the repository (or any directory) git runs in.
	Dir  string
	Args []string
	// Stdin is fed to git; nil gives it nothing.
	Stdin []byte
	// ExtraEnv is set last, over everything else.
	ExtraEnv map[string]string
	// PassStderr copies git's stderr to Options.Stderr as well as capturing it,
	// once the run has ended.
	PassStderr bool
	// ReadOnly keeps the run from taking optional locks.
	ReadOnly bool
	// OKCodes are the exit codes that are not an error. Empty means only 0.
	OKCodes []int
	// Network marks a run that talks to a remote. Unless Interactive is set,
	// ssh is kept from opening /dev/tty, where a web errand would hang and a
	// full-screen interface would be drawn over.
	Network bool
	// Interactive says a person at a terminal can answer ssh's prompts.
	Interactive bool

	// showHooksPath leaves core.hooksPath as configured, for Hooks.
	showHooksPath bool
}

// Result is what a finished run left behind.
type Result struct {
	Stdout, Stderr []byte
	Code           int
}

// Run executes git. An exit code outside OKCodes comes back as a *Error along
// with the Result.
func (g *Git) Run(ctx context.Context, c Cmd) (Result, error) {
	if c.Dir == "" {
		// `-C ""` is noda's own working directory, which may hold any repository.
		return Result{}, errors.New("running git without a directory")
	}
	args := globalArgs(c.Dir, c.showHooksPath)
	if c.Network && !c.Interactive && !g.hasOwnSSH(ctx, c.Dir) {
		args = append(args, "-c", "core.sshCommand=ssh -o BatchMode=yes")
	}
	args = append(args, withoutExternalDiff(c.Args)...)
	command := exec.CommandContext(ctx, g.path, args...)
	command.Env = g.environment(c.Dir, c.ReadOnly, c.ExtraEnv)
	stopGently(command)
	if c.Stdin != nil {
		command.Stdin = bytes.NewReader(c.Stdin)
	}
	var stdout, stderr bytes.Buffer
	command.Stdout = &stdout
	command.Stderr = &stderr

	result := Result{}
	err := command.Run()
	// Redacted once, here: nothing downstream (an Error, a log line, `%#v`) can
	// then leak a URL's secret, and what is passed through is redacted too.
	result.Stdout, result.Stderr = stdout.Bytes(), []byte(remote.RedactText(stderr.String()))
	if c.PassStderr && g.stderr != nil {
		_, _ = g.stderr.Write(result.Stderr)
	}
	var exit *exec.ExitError
	switch {
	case err == nil:
	case errors.As(err, &exit):
		result.Code = exit.ExitCode()
	default:
		return result, fmt.Errorf("running git %s: %w", subcommand(c.Args), err)
	}
	if ctx.Err() != nil {
		return result, fmt.Errorf("git %s: %w", subcommand(c.Args), ctx.Err())
	}
	if !slices.Contains(okCodes(c), result.Code) {
		return result, &Error{Args: c.Args, Code: result.Code, Stderr: string(result.Stderr)}
	}
	return result, nil
}

// environment is sanitizedEnv with discovery stopped at dir's parent: a
// notebook whose `.git` is damaged must not be mistaken for the repository
// above it (a home directory kept in git, say).
func (g *Git) environment(dir string, readOnly bool, extra map[string]string) []string {
	merged := map[string]string{"GIT_CEILING_DIRECTORIES": filepath.Dir(filepath.Clean(dir))}
	for name, value := range extra {
		merged[name] = value
	}
	return sanitizedEnv(g.env, readOnly, merged)
}

// stopGently makes cancellation send SIGTERM, which git answers by removing
// its lock files, and gives a child of git (ssh, a credential helper) a moment
// to close its pipes before Go stops waiting.
func stopGently(command *exec.Cmd) {
	command.Cancel = func() error { return command.Process.Signal(syscall.SIGTERM) }
	command.WaitDelay = 5 * time.Second
}

// withoutExternalDiff keeps a user's diff driver and textconv filters out of
// anything that renders a patch. Commands that never print one ignore the
// options.
func withoutExternalDiff(args []string) []string {
	at := subcommandIndex(args)
	if at < 0 {
		return args
	}
	switch args[at] {
	case "diff", "log", "show":
		out := append([]string{}, args[:at+1]...)
		out = append(out, "--no-ext-diff", "--no-textconv")
		return append(out, args[at+1:]...)
	}
	return args
}

// hasOwnSSH reports whether the user chose the ssh command, which then stays
// as they set it. GIT_SSH_COMMAND outranks core.sshCommand, which outranks
// GIT_SSH.
func (g *Git) hasOwnSSH(ctx context.Context, dir string) bool {
	for _, name := range []string{"GIT_SSH_COMMAND", "GIT_SSH"} {
		if _, set := g.env[name]; set {
			return true
		}
	}
	result, err := g.Run(ctx, Cmd{Dir: dir, Args: []string{"config", "--includes", "--get", "core.sshCommand"}, ReadOnly: true, OKCodes: []int{0, 1, 128}})
	return err == nil && result.Code == 0
}

func okCodes(c Cmd) []int {
	if len(c.OKCodes) == 0 {
		return []int{0}
	}
	return c.OKCodes
}

// globalArgs are the options before the subcommand: no pager, the fixed
// settings, and the directory.
//
// Hooks are switched off because libgit2 ran none: `update-ref`, `reset` and
// `fetch` would otherwise fire `reference-transaction`, and `clone` would run
// `post-checkout`. Only the run that asks where hooks live sees the real path.
func globalArgs(dir string, showHooksPath bool) []string {
	args := []string{"--no-pager"}
	for _, setting := range fixedConfig {
		args = append(args, "-c", setting)
	}
	if !showHooksPath {
		args = append(args, "-c", "core.hooksPath=/dev/null")
	}
	return append(args, "-C", dir)
}
