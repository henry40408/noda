package note

import "strings"

// lines splits text at `\n` or `\r\n`, dropping the line ending and the empty
// piece after a final newline. A bare `\r` stays in the line.
func lines(text string) []string {
	if text == "" {
		return nil
	}
	parts := strings.Split(text, "\n")
	last := len(parts) - 1
	if parts[last] == "" {
		parts = parts[:last]
		last--
	}
	for i := range parts {
		// The final piece has no line ending when the text lacks a newline.
		if i < last || strings.HasSuffix(text, "\n") {
			parts[i] = strings.TrimSuffix(parts[i], "\r")
		}
	}
	return parts
}
