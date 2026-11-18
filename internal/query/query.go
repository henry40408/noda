// Package query is the query `noda search` takes, deliberately fixed at one
// small shape:
//
//	query := group (' ' group)*        every group must match
//	group := term ('OR' term)*         any term in the group will do
//	term  := ['-'] [field ':'] value
//	field := tag | title | id | pinned | text
//
// An AND of ORs, so there are no parentheses; `(a AND b) OR (c AND d)` is two
// searches. `OR` binds tighter than the space: `budget tag:x OR tag:y` is
// `budget AND (tag:x OR tag:y)`, what someone listing alternatives expects.
//
// A leading `-` is always a negation; the field prefix is the escape
// (`text:--flag`). One token is one term, so the grammar has no quoting.
//
// A tag matches whole, an id by folded prefix, text and titles by
// case-insensitive substring (splitting on spaces finds nothing in CJK).
// `pinned:` takes only `true` or `false`.
package query

import (
	"errors"
	"fmt"
	"strings"
	"unicode"

	"github.com/henry40408/noda/internal/note"
)

// Split cuts one line as a shell would: on whitespace, not inside `"` or `'`.
// For the TUI and web inputs with no shell in front, in one place so they
// cannot diverge. A tag may contain a space, so `tag:"24.04 Dark patterns"`
// must stay one token. An unclosed quote runs to the end: the line is still
// being typed.
func Split(text string) []string {
	var pieces []string
	var piece strings.Builder
	var quote rune
	flush := func() {
		if piece.Len() > 0 {
			pieces = append(pieces, piece.String())
			piece.Reset()
		}
	}
	for _, ch := range text {
		switch {
		case quote != 0 && ch == quote:
			quote = 0
		case quote == 0 && (ch == '"' || ch == '\''):
			quote = ch
		case quote == 0 && unicode.IsSpace(ch):
			flush()
		default:
			piece.WriteRune(ch)
		}
	}
	flush()
	return pieces
}

// Scoped is the query narrowing a listing to one tag, quoted when needed so
// Split gives it back as one term.
func Scoped(tag string) string {
	if strings.IndexFunc(tag, unicode.IsSpace) >= 0 {
		return `tag:"` + tag + `"`
	}
	return "tag:" + tag
}

// Query is a parsed search.
type Query struct {
	groups [][]term
	// said is the same grouping as typed, for display (a term has dropped the
	// field and the `-`). Filled by the same loop so the two cannot disagree.
	said [][]string
}

type field int

const (
	fieldTag field = iota
	fieldTitle
	fieldID
	// fieldPinned is `true` or `false` only: compared rather than searched.
	fieldPinned
	// fieldText is title, tags and body: what a bare word searches.
	fieldText
)

type term struct {
	field   field
	value   string
	negated bool
}

// orOperator is uppercase only, so the English word `or` stays searchable.
const orOperator = "OR"

var errOrNeedsTwoSides = errors.New("`OR` needs a term on both sides")

// Parse reads one token per term: `noda search "title:Q3 budget" tag:work` is
// two.
func Parse(tokens []string) (*Query, error) {
	query := &Query{}
	expecting := false
	for _, token := range tokens {
		if strings.TrimSpace(token) == "" {
			continue
		}
		if token == orOperator {
			if len(query.groups) == 0 || expecting {
				return nil, errOrNeedsTwoSides
			}
			expecting = true
			continue
		}
		parsed, err := parseTerm(token)
		if err != nil {
			return nil, err
		}
		query.add(parsed, token, expecting)
		expecting = false
	}
	if expecting {
		return nil, errOrNeedsTwoSides
	}
	if len(query.groups) == 0 {
		return nil, errors.New("search needs something to look for")
	}
	return query, nil
}

func (q *Query) add(parsed term, token string, joinsPrevious bool) {
	if !joinsPrevious {
		q.groups = append(q.groups, []term{parsed})
		q.said = append(q.said, []string{token})
		return
	}
	last := len(q.groups) - 1
	q.groups[last] = append(q.groups[last], parsed)
	q.said[last] = append(q.said[last], token)
}

// Grouping is the grouping in the tokens as given: the outer list and-ed, each
// inner list or-ed. Shown because `OR` binding tighter than a space surprises.
func (q *Query) Grouping() [][]string {
	return q.said
}

// Matches reports whether every group has a term the note satisfies.
func (q *Query) Matches(id string, n note.Note) bool {
	for _, group := range q.groups {
		if !anyMatches(group, id, n) {
			return false
		}
	}
	return true
}

func anyMatches(group []term, id string, n note.Note) bool {
	for _, t := range group {
		if t.matches(id, n) {
			return true
		}
	}
	return false
}

// ExcerptTerms are the terms worth quoting a matching line for: positive
// `text` and `title`, lowercased.
func (q *Query) ExcerptTerms() []string {
	terms := []string{}
	for _, group := range q.groups {
		for _, t := range group {
			if !t.negated && (t.field == fieldText || t.field == fieldTitle) {
				terms = append(terms, note.Lower(t.value))
			}
		}
	}
	return terms
}

func parseTerm(token string) (term, error) {
	rest, negated := strings.CutPrefix(token, "-")
	if rest == "" {
		return term{}, errors.New("`-` needs something after it")
	}
	// First colon only: `title:Rust: a tour` looks for `Rust: a tour`.
	kind, value := fieldText, rest
	if name, after, found := strings.Cut(rest, ":"); found {
		// Only known names are fields, so `https://…` is text.
		if known, ok := fieldNamed(name); ok {
			kind, value = known, after
		}
	}
	if value == "" {
		return term{}, fmt.Errorf("`%s` has nothing to look for", token)
	}
	// Other fields find nothing on nonsense; `pinned:ture` would silently read
	// as `pinned:false`, so it is refused.
	if kind == fieldPinned && value != "true" && value != "false" {
		return term{}, fmt.Errorf("`pinned:` takes `true` or `false`, not `%s`", value)
	}
	return term{field: kind, value: value, negated: negated}, nil
}

func fieldNamed(name string) (field, bool) {
	switch name {
	case "tag":
		return fieldTag, true
	case "title":
		return fieldTitle, true
	case "id":
		return fieldID, true
	case "pinned":
		return fieldPinned, true
	case "text":
		return fieldText, true
	}
	return 0, false
}

func (t term) matches(id string, n note.Note) bool {
	return t.found(id, n) != t.negated
}

func (t term) found(id string, n note.Note) bool {
	switch t.field {
	case fieldTag:
		for _, tag := range n.Tags {
			if tag == t.value {
				return true
			}
		}
		return false
	case fieldID:
		return strings.HasPrefix(note.NormalizeID(id), note.NormalizeID(t.value))
	case fieldTitle:
		return containsIgnoringCase(n.Title, t.value)
	case fieldPinned:
		return n.IsPinned() == (t.value == "true")
	}
	if containsIgnoringCase(n.Title, t.value) || containsIgnoringCase(n.Body, t.value) {
		return true
	}
	for _, tag := range n.Tags {
		if containsIgnoringCase(tag, t.value) {
			return true
		}
	}
	return false
}

func containsIgnoringCase(haystack, needle string) bool {
	return strings.Contains(note.Lower(haystack), note.Lower(needle))
}
