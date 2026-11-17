package note

import (
	"strings"
	"unicode"
	"unicode/utf8"
)

const fallbackSlug = "note"

// MaxFileNameLen is a directory entry's limit in bytes on ext4, APFS and HFS+.
const MaxFileNameLen = 255

// maxSlugLen is in bytes. FileName leaves 243 of MaxFileNameLen for the slug;
// this is well under it because `ls -l` pads the slug column to the widest one.
// The title in the frontmatter keeps the whole text.
const maxSlugLen = 100

// FileName is `<id>-<slug>.md`.
func FileName(id, slug string) string {
	return id + "-" + slug + ".md"
}

// Slugify keeps alphanumerics (so CJK stays legible), collapses everything else
// to one `-`, and cuts to maxSlugLen bytes.
func Slugify(title string) string {
	var slug strings.Builder
	pendingDash := false
	for _, ch := range title {
		if !isAlphanumeric(ch) {
			pendingDash = true
			continue
		}
		if pendingDash && slug.Len() > 0 {
			slug.WriteByte('-')
		}
		pendingDash = false
		for _, lower := range lowerCase(ch) {
			slug.WriteRune(lower)
		}
	}
	if slug.Len() == 0 {
		return fallbackSlug
	}
	return shorten(slug.String())
}

// isAlphanumeric is Unicode's Alphabetic or Numeric property. The standard
// library's letter class leaves out Other_Alphabetic (vowel signs and the like),
// which would split a word in two.
func isAlphanumeric(ch rune) bool {
	return unicode.IsLetter(ch) || unicode.IsNumber(ch) || unicode.Is(unicode.Other_Alphabetic, ch)
}

// lowerCase is a character's full lowercase mapping. U+0130 is the one
// character whose mapping is longer than one character, and the one the
// standard library's single-character mapping gets differently.
func lowerCase(ch rune) []rune {
	if ch == 'İ' {
		return []rune{'i', '̇'}
	}
	return []rune{unicode.ToLower(ch)}
}

// Lower is the full lowercase mapping of a string, for case-insensitive
// comparison. It leaves out the final-sigma rule (`Σ` at a word's end), which
// both sides of a comparison would have to agree on.
func Lower(text string) string {
	var out strings.Builder
	for _, ch := range text {
		for _, lower := range lowerCase(ch) {
			out.WriteRune(lower)
		}
	}
	return out.String()
}

// shorten cuts at the last `-` inside the cap, unless that loses more than half
// the cap; then at the cap, on a UTF-8 character boundary.
func shorten(slug string) string {
	if len(slug) <= maxSlugLen {
		return slug
	}
	cut := maxSlugLen
	for !utf8.RuneStart(slug[cut]) {
		cut--
	}
	dash := strings.LastIndexByte(slug[:cut], '-')
	if dash >= 0 && dash*2 >= maxSlugLen {
		return slug[:dash]
	}
	return slug[:cut]
}
