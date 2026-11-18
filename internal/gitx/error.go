package gitx

import (
	"errors"
	"fmt"
	"strings"

	"github.com/henry40408/noda/internal/remote"
)

// Error is a git run that ended with an exit code its caller did not expect.
type Error struct {
	Args   []string
	Code   int
	Stderr string
}

// Error is the last line git printed, which is where it puts the reason. The
// text is redacted first: git echoes remote URLs, tokens included.
func (e *Error) Error() string {
	reason := reasonLine(remote.RedactText(e.Stderr))
	if reason == "" {
		return fmt.Sprintf("git %s exited with code %d", subcommand(e.Args), e.Code)
	}
	return reason
}

// reasonLine is the last `fatal:` or `error:` line, since git puts its advice
// (`hint:`, indented commands) after the reason. Without one, the last line
// that is not advice.
func reasonLine(stderr string) string {
	var reason, fallback string
	for _, line := range strings.Split(stderr, "\n") {
		text := strings.TrimSpace(line)
		switch {
		case strings.HasPrefix(text, "fatal: "):
			reason = strings.TrimPrefix(text, "fatal: ")
		case strings.HasPrefix(text, "error: "):
			reason = strings.TrimPrefix(text, "error: ")
		case text != "" && !strings.HasPrefix(text, "hint:") && line == text:
			fallback = text
		}
	}
	if reason != "" {
		return reason
	}
	return fallback
}

// subcommand is the git command, past the options (`-c name=value`) before it.
func subcommand(args []string) string {
	if at := subcommandIndex(args); at >= 0 {
		return args[at]
	}
	return ""
}

func subcommandIndex(args []string) int {
	for i := 0; i < len(args); i++ {
		switch {
		case args[i] == "-c" || args[i] == "-C":
			i++
		case !strings.HasPrefix(args[i], "-"):
			return i
		}
	}
	return -1
}

// IsExit reports whether err is a git run that exited with code, for results a
// caller expects, such as `rev-parse --verify --quiet` finding nothing.
func IsExit(err error, code int) bool {
	var gitErr *Error
	return errors.As(err, &gitErr) && gitErr.Code == code
}
