package web

import (
	"fmt"
	"strconv"
	"strings"
	"time"

	"github.com/henry40408/noda/internal/cmd"
	"github.com/henry40408/noda/internal/query"
	"github.com/henry40408/noda/internal/style"
)

// Notebooks is the one screen not inside a notebook, so it has no rail; root
// stops the layout reserving a column for it.
func Notebooks(books []Book) string {
	var rows strings.Builder
	if len(books) == 0 {
		rows.WriteString("<div class=\"empty\"><b>No notebooks yet</b>Run <code>noda init</code> in a terminal to make the first one.</div>")
	}
	for _, book := range books {
		rows.WriteString(bookRow(book))
	}
	return scripted("noda", "root solo", []Asset{AssetKeys},
		"<section class=\"pane\">"+
			"<header class=\"topbar\"><h1 class=\"place\">Notebooks</h1>"+
			"<span class=\"count\">"+tally(books)+"</span></header>"+
			"<main class=\"rows books\">"+rows.String()+"</main></section>")
}

func tally(books []Book) string {
	notes := 0
	for _, book := range books {
		notes += book.Notes
	}
	return plural(len(books), "notebook") + ", " + plural(notes, "note")
}

// bookRow is two links side by side rather than nested: the row goes to the
// listing, the pill to the network screen. With no remote the pill is plain
// words.
func bookRow(book Book) string {
	at := escape(book.Name)
	facts := "<span>" + plural(book.Notes, "note") + "</span>"
	if book.Files > 0 {
		facts += "<span>" + plural(book.Files, "file") + "</span>"
	}
	if book.Uncommitted > 0 {
		facts += "<span class=\"dirty\">" + strconv.Itoa(book.Uncommitted) + " uncommitted</span>"
	}
	// One more fact about the notebook, so it closes the line of facts as
	// "pinned" closes a note's. Nothing beside the name: every row then starts
	// its name and its facts at the same place.
	if book.Active {
		facts += "<span class=\"mark\">active</span>"
	}

	aside := "<span class=\"aside\">no remote</span>"
	if book.Drift != nil {
		aside = strings.Replace(driftPill(book.Name, *book.Drift, ""), "class=\"drift\"", "class=\"aside drift\"", 1)
	}

	return "<div class=\"row split book " + hueClass(book.Name) + "\">" +
		"<a class=\"most\" href=\"/nb/" + at + "\">" +
		"<span class=\"name\">" + at + "</span>" +
		"<span class=\"under\">" + facts + "</span>" +
		"<span class=\"stamp\">last change " + escape(book.Last) + "</span></a>" + aside + "</div>"
}

// Asked is the line in the search field, and everything the page has to say
// about it. One struct because the four are only ever right together.
type Asked struct {
	// Typed is the line exactly as it arrived, which is what goes back in the
	// field.
	Typed string `json:"typed"`
	// Grouping is empty when the line is not a query yet.
	Grouping [][]string `json:"grouping"`
	// Terms is what is worth marking in a title.
	Terms []string `json:"terms"`
	// Problem is why the line is not a query yet. It is said and the notes are
	// left alone: half a query is how every query looks while being typed.
	Problem *string `json:"problem"`
}

// Order is an order and a reversal applied after it, as `--sort` and `-r` are
// at the prompt, rather than one enum of eight.
type Order struct {
	Sort     cmd.Sort
	Reversed bool
}

// asked is the query string this order is asked for by, q carried through it.
// The default order writes nothing, so the default listing has one address.
func (o Order) asked(typed string) string {
	var parts []string
	if typed != "" {
		parts = append(parts, "q="+encoded(typed))
	}
	if o.Sort != cmd.SortSlug {
		parts = append(parts, "sort="+o.Sort.Name())
	}
	if o.Reversed {
		parts = append(parts, "r=1")
	}
	if len(parts) == 0 {
		return ""
	}
	return "?" + strings.Join(parts, "&")
}

// Listing is what the listing's index pane is drawn from. Drift is the pill
// linking to the network screen. Zone is the zone the month runs are cut in: the
// server's own, as `todo`'s "today" is; nil is UTC.
type Listing struct {
	Book  string
	Rows  []Row
	Asked Asked
	Order Order
	Drift string
	Zone  *time.Location
}

// ListingPage is a notebook's notes, narrowed by whatever was typed. front is
// the front page beside them, when the notebook has one.
func ListingPage(list Listing, front *string) string {
	// "indexed": the rows are in the markup, as the script marks a note route
	// once it has fetched them.
	return scripted(listingTitle(list.Book), "split at-list indexed",
		[]Asset{AssetListing, AssetPanes, AssetStamps, AssetSending, AssetKeys},
		listingPanes(list, front)+rail(list.Book, list.Drift, atNotes))
}

// ListingScreen is both of the listing's panes and its title, for a screen
// going back to it from a note in one round trip. Narrowing a search asks for
// ListingPane alone and leaves the note pane and the tab be.
func ListingScreen(list Listing, front *string) string {
	return titled(listingTitle(list.Book)) + listingPanes(list, front)
}

func listingTitle(book string) string { return book + " — noda" }

func listingPanes(list Listing, front *string) string {
	return ListingPane(list) + frontPane(list.Book, front)
}

// ListingPane is the index pane with rows. NotePage sends it empty, since a
// phone never draws it, and the panes script asks for this where it will be
// drawn.
func ListingPane(list Listing) string {
	total := len(list.Rows)
	shown := 0
	for _, row := range list.Rows {
		if row.Shown {
			shown++
		}
	}
	counted := strconv.Itoa(total)
	if list.Asked.Typed != "" {
		counted = fmt.Sprintf("%d of %d", shown, total)
	}
	return indexPane(indexParts{
		book: list.Book, asked: list.Asked, order: &list.Order,
		counted: counted, drift: list.Drift,
		rows: listingRuns(list) + emptySentence(list.Book, list.Asked, total, shown),
	})
}

// listingRuns cuts the rows into runs with a heading each. A run is a stretch of
// consecutive rows under one heading, so a reversed listing, which puts the
// pinned notes last, still reads right.
func listingRuns(list Listing) string {
	var out strings.Builder
	current := ""
	for _, row := range list.Rows {
		if heading := list.heading(row); heading != current {
			if current != "" {
				out.WriteString("</section>")
			}
			out.WriteString("<section class=\"run\"><h2>" + escape(heading) + "</h2>")
			current = heading
		}
		out.WriteString(listingRow(list.Book, row, list.Asked.Terms))
	}
	if current != "" {
		out.WriteString("</section>")
	}
	return out.String()
}

// heading is the run a row belongs to: the pinned notes, then, when the order
// is a date, a month at a time. An order that is not a date has nothing to cut
// by, and a heading of "Notes" says the list is one run on purpose.
func (l Listing) heading(row Row) string {
	switch {
	case row.Pinned:
		return "Pinned"
	case l.Order.Sort != cmd.SortCreated && l.Order.Sort != cmd.SortUpdated:
		return "Notes"
	}
	return monthOf(row.Stamp, l.Zone)
}

// monthOf names the month a stamp falls in, in zone. The row beside it still
// shows the stamp's own day, so a note near midnight at the end of a month can
// sit under a month its day does not name; converting the day too would put the
// server's zone into the note, which `day` is written to avoid.
func monthOf(stamp *string, zone *time.Location) string {
	const undated = "Undated"
	if stamp == nil {
		return undated
	}
	if zone == nil {
		zone = time.UTC
	}
	if at, err := time.Parse(time.RFC3339, *stamp); err == nil {
		return at.In(zone).Format("January 2006")
	}
	// A stamp written by hand may be a bare date, which names no zone to convert
	// from.
	if len(*stamp) >= len("2006-01-02") {
		if at, err := time.Parse("2006-01-02", (*stamp)[:len("2006-01-02")]); err == nil {
			return at.Format("January 2006")
		}
	}
	return undated
}

// listingRow is one note's row in the index pane.
func listingRow(book string, row Row, terms []string) string {
	// The pin mark goes last, after the tags.
	pin := ""
	if row.Pinned {
		pin = "<span class=\"pin\">" + style.PinMark + "</span>"
	}
	hidden := ""
	if !row.Shown {
		hidden = " hidden"
	}
	// A hidden row's title is unmarked, as the script would leave it.
	title := escape(row.Title)
	if row.Shown {
		title = highlight(row.Title, terms)
	}
	return "<a class=\"row " + hueClass(row.ID) + "\"" + hidden + " href=\"/nb/" + escape(book) + "/n/" + escape(row.ID) + "\">" +
		"<span class=\"title\">" + title + "</span>" +
		"<span class=\"under\">" + when(row.Stamp) + tagLine(row.Tags) + pin + "</span>" +
		"<span class=\"id\">" + escape(row.ID) + "</span></a>"
}

// emptySentence: only the no-match sentence is ever hidden; typing cannot fill
// an empty notebook.
func emptySentence(book string, asked Asked, total, shown int) string {
	if total == 0 {
		return "<div class=\"empty\"><b>No notes yet</b>Run <code>noda add \"First note\"</code> in a terminal to start one.</div>"
	}
	hidden := ""
	if shown > 0 {
		hidden = " hidden"
	}
	return "<div class=\"empty\"" + hidden + "><b>No notes match <span class=\"asked\">" + escape(asked.Typed) + "</span></b>" +
		"This notebook holds " + plural(total, "note") + ". <a href=\"/nb/" + escape(book) + "\">Clear the search</a> to see them.</div>"
}

// indexParts is what indexPane frames. A nil order draws no sort bar.
type indexParts struct {
	book    string
	asked   Asked
	order   *Order
	counted string
	drift   string
	rows    string
}

// indexPane is the frame, the same on both routes, with a GET search form a
// scriptless reader can submit from a note page. A note route sends order, rows
// and counted empty for the panes script to fill.
func indexPane(pane indexParts) string {
	var sortValue, reversedValue *string
	sortbarHTML := ""
	if pane.order != nil {
		// The order rides in the form, which would otherwise submit only
		// ?q=… and drop it. Only a non-default order is written.
		if pane.order.Sort != cmd.SortSlug {
			name := pane.order.Sort.Name()
			sortValue = &name
		}
		if pane.order.Reversed {
			one := "1"
			reversedValue = &one
		}
		// Last, so it does not split the query from the three answers above.
		sortbarHTML = sortbar(pane.book, pane.asked, *pane.order)
	}
	problem := ""
	if pane.asked.Problem != nil {
		problem = "<p class=\"problem\">" + escape(*pane.asked.Problem) + "</p>"
	}
	return "<section class=\"pane index\">" +
		"<header class=\"topbar\">" + back("/", "Notebooks", "gh") +
		"<h1 class=\"place\"><span class=\"nb\">" + escape(pane.book) + "</span><span class=\"sec\">Notes</span></h1>" +
		driftPill(pane.book, pane.drift, keyed("gs", "Status")) +
		"<span class=\"count\">" + pane.counted + "</span></header>" +
		"<form class=\"searchbar\" method=\"get\" action=\"/nb/" + escape(pane.book) + "\">" +
		"<input type=\"search\" name=\"q\" value=\"" + escape(pane.asked.Typed) + "\" " +
		"placeholder=\"Search — try tag:ops budget\" " +
		"autocomplete=\"off\" autocapitalize=\"off\" spellcheck=\"false\" " +
		"enterkeyhint=\"search\" aria-label=\"Search this notebook\"" + keyed("/", "") + ">" +
		hiddenInput("sort", sortValue) + hiddenInput("r", reversedValue) +
		// Written by the server and only unhidden by the script, so the
		// sentence is testable.
		hint() + grouping(pane.asked.Grouping) + problem + sortbarHTML +
		"</form><main class=\"rows\">" + pane.rows + "</main></section>"
}

// hiddenInput with nil writes nothing: a hidden input carrying a default would
// put it in the address on every search.
func hiddenInput(name string, value *string) string {
	if value == nil {
		return ""
	}
	return "<input type=\"hidden\" name=\"" + escape(name) + "\" value=\"" + escape(*value) + "\">"
}

// sortbar is the four orders `--sort` names as four links, no menu, so it works
// without the script. Pressing the order in force toggles `-r`, like a sortable
// heading, and its chip carries the direction arrow. Choosing a different order
// drops the reversal. `O` presses the one in force and `o` the one after it, so
// the keys are written on the chips and move with the order.
func sortbar(book string, asked Asked, order Order) string {
	var out strings.Builder
	out.WriteString("<nav class=\"sortbar\" aria-label=\"Order\"><span class=\"lab\">Order</span>")
	for _, sort := range cmd.AllSorts {
		here := sort == order.Sort
		next := Order{Sort: sort, Reversed: here && !order.Reversed}
		current, label, arrow, key := "", "Order by "+sort.Name(), "", ""
		switch {
		case here:
			current = " aria-current=\"true\""
			label = orderedLabel(sort, order.Reversed)
			arrow = svg(arrowFor(order.Reversed))
			key = keyed("O", "Reverse the order")
		case sort == order.Sort.Next():
			key = keyed("o", "Next order")
		}
		out.WriteString("<a href=\"/nb/" + escape(book) + escape(next.asked(asked.Typed)) + "\"" + current +
			" aria-label=\"" + escape(label) + "\"" + key + ">" + escape(sort.Name()) + arrow + "</a>")
	}
	out.WriteString("</nav>")
	return out.String()
}

// orderedLabel says direction and effect in words: the arrow is aria-hidden.
func orderedLabel(sort cmd.Sort, reversed bool) string {
	reversal, press := "", "reverse"
	if reversed {
		reversal, press = ", reversed", "undo"
	}
	return "Ordered by " + sort.Name() + reversal + ". Press to " + press + "."
}

func arrowFor(reversed bool) string {
	if reversed {
		return iconUpwards
	}
	return iconDownwards
}

// grouping is what was typed, as noda grouped it: each pill a group joined by
// `or`, the pills joined by `and`, in the reader's own tokens.
//
// It exists because `a OR b c` is `(a OR b) AND c`: OR binding tighter than a
// space is backwards from other search boxes, and neither reading looks wrong
// from a list of notes. Written hidden when there is no grouping, for hint's
// reason.
func grouping(groups [][]string) string {
	if len(groups) == 0 {
		return "<div class=\"parse\" hidden></div>"
	}
	var out strings.Builder
	out.WriteString("<div class=\"parse\">")
	for at, group := range groups {
		if at > 0 {
			out.WriteString("<span class=\"and\">and</span>")
		}
		out.WriteString("<span class=\"g\">")
		for termAt, term := range group {
			if termAt > 0 {
				out.WriteString("<i>or</i>")
			}
			// Only a tag: term is coloured, in the tag's colour.
			class := ""
			if strings.HasPrefix(term, "tag:") || strings.HasPrefix(term, "-tag:") {
				class = " class=\"t\""
			}
			out.WriteString("<b" + class + ">" + escape(term) + "</b>")
		}
		out.WriteString("</span>")
	}
	out.WriteString("</div>")
	return out.String()
}

// frontPane is the reading pane with no note picked, which only a two-pane
// screen sees: the notebook's README.md if it has one, otherwise an invitation.
// The README wears the notebook's colour, as a note wears its own.
func frontPane(book string, front *string) string {
	if front == nil {
		return "<section class=\"pane read\"><main><div class=\"empty\"><b>Pick a note</b>Its text opens here. " +
			"Narrow the list with a search, or start a new note.</div></main></section>"
	}
	return "<section class=\"pane read\"><main class=\"note " + hueClass(book) + "\"><article>" +
		"<div class=\"note-head\"><div class=\"filename\">README.md</div></div>" +
		"<div class=\"body\">" + *front + "</div></article></main></section>"
}

// hint is unhidden by the script only when its answer and the server's can
// differ: a bare word or `text:` reads the body, which is not on the page. It
// says something else when the filter stands aside altogether (data-aside), as
// "filtered by title and tag" over rows that were not filtered reads as wrong.
func hint() string {
	return "<p class=\"hint\" hidden data-aside=\"Not filtered yet — press ⏎ to search.\">" +
		"Filtered by title and tag — press <kbd>⏎</kbd> to search the text.</p>"
}

// simpleParts is a screen that is a heading and a list of rows: the way back, a
// count, and the rail.
type simpleParts struct {
	tab       string // the document's title
	book      string
	drift     string
	here      screen
	backTo    string
	backLabel string
	heading   string
	count     int
	laid      string
	body      string
}

func simplePage(page simpleParts) string {
	return scripted(page.tab, "", []Asset{AssetKeys},
		"<section class=\"pane\">"+
			"<header class=\"topbar\">"+back(page.backTo, page.backLabel, "Escape")+"<h1 class=\"place\">"+escape(page.heading)+"</h1>"+
			"<span class=\"count\">"+strconv.Itoa(page.count)+"</span></header>"+
			"<main class=\""+page.laid+"\">"+page.body+"</main></section>"+rail(page.book, page.drift, page.here))
}

// Files is one row per file: size, type and how many notes point at it (zero
// being `doctor --links`' orphan).
func Files(book string, held []Held, drift string) string {
	body := "<div class=\"empty\"><b>No files yet</b>Run <code>noda file add diagram.png</code> in a terminal to put one here.</div>"
	if len(held) > 0 {
		body = fileRows(book, held)
	}
	return simplePage(simpleParts{
		tab: "Files — " + book + " — noda", book: book, drift: drift, here: atFiles,
		backTo: "/nb/" + escape(book), backLabel: book, heading: "Files", count: len(held),
		laid: "rows files", body: body,
	})
}

func fileRows(book string, held []Held) string {
	var out strings.Builder
	for _, file := range held {
		// Side by side, since links cannot nest: the row goes to the file, the
		// count to its backlinks. Zero is not a link.
		asks := "<span class=\"aside lone\">nothing links to it</span>"
		if file.Used > 0 {
			asks = "<a class=\"aside\" href=\"/nb/" + escape(book) + "/f/" + escape(file.Name) + "/backlinks\">in " +
				plural(file.Used, "note") + "</a>"
		}
		out.WriteString("<div class=\"row split\">" +
			"<a class=\"most\" href=\"/nb/" + escape(book) + "/f/" + escape(file.Name) + "\"><span class=\"title mono\">" + escape(file.Name) + "</span>" +
			"<span class=\"under\"><span>" + size(file.Size) + "</span><span>" + escape(file.Kind) + "</span></span></a>" + asks + "</div>")
	}
	return out.String()
}

// size is powers of two and one decimal place, as file managers show it.
func size(bytes uint64) string {
	value := float64(bytes)
	switch {
	case bytes < 1024:
		return fmt.Sprintf("%d B", bytes)
	case bytes < 1_048_576:
		return fmt.Sprintf("%.1f KB", value/1024)
	case bytes < 1_073_741_824:
		return fmt.Sprintf("%.1f MB", value/1_048_576)
	}
	return fmt.Sprintf("%.1f GB", value/1_073_741_824)
}

// Tags is tags commonest first, alphabetical within a count, each linking to
// the listing narrowed to it. query.Scoped writes the query, because a tag with
// a space has to arrive quoted.
func Tags(book string, tallies []Tally, drift string) string {
	body := "<div class=\"empty\"><b>No tags yet</b>Tags come from a note's frontmatter. Open a note and press Tags to add one.</div>"
	if len(tallies) > 0 {
		body = tagRows(book, tallies)
	}
	return simplePage(simpleParts{
		tab: "Tags — " + book + " — noda", book: book, drift: drift, here: atTags,
		backTo: "/nb/" + escape(book), backLabel: book, heading: "Tags", count: len(tallies),
		laid: "rows tallies", body: body,
	})
}

// tagRows draws each tag with its weight against the commonest. The bar is a
// class in steps of a tenth, not an inline style, so a strict style-src would
// still hold.
func tagRows(book string, tallies []Tally) string {
	most := 1
	for _, tally := range tallies {
		most = max(most, tally.Notes)
	}
	var out strings.Builder
	for _, tally := range tallies {
		// Rounded half up in integers, so no float decides which step a tag is in.
		step := max(1, (tally.Notes*20+most)/(2*most))
		out.WriteString("<a class=\"row tally\" href=\"/nb/" + escape(book) + "?q=" + escape(encoded(query.Scoped(tally.Tag))) + "\">" +
			"<span class=\"name\">" + escape(tally.Tag) + "</span>" +
			"<span class=\"bar w" + strconv.Itoa(step) + "\"></span>" +
			"<span class=\"n\">" + strconv.Itoa(tally.Notes) + "<span class=\"sr\"> " + noun(tally.Notes, "note") + "</span></span></a>")
	}
	return out.String()
}

// Todo is everything in the notebook that is not done, in the order `noda todo`
// prints, cut into three runs by what the date says about an item.
//
// No box can be ticked here: an item has no stable address (line numbers move,
// text collides) and giving it an id would make the file noda-only. The row
// goes to the note instead.
func Todo(book string, tasks []Task, drift string) string {
	body := "<div class=\"empty\"><b>Nothing to do</b>An unticked <code>- [ ]</code> in any note " +
		"turns up here, with a <code>due:2026-08-20</code> if you give it one.</div>"
	if len(tasks) > 0 {
		body = taskRuns(book, tasks)
	}
	return simplePage(simpleParts{
		tab: "Todo — " + book + " — noda", book: book, drift: drift, here: atTodo,
		backTo: "/nb/" + escape(book), backLabel: book, heading: "Todo", count: len(tasks),
		laid: "rows tasks", body: body,
	})
}

// taskRuns keeps the order the tasks came in, which is soonest first, so each
// run is already sorted.
func taskRuns(book string, tasks []Task) string {
	var overdue, dated, whenever []Task
	for _, task := range tasks {
		switch {
		case task.Due == nil:
			whenever = append(whenever, task)
		case task.Overdue:
			overdue = append(overdue, task)
		default:
			dated = append(dated, task)
		}
	}
	return taskRun(book, "Overdue", " late", overdue) + taskRun(book, "Dated", "", dated) + taskRun(book, "Whenever", "", whenever)
}

func taskRun(book, heading, class string, tasks []Task) string {
	if len(tasks) == 0 {
		return ""
	}
	var out strings.Builder
	out.WriteString("<section class=\"run" + class + "\"><h2>" + heading + " <span>" + strconv.Itoa(len(tasks)) + "</span></h2>")
	for _, task := range tasks {
		due := ""
		if task.Due != nil {
			due = escape(*task.Due)
		}
		out.WriteString("<a class=\"row task " + hueClass(task.ID) + "\" href=\"/nb/" + escape(book) + "/n/" + escape(task.ID) + "\">" +
			"<span class=\"due\">" + due + "</span><span class=\"title\">" + escape(task.Text) + "</span>" +
			"<span class=\"in\">" + escape(task.Title) + "</span></a>")
	}
	out.WriteString("</section>")
	return out.String()
}

// Backlinks is what links to a note, or to one of the notebook's files. Inbound
// only: what a note points at is already in the note. It is not on the note's own
// page's way to anywhere any more (the note carries its backlinks), but a file
// has no page of its own, and the address stays for both.
func Backlinks(book string, subject Subject, rows []Row, drift string) string {
	here, backLabel, class := atNotes, subject.What, "subject"
	if subject.Mono {
		here, backLabel, class = atFiles, "Files", "mono"
	}
	var body strings.Builder
	body.WriteString("<p class=\"said\">Notes that link to <span class=\"" + class + "\">" + escape(subject.What) + "</span></p>")
	if len(rows) == 0 {
		body.WriteString("<div class=\"empty\"><b>Nothing links here</b>No note in " + escape(book) +
			" points at " + escape(subject.What) + ".</div>")
	}
	for _, row := range rows {
		body.WriteString("<a class=\"row " + hueClass(row.ID) + "\" href=\"/nb/" + escape(book) + "/n/" + escape(row.ID) + "\">" +
			"<span class=\"title\">" + escape(row.Title) + "</span>" +
			"<span class=\"under\">" + tagLine(row.Tags) + "</span>" +
			"<span class=\"id\">" + escape(row.ID) + "</span></a>")
	}
	return simplePage(simpleParts{
		tab: "Links to " + subject.What + " — noda", book: book, drift: drift, here: here,
		backTo: subject.At, backLabel: backLabel, heading: "Linked from", count: len(rows),
		laid: "rows", body: body.String(),
	})
}

// NotePage is one note, and, on a wide enough screen, the listing it came from.
// drift is for the index pane's pill and the rail.
func NotePage(book string, reading Reading, drift string) string {
	// No "indexed": the index pane is sent empty, since a listing is ~290 bytes
	// a note (half a megabyte at two thousand) and none of it is drawn below
	// 1024px. The panes script fetches it where the column shows. No order
	// either: 450 bytes of a control over no rows.
	return scripted(noteTitle(reading), "split at-note",
		[]Asset{AssetPanes, AssetStamps, AssetSending, AssetKeys},
		indexPane(indexParts{book: book, drift: drift})+readPane(book, reading)+rail(book, drift, atNotes))
}

// NotePane is the same note, for a reader who already has the page around it:
// all a swap uses, the rest (48 of 52 KB, measured) being on screen already.
// The margin is part of it, so the links to the note arrive with the note.
func NotePane(book string, reading Reading) string {
	return titled(noteTitle(reading)) + readPane(book, reading)
}

func noteTitle(reading Reading) string { return reading.Title + " — noda" }

// readPane is shared by NotePage and NotePane, so the swap is part of the page.
func readPane(book string, reading Reading) string {
	pinned := ""
	if reading.Pinned {
		pinned = "<span class=\"pin\">" + style.PinMark + "</span>"
	}
	return "<section class=\"pane read\">" +
		"<header class=\"topbar\">" + back("/nb/"+escape(book), book, "Escape") +
		// The note's title: the index pane beside it already names the notebook.
		"<span class=\"place\">" + escape(reading.Title) + "</span></header>" +
		"<main class=\"note " + hueClass(reading.ID) + "\"><article>" +
		"<div class=\"note-head\"><div class=\"filename\"><span class=\"id\">" + escape(reading.ID) + "</span>" +
		"-" + escape(reading.Slug) + ".md" + pinned + "</div><h1>" + escape(reading.Title) + "</h1></div>" +
		"<div class=\"body\">" + reading.Rendered + "</div></article>" +
		margin(book, reading) + "</main></section>"
}

// margin is everything about a note that is not its text: what can be done to
// it, its tags and stamps, and which notes link to it.
func margin(book string, reading Reading) string {
	at := "/nb/" + escape(book) + "/n/" + escape(reading.ID)
	return "<aside class=\"margin\">" + readBar(at, reading.Pinned) +
		"<dl class=\"about\">" +
		aboutRow("Tags", "chips", tagChips(book, reading.Tags)) +
		aboutRow("Updated", "", fullTime(reading.Updated)) +
		aboutRow("Created", "", fullTime(reading.Created)) +
		"</dl>" + linkedFrom(book, reading.LinkedFrom) + "</aside>"
}

// aboutRow is a term and what it says, or nothing when there is nothing to say.
func aboutRow(term, class, content string) string {
	if content == "" {
		return ""
	}
	if class != "" {
		class = " class=\"" + class + "\""
	}
	return "<div><dt>" + term + "</dt><dd" + class + ">" + content + "</dd></div>"
}

// tagChips link to the listing narrowed to each tag, written by query.Scoped as
// the tag screen does.
func tagChips(book string, tags []string) string {
	if len(tags) == 0 {
		return "<span class=\"none\">none</span>"
	}
	var out strings.Builder
	for _, tag := range tags {
		out.WriteString("<a class=\"tag\" href=\"/nb/" + escape(book) + "?q=" + escape(encoded(query.Scoped(tag))) + "\">" + escape(tag) + "</a>")
	}
	return out.String()
}

// linkedFrom is the notes that point at this one, each in its own colour.
func linkedFrom(book string, rows []Row) string {
	var out strings.Builder
	out.WriteString("<section class=\"beside\"><h2>Linked from <span>" + strconv.Itoa(len(rows)) + "</span></h2>")
	if len(rows) == 0 {
		out.WriteString("<p class=\"none\">No other note links here.</p>")
	}
	for _, row := range rows {
		out.WriteString("<a class=\"mini " + hueClass(row.ID) + "\" href=\"/nb/" + escape(book) + "/n/" + escape(row.ID) + "\">" + escape(row.Title) + "</a>")
	}
	out.WriteString("</section>")
	return out.String()
}

// readBar is what can be done to a note: five things, none marked current.
// Delete is safe on the bar because /delete is a confirmation page. Pin is
// labelled with what a press does, and is the one item that writes without a
// page in between: it is undone by pressing it again.
func readBar(at string, pinned bool) string {
	pin := barItem{icon: iconPin, label: "Pin", act: postTo(at+"/pin", "pin"), key: "p", then: "Unpin"}
	if pinned {
		pin = barItem{icon: iconPin, label: "Unpin", act: postTo(at+"/unpin", "pin"), key: "p", then: "Pin"}
	}
	return actionBar([]barItem{
		{icon: iconEdit, label: "Edit", act: goTo(at + "/edit"), key: "e"},
		{icon: iconTags, label: "Tags", act: goTo(at + "/tags"), key: "t"},
		{icon: iconRename, label: "Rename", act: goTo(at + "/rename"), key: "r"},
		pin,
		{icon: iconTrash, label: "Delete", act: goTo(at + "/delete"), mark: markDanger, key: "D"},
	}, "This note")
}
