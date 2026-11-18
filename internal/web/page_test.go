package web

import (
	"strings"
	"testing"

	"github.com/henry40408/noda/internal/cmd"
	"github.com/henry40408/noda/internal/version"
)

// goldenVersion is the build the goldens were captured from.
const goldenVersion = "0.1.3-4-g9e17994"

type pageCase struct {
	Name  string `json:"name"`
	Fn    string `json:"fn"`
	Book  string `json:"book"`
	Rows  []Row  `json:"rows"`
	Asked Asked  `json:"asked"`
	Order struct {
		Sort     string `json:"sort"`
		Reversed bool   `json:"reversed"`
	} `json:"order"`
	Drift    string   `json:"drift"`
	Front    *string  `json:"front"`
	Text     string   `json:"text"`
	Books    []Book   `json:"books"`
	Held     []Held   `json:"held"`
	Tallies  []Tally  `json:"tallies"`
	Tasks    []Task   `json:"tasks"`
	Subject  Subject  `json:"subject"`
	Reading  Reading  `json:"reading"`
	Draft    Draft    `json:"draft"`
	Problem  *string  `json:"problem"`
	About    About    `json:"about"`
	Body     string   `json:"body"`
	Was      string   `json:"was"`
	Theirs   string   `json:"theirs"`
	Mine     string   `json:"mine"`
	Now      string   `json:"now"`
	Merged   string   `json:"merged"`
	Title    string   `json:"title"`
	Tags     []string `json:"tags"`
	Standing Standing `json:"standing"`
	Errand   *Errand  `json:"errand"`
	Heading  string   `json:"heading"`
	Detail   string   `json:"detail"`
}

func (c pageCase) listing() Listing {
	sort, _ := cmd.SortNamed(c.Order.Sort)
	return Listing{Book: c.Book, Rows: c.Rows, Asked: c.Asked, Order: Order{Sort: sort, Reversed: c.Order.Reversed}, Drift: c.Drift}
}

func (c pageCase) render(t *testing.T) string {
	switch c.Fn {
	case "listing":
		return ListingPage(c.listing(), c.Front)
	case "listing_screen":
		return ListingScreen(c.listing(), c.Front)
	case "listing_pane":
		return ListingPane(c.listing())
	case "escape":
		return escape(c.Text)
	case "notebooks":
		return Notebooks(c.Books)
	case "files":
		return Files(c.Book, c.Held)
	case "tags":
		return Tags(c.Book, c.Tallies)
	case "todo":
		return Todo(c.Book, c.Tasks)
	case "backlinks":
		return Backlinks(c.Book, c.Subject, c.Rows)
	case "backlinks_rows":
		return BacklinksRows(c.Book, c.Subject, c.Rows)
	case "note":
		return NotePage(c.Book, c.Reading, c.Drift)
	case "note_pane":
		return NotePane(c.Book, c.Reading)
	case "composing":
		return Composing(c.Book, c.Draft, c.Problem)
	case "editing":
		return Editing(Edit{Book: c.Book, About: c.About, Body: c.Body, Was: c.Was, Problem: c.Problem})
	case "clashed":
		return Clashed(Clash{Book: c.Book, About: c.About, Theirs: c.Theirs, Mine: c.Mine, Now: c.Now})
	case "conflicted":
		return Conflicted(c.Book, c.About, c.Merged, c.Now)
	case "renaming":
		return Renaming(c.Book, c.About, c.Title, c.Problem)
	case "tagging":
		return Tagging(c.Book, c.About, c.Tags, c.Problem)
	case "deleting":
		return Deleting(c.Book, c.About)
	case "standing":
		return StandingPage(c.Book, c.Standing, c.Errand)
	case "standing_main":
		return StandingMain(c.Book, c.Standing, c.Errand)
	case "failure":
		return Failure(c.Heading, c.Detail)
	}
	t.Fatalf("unknown fn %s", c.Fn)
	return ""
}

func TestPagesMatchGolden(t *testing.T) {
	var cases []pageCase
	var expected []struct{ Name, Out string }
	readJSON(t, "../../testdata/golden/page/cases.json", &cases)
	readJSON(t, "../../testdata/golden/page/expected.json", &expected)
	for i, c := range cases {
		if expected[i].Name != c.Name {
			t.Fatalf("order differs at %d", i)
		}
		t.Run(c.Name, func(t *testing.T) {
			want := strings.ReplaceAll(expected[i].Out, goldenVersion, version.String())
			if got := c.render(t); got != want {
				t.Errorf("\n got %q\nwant %q", got, want)
			}
		})
	}
}
