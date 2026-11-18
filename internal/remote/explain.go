package remote

import "strings"

const (
	httpsHint = "noda reads HTTPS credentials from git's credential helper — check `git config credential.helper`, " +
		"or store a token with `git credential approve`"
	sshHint = "noda connects over SSH the way `ssh` does — check `ssh-add -l` and add your key with `ssh-add`"
)

// authenticationMarks are what git and ssh print when a login fails, in lower
// case. Rust matched the last two in libgit2's messages; the others are
// what ssh and git print.
var authenticationMarks = []string{
	"permission denied (publickey",
	"host key verification failed",
	"could not read username",
	"terminal prompts disabled",
	"authentication failed",
	"repository not found",
	"username/password",
	"credential",
}

// reasonPrefixes start the line that says what went wrong; git's own advice
// and its "Could not read from remote repository" trailer come after it.
var reasonPrefixes = []string{"fatal: ", "error: ", "ssh: ", "remote: "}

// Explain turns what git printed on stderr into one message. A failed login
// gets the way out of it; any other failure is the line that states the
// reason. The text is redacted, since git echoes remote URLs. ok is false when
// stderr holds nothing to say, and the caller keeps its own error.
func Explain(stderr, url string) (message string, ok bool) {
	text := RedactText(stderr)
	reason := reasonLine(text)
	if reason == "" {
		return "", false
	}
	if !isAuthentication(text) {
		return reason, true
	}
	hint := sshHint
	if strings.HasPrefix(url, "http") {
		hint = httpsHint
	}
	return Redact(url) + ": " + reason + "\n" + hint, true
}

// reasonLine is the first line that names a failed login (ssh prints it
// without a prefix), else the first line git marks as its failure.
func reasonLine(text string) string {
	lines := strings.Split(text, "\n")
	for _, line := range lines {
		line = strings.TrimSpace(line)
		if isAuthentication(line) {
			return trimReasonPrefix(line)
		}
	}
	for _, line := range lines {
		line = strings.TrimSpace(line)
		for _, prefix := range reasonPrefixes {
			if strings.HasPrefix(line, prefix) {
				return strings.TrimPrefix(line, prefix)
			}
		}
	}
	return ""
}

func trimReasonPrefix(line string) string {
	for _, prefix := range reasonPrefixes {
		line = strings.TrimPrefix(line, prefix)
	}
	return line
}

func isAuthentication(text string) bool {
	lower := strings.ToLower(text)
	for _, mark := range authenticationMarks {
		if strings.Contains(lower, mark) {
			return true
		}
	}
	return false
}

// NameFromURL is the notebook name a clone URL implies: its last path
// segment, minus `.git`. Empty when there is none.
func NameFromURL(url string) string {
	name := strings.TrimRight(url, "/")
	if cut := strings.LastIndexAny(name, "/:"); cut >= 0 {
		name = name[cut+1:]
	}
	for strings.HasSuffix(name, ".git") {
		name = strings.TrimSuffix(name, ".git")
	}
	return name
}
