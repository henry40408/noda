package web

import (
	"strings"
	"unicode/utf8"
)

// parameter is the first value of a name in a query string or a form body, or
// empty. By hand rather than net/url, whose parser drops a pair for one bad
// escape and treats `;` as a separator: a stray `%` stays as typed here.
func parameter(query, name string) string {
	for _, pair := range strings.Split(query, "&") {
		key, value, _ := strings.Cut(pair, "=")
		if decode(key) == name {
			return decode(value)
		}
	}
	return ""
}

// parameters is every value of a repeated name, as a form sends one per ticked
// box.
func parameters(body, name string) []string {
	var values []string
	for _, pair := range strings.Split(body, "&") {
		key, value, _ := strings.Cut(pair, "=")
		if decode(key) == name {
			values = append(values, decode(value))
		}
	}
	return values
}

// decode works on bytes and converts once, so a character spread over several
// `%xx` escapes is not cut apart.
func decode(value string) string {
	out := make([]byte, 0, len(value))
	for at := 0; at < len(value); {
		switch {
		// A form sends a space this way.
		case value[at] == '+':
			out = append(out, ' ')
			at++
		case value[at] == '%' && at+2 < len(value) && isHex(value[at+1]) && isHex(value[at+2]):
			out = append(out, hexValue(value[at+1])<<4|hexValue(value[at+2]))
			at += 3
		default:
			out = append(out, value[at])
			at++
		}
	}
	if utf8.Valid(out) {
		return string(out)
	}
	return strings.ToValidUTF8(string(out), "�")
}

func isHex(c byte) bool {
	return c >= '0' && c <= '9' || c >= 'a' && c <= 'f' || c >= 'A' && c <= 'F'
}

func hexValue(c byte) byte {
	switch {
	case c >= 'a':
		return c - 'a' + 10
	case c >= 'A':
		return c - 'A' + 10
	}
	return c - '0'
}
