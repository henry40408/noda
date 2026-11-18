package main

import (
	"bufio"
	"errors"
	"fmt"
	"io"
	"strings"

	"github.com/henry40408/noda/internal/cmd"
	"github.com/henry40408/noda/internal/gitx"
	"github.com/henry40408/noda/internal/paths"
	"github.com/henry40408/noda/internal/style"
	"github.com/henry40408/noda/internal/version"
)

const (
	exitFailure = 1
	exitUsage   = 2
)

// process is everything run needs from the outside world. The environment is
// read once, in main, and travels from here.
type process struct {
	args   []string
	env    map[string]string
	stdin  io.Reader
	stdout io.Writer
	stderr io.Writer
	// The terminal facts decide colour and whether git may ask questions.
	stdinIsTerminal  bool
	stdoutIsTerminal bool
	stderrIsTerminal bool
}

func (p process) lookupEnv(name string) (string, bool) {
	value, ok := p.env[name]
	return value, ok
}

// run returns the exit code.
func run(p process) int {
	// First, before the arguments: a retired variable stops every command,
	// `--version` and `--help` included, however the line looks.
	if err := ensureNoLegacyEnvVars(p.lookupEnv); err != nil {
		return p.fail(err)
	}
	err := p.execute()
	if err == nil || isBrokenPipe(err) {
		return 0
	}
	return p.fail(err)
}

func (p process) execute() error {
	root := buildSpec()
	line, err := parseCommandLine(root, p.args, p.lookupEnv)
	var request helpRequest
	if errors.As(err, &request) {
		return p.printRequest(request)
	}
	if err != nil {
		return err
	}
	return p.dispatch(line)
}

func (p process) printRequest(request helpRequest) error {
	if request.version {
		return p.print("noda " + version.String())
	}
	return p.print(request.text)
}

func (p process) print(text string) error {
	return printOutput(p.stdout, text, style.Wanted(p.stdoutIsTerminal, p.lookupEnv))
}

// fail reports an error and picks the exit code: a command line that does not
// parse is 2, everything else 1.
func (p process) fail(err error) int {
	text := err.Error()
	var usage usageError
	if errors.As(err, &usage) {
		fmt.Fprintln(p.stderr, text)
		return exitUsage
	}
	// An error may quote coloured output, and a pipe must not receive escapes.
	if !style.Wanted(p.stderrIsTerminal, p.lookupEnv) {
		text = style.Strip(text)
	}
	fmt.Fprintf(p.stderr, "noda: %s\n", text)
	return exitFailure
}

// environment builds what the commands run in. git is looked for only here, so
// `--version` and `--help` work without it.
func (p process) environment() (cmd.Env, error) {
	git, err := gitx.New(gitx.Options{Env: p.env, Stderr: p.stderr})
	if err != nil {
		return cmd.Env{}, err
	}
	where, err := paths.New(p.lookupEnv)
	if err != nil {
		return cmd.Env{}, err
	}
	env := cmd.Env{Paths: where, Git: git, Interactive: p.stdinIsTerminal}
	if p.stdinIsTerminal {
		env.Confirm = p.confirm
	}
	return env, nil
}

// confirm asks on stderr, so stdout carries only the outcome.
func (p process) confirm(question string) (bool, error) {
	if _, err := io.WriteString(p.stderr, question); err != nil {
		return false, err
	}
	answer, err := bufio.NewReader(p.stdin).ReadString('\n')
	if err != nil && !errors.Is(err, io.EOF) {
		return false, err
	}
	switch strings.ToLower(strings.TrimSpace(answer)) {
	case "y", "yes":
		return true, nil
	}
	return false, nil
}
