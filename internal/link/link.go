// Package link finds the files a note's body points at. The body is parsed as
// Markdown, not grepped: a reference-style destination sits at the bottom, a
// link inside a fence is not a link, and `%20` is a space. Each, got wrong,
// reports a referenced file as an orphan. Only local destinations inside the
// notebook come back.
package link

import (
	"sort"
	"strings"
	"unicode/utf8"

	"github.com/yuin/goldmark/ast"

	"github.com/henry40408/noda/internal/markdown"
)

// Targets is every notebook-relative path the body links to or embeds,
// normalised to compare against a directory listing, sorted and unique.
func Targets(body string) []string {
	found := map[string]struct{}{}
	markdown.Plain(body).Walk(func(node ast.Node) {
		destination, ok := destinationOf(node)
		if !ok {
			return
		}
		if path, ok := Target(destination); ok {
			found[path] = struct{}{}
		}
	})
	paths := make([]string, 0, len(found))
	for path := range found {
		paths = append(paths, path)
	}
	sort.Strings(paths)
	return paths
}

func destinationOf(node ast.Node) (string, bool) {
	switch n := node.(type) {
	case *ast.Link:
		return markdown.Unescape(n.Destination), true
	case *ast.Image:
		return markdown.Unescape(n.Destination), true
	}
	return "", false
}

// Target is the notebook-relative file a destination names. Also what `web`
// uses to decide which file it will serve, so there is one "inside the
// notebook" check.
func Target(destination string) (string, bool) {
	if destination == "" || destination[0] == '#' || destination[0] == '/' {
		return "", false
	}
	if _, ok := Scheme(destination); ok {
		return "", false
	}
	// `diagram.png#x` and `diagram.png?x` both name `diagram.png`: neither
	// character survives in a filename synced to Windows.
	path, _, _ := strings.Cut(destination, "#")
	path, _, _ = strings.Cut(path, "?")
	return normalize(percentDecode(path))
}

// Scheme is the URL scheme a destination carries (not matched as `://`:
// `mailto:` has none). The name matters to `web`, which follows `https:` but
// must never run `javascript:`.
func Scheme(destination string) (string, bool) {
	if destination == "" || !isASCIILetter(destination[0]) {
		return "", false
	}
	for index := 1; index < len(destination); index++ {
		switch ch := destination[index]; {
		case ch == ':':
			return destination[:index], true
		case isASCIILetter(ch), ch >= '0' && ch <= '9', ch == '+', ch == '-', ch == '.':
		default:
			return "", false
		}
	}
	return "", false
}

func isASCIILetter(ch byte) bool {
	return ch >= 'a' && ch <= 'z' || ch >= 'A' && ch <= 'Z'
}

func percentDecode(text string) string {
	out := make([]byte, 0, len(text))
	for index := 0; index < len(text); index++ {
		if text[index] == '%' && index+2 < len(text) {
			high, okHigh := hexValue(text[index+1])
			low, okLow := hexValue(text[index+2])
			if okHigh && okLow {
				out = append(out, high*16+low)
				index += 2
				continue
			}
		}
		out = append(out, text[index])
	}
	if !utf8.Valid(out) {
		return text
	}
	return string(out)
}

func hexValue(b byte) (byte, bool) {
	switch {
	case b >= '0' && b <= '9':
		return b - '0', true
	case b >= 'a' && b <= 'f':
		return b - 'a' + 10, true
	case b >= 'A' && b <= 'F':
		return b - 'A' + 10, true
	}
	return 0, false
}

// normalize collapses `.` and `..`; false for a path that climbs out of the
// notebook.
func normalize(path string) (string, bool) {
	var parts []string
	for _, part := range strings.Split(path, "/") {
		switch part {
		case "", ".":
		case "..":
			if len(parts) == 0 {
				return "", false
			}
			parts = parts[:len(parts)-1]
		default:
			parts = append(parts, part)
		}
	}
	if len(parts) == 0 {
		return "", false
	}
	return strings.Join(parts, "/"), true
}
