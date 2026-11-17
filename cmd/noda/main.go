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
	catchBrokenPipe()
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

// catchBrokenPipe makes a write to a closed stdout (`noda ls | head -1`)
// return EPIPE, which run treats as a normal end, instead of killing the
// process with the signal. It is Notify and not Ignore because an ignored
// signal is inherited by git and $EDITOR, whose `yes | head -1` would then
// fail with "Broken pipe" instead of ending quietly; a caught one is reset to
// the default on exec.
func catchBrokenPipe() {
	signal.Notify(make(chan os.Signal, 1), syscall.SIGPIPE)
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
