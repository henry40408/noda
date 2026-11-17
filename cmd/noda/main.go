// Command noda is a git-native notebook.
package main

import (
	"os"
	"os/signal"
	"strings"
	"syscall"

	"golang.org/x/term"
)

func main() {
	// Without this, a write to a closed stdout (`noda ls | head -1`) kills the
	// process with the signal instead of returning EPIPE, which run treats as
	// a normal end.
	signal.Ignore(syscall.SIGPIPE)
	os.Exit(run(process{
		args:             os.Args[1:],
		env:              environment(os.Environ()),
		stdin:            os.Stdin,
		stdout:           os.Stdout,
		stderr:           os.Stderr,
		stdinIsTerminal:  term.IsTerminal(int(os.Stdin.Fd())),
		stdoutIsTerminal: term.IsTerminal(int(os.Stdout.Fd())),
		stderrIsTerminal: term.IsTerminal(int(os.Stderr.Fd())),
	}))
}

// environment turns os.Environ's KEY=VALUE lines into a map. It is the one
// place the process environment is read.
func environment(lines []string) map[string]string {
	env := make(map[string]string, len(lines))
	for _, line := range lines {
		name, value, found := strings.Cut(line, "=")
		if found {
			env[name] = value
		}
	}
	return env
}
