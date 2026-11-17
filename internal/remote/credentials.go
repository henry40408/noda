package remote

import (
	"net/url"
	"strings"
)

// Credentials are the userinfo of an HTTP(S) remote URL.
type Credentials struct {
	Username, Password string
}

// SplitCredentials takes the userinfo out of an HTTP(S) URL. git would put the
// whole URL on the command line of its helper process, where any local user can
// read it; the caller hands the credentials over by environment instead. A URL
// with no userinfo, or over another scheme, comes back unchanged with false.
func SplitCredentials(raw string) (clean string, credentials Credentials, found bool) {
	scheme, rest, ok := strings.Cut(raw, "://")
	if !ok || (scheme != "http" && scheme != "https") {
		return raw, Credentials{}, false
	}
	authority := rest
	if end := strings.IndexAny(rest, "/?#"); end >= 0 {
		authority = rest[:end]
	}
	at := strings.LastIndexByte(authority, '@')
	if at < 0 {
		return raw, Credentials{}, false
	}
	username, password, _ := strings.Cut(authority[:at], ":")
	return scheme + "://" + rest[at+1:], Credentials{Username: unescape(username), Password: unescape(password)}, true
}

// unescape reads percent-encoding as git does, and leaves text that is not
// valid encoding as it is.
func unescape(text string) string {
	decoded, err := url.PathUnescape(text)
	if err != nil {
		return text
	}
	return decoded
}
