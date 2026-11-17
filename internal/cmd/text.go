package cmd

import (
	"strings"
	"unicode"
)

// trimEnd is Rust's `trim_end`: every Unicode space goes, so a row padded for
// a column that turned out empty leaves no trailing blanks.
func trimEnd(text string) string {
	return strings.TrimRightFunc(text, unicode.IsSpace)
}
