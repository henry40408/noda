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
	lines := strings.Split(strings.TrimSpace(remote.RedactText(e.Stderr)), "\n")
	last := strings.TrimSpace(lines[len(lines)-1])
	last = strings.TrimPrefix(last, "fatal: ")
	last = strings.TrimPrefix(last, "error: ")
	if last == "" {
		return fmt.Sprintf("git %s exited with code %d", subcommand(e.Args), e.Code)
	}
	return last
}

func subcommand(args []string) string {
	if len(args) == 0 {
		return ""
	}
	return args[0]
}

// IsExit reports whether err is a git run that exited with code, for results a
// caller expects, such as `rev-parse --verify --quiet` finding nothing.
func IsExit(err error, code int) bool {
	var gitErr *Error
	return errors.As(err, &gitErr) && gitErr.Code == code
}
