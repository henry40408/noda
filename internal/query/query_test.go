package query

import (
	"encoding/json"
	"os"
	"reflect"
	"slices"
	"strings"
	"testing"

	"github.com/henry40408/noda/internal/note"
)

func readJSON(t *testing.T, path string, into any) {
	t.Helper()
	raw, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	if err := json.Unmarshal(raw, into); err != nil {
		t.Fatal(err)
	}
}

func asAny[T any](values []T) []any {
	out := []any{}
	for _, v := range values {
		out = append(out, v)
	}
	return out
}

func TestQueriesMatchTheRustGolden(t *testing.T) {
	var cases []struct{ Name, Line string }
	var expected []struct {
		Name string         `json:"name"`
		Out  map[string]any `json:"out"`
	}
	var fixtures []struct {
		ID     string   `json:"id"`
		Title  string   `json:"title"`
		Tags   []string `json:"tags"`
		Pinned *string  `json:"pinned"`
		Body   string   `json:"body"`
	}
	readJSON(t, "../../testdata/golden/query/cases.json", &cases)
	readJSON(t, "../../testdata/golden/query/expected.json", &expected)
	readJSON(t, "../../testdata/golden/query/notes.json", &fixtures)
	if len(cases) != len(expected) || len(cases) != 58 || len(fixtures) != 5 {
		t.Fatalf("%d cases, %d expected, %d notes", len(cases), len(expected), len(fixtures))
	}
	for i, c := range cases {
		if expected[i].Name != c.Name {
			t.Fatalf("case %d is %s, expected %s", i, c.Name, expected[i].Name)
		}
		tokens := Split(c.Line)
		got := map[string]any{"tokens": asAny(tokens), "parse": nil}
		parsed, err := Parse(tokens)
		if err != nil {
			got["parse"] = map[string]any{"err": err.Error()}
		} else {
			matches := []any{}
			for _, f := range fixtures {
				n := note.Note{Title: f.Title, Tags: f.Tags, Pinned: f.Pinned, Body: f.Body}
				if parsed.Matches(f.ID, n) {
					matches = append(matches, f.ID)
				}
			}
			grouping := [][]string(parsed.Grouping())
			said := []any{}
			for _, group := range grouping {
				said = append(said, asAny(group))
			}
			got["parse"] = map[string]any{"ok": map[string]any{
				"excerpt_terms": asAny(parsed.ExcerptTerms()), "grouping": said, "matches": matches,
			}}
		}
		if !reflect.DeepEqual(got, expected[i].Out) {
			t.Errorf("%s: %q\n got  %#v\n want %#v", c.Name, c.Line, got, expected[i].Out)
		}
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
