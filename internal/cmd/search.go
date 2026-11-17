package cmd

import (
	"strings"
	"unicode"
	"unicode/utf8"

	"github.com/henry40408/noda/internal/note"
	"github.com/henry40408/noda/internal/query"
	"github.com/henry40408/noda/internal/style"
)

const (
	// excerptWidth is the characters of a matching line to show.
	excerptWidth = 72
	// excerptLead is at most this many before the match, so the match stays
	// visible.
	excerptLead = 28
)

// Search is a case-insensitive substring match, not a word match: Chinese or
// Japanese notes have no spaces to tokenise on. Several terms mean all of them.
func Search(e Env, tokens []string) (string, error) {
	parsed, err := query.Parse(tokens)
	if err != nil {
		return "", err
	}
	terms := parsed.ExcerptTerms()

	active, err := e.openActive()
	if err != nil {
		return "", err
	}
	notes, err := active.Notes()
	if err != nil {
		return "", err
	}

	type hit struct {
		id, title, tags, excerpt string
		hasExcerpt               bool
	}
	var hits []hit
	for _, file := range notes {
		// The parsed note, so frontmatter keys are not searchable text.
		if !parsed.Matches(file.ID, file.Note) {
			continue
		}
		shown, ok := excerpt(file.Note.Body, terms)
		hits = append(hits, hit{
			id: file.ID, title: file.Note.Title, tags: strings.Join(file.Note.Tags, ", "),
			excerpt: shown, hasExcerpt: ok,
		})
	}
	if len(hits) == 0 {
		return "", nil
	}

	// `ls`'s row shape.
	idWidth := 0
	for _, h := range hits {
		idWidth = max(idWidth, DisplayWidth(h.id))
	}
	var out strings.Builder
	for _, h := range hits {
		line := pad(h.id, idWidth) + "  " + h.title
		if h.tags != "" {
			line += "  [" + h.tags + "]"
		}
		out.WriteString(strings.TrimRightFunc(line, unicode.IsSpace) + "\n")
		// Only a body hit needs quoting; title and tags are on the row.
		if h.hasExcerpt {
			out.WriteString(strings.Repeat(" ", idWidth+2) + h.excerpt + "\n")
		}
	}
	return out.String(), nil
}

// excerpt is the first body line holding a term, cut to fit, with the match
// highlighted.
func excerpt(body string, terms []string) (string, bool) {
	for _, line := range strings.Split(body, "\n") {
		line = strings.TrimSuffix(line, "\r")
		for _, term := range terms {
			start, end, ok := FindIgnoringCase(line, term)
			if !ok {
				continue
			}
			before := lastChars(line[:start], excerptLead)
			room := max(0, excerptWidth-utf8.RuneCountInString(before)-utf8.RuneCountInString(line[start:end]))
			return before + style.Match.Paint(line[start:end]) + firstChars(line[end:], room), true
		}
	}
	return "", false
}

// FindIgnoringCase is a case-insensitive `strings.Index`, as byte offsets into
// haystack. Lowercasing can change a character's byte length, so offsets are
// mapped back and always land on character boundaries. Shared with the TUI,
// which highlights the same match.
func FindIgnoringCase(haystack, needle string) (start, end int, found bool) {
	var lowered strings.Builder
	origin := make([]int, 0, len(haystack)+1)
	for index, ch := range haystack {
		lower := note.Lower(string(ch))
		for range len(lower) {
			origin = append(origin, index)
		}
		lowered.WriteString(lower)
	}
	origin = append(origin, len(haystack))

	at := strings.Index(lowered.String(), needle)
	if at < 0 {
		return 0, 0, false
	}
	return origin[at], origin[at+len(needle)], true
}

// lastChars is the last max characters, with a leading `…` when something was
// cut.
func lastChars(text string, max int) string {
	runes := []rune(text)
	if len(runes) <= max {
		return text
	}
	return "…" + strings.TrimLeft(string(runes[len(runes)-max:]), " \t\n\r\f\v  ")
}

// firstChars is the first max characters, with a trailing `…` when something
// was cut.
func firstChars(text string, max int) string {
	runes := []rune(text)
	if len(runes) <= max {
		return text
	}
	return strings.TrimRight(string(runes[:max]), " \t\n\r\f\v  ") + "…"
}
