package web

import (
	"regexp"
	"strings"
	"testing"
	"time"

	"github.com/henry40408/noda/internal/cmd"
	"github.com/henry40408/noda/internal/version"
)

const evil = `<script>alert('x')</script> & "q"`
const evilEscaped = `&lt;script&gt;alert(&#39;x&#39;)&lt;/script&gt; &amp; &quot;q&quot;`

func ptr(s string) *string { return &s }

// shows fails for each fragment missing from got.
func shows(t *testing.T, got string, fragments ...string) {
	t.Helper()
	for _, fragment := range fragments {
		if !strings.Contains(got, fragment) {
			t.Errorf("missing %q in\n%s", fragment, got)
		}
	}
}

// omits fails for each fragment present in got.
func omits(t *testing.T, got string, fragments ...string) {
	t.Helper()
	for _, fragment := range fragments {
		if strings.Contains(got, fragment) {
			t.Errorf("unexpected %q in\n%s", fragment, got)
		}
	}
}

// inOrder fails unless the fragments appear in got in the order given.
func inOrder(t *testing.T, got string, fragments ...string) {
	t.Helper()
	from := 0
	for _, fragment := range fragments {
		at := strings.Index(got[from:], fragment)
		if at < 0 {
			t.Errorf("%q missing or out of order after offset %d in\n%s", fragment, from, got)
			return
		}
		from += at + len(fragment)
	}
}

func TestEscape(t *testing.T) {
	for _, c := range []struct{ name, in, want string }{
		{"plain text is untouched", "hello", "hello"},
		{"all five characters", `<>&"'`, "&lt;&gt;&amp;&quot;&#39;"},
		{"markup and quotes", evil, evilEscaped},
		{"unicode passes through", "日本語 😀 é", "日本語 😀 é"},
		{"empty stays empty", "", ""},
	} {
		t.Run(c.name, func(t *testing.T) {
			if got := escape(c.in); got != c.want {
				t.Errorf("got %q, want %q", got, c.want)
			}
		})
	}
}

func TestFailure(t *testing.T) {
	for _, c := range []struct{ name, heading, detail, title, empty string }{
		{"heading and detail", "Not answered", "That host is not on the list.",
			"<title>Not answered — noda</title>",
			`<div class="empty"><b>Not answered</b>That host is not on the list.</div>`},
		{"both are escaped", evil, evil,
			"<title>" + evilEscaped + " — noda</title>",
			`<div class="empty"><b>` + evilEscaped + `</b>` + evilEscaped + `</div>`},
		{"empty strings", "", "",
			"<title> — noda</title>", `<div class="empty"><b></b></div>`},
		{"unicode", "找不到", "沒有這個筆記 😀",
			"<title>找不到 — noda</title>", `<div class="empty"><b>找不到</b>沒有這個筆記 😀</div>`},
	} {
		t.Run(c.name, func(t *testing.T) {
			shows(t, Failure(c.heading, c.detail), c.title, c.empty,
				`<a class="back" href="/" aria-label="Back to Notebooks"><svg`, `class="app root solo"`)
		})
	}
}

func TestPageShell(t *testing.T) {
	page := shell("x", "", "")
	shows(t, page, "<!doctype html>\n<html lang=\"en\">", `<meta name=\"referrer\" content=\"same-origin\">`[0:0]+`<meta name="referrer" content="same-origin">`,
		`<div class="app">`, "</html>\n")
	omits(t, page, "http-equiv=\"refresh\"")
	shows(t, shell("x", "split at-list", ""), `<div class="app split at-list">`)
}

func TestNotebooks(t *testing.T) {
	drift := "2 to push, 1 to pull"
	for _, c := range []struct {
		name  string
		books []Book
		want  []string
		not   []string
	}{
		{"no notebooks says how to make one", nil,
			[]string{"<b>No notebooks yet</b>Run <code>noda init</code>", `<span class="count">0 notebooks, 0 notes</span>`}, nil},
		{"one active notebook with no remote",
			[]Book{{Name: "default", Notes: 1, Active: true, Last: "2024-03-01 10:00"}},
			[]string{`<span class="count">1 notebook, 1 note</span>`, `<span class="mark">active</span>`,
				`<span>1 note</span>`, `<span class="aside">no remote</span>`,
				`<div class="row split book h5">`, `<a class="most" href="/nb/default">`,
				`<span class="stamp">last change 2024-03-01 10:00</span>`},
			[]string{"file", "uncommitted", "/status"}},
		{"files, uncommitted and drift are shown when present, notes are summed",
			[]Book{{Name: "a", Notes: 3}, {Name: "work", Notes: 12, Files: 2, Uncommitted: 1, Drift: &drift}},
			[]string{`2 notebooks, 15 notes`, `<span>2 files</span>`, `<span class="dirty">1 uncommitted</span>`,
				`<a class="aside drift" href="/nb/work/status" aria-label="Status: 2 to push, 1 to pull">`,
				`<div class="row split book h7">`},
			[]string{"active"}},
		{"one file is singular", []Book{{Name: "a", Files: 1}}, []string{`<span>1 file</span>`}, nil},
		{"the name is escaped", []Book{{Name: evil, Drift: ptr(evil)}},
			[]string{`href="/nb/` + evilEscaped + `"`, `<span class="name">` + evilEscaped + `</span>`,
				"Status: " + evilEscaped}, []string{"<script>"}},
	} {
		t.Run(c.name, func(t *testing.T) {
			got := Notebooks(c.books)
			shows(t, got, c.want...)
			omits(t, got, c.not...)
			shows(t, got, `class="app root solo"`, `<main class="rows books">`, AssetKeys.Href())
			omits(t, got, `class="foot rail"`)
		})
	}
}

func TestNotebooksListsInGivenOrder(t *testing.T) {
	got := Notebooks([]Book{{Name: "first"}, {Name: "second"}})
	inOrder(t, got, `href="/nb/first"`, `href="/nb/second"`)
}

// The mark closes the line of facts, as "pinned" closes a note's, so every
// row's facts start at the same place.
func TestNotebookActiveMarkComesLastInTheFacts(t *testing.T) {
	got := bookRow(Book{Name: "work", Notes: 2, Files: 1, Uncommitted: 3, Active: true})
	inOrder(t, got, `<span>2 notes</span>`, `<span>1 file</span>`, `<span class="dirty">`, `<span class="mark">active</span></span>`)
}

var someRows = []Row{
	{ID: "01abc234", Title: "Budget review", Tags: []string{"work"}, Stamp: ptr("2024-03-01T10:00:00Z"), Shown: true, Pinned: true},
	{ID: "02def567", Title: "Meeting notes", Tags: []string{"work", "q3"}, Stamp: ptr("2024-02-01T10:00:00+08:00"), Shown: true},
	{ID: "04kmn123", Title: "日本語メモ", Tags: []string{"日本語", "has space"}, Stamp: ptr("2024-01-01T00:00:00Z")},
}

func listingOf(rows []Row, typed string, order Order) Listing {
	return Listing{Book: "work", Rows: rows, Asked: Asked{Typed: typed}, Order: order}
}

func TestListingPaneRows(t *testing.T) {
	got := ListingPane(listingOf(someRows, "", Order{}))
	inOrder(t, got, "Budget review", "Meeting notes", "日本語メモ")
	shows(t, got,
		`<span class="count">3</span>`,
		`<a class="row h10" href="/nb/work/n/01abc234">`,
		`<span class="title">Budget review</span>`,
		`<span class="id">01abc234</span></a>`,
		`<a class="row h5" href="/nb/work/n/02def567">`,
		`<time class="when" datetime="2024-02-01T10:00:00+08:00">2024-02-01</time>`,
		`<span class="tags"><span>work</span><span>q3</span></span>`,
		`<span class="tags"><span>日本語</span><span>has space</span></span>`,
		`<span class="pin">pinned</span>`)
	inOrder(t, got, `<time class="when"`, `<span class="tags">`, `<span class="pin">pinned</span></span><span class="id">`)
	if n := strings.Count(got, `class="pin"`); n != 1 {
		t.Errorf("%d pin marks, want 1 (only the pinned row)", n)
	}
	omits(t, got, `<p class="problem">`)
}

func TestListingRowsWithoutAStampHaveNoDay(t *testing.T) {
	got := ListingPane(listingOf([]Row{{ID: "01abc234", Title: "t", Shown: true}}, "", Order{}))
	shows(t, got, `<span class="under"></span><span class="id">01abc234</span>`)
	omits(t, got, `<time`)
}

func TestListingRowsAreEscaped(t *testing.T) {
	list := listingOf([]Row{{ID: "x", Title: evil, Tags: []string{evil}, Stamp: ptr(evil), Shown: true}}, evil, Order{})
	list.Book = evil
	got := ListingPane(list)
	omits(t, got, "<script>")
	shows(t, got, `<span class="title">`+evilEscaped+`</span>`, `<span class="tags"><span>`+evilEscaped+`</span></span>`,
		`value="`+evilEscaped+`"`, `<span class="nb">`+evilEscaped+`</span>`)
}

func TestListingHiddenRowsKeepAnUnmarkedTitle(t *testing.T) {
	list := Listing{Book: "work", Rows: []Row{
		{ID: "01aaaaaa", Title: "Budget plan", Shown: true},
		{ID: "02bbbbbb", Title: "Budget old", Shown: false},
	}, Asked: Asked{Typed: "budget", Terms: []string{"budget"}}}
	got := ListingPane(list)
	shows(t, got,
		`<a class="row `+hueClass("01aaaaaa")+`" href="/nb/work/n/01aaaaaa">`, `<mark>Budget</mark> plan`,
		`<a class="row `+hueClass("02bbbbbb")+`" hidden href="/nb/work/n/02bbbbbb">`, `<span class="title">Budget old</span>`,
		`<span class="count">1 of 2</span>`)
}

func TestListingCountIsPlainWithoutAQuery(t *testing.T) {
	got := ListingPane(listingOf(someRows, "", Order{}))
	omits(t, got, " of 3")
	got = ListingPane(listingOf(someRows, "a", Order{}))
	shows(t, got, `<span class="count">2 of 3</span>`)
}

// A run is a heading over the rows that follow it. Only the two date orders cut
// by month, and a pinned note is never filed under a month.
func TestListingRuns(t *testing.T) {
	pinned := Row{ID: "p", Title: "P", Stamp: ptr("2024-03-05T10:00:00Z"), Pinned: true, Shown: true}
	march := Row{ID: "m", Title: "M", Stamp: ptr("2024-03-01T10:00:00Z"), Shown: true}
	february := Row{ID: "f", Title: "F", Stamp: ptr("2024-02-01T10:00:00+08:00"), Shown: true}
	lateJanuary := Row{ID: "j", Title: "J", Stamp: ptr("2024-01-31T23:30:00Z"), Shown: true}
	bare := Row{ID: "b", Title: "B", Stamp: ptr("2023-12-05"), Shown: true}
	undated := Row{ID: "u", Title: "U", Shown: true}
	forward := []Row{pinned, march, february, lateJanuary, bare, undated}
	backward := []Row{undated, bare, lateJanuary, february, march, pinned}
	east := time.FixedZone("east", 8*3600)
	byName := func(name string) cmd.Sort { sort, _ := cmd.SortNamed(name); return sort }

	for _, c := range []struct {
		name     string
		rows     []Row
		order    Order
		zone     *time.Location
		headings []string
	}{
		{"the default order has one run besides the pinned", forward, Order{}, nil,
			[]string{"Pinned", "Notes"}},
		{"title is no date either", forward, Order{Sort: byName("title")}, nil,
			[]string{"Pinned", "Notes"}},
		{"updated cuts by month", forward, Order{Sort: byName("updated")}, nil,
			[]string{"Pinned", "March 2024", "February 2024", "January 2024", "December 2023", "Undated"}},
		{"created cuts by month", forward, Order{Sort: byName("created")}, nil,
			[]string{"Pinned", "March 2024", "February 2024", "January 2024", "December 2023", "Undated"}},
		{"a reversed order keeps the runs, with the pinned last", backward, Order{Sort: byName("updated"), Reversed: true}, nil,
			[]string{"Undated", "December 2023", "January 2024", "February 2024", "March 2024", "Pinned"}},
		{"the server's zone decides the month", forward, Order{Sort: byName("updated")}, east,
			[]string{"Pinned", "March 2024", "February 2024", "December 2023", "Undated"}},
	} {
		t.Run(c.name, func(t *testing.T) {
			list := listingOf(c.rows, "", c.order)
			list.Zone = c.zone
			got := ListingPane(list)
			var seen []string
			for _, part := range strings.Split(got, `<section class="run"><h2>`)[1:] {
				seen = append(seen, part[:strings.Index(part, "</h2>")])
			}
			if strings.Join(seen, "|") != strings.Join(c.headings, "|") {
				t.Errorf("runs %v, want %v", seen, c.headings)
			}
		})
	}
}

func TestListingRunHoldsItsRows(t *testing.T) {
	rows := []Row{
		{ID: "01aaaaaa", Title: "A", Stamp: ptr("2024-03-02T00:00:00Z"), Shown: true},
		{ID: "02bbbbbb", Title: "B", Stamp: ptr("2024-03-01T00:00:00Z"), Shown: true},
		{ID: "03cccccc", Title: "C", Stamp: ptr("2024-02-01T00:00:00Z"), Shown: true},
	}
	sort, _ := cmd.SortNamed("updated")
	got := ListingPane(listingOf(rows, "", Order{Sort: sort}))
	inOrder(t, got, `<h2>March 2024</h2>`, `>A</span>`, `>B</span>`, `</section>`, `<h2>February 2024</h2>`, `>C</span>`, `</section><div class="empty" hidden>`)
}

func TestMonthOf(t *testing.T) {
	west := time.FixedZone("west", -5*3600)
	east := time.FixedZone("east", 9*3600)
	for _, c := range []struct {
		name  string
		stamp *string
		zone  *time.Location
		want  string
	}{
		{"no stamp is undated", nil, nil, "Undated"},
		{"utc when no zone is given", ptr("2024-03-31T23:30:00Z"), nil, "March 2024"},
		{"the zone moves a stamp into the next month", ptr("2024-03-31T23:30:00Z"), east, "April 2024"},
		{"the zone moves a stamp into the last month", ptr("2024-04-01T02:00:00Z"), west, "March 2024"},
		{"the stamp's own offset is converted", ptr("2024-04-01T00:30:00+09:00"), nil, "March 2024"},
		{"a bare date names no zone to convert from", ptr("2024-04-01"), west, "April 2024"},
		{"a date with a time but no offset keeps its date", ptr("2024-04-01T00:30:00"), west, "April 2024"},
		{"something that is no date is undated", ptr("soon"), nil, "Undated"},
		{"a long non-date is undated", ptr("not a date at all"), nil, "Undated"},
		{"the year is part of the heading", ptr("2023-12-31T12:00:00Z"), nil, "December 2023"},
	} {
		t.Run(c.name, func(t *testing.T) {
			if got := monthOf(c.stamp, c.zone); got != c.want {
				t.Errorf("monthOf = %q, want %q", got, c.want)
			}
		})
	}
}

func TestListingEmptySentence(t *testing.T) {
	for _, c := range []struct {
		name  string
		rows  []Row
		typed string
		want  []string
		not   []string
	}{
		{"an empty notebook says how to start", nil, "",
			[]string{`<div class="empty"><b>No notes yet</b>Run <code>noda add "First note"</code>`, `<span class="count">0</span>`},
			[]string{"No notes match", `class="run"`}},
		{"nothing matched shows the sentence", []Row{{ID: "a", Title: "t"}, {ID: "b", Title: "u"}}, evil,
			[]string{`<div class="empty"><b>No notes match <span class="asked">` + evilEscaped + `</span></b>`,
				"This notebook holds 2 notes. <a href=\"/nb/work\">Clear the search</a>"},
			[]string{`class="empty" hidden`}},
		{"a single note is singular", []Row{{ID: "a", Title: "t"}}, "x",
			[]string{"This notebook holds 1 note. "}, nil},
		{"some matches hide the sentence", someRows, "a",
			[]string{`<div class="empty" hidden><b>No notes match`}, []string{`<div class="empty"><b>`}},
	} {
		t.Run(c.name, func(t *testing.T) {
			got := ListingPane(listingOf(c.rows, c.typed, Order{}))
			shows(t, got, c.want...)
			omits(t, got, c.not...)
		})
	}
}

func TestListingSearchForm(t *testing.T) {
	list := listingOf(nil, "budget tag:work", Order{})
	list.Asked.Problem = ptr("`OR` needs a term on both sides")
	list.Drift = "in sync"
	got := ListingPane(list)
	shows(t, got,
		`<form class="searchbar" method="get" action="/nb/work">`,
		`<input type="search" name="q" value="budget tag:work" `,
		`aria-label="Search this notebook" data-key="/">`,
		`<p class="hint" hidden data-aside="Not filtered yet — press ⏎ to search.">`,
		`<p class="problem">`+"`OR` needs a term on both sides"+`</p>`,
		`<a class="drift" href="/nb/work/status" aria-label="Status: in sync" data-key="gs" data-does="Status">`,
		`<a class="back" href="/" aria-label="Back to Notebooks" data-key="gh" data-does="Notebooks">`,
		`<h1 class="place"><span class="nb">work</span><span class="sec">Notes</span></h1>`)
	inOrder(t, got, `<input type="search"`, `<p class="hint"`, `<div class="parse"`, `<p class="problem">`, `<nav class="sortbar"`)
}

func TestListingGrouping(t *testing.T) {
	for _, c := range []struct {
		name   string
		groups [][]string
		want   string
	}{
		{"no grouping is hidden", nil, `<div class="parse" hidden></div>`},
		{"one term", [][]string{{"budget"}}, `<div class="parse"><span class="g"><b>budget</b></span></div>`},
		{"groups join with and, terms with or",
			[][]string{{"a", "b"}, {"c"}},
			`<div class="parse"><span class="g"><b>a</b><i>or</i><b>b</b></span><span class="and">and</span><span class="g"><b>c</b></span></div>`},
		{"tag terms are coloured", [][]string{{"tag:work", "-tag:old", "tagged"}},
			`<b class="t">tag:work</b><i>or</i><b class="t">-tag:old</b><i>or</i><b>tagged</b>`},
		{"terms are escaped", [][]string{{evil}}, `<b>` + evilEscaped + `</b>`},
	} {
		t.Run(c.name, func(t *testing.T) {
			list := listingOf(nil, "q", Order{})
			list.Asked.Grouping = c.groups
			shows(t, ListingPane(list), c.want)
		})
	}
}

func TestListingOrder(t *testing.T) {
	const down = `<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 5v14"/>`
	const up = `<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 19V5"/>`
	const reverse, next = ` data-key="O" data-does="Reverse the order">`, ` data-key="o" data-does="Next order">`
	for _, c := range []struct {
		name  string
		sort  string
		rev   bool
		typed string
		want  []string
		not   []string
	}{
		{"the default order is written nowhere", "slug", false, "",
			[]string{`<nav class="sortbar" aria-label="Order"><span class="lab">Order</span>`,
				`<a href="/nb/work?r=1" aria-current="true" aria-label="Ordered by slug. Press to reverse."` + reverse + `slug` + down,
				`<a href="/nb/work?sort=created" aria-label="Order by created"` + next + `created</a>`,
				`<a href="/nb/work?sort=updated" aria-label="Order by updated">updated</a>`,
				`<a href="/nb/work?sort=title" aria-label="Order by title">title</a>`},
			[]string{`type="hidden"`, "Press to undo", up}},
		{"a reversed default order carries r", "slug", true, "",
			[]string{`<input type="hidden" name="r" value="1">`,
				`<a href="/nb/work" aria-current="true" aria-label="Ordered by slug, reversed. Press to undo."` + reverse + `slug` + up},
			[]string{`name="sort"`, down}},
		{"a chosen order rides in the form", "updated", false, "",
			[]string{`<input type="hidden" name="sort" value="updated">`,
				`<a href="/nb/work?sort=updated&amp;r=1" aria-current="true" aria-label="Ordered by updated. Press to reverse."` + reverse + `updated` + down,
				`<a href="/nb/work?sort=title" aria-label="Order by title"` + next + `title</a>`,
				`<a href="/nb/work" aria-label="Order by slug">slug</a>`},
			[]string{`name="r"`}},
		{"the last order is followed by the first", "title", false, "",
			[]string{`<a href="/nb/work" aria-label="Order by slug"` + next + `slug</a>`}, nil},
		{"the query is kept and encoded in every link", "created", true, "budget tag:work",
			[]string{`<input type="hidden" name="sort" value="created"><input type="hidden" name="r" value="1">`,
				`<a href="/nb/work?q=budget%20tag%3Awork&amp;sort=created" aria-current="true" aria-label="Ordered by created, reversed. Press to undo."`,
				`<a href="/nb/work?q=budget%20tag%3Awork" aria-label="Order by slug">slug</a>`},
			nil},
	} {
		t.Run(c.name, func(t *testing.T) {
			sort, _ := cmd.SortNamed(c.sort)
			got := ListingPane(listingOf(nil, c.typed, Order{Sort: sort, Reversed: c.rev}))
			shows(t, got, c.want...)
			omits(t, got, c.not...)
			if n := strings.Count(got, `aria-current="true"`); n != 1 {
				t.Errorf("%d current orders, want 1", n)
			}
			// One key for the order in force and one for the next, never two of
			// either, or the script would press the wrong chip.
			if n := strings.Count(got, ` data-key="O"`); n != 1 {
				t.Errorf("%d reverse keys, want 1", n)
			}
			if n := strings.Count(got, ` data-key="o"`); n != 1 {
				t.Errorf("%d next-order keys, want 1", n)
			}
		})
	}
}

func TestListingPageAndScreen(t *testing.T) {
	list := listingOf(someRows, "", Order{})
	list.Drift = "in sync"
	page := ListingPage(list, nil)
	shows(t, page, "<!doctype html>", "<title>work — noda</title>", `class="app split at-list indexed"`,
		`<b>Pick a note</b>`, `<div class="foot rail">`,
		`<a href="/nb/work" aria-current="page" data-key="gn"><svg viewBox="0 0 24 24" aria-hidden="true">`,
		AssetListing.Href(), AssetPanes.Href(), AssetStamps.Href(), AssetSending.Href(), AssetKeys.Href())
	omits(t, page, AssetStanding.Href(), AssetWatching.Href())
	screen := ListingScreen(list, nil)
	if !strings.HasPrefix(screen, "<title>work — noda</title><section class=\"pane index\">") {
		t.Errorf("a screen leads with the title and the index pane, got %.80q", screen)
	}
	omits(t, screen, "<!doctype html>", `class="foot rail"`)
	shows(t, screen, `<b>Pick a note</b>`)
}

func TestListingFrontPane(t *testing.T) {
	list := listingOf(nil, "", Order{})
	front := "<h1>Welcome</h1>"
	for name, got := range map[string]string{"page": ListingPage(list, &front), "screen": ListingScreen(list, &front)} {
		t.Run(name, func(t *testing.T) {
			shows(t, got, `<div class="filename">README.md</div>`, `<div class="body"><h1>Welcome</h1></div>`,
				`<main class="note h7">`)
			omits(t, got, "Pick a note")
		})
	}
}

func TestListingPanePinsComeInGivenOrderAndPaneIsIndexOnly(t *testing.T) {
	got := ListingPane(listingOf(someRows, "", Order{}))
	omits(t, got, "Pick a note", "<title>")
}

func TestRail(t *testing.T) {
	for _, c := range []struct {
		name    string
		here    screen
		current string
	}{
		{"notes", atNotes, `<a href="/nb/work" aria-current="page" data-key="gn">`},
		{"tags", atTags, `<a href="/nb/work/tags" aria-current="page" data-key="gt">`},
		{"todo", atTodo, `<a href="/nb/work/todo" aria-current="page" data-key="gd">`},
		{"files", atFiles, `<a href="/nb/work/files" aria-current="page" data-key="gf">`},
		{"status", atStatus, `<a class="sync" href="/nb/work/status" aria-current="page" aria-label="Status: 1 to pull" data-key="gs" data-does="Status">`},
	} {
		t.Run(c.name, func(t *testing.T) {
			got := rail("work", "1 to pull", c.here)
			shows(t, got, c.current)
			if n := strings.Count(got, `aria-current="page"`); n != 1 {
				t.Errorf("%d current marks, want 1", n)
			}
		})
	}
}

func TestRailContents(t *testing.T) {
	got := rail("work", "1 to pull", atNotes)
	shows(t, got,
		`<div class="foot rail">`,
		`<a class="book h7" href="/" aria-label="All notebooks" data-key="gh" data-does="Notebooks">`,
		`<span class="name">work</span><span class="all">All notebooks</span>`,
		`<a class="fab" href="/nb/work/new" data-key="n">`,
		`<nav class="actionbar" aria-label="Notebook">`,
		`<span>Notes</span>`, `<span>Tags</span>`, `<span>Todo</span>`, `<span>Files</span>`,
		`<span>1 to pull</span>`,
		`<span class="ver" title="noda `+escape(version.String())+`">`+escape(version.String())+`</span>`)
	inOrder(t, got, `class="book`, `class="fab"`, `class="actionbar"`, `class="sync"`, `class="ver"`)
}

func TestRailEscapes(t *testing.T) {
	got := rail(evil, evil, atNotes)
	omits(t, got, "<script>")
	shows(t, got, `<span class="name">`+evilEscaped+`</span>`, `Status: `+evilEscaped)
}

// The listing and the note page draw a pill and a rail that both name the
// notebook's notebook and status, so two keys repeat: the script presses the
// visible one. Nothing else may repeat, or a key would press whichever control
// came first.
func TestEveryPageWritesEachKeyOnce(t *testing.T) {
	about := About{ID: "01abc234", Slug: "budget-review", Title: "Budget review"}
	reading := aReading()
	reading.LinkedFrom = someRows[:2]
	pages := map[string]string{
		"listing":    ListingPage(listingOf(someRows, "", Order{}), nil),
		"note":       NotePage("work", reading, "in sync"),
		"files":      Files("work", nil, "in sync"),
		"tags":       Tags("work", nil, "in sync"),
		"todo":       Todo("work", nil, "in sync"),
		"backlinks":  Backlinks("work", Subject{What: "x", At: "/nb/work/n/x"}, nil, "in sync"),
		"status":     StandingPage("work", aStanding, nil),
		"notebooks":  Notebooks([]Book{{Name: "work"}}),
		"composing":  Composing("work", Draft{}, nil),
		"editing":    Editing(Edit{Book: "work", About: about}),
		"renaming":   Renaming("work", about, "t", nil),
		"tagging":    Tagging("work", about, []string{"a"}, nil),
		"deleting":   Deleting("work", about),
		"failure":    Failure("h", "d"),
		"clash":      Clashed(Clash{Book: "work", About: about}),
		"conflicted": Conflicted("work", about, "m", "n"),
	}
	repeated := map[string]bool{"gh": true, "gs": true}
	keys := regexp.MustCompile(` data-key="([^"]+)"`)
	for name, page := range pages {
		t.Run(name, func(t *testing.T) {
			seen := map[string]int{}
			for _, found := range keys.FindAllStringSubmatch(page, -1) {
				seen[found[1]]++
			}
			for key, n := range seen {
				if n > 1 && !repeated[key] {
					t.Errorf("key %q is written %d times", key, n)
				}
				if n > 2 {
					t.Errorf("key %q is written %d times, at most the pane and the rail", key, n)
				}
			}
		})
	}
}

func TestFiles(t *testing.T) {
	held := []Held{
		{Name: "diagram.png", Size: 12345, Kind: "image/png", Used: 2},
		{Name: "doc.pdf", Size: 0, Kind: "application/pdf"},
		{Name: evil, Size: 1048576, Kind: "text/plain", Used: 1},
		{Name: "big.bin", Size: 5368709120, Kind: "application/octet-stream"},
		{Name: "edge", Size: 1023, Kind: "a"},
		{Name: "edge2", Size: 1024, Kind: "a"},
		{Name: "edge3", Size: 1048575, Kind: "a"},
		{Name: "edge4", Size: 1073741823, Kind: "a"},
		{Name: "edge5", Size: 1073741824, Kind: "a"},
	}
	got := Files("work", held, "in sync")
	shows(t, got, "<title>Files — work — noda</title>", `<main class="rows files">`, `<span class="count">9</span>`,
		`<h1 class="place">Files</h1>`,
		`<a class="back" href="/nb/work" aria-label="Back to work" data-key="Escape">`,
		`<span class="title mono">diagram.png</span>`,
		`<span class="under"><span>12.1 KB</span><span>image/png</span></span>`,
		`<span>0 B</span><span>application/pdf</span>`,
		`<span>1.0 MB</span><span>text/plain</span>`, `<span>5.0 GB</span><span>application/octet-stream</span>`,
		`1023 B`, `1.0 KB`, `1024.0 KB`, `1024.0 MB`, `1.0 GB`,
		`<a class="aside" href="/nb/work/f/diagram.png/backlinks">in 2 notes</a>`,
		`<a class="aside" href="/nb/work/f/`+evilEscaped+`/backlinks">in 1 note</a>`,
		`<span class="aside lone">nothing links to it</span>`,
		`<div class="row split"><a class="most" href="/nb/work/f/diagram.png">`,
		`<a href="/nb/work/files" aria-current="page" data-key="gf">`, `<span>in sync</span>`)
	omits(t, got, "<script>", "No files yet")
}

func TestFilesNone(t *testing.T) {
	got := Files(evil, nil, "in sync")
	shows(t, got, `<main class="rows files"><div class="empty"><b>No files yet</b>Run <code>noda file add diagram.png</code>`,
		`<span class="count">0</span>`, "<title>Files — "+evilEscaped+" — noda</title>")
	omits(t, got, "<script>")
}

func TestTags(t *testing.T) {
	got := Tags("work", []Tally{{"work", 5}, {"has space", 1}, {evil, 2}, {"日本語", 3}}, "in sync")
	shows(t, got, "<title>Tags — work — noda</title>", `<main class="rows tallies">`, `<span class="count">4</span>`,
		`<a class="row tally" href="/nb/work?q=tag%3Awork">`,
		`<a class="row tally" href="/nb/work?q=tag%3A%22has%20space%22">`,
		`<span class="name">has space</span>`,
		`<span class="n">5<span class="sr"> notes</span></span>`, `<span class="n">1<span class="sr"> note</span></span>`,
		`<span class="name">`+evilEscaped+`</span>`,
		`?q=tag%3A%E6%97%A5%E6%9C%AC%E8%AA%9E"`,
		`<a href="/nb/work/tags" aria-current="page" data-key="gt">`)
	omits(t, got, "<script>", "No tags yet")
	inOrder(t, got, ">work<", ">has space<", ">日本語<")
}

// The bar is a class in steps of a tenth of the commonest tag, rounded half up
// and never below one, so a single note still draws a bar and the page needs
// no inline style.
func TestTagBarsAreStepsOfTheCommonest(t *testing.T) {
	for _, c := range []struct {
		notes, most int
		want        string
	}{
		{5, 5, "w10"}, {1, 5, "w2"}, {2, 5, "w4"}, {3, 5, "w6"},
		{1, 4, "w3"}, {1, 100, "w1"}, {1, 1, "w10"}, {0, 7, "w1"},
		{14, 28, "w5"}, {13, 28, "w5"}, {29, 29, "w10"}, {7, 8, "w9"},
	} {
		got := tagRows("work", []Tally{{"a", c.notes}, {"b", c.most}})
		bar := strings.SplitN(got, `class="bar `, 2)[1]
		if !strings.HasPrefix(bar, c.want+`"`) {
			t.Errorf("%d of %d: bar %.6q, want %s", c.notes, c.most, bar, c.want)
		}
	}
}

func TestTagsNone(t *testing.T) {
	got := Tags("work", nil, "in sync")
	shows(t, got, `<main class="rows tallies"><div class="empty"><b>No tags yet</b>`, `<span class="count">0</span>`)
}

func TestTodo(t *testing.T) {
	tasks := []Task{
		{ID: "01abc234", Title: "Budget review", Text: "pay the bill", Due: ptr("2024-01-02"), Overdue: true},
		{ID: "02def567", Title: "Meeting notes", Text: "call back"},
		{ID: "03ghj890", Title: evil, Text: evil, Due: ptr("2024-12-31")},
	}
	got := Todo("work", tasks, "in sync")
	shows(t, got, "<title>Todo — work — noda</title>", `<span class="count">3</span>`, `<main class="rows tasks">`,
		`<section class="run late"><h2>Overdue <span>1</span></h2>`,
		`<section class="run"><h2>Dated <span>1</span></h2>`,
		`<section class="run"><h2>Whenever <span>1</span></h2>`,
		`<a class="row task h10" href="/nb/work/n/01abc234"><span class="due">2024-01-02</span>`+
			`<span class="title">pay the bill</span><span class="in">Budget review</span></a>`,
		`<span class="due"></span><span class="title">call back</span><span class="in">Meeting notes</span>`,
		`<span class="due">2024-12-31</span><span class="title">`+evilEscaped+`</span><span class="in">`+evilEscaped+`</span>`,
		`<a href="/nb/work/todo" aria-current="page" data-key="gd">`)
	omits(t, got, "<script>", "Nothing to do")
	inOrder(t, got, "Overdue", "Dated", "Whenever")
	if n := strings.Count(got, `class="run late"`); n != 1 {
		t.Errorf("%d late runs, want 1", n)
	}
}

func TestTodoRunsAreCutByWhatTheDateSays(t *testing.T) {
	// The input order is the CLI's, soonest first, so a run keeps it.
	tasks := []Task{
		{ID: "a", Text: "late one", Due: ptr("2024-01-01"), Overdue: true},
		{ID: "b", Text: "late two", Due: ptr("2024-01-05"), Overdue: true},
		{ID: "c", Text: "dated", Due: ptr("2099-01-01")},
		{ID: "d", Text: "whenever"},
	}
	got := taskRuns("work", tasks)
	inOrder(t, got, "Overdue <span>2</span>", "late one", "late two", "Dated <span>1</span>", "dated", "Whenever <span>1</span>", "whenever")
	only := taskRuns("work", tasks[2:3])
	omits(t, only, "Overdue", "Whenever", `class="run late"`)
	shows(t, only, "Dated")
}

func TestTodoNone(t *testing.T) {
	shows(t, Todo("work", nil, "in sync"), `<main class="rows tasks"><div class="empty"><b>Nothing to do</b>`, `<span class="count">0</span>`)
}

func TestBacklinks(t *testing.T) {
	for _, c := range []struct {
		name    string
		subject Subject
		spanned string
		label   string
		current string
	}{
		{"a note", Subject{What: "Budget review", At: "/nb/work/n/01abc234"}, `<span class="subject">Budget review</span>`,
			"Back to Budget review", `<a href="/nb/work" aria-current="page" data-key="gn">`},
		{"a file is monospace and goes back to the files", Subject{What: "diagram.png", At: "/nb/work/files", Mono: true},
			`<span class="mono">diagram.png</span>`, "Back to Files", `<a href="/nb/work/files" aria-current="page" data-key="gf">`},
		{"the subject is escaped", Subject{What: evil, At: "/nb/work/n/x"}, `<span class="subject">` + evilEscaped + `</span>`,
			"Back to " + evilEscaped, `aria-current="page" data-key="gn"`},
	} {
		t.Run(c.name, func(t *testing.T) {
			page := Backlinks("work", c.subject, someRows[:2], "in sync")
			shows(t, page, c.spanned, `<p class="said">Notes that link to `,
				`<a class="row h5" href="/nb/work/n/02def567"><span class="title">Meeting notes</span>`+
					`<span class="under"><span class="tags"><span>work</span><span>q3</span></span></span><span class="id">02def567</span></a>`,
				`<a class="back" href="`+escape(c.subject.At)+`" aria-label="`+c.label+`" data-key="Escape">`,
				`<h1 class="place">Linked from</h1><span class="count">2</span>`, `<main class="rows">`, c.current)
			omits(t, page, "Nothing links here", "<script>")
			inOrder(t, page, "Budget review", "Meeting notes")
		})
	}
}

func TestBacklinksNone(t *testing.T) {
	subject := Subject{What: evil, At: "/nb/work/n/x"}
	page := Backlinks(evil, subject, nil, "in sync")
	shows(t, page, `<div class="empty"><b>Nothing links here</b>No note in `+evilEscaped+` points at `+evilEscaped+`.</div>`,
		`<span class="count">0</span>`, "<title>Links to "+evilEscaped+" — noda</title>")
}

func aReading() Reading {
	return Reading{
		ID: "01abc234", Slug: "budget-review", Title: "Budget review", Tags: []string{"work", "q3"},
		Created: ptr("2024-01-01T00:00:00Z"), Updated: ptr("2024-03-01T10:00:00+08:00"),
		Rendered: "<p>the q3 budget is late</p>\n",
	}
}

func TestNote(t *testing.T) {
	for _, c := range []struct {
		name   string
		change func(*Reading)
		want   []string
		not    []string
	}{
		{"a plain note", func(*Reading) {},
			[]string{`<h1>Budget review</h1>`, `<main class="note h10">`,
				`<div class="filename"><span class="id">01abc234</span>-budget-review.md</div>`,
				`<div class="body"><p>the q3 budget is late</p>` + "\n" + `</div>`,
				`<dd class="chips"><a class="tag" href="/nb/work?q=tag%3Awork">work</a><a class="tag" href="/nb/work?q=tag%3Aq3">q3</a></dd>`,
				`<div><dt>Updated</dt><dd><time datetime="2024-03-01T10:00:00+08:00" data-clock>2024-03-01T10:00:00+08:00</time></dd></div>`,
				`<div><dt>Created</dt><dd><time datetime="2024-01-01T00:00:00Z" data-clock>2024-01-01T00:00:00Z</time></dd></div>`,
				`<span>Pin</span>`, `action="/nb/work/n/01abc234/pin"`, `id="pin" hidden></form>`, `data-then="Unpin"`,
				`<a href="/nb/work/n/01abc234/edit" data-key="e">`, `<a href="/nb/work/n/01abc234/tags" data-key="t">`,
				`<a href="/nb/work/n/01abc234/rename" data-key="r">`,
				`<a href="/nb/work/n/01abc234/delete" class="danger" data-key="D">`,
				`<nav class="actionbar" aria-label="This note">`,
				`<a class="back" href="/nb/work" aria-label="Back to work" data-key="Escape">`,
				`<span class="place">Budget review</span>`},
			[]string{"Unpin</span>", "/unpin", "/backlinks", ">Links<"}},
		{"a pinned note offers Unpin", func(r *Reading) { r.Pinned = true },
			[]string{`<span>Unpin</span>`, `action="/nb/work/n/01abc234/unpin"`, `data-then="Pin"`,
				`-budget-review.md<span class="pin">pinned</span></div>`},
			[]string{"<span>Pin</span>"}},
		{"no tags says none", func(r *Reading) { r.Tags = nil },
			[]string{`<dd class="chips"><span class="none">none</span></dd>`}, []string{`class="tag"`}},
		{"no stamps leaves them out", func(r *Reading) { r.Created, r.Updated = nil, nil },
			[]string{`<dt>Tags</dt>`}, []string{"data-clock", "<dt>Created", "<dt>Updated"}},
		{"only one stamp", func(r *Reading) { r.Created = nil },
			[]string{`<dt>Updated</dt>`}, []string{"<dt>Created"}},
		{"the fields are escaped but the rendered body is not", func(r *Reading) {
			r.Slug, r.Title, r.Tags, r.Rendered = evil, evil, []string{evil}, "<p>x</p>"
		}, []string{`<h1>` + evilEscaped + `</h1>`, `-` + evilEscaped + `.md</div>`,
			`>` + evilEscaped + `</a></dd>`, `<div class="body"><p>x</p></div>`,
			`<title>` + evilEscaped + ` — noda</title>`}, []string{"<script>"}},
	} {
		t.Run(c.name, func(t *testing.T) {
			reading := aReading()
			c.change(&reading)
			page, pane := NotePage("work", reading, "1 to pull"), NotePane("work", reading)
			shows(t, page, c.want...)
			shows(t, pane, c.want...)
			omits(t, page, c.not...)
			omits(t, pane, c.not...)
		})
	}
}

func TestNoteMarginOrder(t *testing.T) {
	got := NotePane("work", aReading())
	inOrder(t, got, `<aside class="margin">`, `<nav class="actionbar"`, `<dl class="about">`, `<dt>Tags</dt>`, `<dt>Updated</dt>`,
		`<dt>Created</dt>`, `</dl>`, `<section class="beside">`, `</aside>`)
}

// The notes that link here arrive with the note, in the page and in the swap:
// a margin that waited for a second request would be empty on first sight.
func TestNoteCarriesItsBacklinks(t *testing.T) {
	reading := aReading()
	reading.LinkedFrom = someRows[:2]
	for name, got := range map[string]string{"page": NotePage("work", reading, "in sync"), "pane": NotePane("work", reading)} {
		t.Run(name, func(t *testing.T) {
			shows(t, got, `<section class="beside"><h2>Linked from <span>2</span></h2>`,
				`<a class="mini h10" href="/nb/work/n/01abc234">Budget review</a>`,
				`<a class="mini h5" href="/nb/work/n/02def567">Meeting notes</a>`)
			omits(t, got, "No other note links here.")
		})
	}
}

func TestNoteWithNoBacklinksSaysSo(t *testing.T) {
	got := NotePane("work", aReading())
	shows(t, got, `<section class="beside"><h2>Linked from <span>0</span></h2><p class="none">No other note links here.</p></section>`)
	omits(t, got, `class="mini`)
}

func TestNoteBacklinksAreEscaped(t *testing.T) {
	reading := aReading()
	reading.LinkedFrom = []Row{{ID: "x", Title: evil}}
	got := NotePane("work", reading)
	shows(t, got, `>`+evilEscaped+`</a></section>`)
	omits(t, got, "<script>")
}

func TestNotePageAndPaneDiffer(t *testing.T) {
	page, pane := NotePage("work", aReading(), "1 to pull"), NotePane("work", aReading())
	shows(t, page, "<!doctype html>", `class="app split at-note"`, `<div class="foot rail">`,
		`<a class="drift" href="/nb/work/status" aria-label="Status: 1 to pull" data-key="gs" data-does="Status">`,
		`<section class="pane index">`, AssetPanes.Href(), AssetStamps.Href(), AssetSending.Href(), AssetKeys.Href())
	omits(t, page, "indexed", `class="sortbar"`, `<div class="empty">`, AssetListing.Href())
	shows(t, pane, "<title>Budget review — noda</title><section class=\"pane read\">", `<aside class="margin">`)
	omits(t, pane, "<!doctype html>", `class="drift"`, `<section class="pane index">`, `class="foot rail"`)
}

func TestFormPages(t *testing.T) {
	about := About{ID: "01abc234", Slug: "budget-review", Title: "Budget review"}
	at := "/nb/work/n/01abc234"
	problem := ptr("saved elsewhere")
	for _, c := range []struct {
		name string
		page string
		want []string
		not  []string
	}{
		{"composing", Composing("work", Draft{Title: "T", Tags: "a, b", Body: "text"}, nil),
			[]string{"<title>New note — noda</title>", `<h1 class="place">New note</h1>`, `action="/nb/work/new"`,
				`name="title" value="T"`, `name="tags" value="a, b"`, `<div class="pair">`,
				`<div class="grow"><label for="b">Note</label><textarea id="b" name="body" autofocus>text</textarea></div>`,
				`data-busy="Adding…">Add note</button>`, `<a class="button" href="/nb/work">Cancel</a>`,
				`<a class="back" href="/nb/work" aria-label="Back to work"><svg`,
				`class="app solo"`, `<section class="pane form tall">`, AssetSending.Href(), AssetKeys.Href()},
			[]string{`class="said bad"`, "http-equiv", AssetWatching.Href(), `data-key="Escape"`}},
		{"composing refused keeps the draft and says why", Composing("work", Draft{Title: evil, Tags: evil, Body: evil}, ptr(evil)),
			[]string{`<p class="said bad">` + evilEscaped + `</p>`, `name="title" value="` + evilEscaped + `"`,
				`name="tags" value="` + evilEscaped + `"`, `autofocus>` + evilEscaped + `</textarea>`},
			[]string{"<script>"}},
		{"composing in an odd notebook", Composing(evil, Draft{}, nil),
			[]string{`action="/nb/` + evilEscaped + `/new"`}, []string{"<script>"}},
		{"editing", Editing(Edit{Book: "work", About: about, Body: "body\ntext", Was: "abc123"}),
			[]string{"<title>Edit — noda</title>", `<h1 class="place">Edit</h1>`, `action="` + at + `/edit"`,
				`<input type="hidden" name="fingerprint" value="abc123">`, "autofocus>body\ntext</textarea>",
				`<label for="b">Text of the note — the title and the tags have their own screens</label>`,
				`<a class="back" href="` + at + `" aria-label="Back to Budget review"><svg`,
				`<a class="button" href="` + at + `">Cancel</a>`, `data-busy="Saving…">Save</button>`,
				`<span class="filename"><span class="id">01abc234</span>-budget-review.md</span>`,
				`<section class="pane form tall">`, AssetWatching.Href(), AssetSending.Href()},
			[]string{`class="said bad"`}},
		{"editing refused", Editing(Edit{Book: "work", About: about, Body: evil, Was: evil, Problem: problem}),
			[]string{`<p class="said bad">saved elsewhere</p>`, "autofocus>" + evilEscaped + "</textarea>",
				`name="fingerprint" value="` + evilEscaped + `"`}, []string{"<script>"}},
		{"editing a note with an odd title", Editing(Edit{Book: "work", About: About{ID: "01abc234", Title: evil}}),
			[]string{`aria-label="Back to ` + evilEscaped + `"`, `<span>` + evilEscaped + `</span>`}, []string{"<script>"}},
		{"clashed shows theirs read-only and mine editable",
			Clashed(Clash{Book: "work", About: about, Theirs: "theirs " + evil, Mine: "mine", Now: "def456"}),
			[]string{`<b>This note changed while you were writing.</b>`, `name="fingerprint" value="def456"`,
				`<div class="versus grow"><div><label>Saved now</label><pre class="theirs">theirs ` + evilEscaped + `</pre></div>`,
				`<label for="b">What you wrote</label>`, `autofocus>mine</textarea>`, AssetWatching.Href(),
				`action="` + at + `/edit"`, `<h1 class="place">Edit</h1>`}, []string{"<script>"}},
		{"conflicted keeps the markers in one field",
			Conflicted("work", about, "<<<<<<< what you wrote\nmine\n=======\ntheirs\n>>>>>>> saved since\n", "def456"),
			[]string{`<b>Someone else saved while you were writing.</b>`, `name="fingerprint" value="def456"`,
				"autofocus>&lt;&lt;&lt;&lt;&lt;&lt;&lt; what you wrote\nmine\n=======\ntheirs\n&gt;&gt;&gt;&gt;&gt;&gt;&gt; saved since\n</textarea>",
				`<label for="b">Text of the note, with both versions</label>`, AssetWatching.Href()}, []string{"<<<<"}},
		{"renaming", Renaming("work", about, "New title", nil),
			[]string{"<title>Rename — noda</title>", `<h1 class="place">Rename</h1>`, `action="` + at + `/rename"`,
				`name="title" value="New title" autofocus>`, `data-busy="Renaming…">Rename</button>`,
				`<small class="hint">The id <span class="id">01abc234</span> never moves`,
				`aria-label="Back to Budget review"`, `<section class="pane form">`},
			[]string{`class="said bad"`, AssetWatching.Href(), "tall"}},
		{"renaming refused", Renaming("work", about, evil, ptr("empty title")),
			[]string{`<p class="said bad">empty title</p>`, `value="` + evilEscaped + `"`, `aria-label="Back to Budget review"`},
			[]string{"<script>"}},
		{"deleting names the note and the file and offers a way out", Deleting("work", about),
			[]string{"<title>Delete — noda</title>", `<h1 class="place">Delete</h1>`, `<p class="said ask"><b>Delete Budget review?</b></p>`,
				`<p class="doomed filename"><span class="id">01abc234</span>-budget-review.md</p>`,
				`action="` + at + `/delete"`,
				`<a class="button" href="` + at + `" autofocus data-key="Escape">Keep it</a>`,
				`<button class="danger" type="submit" data-busy="Deleting…">Delete</button>`,
				`<code>noda restore</code>`}, nil},
		{"deleting escapes the title", Deleting("work", About{ID: "x", Title: evil}),
			[]string{`<b>Delete ` + evilEscaped + `?</b>`}, []string{"<script>"}},
	} {
		t.Run(c.name, func(t *testing.T) {
			shows(t, c.page, c.want...)
			omits(t, c.page, c.not...)
		})
	}
}

// Keep it comes before Delete, and is the one the page opens on, so a stray
// Enter on arrival is the harmless answer.
func TestDeleteOffersKeepingFirst(t *testing.T) {
	got := Deleting("work", About{ID: "01abc234", Slug: "s", Title: "t"})
	inOrder(t, got, `>Keep it</a>`, `>Delete</button>`)
	if n := strings.Count(got, "autofocus"); n != 1 {
		t.Errorf("%d autofocus, want 1", n)
	}
}

func TestTagging(t *testing.T) {
	about := About{ID: "01abc234", Title: "Budget review"}
	at := "/nb/work/n/01abc234"
	for _, c := range []struct {
		name    string
		tags    []string
		problem *string
		want    []string
		not     []string
	}{
		{"a box per tag, each also sent as saw", []string{"a", "b c"}, nil,
			[]string{`<input type="hidden" name="saw" value="a"><label class="tick" for="t0">` +
				`<input id="t0" type="checkbox" name="keep" value="a" checked><span>a</span></label>`,
				`<input type="hidden" name="saw" value="b c"><label class="tick" for="t1">`,
				`<div><label>On this note — untick one to take it off</label><div class="ticks">`, `action="` + at + `/tags"`,
				`<title>Tags — noda</title>`, `name="add"`, `<a class="button" href="` + at + `">Cancel</a>`,
				`data-busy="Saving…">Save</button>`, `aria-label="Back to Budget review"`},
			[]string{"no tags yet", `class="said bad"`}},
		{"no tags says so", nil, nil, []string{`<p class="said">This note has no tags yet.</p>`}, []string{`class="ticks"`}},
		{"refusal and escaping", []string{evil}, ptr("bad tag"),
			[]string{`<p class="said bad">bad tag</p>`, `value="` + evilEscaped + `"`, `<span>` + evilEscaped + `</span>`},
			[]string{"<script>"}},
	} {
		t.Run(c.name, func(t *testing.T) {
			got := Tagging("work", about, c.tags, c.problem)
			shows(t, got, c.want...)
			omits(t, got, c.not...)
		})
	}
}

var aStanding = Standing{Branch: "main", Notes: 3, Files: 1, Remote: ptr("https://***@example.com/x.git"), Drift: "in sync"}

func TestStandingFacts(t *testing.T) {
	for _, c := range []struct {
		name  string
		stand func(*Standing)
		want  []string
		not   []string
	}{
		{"a clean notebook with a remote", func(*Standing) {},
			[]string{`<div class="lead"><b>in sync</b><span>clean</span></div>`,
				`<div><dt>Branch</dt><dd class="mono">main</dd></div>`, `<div><dt>Holds</dt><dd>3 notes, 1 file</dd></div>`,
				`<div><dt>Remote</dt><dd class="mono">https://***@example.com/x.git</dd></div>`,
				`<div><dt>Version</dt><dd class="mono">` + escape(version.String()) + `</dd></div>`},
			[]string{"Problem", "noda remote set"}},
		{"no remote leads with it and says how to set one", func(s *Standing) { s.Remote, s.Drift = nil, "no remote" },
			[]string{`<div class="lead"><b>No remote</b><span>clean</span></div>`,
				`<dd>none — set one with ` + "`noda remote set &lt;url&gt;`" + `</dd>`},
			[]string{"in sync", `<dd class="mono">none`}},
		{"no files leaves the file count out", func(s *Standing) { s.Files = 0 },
			[]string{`<dd>3 notes</dd>`}, []string{"file</dd>"}},
		{"one uncommitted file", func(s *Standing) { s.Uncommitted = 1 }, []string{"<span>1 file uncommitted</span>"}, nil},
		{"several uncommitted files", func(s *Standing) { s.Uncommitted = 4 }, []string{"<span>4 files uncommitted</span>"}, []string{"clean"}},
		{"the drift leads", func(s *Standing) { s.Drift = "2 to push, 1 to pull" },
			[]string{`<div class="lead"><b>2 to push, 1 to pull</b>`}, nil},
		{"problems get a row each and are escaped", func(s *Standing) { s.Problems = []string{"2 broken links", evil} },
			[]string{`<div><dt>Problem</dt><dd>2 broken links</dd></div>`, evilEscaped},
			[]string{"<script>"}},
		{"the branch is escaped", func(s *Standing) { s.Branch = evil }, []string{evilEscaped}, []string{"<script>"}},
	} {
		t.Run(c.name, func(t *testing.T) {
			standing := aStanding
			c.stand(&standing)
			got := StandingMain("work", standing, nil)
			shows(t, got, c.want...)
			omits(t, got, c.not...)
		})
	}
}

func TestStandingFactsAreInOrder(t *testing.T) {
	standing := aStanding
	standing.Problems = []string{"p"}
	inOrder(t, StandingMain("work", standing, nil), `class="lead"`, `class="abreast"`, ">Branch<", ">Remote<", ">Holds<", ">Problem<", ">Version<")
}

func TestStandingErrand(t *testing.T) {
	for _, c := range []struct {
		name   string
		errand *Errand
		want   []string
		not    []string
	}{
		{"no errand", nil, []string{`<button class="go" type="submit" data-doing="Syncing">Sync</button>`,
			`<button type="submit" data-doing="Pulling">Pull</button>`, `<button type="submit" data-doing="Pushing">Push</button>`,
			`<small>Commit what changed, pull, then push.</small>`, `<small>Bring in what the remote has.</small>`,
			`<small>Send what is committed here.</small>`},
			[]string{"disabled", `class="said`, "http-equiv"}},
		{"running", &Errand{Doing: "Syncing", Done: "Synced", Seconds: 3},
			[]string{`<meta http-equiv="refresh" content="2">`, `<p class="said working"><b>Syncing…</b> 3 seconds</p>`,
				`<button class="go" type="submit" data-doing="Syncing" disabled>Sync</button>`,
				`data-doing="Pulling" disabled>Pull</button>`, `data-doing="Pushing" disabled>Push</button>`},
			[]string{"outcome"}},
		{"running for one second", &Errand{Doing: "Syncing", Seconds: 1}, []string{"1 second</p>"}, []string{"1 seconds"}},
		{"done", &Errand{Doing: "Syncing", Done: "Synced", Said: ptr("sync: ok")},
			[]string{`<p class="said"><b>Synced</b><span class="outcome">sync: ok</span></p>`},
			[]string{"disabled", "http-equiv", " bad"}},
		{"failed and escaped", &Errand{Doing: "Syncing", Done: "Synced", Said: ptr(evil), Failed: true, Seconds: 61},
			[]string{`<p class="said bad"><b>Synced</b><span class="outcome">` + evilEscaped + `</span></p>`},
			[]string{"disabled", "http-equiv", "<script>"}},
	} {
		t.Run(c.name, func(t *testing.T) {
			for _, got := range []string{StandingPage("work", aStanding, c.errand), StandingMain("work", aStanding, c.errand)} {
				shows(t, got, c.want...)
				omits(t, got, c.not...)
				shows(t, got, `<form method="post" action="/nb/work/status/sync">`, `action="/nb/work/status/pull"`, `action="/nb/work/status/push"`)
			}
		})
	}
}

func TestStandingPageAndMain(t *testing.T) {
	page, main := StandingPage("work", aStanding, nil), StandingMain("work", aStanding, nil)
	shows(t, page, "<!doctype html>", "<title>Status — work — noda</title>", AssetStanding.Href(), AssetKeys.Href(),
		`<h1 class="place">Status</h1>`, `<div class="foot rail">`,
		`<a class="back" href="/nb/work" aria-label="Back to work" data-key="Escape">`,
		`<a class="sync" href="/nb/work/status" aria-current="page" aria-label="Status: in sync"`)
	if n := strings.Count(page, `aria-current="page"`); n != 1 {
		t.Errorf("%d current marks on the status page, want 1 (the sync link)", n)
	}
	if !strings.HasPrefix(main, `<main class="status">`) {
		t.Errorf("a calm main starts with <main>, got %.40q", main)
	}
	omits(t, main, "<!doctype html>", `class="foot rail"`)
	// The rail's word for the drift rides with the poll, or it would keep
	// saying what was true before the errand.
	shows(t, main, `</main><a class="sync" href="/nb/work/status" aria-current="page" aria-label="Status: in sync"`)
	running := StandingMain("work", aStanding, &Errand{Doing: "Syncing"})
	if !strings.HasPrefix(running, "<meta http-equiv=\"refresh\" content=\"2\">\n<main class=\"status\">") {
		t.Errorf("a running main leads with the refresh, got %.80q", running)
	}
	shows(t, StandingPage("work", aStanding, &Errand{Doing: "Syncing"}), `<meta http-equiv="refresh" content="2">`)
}

func TestHighlight(t *testing.T) {
	for _, c := range []struct {
		name, text string
		terms      []string
		want       string
	}{
		{"no terms only escapes", "a & b", nil, "a &amp; b"},
		{"a later match keeps the text before it", "the Budget", []string{"budget"}, "the <mark>Budget</mark>"},
		{"every occurrence is marked", "ab x ab", []string{"ab"}, "<mark>ab</mark> x <mark>ab</mark>"},
		{"the earlier of two terms wins", "xx b a", []string{"a", "b"}, "xx <mark>b</mark> <mark>a</mark>"},
		{"the longer of two terms at one place wins", "budgeting", []string{"budget", "bud", "budgeting"}, "<mark>budgeting</mark>"},
		{"the shorter term listed first loses to the longer", "abc", []string{"a", "abc"}, "<mark>abc</mark>"},
		{"a term that is not there is skipped", "abc", []string{"zz", "b"}, "a<mark>b</mark>c"},
		{"an empty term marks nothing", "abc", []string{"", "c"}, "ab<mark>c</mark>"},
		{"only an empty term leaves the text alone", "abc", []string{""}, "abc"},
		{"the match is escaped, not searched escaped", "a&b", []string{"&"}, "a<mark>&amp;</mark>b"},
		{"a case mapping that changes width is left unmarked", "İx", []string{"x"}, "İx"},
	} {
		t.Run(c.name, func(t *testing.T) {
			if got := highlight(c.text, c.terms); got != c.want {
				t.Errorf("got %q, want %q", got, c.want)
			}
		})
	}
}
