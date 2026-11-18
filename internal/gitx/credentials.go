package gitx

import "github.com/henry40408/noda/internal/remote"

const (
	usernameVariable = "NODA_GIT_USERNAME"
	passwordVariable = "NODA_GIT_PASSWORD"

	// credentialHelper answers `get` with the login in the environment; git runs
	// it through the shell, as it does for any `!` helper.
	credentialHelper = `!f() { test "$1" = get && printf 'username=%s\npassword=%s\n' "$` + usernameVariable + `" "$` + passwordVariable + `"; }; f`
)

// credentialArgs clears every configured helper, then installs the one that
// reads the environment, so no other helper or a prompt gets asked first.
func credentialArgs(credentials *remote.Credentials) []string {
	if credentials == nil {
		return nil
	}
	return []string{"-c", "credential.helper=", "-c", "credential.helper=" + credentialHelper}
}

// withCredentials is extra with the login added, without changing extra.
func withCredentials(extra map[string]string, credentials *remote.Credentials) map[string]string {
	if credentials == nil {
		return extra
	}
	merged := map[string]string{}
	for name, value := range extra {
		merged[name] = value
	}
	merged[usernameVariable] = credentials.Username
	merged[passwordVariable] = credentials.Password
	return merged
}
