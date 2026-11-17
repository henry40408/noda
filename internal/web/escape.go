package web

import "strings"

// escape is hand-written: five characters, so the one place escaping happens
// reads in one screen.
func escape(text string) string {
	var out strings.Builder
	out.Grow(len(text))
	for _, c := range text {
		switch c {
		case '&':
			out.WriteString("&amp;")
		case '<':
			out.WriteString("&lt;")
		case '>':
			out.WriteString("&gt;")
		case '"':
			out.WriteString("&quot;")
		case '\'':
			out.WriteString("&#39;")
		default:
			out.WriteRune(c)
		}
	}
	return out.String()
}

// escapeText is the body-text escape of the Markdown renderer: quotes stay.
func escapeText(text string) string {
	if !strings.ContainsAny(text, "&<>") {
		return text
	}
	return strings.NewReplacer("&", "&amp;", "<", "&lt;", ">", "&gt;").Replace(text)
}

// escapeAttribute is escapeText plus the double quote, for alt, title and
// class values.
func escapeAttribute(text string) string {
	return strings.NewReplacer("&", "&amp;", "<", "&lt;", ">", "&gt;", "\"", "&quot;").Replace(text)
}

// escapeHref percent-encodes what a URL cannot carry and escapes what an
// attribute cannot, as the Markdown renderer does for a destination.
func escapeHref(url string) string {
	var out strings.Builder
	for i := 0; i < len(url); i++ {
		b := url[i]
		switch {
		case b == '&':
			out.WriteString("&amp;")
		case b == '\'':
			out.WriteString("&#x27;")
		case b == '"':
			out.WriteString("&quot;")
		case isHrefSafe(b):
			out.WriteByte(b)
		default:
			out.WriteString("%" + strings.ToUpper(string("0123456789abcdef"[b>>4])+string("0123456789abcdef"[b&15])))
		}
	}
	return out.String()
}

func isHrefSafe(b byte) bool {
	switch {
	case b >= 'a' && b <= 'z', b >= 'A' && b <= 'Z', b >= '0' && b <= '9':
		return true
	}
	return strings.IndexByte("!#$%()*+,-./:;=?@_~", b) >= 0
}

// encoded percent-encodes everything outside the unreserved set, which is right
// both for `filename*` (`réunion.pdf`) and a query string (`tag:"24.04 Dark
// patterns"`).
func encoded(name string) string {
	var out strings.Builder
	for i := 0; i < len(name); i++ {
		b := name[i]
		if isUnreserved(b) {
			out.WriteByte(b)
			continue
		}
		out.WriteString("%" + strings.ToUpper(string("0123456789abcdef"[b>>4])+string("0123456789abcdef"[b&15])))
	}
	return out.String()
}

func isUnreserved(b byte) bool {
	switch {
	case b >= 'a' && b <= 'z', b >= 'A' && b <= 'Z', b >= '0' && b <= '9':
		return true
	}
	return strings.IndexByte("-_.~", b) >= 0
}
