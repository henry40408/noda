// Package remote deals with the URLs of a notebook's remote: keeping a secret
// out of what is shown, and explaining what git says when a remote fails.
package remote

import (
	"regexp"
	"strings"
)

// Redact hides the secret in a URL's userinfo, so a token in a remote never
// reaches the screen or a log.
//
// The whole userinfo goes, since Gitea and Forgejo take the token as the
// username.
func Redact(url string) string {
	// scp syntax: `git@` hides nothing.
	scheme, rest, found := strings.Cut(url, "://")
	if !found {
		return url
	}
	authority := rest
	if end := strings.IndexAny(rest, "/?#"); end >= 0 {
		authority = rest[:end]
	}
	// The last `@` within the authority: a password may hold one, and a path
	// may too.
	at := strings.LastIndexByte(authority, '@')
	if at < 0 {
		return url
	}
	// Over SSH a bare username is no secret; over HTTP(S) it may be a token.
	secret := strings.Contains(authority[:at], ":") || scheme == "http" || scheme == "https"
	if !secret {
		return url
	}
	return scheme + "://***@" + rest[at+1:]
}

// urlInText finds a URL inside prose such as git's stderr, where it sits in
// quotes: `unable to access 'https://token@host/repo/'`.
var urlInText = regexp.MustCompile(`[A-Za-z][A-Za-z0-9+.-]*://[^\s'"<>]+`)

// RedactText applies Redact to every URL in text. Every git stderr passes
// through it before it is shown or logged.
func RedactText(text string) string {
	return urlInText.ReplaceAllStringFunc(text, Redact)
}
