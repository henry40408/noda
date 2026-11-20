package query

import (
	"reflect"
	"slices"
	"strings"
	"testing"

	"github.com/henry40408/noda/internal/note"
)

func pinnedAs(value string) *string { return &value }

// searchableNotes is the notebook the table below searches. The ids differ
// only by prefix so an id: term picks one; the last note is deliberately odd.
func searchableNotes() map[string]note.Note {
	return map[string]note.Note{
		"01abc234": {Title: "Budget review", Tags: []string{"work"}, Body: "the q3 budget is late"},
		"02def567": {Title: "Meeting notes", Tags: []string{"work", "q3"}, Pinned: pinnedAs("true"), Body: "# Agenda\n\nthe budget, again"},
		"03ghj890": {Title: "Reading list", Tags: []string{}, Pinned: pinnedAs("false"), Body: "a book"},
		"04kmn123": {Title: "\u65e5\u672c\u8a9e\u30e1\u30e2", Tags: []string{"\u65e5\u672c\u8a9e", "has space"}, Pinned: pinnedAs("TRUE"), Body: "\u672c\u6587 text"},
		"05pqr456": {Title: "Odd one", Tags: []string{"Work"}, Pinned: pinnedAs("yes"), Body: "--flag and OR"},
	}
}

func matchingIDs(q *Query) []string {
	ids := []string{}
	for id, n := range searchableNotes() {
		if q.Matches(id, n) {
			ids = append(ids, id)
		}
	}
	slices.Sort(ids)
	return ids
}

func TestParsedQueryTable(t *testing.T) {
	cases := []struct {
		name     string
		line     string
		grouping [][]string
		excerpts []string
		matches  []string
	}{
		{"bare word searches title and body", "budget", [][]string{{"budget"}}, []string{"budget"}, []string{"01abc234", "02def567"}},
		{"bare word is case folded and the excerpt term is lowercased", "BUDGET", [][]string{{"BUDGET"}}, []string{"budget"}, []string{"01abc234", "02def567"}},
		{"tag matches whole and case sensitively", "tag:Work", [][]string{{"tag:Work"}}, []string{}, []string{"05pqr456"}},
		{"negated tag keeps the rest", "-tag:work", [][]string{{"-tag:work"}}, []string{}, []string{"03ghj890", "04kmn123", "05pqr456"}},
		{"title does not search the body", "title:review", [][]string{{"title:review"}}, []string{"review"}, []string{"01abc234"}},
		{"negated title is not an excerpt term", "-title:review", [][]string{{"-title:review"}}, []string{}, []string{"02def567", "03ghj890", "04kmn123", "05pqr456"}},
		{"id matches a prefix", "id:01", [][]string{{"id:01"}}, []string{}, []string{"01abc234"}},
		{"id folds the Crockford look-alike O to 0", "id:O1", [][]string{{"id:O1"}}, []string{}, []string{"01abc234"}},
		{"id with no prefix match finds nothing", "id:zz", [][]string{{"id:zz"}}, []string{}, []string{}},
		{"negated id excludes the prefix", "-id:01", [][]string{{"-id:01"}}, []string{}, []string{"02def567", "03ghj890", "04kmn123", "05pqr456"}},
		{"pinned true is case insensitive in the note", "pinned:true", [][]string{{"pinned:true"}}, []string{}, []string{"02def567", "04kmn123"}},
		{"pinned false includes a note with an odd value", "pinned:false", [][]string{{"pinned:false"}}, []string{}, []string{"01abc234", "03ghj890", "05pqr456"}},
		{"negated pinned is the other state", "-pinned:true", [][]string{{"-pinned:true"}}, []string{}, []string{"01abc234", "03ghj890", "05pqr456"}},
		{"text searches the body", "text:agenda", [][]string{{"text:agenda"}}, []string{"agenda"}, []string{"02def567"}},
		{"negated text is not an excerpt term", "-text:agenda", [][]string{{"-text:agenda"}}, []string{}, []string{"01abc234", "03ghj890", "04kmn123", "05pqr456"}},
		{"text prefix escapes a leading hyphen", "text:--flag", [][]string{{"text:--flag"}}, []string{"--flag"}, []string{"05pqr456"}},
		{"negated escaped hyphen excludes it", "-text:--flag", [][]string{{"-text:--flag"}}, []string{}, []string{"01abc234", "02def567", "03ghj890", "04kmn123"}},
		{"separate groups are anded", "budget tag:work", [][]string{{"budget"}, {"tag:work"}}, []string{"budget"}, []string{"01abc234", "02def567"}},
		{"a positive and a negative group", "tag:work -tag:q3", [][]string{{"tag:work"}, {"-tag:q3"}}, []string{}, []string{"01abc234"}},
		{"OR joins two terms in one group", "budget OR book", [][]string{{"budget", "book"}}, []string{"budget", "book"}, []string{"01abc234", "02def567", "03ghj890"}},
		{"OR binds tighter than the space", "budget tag:q3 OR tag:work", [][]string{{"budget"}, {"tag:q3", "tag:work"}}, []string{"budget"}, []string{"01abc234", "02def567"}},
		{"OR chains into one group", "a OR b OR c", [][]string{{"a", "b", "c"}}, []string{"a", "b", "c"}, []string{"01abc234", "02def567", "03ghj890", "04kmn123", "05pqr456"}},
		{"OR with a negated side", "tag:work OR -tag:q3", [][]string{{"tag:work", "-tag:q3"}}, []string{}, []string{"01abc234", "02def567", "03ghj890", "04kmn123", "05pqr456"}},
		{"excerpt terms keep title and text order", "title:meeting text:budget", [][]string{{"title:meeting"}, {"text:budget"}}, []string{"meeting", "budget"}, []string{"02def567"}},
		{"lowercase or is a word", "or", [][]string{{"or"}}, []string{"or"}, []string{"01abc234", "02def567", "05pqr456"}},
		{"capitalised Or is a word too", "Or tag:x", [][]string{{"Or"}, {"tag:x"}}, []string{"or"}, []string{}},
		{"field names are case sensitive so the prefix stays text", "TAG:work", [][]string{{"TAG:work"}}, []string{"tag:work"}, []string{}},
		{"unknown prefix is text", "unknown:x", [][]string{{"unknown:x"}}, []string{"unknown:x"}, []string{}},
		{"empty field name is text", ":x", [][]string{{":x"}}, []string{":x"}, []string{}},
		{"bare CJK is a substring", "\u65e5\u672c", [][]string{{"\u65e5\u672c"}}, []string{"\u65e5\u672c"}, []string{"04kmn123"}},
		{"title CJK is a substring", "title:\u65e5\u672c", [][]string{{"title:\u65e5\u672c"}}, []string{"\u65e5\u672c"}, []string{"04kmn123"}},
		{"tag CJK matches whole", "tag:\u65e5\u672c\u8a9e", [][]string{{"tag:\u65e5\u672c\u8a9e"}}, []string{}, []string{"04kmn123"}},
		{"double quotes keep a tag with a space in one term", `tag:"has space"`, [][]string{{"tag:has space"}}, []string{}, []string{"04kmn123"}},
		{"single quotes keep a tag with a space in one term", "tag:'has space'", [][]string{{"tag:has space"}}, []string{}, []string{"04kmn123"}},
		{"quoted words are one term", `"two words"`, [][]string{{"two words"}}, []string{"two words"}, []string{}},
		{"unclosed quote runs to the end", `tag:"unclosed`, [][]string{{"tag:unclosed"}}, []string{}, []string{}},
		{"every bare word must match", "a b c", [][]string{{"a"}, {"b"}, {"c"}}, []string{"a", "b", "c"}, []string{}},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			q, err := Parse(Split(c.line))
			if err != nil {
				t.Fatalf("%q: %v", c.line, err)
			}
			if got := q.Grouping(); !reflect.DeepEqual(got, c.grouping) {
				t.Errorf("grouping: got %q, want %q", got, c.grouping)
			}
			if got := q.ExcerptTerms(); !slices.Equal(got, c.excerpts) {
				t.Errorf("excerpt terms: got %q, want %q", got, c.excerpts)
			}
			if got := matchingIDs(q); !slices.Equal(got, c.matches) {
				t.Errorf("matches: got %q, want %q", got, c.matches)
			}
		})
	}
}

func TestParseErrorTable(t *testing.T) {
	cases := []struct{ name, line, want string }{
		{"empty line", "", "search needs something to look for"},
		{"blank line", "   ", "search needs something to look for"},
		{"lone OR", "OR", "`OR` needs a term on both sides"},
		{"OR at the start", "OR tag:x", "`OR` needs a term on both sides"},
		{"OR at the end", "tag:x OR", "`OR` needs a term on both sides"},
		{"two ORs in a row", "a OR OR b", "`OR` needs a term on both sides"},
		{"lone hyphen", "-", "`-` needs something after it"},
		{"tag with no value", "tag:", "`tag:` has nothing to look for"},
		{"title with no value", "title:", "`title:` has nothing to look for"},
		{"id with no value", "id:", "`id:` has nothing to look for"},
		{"pinned with no value", "pinned:", "`pinned:` has nothing to look for"},
		{"pinned with a typo", "pinned:maybe", "`pinned:` takes `true` or `false`, not `maybe`"},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			_, err := Parse(Split(c.line))
			if err == nil || err.Error() != c.want {
				t.Errorf("%q: got %v, want %q", c.line, err, c.want)
			}
		})
	}
}

func TestAFieldSplitsTheWayAShellDoes(t *testing.T) {
	cases := map[string][]string{
		"+work -q3":                        {"+work", "-q3"},
		"  +work   ":                       {"+work"},
		"   ":                              nil,
		"-'a b' +c":                        {"-a b", "+c"},
		`-"a b"`:                           {"-a b"},
		`"-a b"`:                           {"-a b"},
		`-"a b`:                            {"-a b"},
		`tag:"24.04 Dark patterns" budget`: {"tag:24.04 Dark patterns", "budget"},
	}
	for line, want := range cases {
		if got := Split(line); !slices.Equal(got, want) {
			t.Errorf("%q: %q, want %q", line, got, want)
		}
	}
}

func TestScopedQuotesATagThatHasASpaceSoSplitGivesOneTerm(t *testing.T) {
	if Scoped("work") != "tag:work" {
		t.Fatal(Scoped("work"))
	}
	if got := Split(Scoped("24.04 Dark patterns")); !slices.Equal(got, []string{"tag:24.04 Dark patterns"}) {
		t.Fatal(got)
	}
}

func parsed(t *testing.T, text string) *Query {
	t.Helper()
	q, err := Parse(strings.Split(text, " "))
	if err != nil {
		t.Fatal(err)
	}
	return q
}

func aNote(title string, tags []string, body string) note.Note {
	return note.Note{Title: title, Tags: tags, Body: body}
}

const anID = "k3f9m2p1"

func TestABareWordSearchesTheTitleTheTagsAndTheBody(t *testing.T) {
	n := aNote("Meeting Notes", []string{"work"}, "the Q3 budget\n")
	for word, want := range map[string]bool{"budget": true, "meeting": true, "work": true, "hiring": false} {
		if got := parsed(t, word).Matches(anID, n); got != want {
			t.Errorf("%s: %v", word, got)
		}
	}
}

func TestPinnedAsksForOneOfTwoStatesAndTheNegationAsksForTheOther(t *testing.T) {
	pin := note.PinnedValue
	pinned := aNote("Alpha", nil, "x\n")
	pinned.Pinned = &pin
	loose := aNote("Beta", nil, "x\n")
	odd := aNote("Gamma", nil, "x\n")
	yes := "yes"
	odd.Pinned = &yes

	checks := []struct {
		query string
		note  note.Note
		want  bool
	}{
		{"pinned:true", pinned, true}, {"pinned:true", loose, false},
		{"pinned:false", loose, true}, {"pinned:false", pinned, false},
		{"-pinned:true", loose, true},
		// IsPinned's answer is compared, not the field's presence.
		{"pinned:false", odd, true},
	}
	for _, c := range checks {
		if got := parsed(t, c.query).Matches(anID, c.note); got != c.want {
			t.Errorf("%s on %q: %v", c.query, c.note.Title, got)
		}
	}
}

func TestPinnedRefusesAValueThatIsNotOneOfTheTwo(t *testing.T) {
	for _, bad := range []string{"pinned:ture", "pinned:yes", "-pinned:1"} {
		if _, err := Parse([]string{bad}); err == nil {
			t.Errorf("%s should be refused", bad)
		}
	}
	for _, good := range []string{"pinned:true", "pinnedness"} {
		if _, err := Parse([]string{good}); err != nil {
			t.Errorf("%s: %v", good, err)
		}
	}
}

func TestSeparateTermsMustAllMatch(t *testing.T) {
	n := aNote("Meeting Notes", nil, "the Q3 budget\n")
	if !parsed(t, "q3 budget").Matches(anID, n) || parsed(t, "q3 hiring").Matches(anID, n) {
		t.Fatal("terms are anded")
	}
}

func TestOrIsSatisfiedByEitherSide(t *testing.T) {
	q := parsed(t, "tag:work OR tag:home")
	if !q.Matches(anID, aNote("a", []string{"work"}, "")) || !q.Matches(anID, aNote("a", []string{"home"}, "")) {
		t.Fatal("either side will do")
	}
	if q.Matches(anID, aNote("a", []string{"q3"}, "")) {
		t.Fatal("neither side")
	}
}

func TestOrBindsTighterThanTheSpaceBetweenGroups(t *testing.T) {
	q := parsed(t, "budget tag:work OR tag:q3")
	if !q.Matches(anID, aNote("A", []string{"work", "q3"}, "budget\n")) {
		t.Fatal("both groups hold")
	}
	if q.Matches(anID, aNote("A", []string{"home"}, "budget\n")) {
		t.Fatal("the bare term is ANDed, not swallowed by the OR")
	}
	if q.Matches(anID, aNote("A", []string{"work"}, "hiring\n")) {
		t.Fatal("the bare term has to match")
	}
}

func TestTheGroupingShownIsTheGroupingApplied(t *testing.T) {
	want := [][]string{{"budget"}, {"tag:work", "tag:q3"}}
	if got := parsed(t, "budget tag:work OR tag:q3").Grouping(); !reflect.DeepEqual(got, want) {
		t.Fatal(got)
	}
	want = [][]string{{"tag:a", "tag:b"}, {"tag:c", "tag:d"}}
	if got := parsed(t, "tag:a OR tag:b tag:c OR tag:d").Grouping(); !reflect.DeepEqual(got, want) {
		t.Fatal(got)
	}
}

func TestTheGroupingKeepsTheWordsThatWereTyped(t *testing.T) {
	q, err := Parse(Split(`-tag:archived title:"Q3 budget"`))
	if err != nil {
		t.Fatal(err)
	}
	if want := [][]string{{"-tag:archived"}, {"title:Q3 budget"}}; !reflect.DeepEqual(q.Grouping(), want) {
		t.Fatal(q.Grouping())
	}
}

func TestEachFieldMatchesTheWayNodaMatchesThatThing(t *testing.T) {
	n := aNote("Meeting Notes", []string{"work"}, "body\n")
	for text, want := range map[string]bool{
		"tag:work": true, "tag:wor": false, "title:meeting": true, "title:body": false,
		"id:k3f9": true, "id:K3F9": true, "id:q7x2": false,
	} {
		if got := parsed(t, text).Matches(anID, n); got != want {
			t.Errorf("%s: %v", text, got)
		}
	}
}

func TestCJKIsMatchedBySubstring(t *testing.T) {
	n := aNote("會議記錄", []string{"工作"}, "討論第三季預算\n")
	for _, text := range []string{"第三季預算", "title:會議", "tag:工作"} {
		if !parsed(t, text).Matches(anID, n) {
			t.Errorf("%s", text)
		}
	}
}

func TestOnlyAnUppercaseOrIsTheOperator(t *testing.T) {
	q := parsed(t, "or")
	if !q.Matches(anID, aNote("x", nil, "this or that\n")) || q.Matches(anID, aNote("x", nil, "neither\n")) {
		t.Fatal("`or` is a word")
	}
}

func TestTextIsTheWayToLookForSomethingStartingWithAHyphen(t *testing.T) {
	has := aNote("x", nil, "a --flag in the body\n")
	plain := aNote("x", nil, "nothing like it\n")
	if !parsed(t, "text:--flag").Matches(anID, has) || parsed(t, "text:--flag").Matches(anID, plain) {
		t.Fatal("text: searches for the hyphen")
	}
	if !parsed(t, "--flag").Matches(anID, plain) {
		t.Fatal("without the prefix it reads as `not -flag`, which the plain note satisfies")
	}
}

func TestAnUnknownPrefixIsTextRatherThanAField(t *testing.T) {
	if !parsed(t, "https://example.com/x").Matches(anID, aNote("x", nil, "see https://example.com/x\n")) {
		t.Fatal("a URL is text")
	}
}

func TestAValueKeepsEveryColonAfterTheFirst(t *testing.T) {
	q, err := Parse([]string{"title:Rust: a tour"})
	if err != nil || !q.Matches(anID, aNote("Rust: a tour", nil, "body\n")) {
		t.Fatal(err)
	}
}

func TestAQueryThatSaysNothingIsRefused(t *testing.T) {
	for _, bad := range [][]string{nil, {"OR"}, {"a", "OR"}, {"OR", "a"}, {"-"}, {"tag:"}} {
		if _, err := Parse(bad); err == nil {
			t.Errorf("%q should be refused", bad)
		}
	}
}

func TestOnlyTextTermsAreWorthQuotingALineFor(t *testing.T) {
	got := parsed(t, "budget title:meeting tag:work -hiring").ExcerptTerms()
	if !slices.Equal(got, []string{"budget", "meeting"}) {
		t.Fatal(got)
	}
}

func TestScopedQuotesATagThatStartsWithASpace(t *testing.T) {
	if got := Scoped(" lead"); got != `tag:" lead"` {
		t.Fatal(got)
	}
}
