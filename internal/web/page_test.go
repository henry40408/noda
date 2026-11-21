package web

import (
	"strings"
	"testing"

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
				`<a class="back" href="/" aria-label="Back to the notebooks">`)
		})
	}
}

func TestPageShell(t *testing.T) {
	page := Failure("x", "y")
	shows(t, page, "<!doctype html>\n<html lang=\"en\">", `<meta name="referrer" content="same-origin">`,
		`<div class="app">`, "</html>\n")
	omits(t, page, "http-equiv=\"refresh\"")
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
			[]string{"<b>No notebooks yet</b>Run <code>noda init</code>", `0 notebooks<span class="more"> · 0 notes</span>`}, nil},
		{"one active notebook with no remote",
			[]Book{{Name: "default", Notes: 1, Active: true, Last: "2024-03-01 10:00"}},
			[]string{`1 notebook<span class="more"> · 1 note</span>`, `<span class="sr">Active — </span>`,
				`<span class="holds">1 note</span>`, `<span class="aside">no remote</span>`,
				`href="/nb/default"`, `<span class="stamp">2024-03-01 10:00</span>`},
			[]string{"holds files", "uncommitted", "/status"}},
		{"files, uncommitted and drift are shown when present, notes are summed",
			[]Book{{Name: "a", Notes: 3}, {Name: "work", Notes: 12, Files: 2, Uncommitted: 1, Drift: &drift}},
			[]string{`2 notebooks<span class="more"> · 15 notes</span>`, `<span class="holds files">2 files</span>`,
				`<span class="holds">1 uncommitted</span>`,
				`<a class="aside" href="/nb/work/status" aria-label="Status: 2 to push, 1 to pull">`},
			[]string{"Active"}},
		{"one file is singular", []Book{{Name: "a", Files: 1}},
			[]string{`<span class="holds files">1 file</span>`}, nil},
		{"the name is escaped", []Book{{Name: evil, Drift: ptr(evil)}},
			[]string{`href="/nb/` + evilEscaped + `"`, `<span class="name">` + evilEscaped + `</span>`,
				"Status: " + evilEscaped}, []string{"<script>"}},
	} {
		t.Run(c.name, func(t *testing.T) {
			got := Notebooks(c.books)
			shows(t, got, c.want...)
			omits(t, got, c.not...)
			shows(t, got, `class="app root"`)
		})
	}
}

func TestNotebooksListsInGivenOrder(t *testing.T) {
	got := Notebooks([]Book{{Name: "first"}, {Name: "second"}})
	inOrder(t, got, `href="/nb/first"`, `href="/nb/second"`)
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
		`<a class="row" href="/nb/work/n/01abc234">`,
		`<span class="id">01abc234</span><span class="day">2024-03-01</span>`,
		`<span class="day">2024-02-01</span>`,
		`<time class="when" datetime="2024-02-01T10:00:00+08:00">2024-02-01</time>`,
		`<span class="tags">work, q3</span>`,
		`<span class="tags">日本語, has space</span>`,
		`<span class="pin">`)
	if n := strings.Count(got, `class="pin"`); n != 1 {
		t.Errorf("%d pin marks, want 1 (only the pinned row)", n)
	}
	omits(t, got, `<p class="problem">`)
}

func TestListingRowsWithoutAStampHaveNoDay(t *testing.T) {
	got := ListingPane(listingOf([]Row{{ID: "01abc234", Title: "t", Shown: true}}, "", Order{}))
	shows(t, got, `<div class="ident"><span class="id">01abc234</span></div>`)
	omits(t, got, `class="day"`, `<time`)
}

func TestListingRowsAreEscaped(t *testing.T) {
	list := listingOf([]Row{{ID: "x", Title: evil, Tags: []string{evil}, Stamp: ptr(evil), Shown: true}}, evil, Order{})
	list.Book = evil
	got := ListingPane(list)
	omits(t, got, "<script>")
	shows(t, got, `<div class="title">`+evilEscaped+`</div>`, `<span class="tags">`+evilEscaped+`</span>`,
		`value="`+evilEscaped+`"`, `<span class="here">`+evilEscaped+`</span>`)
}

func TestListingHiddenRowsKeepAnUnmarkedTitle(t *testing.T) {
	list := Listing{Book: "work", Rows: []Row{
		{ID: "01aaaaaa", Title: "Budget plan", Shown: true},
		{ID: "02bbbbbb", Title: "Budget old", Shown: false},
	}, Asked: Asked{Typed: "budget", Terms: []string{"budget"}}}
	got := ListingPane(list)
	shows(t, got,
		`<a class="row" href="/nb/work/n/01aaaaaa">`, `<mark>Budget</mark> plan`,
		`<a class="row" hidden href="/nb/work/n/02bbbbbb">`, `<div class="title">Budget old</div>`,
		`<span class="count">1 of 2</span>`)
}

func TestListingCountIsPlainWithoutAQuery(t *testing.T) {
	got := ListingPane(listingOf(someRows, "", Order{}))
	omits(t, got, " of 3")
	got = ListingPane(listingOf(someRows, "a", Order{}))
	shows(t, got, `<span class="count">2 of 3</span>`)
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
			[]string{"No notes match"}},
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
		`<p class="hint" hidden>`,
		`<p class="problem">`+"`OR` needs a term on both sides"+`</p>`,
		`<a class="drift" href="/nb/work/status" aria-label="Status: in sync">`)
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
	for _, c := range []struct {
		name   string
		sort   string
		rev    bool
		typed  string
		want   []string
		not    []string
		arrows int
	}{
		{"the default order is written nowhere", "slug", false, "",
			[]string{`<a href="/nb/work?r=1" aria-current="true" aria-label="Ordered by slug. Press to reverse.">`,
				`<a href="/nb/work?sort=created" aria-label="Order by created">`,
				`<a href="/nb/work?sort=updated" aria-label="Order by updated">`,
				`<a href="/nb/work?sort=title" aria-label="Order by title">`},
			[]string{`type="hidden"`, "Press to undo"}, 1},
		{"a reversed default order carries r", "slug", true, "",
			[]string{`<input type="hidden" name="r" value="1">`,
				`<a href="/nb/work" aria-current="true" aria-label="Ordered by slug, reversed. Press to undo.">`},
			[]string{`name="sort"`}, 1},
		{"a chosen order rides in the form", "title", false, "",
			[]string{`<input type="hidden" name="sort" value="title">`,
				`<a href="/nb/work?sort=title&amp;r=1" aria-current="true" aria-label="Ordered by title. Press to reverse.">`,
				`<a href="/nb/work" aria-label="Order by slug">`},
			[]string{`name="r"`}, 1},
		{"the query is kept and encoded in every link", "created", true, "budget tag:work",
			[]string{`<input type="hidden" name="sort" value="created"><input type="hidden" name="r" value="1">`,
				`<a href="/nb/work?q=budget%20tag%3Awork&amp;sort=created" aria-current="true" aria-label="Ordered by created, reversed. Press to undo.">`,
				`<a href="/nb/work?q=budget%20tag%3Awork" aria-label="Order by slug">`},
			nil, 1},
		{"updated", "updated", false, "",
			[]string{`<input type="hidden" name="sort" value="updated">`}, nil, 1},
	} {
		t.Run(c.name, func(t *testing.T) {
			sort, _ := cmd.SortNamed(c.sort)
			got := ListingPane(listingOf(nil, c.typed, Order{Sort: sort, Reversed: c.rev}))
			shows(t, got, c.want...)
			omits(t, got, c.not...)
			if n := strings.Count(got, `aria-current="true"`); n != 1 {
				t.Errorf("%d current orders, want 1", n)
			}
			arrow := `<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 5v14"/>`
			if c.rev {
				arrow = `<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 19V5"/>`
			}
			shows(t, got, arrow)
		})
	}
}

func TestListingPageAndScreen(t *testing.T) {
	list := listingOf(someRows, "", Order{})
	page := ListingPage(list, nil)
	shows(t, page, "<!doctype html>", "<title>work — noda</title>", `class="app split at-list indexed"`,
		`<b>Pick a note</b>`, `<div class="foot">`, `aria-current="page"><svg viewBox="0 0 24 24" aria-hidden="true">`)
	screen := ListingScreen(list, nil)
	if !strings.HasPrefix(screen, "<title>work — noda</title><section class=\"pane index\">") {
		t.Errorf("a screen leads with the title and the index pane, got %.80q", screen)
	}
	omits(t, screen, "<!doctype html>", `<div class="foot">`)
	shows(t, screen, `<b>Pick a note</b>`)
}

func TestListingFrontPane(t *testing.T) {
	list := listingOf(nil, "", Order{})
	front := "<h1>Welcome</h1>"
	for name, got := range map[string]string{"page": ListingPage(list, &front), "screen": ListingScreen(list, &front)} {
		t.Run(name, func(t *testing.T) {
			shows(t, got, `<span class="count mono">README.md</span>`, `<div class="body"><h1>Welcome</h1></div>`,
				`<span class="here lead">work</span>`)
			omits(t, got, "Pick a note")
		})
	}
}

func TestListingPanePinsComeInGivenOrderAndPaneIsIndexOnly(t *testing.T) {
	got := ListingPane(listingOf(someRows, "", Order{}))
	omits(t, got, "Pick a note", "<title>")
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
	got := Files("work", held)
	shows(t, got, "<title>Files — work — noda</title>", `<main class="rows cols wide">`, `<span class="count">9</span>`,
		`<a class="back" href="/nb/work"`,
		`<div class="title mono">diagram.png</div>`,
		`12.1 KB`+separator+`image/png`, `0 B`+separator+`application/pdf`,
		`1.0 MB`+separator+`text/plain`, `5.0 GB`+separator+`application/octet-stream`,
		`1023 B`, `1.0 KB`, `1024.0 KB`, `1024.0 MB`, `1.0 GB`,
		`<a class="aside" href="/nb/work/f/diagram.png/backlinks">in 2 notes</a>`,
		`<a class="aside" href="/nb/work/f/`+evilEscaped+`/backlinks">in 1 note</a>`,
		`<span class="aside">nothing links to it</span>`,
		`<a class="most" href="/nb/work/f/diagram.png">`)
	omits(t, got, "<script>", "No files yet")
	if !strings.Contains(strings.SplitN(got, "Files</span>", 2)[1], `aria-current="page"`) {
		t.Error("the Files item is not marked current")
	}
}

func TestFilesNone(t *testing.T) {
	got := Files(evil, nil)
	shows(t, got, `<main class="rows"><div class="empty"><b>No files yet</b>Run <code>noda file add diagram.png</code>`,
		`<span class="count">0</span>`, "<title>Files — "+evilEscaped+" — noda</title>")
	omits(t, got, "cols", "<script>")
}

func TestTags(t *testing.T) {
	got := Tags("work", []Tally{{"work", 5}, {"has space", 1}, {evil, 2}, {"日本語", 3}})
	shows(t, got, "<title>Tags — work — noda</title>", `<main class="rows cols">`, `<span class="count">4</span>`,
		`<a class="row" href="/nb/work?q=tag%3Awork">`,
		`<a class="row" href="/nb/work?q=tag%3A%22has%20space%22">`,
		`<div class="name tags">has space</div>`,
		`<span class="when">5 notes</span>`, `<span class="when">1 note</span>`,
		`<div class="name tags">`+evilEscaped+`</div>`,
		`?q=tag%3A%E6%97%A5%E6%9C%AC%E8%AA%9E"`)
	omits(t, got, "<script>", "No tags yet")
	inOrder(t, got, ">work<", ">has space<", ">日本語<")
}

func TestTagsNone(t *testing.T) {
	got := Tags("work", nil)
	shows(t, got, `<main class="rows"><div class="empty"><b>No tags yet</b>`, `<span class="count">0</span>`)
	omits(t, got, "cols")
}

func TestTodo(t *testing.T) {
	tasks := []Task{
		{ID: "01abc234", Title: "Budget review", Text: "pay the bill", Due: ptr("2024-01-02"), Overdue: true},
		{ID: "02def567", Title: "Meeting notes", Text: "call back"},
		{ID: "03ghj890", Title: evil, Text: evil, Due: ptr("2024-12-31")},
	}
	got := Todo("work", tasks)
	shows(t, got, "<title>Todo — work — noda</title>", `<span class="count">3</span>`,
		`<a class="row" href="/nb/work/n/01abc234"><div class="title">pay the bill</div>`,
		`<span class="overdue">2024-01-02</span>`+separator+`<span class="in">Budget review</span>`,
		`<div class="under"><span class="in">Meeting notes</span></div>`,
		`<span class="when">2024-12-31</span>`+separator+`<span class="in">`+evilEscaped+`</span>`)
	omits(t, got, "<script>", "Nothing to do")
	if n := strings.Count(got, `class="overdue"`); n != 1 {
		t.Errorf("%d overdue marks, want 1", n)
	}
}

func TestTodoNone(t *testing.T) {
	shows(t, Todo("work", nil), `<main class="rows"><div class="empty"><b>Nothing to do</b>`, `<span class="count">0</span>`)
}

func TestBacklinks(t *testing.T) {
	for _, c := range []struct {
		name    string
		subject Subject
		spanned string
	}{
		{"a note", Subject{What: "Budget review", At: "/nb/work/n/01abc234"}, `<span class="subject">Budget review</span>`},
		{"a file is monospace", Subject{What: "diagram.png", At: "/nb/work/f/diagram.png", Mono: true}, `<span class="mono">diagram.png</span>`},
		{"the subject is escaped", Subject{What: evil, At: "/nb/work/n/x"}, `<span class="subject">` + evilEscaped + `</span>`},
	} {
		t.Run(c.name, func(t *testing.T) {
			rows := BacklinksRows("work", c.subject, someRows[:2])
			shows(t, rows, c.spanned, `<p class="said">What links to `,
				`<a class="row" href="/nb/work/n/02def567"><div class="title">Meeting notes</div><div class="under"><span class="tags">work, q3</span></div></a>`)
			omits(t, rows, "Nothing links here", "<script>")
			inOrder(t, rows, "Budget review", "Meeting notes")

			page := Backlinks("work", c.subject, someRows[:2])
			shows(t, page, `<a class="back" href="`+escape(c.subject.At)+`"`, `<span class="here">Backlinks</span>`,
				`<span class="count">2</span>`, rows)
		})
	}
}

func TestBacklinksNone(t *testing.T) {
	subject := Subject{What: evil, At: "/nb/work/n/x"}
	rows := BacklinksRows(evil, subject, nil)
	shows(t, rows, `<div class="empty"><b>Nothing links here</b>No note in `+evilEscaped+` points at `+evilEscaped+`.</div>`)
	shows(t, Backlinks("work", subject, nil), `<span class="count">0</span>`, "<title>Links to "+evilEscaped+" — noda</title>")
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
			[]string{`<h1>Budget review</h1>`,
				`<span class="id">01abc234</span><span class="slug">-budget-review</span><span class="ext">.md</span>`,
				`<div class="body"><p>the q3 budget is late</p>` + "\n" + `</div>`,
				`<span class="tags">work, q3</span>`,
				`<span class="when">created <time datetime="2024-01-01T00:00:00Z" data-clock>2024-01-01T00:00:00Z</time></span>`,
				`<span class="when">updated <time datetime="2024-03-01T10:00:00+08:00" data-clock>`,
				`<span>Pin</span>`, `action="/nb/work/n/01abc234/pin"`,
				`<a href="/nb/work/n/01abc234/edit">`, `<a href="/nb/work/n/01abc234/backlinks">`,
				`<a href="/nb/work/n/01abc234/delete" class="danger">`,
				`<a class="back" href="/nb/work" aria-label="Back to work">`},
			[]string{"Unpin", "/unpin"}},
		{"a pinned note offers Unpin", func(r *Reading) { r.Pinned = true },
			[]string{`<span>Unpin</span>`, `action="/nb/work/n/01abc234/unpin"`}, []string{"<span>Pin</span>"}},
		{"no tags leaves the tag line out", func(r *Reading) { r.Tags = nil },
			[]string{`<div class="note-meta"><span class="when">created`}, []string{`class="tags"`}},
		{"no stamps leaves them out", func(r *Reading) { r.Created, r.Updated = nil, nil },
			[]string{`<span class="tags">work, q3</span></div>`}, []string{"data-clock", "created", "updated"}},
		{"only one stamp", func(r *Reading) { r.Created = nil },
			[]string{`updated <time`}, []string{"created"}},
		{"the fields are escaped but the rendered body is not", func(r *Reading) {
			r.Slug, r.Title, r.Tags, r.Rendered = evil, evil, []string{evil}, "<p>x</p>"
		}, []string{`<h1>` + evilEscaped + `</h1>`, `<span class="slug">-` + evilEscaped + `</span>`,
			`<span class="tags">` + evilEscaped + `</span>`, `<div class="body"><p>x</p></div>`,
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

func TestNotePageAndPaneDiffer(t *testing.T) {
	page, pane := NotePage("work", aReading(), "1 to pull"), NotePane("work", aReading())
	shows(t, page, "<!doctype html>", `class="app split at-note"`, `<div class="foot">`,
		`<a class="drift" href="/nb/work/status" aria-label="Status: 1 to pull">`,
		`<section class="pane index">`)
	omits(t, page, "indexed", `class="sortbar"`, `<div class="empty">`)
	shows(t, pane, "<title>Budget review — noda</title><section class=\"pane read\">")
	omits(t, pane, "<!doctype html>", `class="drift"`, `<section class="pane index">`, `<div class="foot">`)
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
			[]string{"<title>New note — noda</title>", `action="/nb/work/new"`, `name="title" value="T"`,
				`name="tags" value="a, b"`, `<textarea id="b" name="body" autofocus>text</textarea>`,
				`>Add note</button>`, `<a class="button" href="/nb/work">Cancel</a>`, `<a class="back" href="/nb/work"`},
			[]string{`class="said bad"`, "http-equiv", AssetWatching.Href()}},
		{"composing refused keeps the draft and says why", Composing("work", Draft{Title: evil, Tags: evil, Body: evil}, ptr(evil)),
			[]string{`<p class="said bad">` + evilEscaped + `</p>`, `name="title" value="` + evilEscaped + `"`,
				`name="tags" value="` + evilEscaped + `"`, `autofocus>` + evilEscaped + `</textarea>`},
			[]string{"<script>"}},
		{"composing in an odd notebook", Composing(evil, Draft{}, nil),
			[]string{`action="/nb/` + evilEscaped + `/new"`}, []string{"<script>"}},
		{"editing", Editing(Edit{Book: "work", About: about, Body: "body\ntext", Was: "abc123"}),
			[]string{"<title>Budget review — noda</title>", `action="` + at + `/edit"`,
				`<input type="hidden" name="fingerprint" value="abc123">`, "autofocus>body\ntext</textarea>",
				`<p class="said">Editing the body.`, `<a class="button" href="` + at + `">Cancel</a>`, AssetWatching.Href()},
			[]string{`class="said bad"`}},
		{"editing refused", Editing(Edit{Book: "work", About: about, Body: evil, Was: evil, Problem: problem}),
			[]string{`<p class="said bad">saved elsewhere</p><p class="said">Editing`, "autofocus>" + evilEscaped + "</textarea>",
				`name="fingerprint" value="` + evilEscaped + `"`}, []string{"<script>"}},
		{"editing a note with an odd title", Editing(Edit{Book: "work", About: About{ID: "01abc234", Title: evil}}),
			[]string{"<title>" + evilEscaped + " — noda</title>", `<span class="here">` + evilEscaped + `</span>`}, []string{"<script>"}},
		{"clashed shows theirs read-only and mine editable",
			Clashed(Clash{Book: "work", About: about, Theirs: "theirs " + evil, Mine: "mine", Now: "def456"}),
			[]string{`<b>This note changed while you were writing.</b>`, `name="fingerprint" value="def456"`,
				`<pre class="theirs">theirs ` + evilEscaped + `</pre>`, `autofocus>mine</textarea>`, AssetWatching.Href(),
				`action="` + at + `/edit"`}, []string{"<script>"}},
		{"conflicted keeps the markers in one field",
			Conflicted("work", about, "<<<<<<< what you wrote\nmine\n=======\ntheirs\n>>>>>>> saved since\n", "def456"),
			[]string{`<b>Someone else saved while you were writing.</b>`, `name="fingerprint" value="def456"`,
				"autofocus>&lt;&lt;&lt;&lt;&lt;&lt;&lt; what you wrote\nmine\n=======\ntheirs\n&gt;&gt;&gt;&gt;&gt;&gt;&gt; saved since\n</textarea>",
				AssetWatching.Href()}, []string{"<<<<"}},
		{"renaming", Renaming("work", about, "New title", nil),
			[]string{"<title>Rename — noda</title>", `action="` + at + `/rename"`, `name="title" value="New title" autofocus`,
				`>Rename</button>`, `The id never moves`}, []string{`class="said bad"`, AssetWatching.Href()}},
		{"renaming refused", Renaming("work", about, evil, ptr("empty title")),
			[]string{`<p class="said bad">empty title</p><p class="said">The id never moves`, `value="` + evilEscaped + `"`},
			[]string{"<script>"}},
		{"deleting names the note and offers a way out", Deleting("work", about),
			[]string{"<title>Delete — noda</title>", `<b>Delete Budget review?</b>`, `action="` + at + `/delete"`,
				`<button class="danger" type="submit">Delete</button>`, `<a class="button" href="` + at + `">Keep it</a>`}, nil},
		{"deleting escapes the title", Deleting("work", About{ID: "x", Title: evil}),
			[]string{`<b>Delete ` + evilEscaped + `?</b>`}, []string{"<script>"}},
	} {
		t.Run(c.name, func(t *testing.T) {
			shows(t, c.page, c.want...)
			omits(t, c.page, c.not...)
		})
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
				`<div><label>On this note</label><div class="ticks">`, `action="` + at + `/tags"`,
				`<title>Tags — noda</title>`, `name="add"`, `<a class="button" href="` + at + `">Cancel</a>`},
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
			[]string{`<span class="mono">main</span>`, `<span class="when">3 notes, 1 file</span>`, `<span class="when">clean</span>`,
				`<span class="mono">https://***@example.com/x.git</span>`, `<div class="name">Drift</div><div class="under"><span class="when">in sync</span>`,
				`<span class="mono">` + escape(version.String()) + `</span>`},
			[]string{"Problem", "noda remote set"}},
		{"no remote says how to set one and has no drift", func(s *Standing) { s.Remote = nil },
			[]string{"none — set one with `noda remote set &lt;url&gt;`"}, []string{">Drift<", "in sync"}},
		{"no files leaves the file count out", func(s *Standing) { s.Files = 0 },
			[]string{`3 notes</span>`}, []string{"file"}},
		{"one uncommitted file", func(s *Standing) { s.Uncommitted = 1 }, []string{"1 file uncommitted"}, nil},
		{"several uncommitted files", func(s *Standing) { s.Uncommitted = 4 }, []string{"4 files uncommitted"}, []string{"clean"}},
		{"problems get a row each and are escaped", func(s *Standing) { s.Problems = []string{"2 broken links", evil} },
			[]string{`<div class="name">Problem</div><div class="under"><span class="when">2 broken links</span>`, evilEscaped},
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
	inOrder(t, StandingMain("work", standing, nil), ">Branch<", ">Holds<", ">Changes<", ">Remote<", ">Drift<", ">Problem<", ">Version<")
}

func TestStandingErrand(t *testing.T) {
	for _, c := range []struct {
		name   string
		errand *Errand
		want   []string
		not    []string
	}{
		{"no errand", nil, []string{`<button class="go" type="submit">Sync</button>`},
			[]string{"disabled", `class="said`, "http-equiv"}},
		{"running", &Errand{Doing: "Syncing", Done: "Synced", Seconds: 3},
			[]string{`<meta http-equiv="refresh" content="2">`, `<p class="said working"><b>Syncing…</b> 3 seconds</p>`,
				`<button class="go" type="submit" disabled>Sync</button>`, `type="submit" disabled>Pull</button>`, `type="submit" disabled>Push</button>`},
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
			for name, got := range map[string]string{
				"page": StandingPage("work", aStanding, c.errand), "main": StandingMain("work", aStanding, c.errand),
			} {
				shows(t, got, c.want...)
				omits(t, got, c.not...)
				shows(t, got, `<form method="post" action="/nb/work/status/sync">`, `action="/nb/work/status/pull"`, `action="/nb/work/status/push"`)
				_ = name
			}
		})
	}
}

func TestStandingPageAndMain(t *testing.T) {
	page, main := StandingPage("work", aStanding, nil), StandingMain("work", aStanding, nil)
	shows(t, page, "<!doctype html>", "<title>Status — work — noda</title>", AssetStanding.Href(),
		`<span class="here">Status</span><span class="count">work</span>`, `<div class="foot">`)
	omits(t, page, `aria-current="page"`)
	if !strings.HasPrefix(main, "<main>") {
		t.Errorf("a calm main starts with <main>, got %.40q", main)
	}
	omits(t, main, "<!doctype html>")
	running := StandingMain("work", aStanding, &Errand{Doing: "Syncing"})
	if !strings.HasPrefix(running, "<meta http-equiv=\"refresh\" content=\"2\">\n<main>") {
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
