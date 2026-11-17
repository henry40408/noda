package web

import (
	"fmt"
	"strconv"
	"strings"

	"github.com/henry40408/noda/internal/cmd"
	"github.com/henry40408/noda/internal/query"
	"github.com/henry40408/noda/internal/style"
)

const separator = "<span class=\"sep\">·</span>"

// joinPieces joins what is not empty.
func joinPieces(pieces ...string) string {
	kept := make([]string, 0, len(pieces))
	for _, piece := range pieces {
		if piece != "" {
			kept = append(kept, piece)
		}
	}
	return strings.Join(kept, separator)
}

// Notebooks is the one screen not inside a notebook, so it has no rail or bar;
// root stops the layout reserving a column for the rail.
func Notebooks(books []Book) string {
	var rows strings.Builder
	if len(books) == 0 {
		rows.WriteString("<div class=\"empty\"><b>No notebooks yet</b>Run <code>noda init</code> in a terminal to make the first one.</div>")
	}
	for _, book := range books {
		rows.WriteString(bookRow(book))
	}
	return shell("noda", "root",
		"<section class=\"pane\">"+
			"<header class=\"topbar\"><span class=\"here lead\">noda</span>"+
			"<span class=\"count\">"+tally(books)+"</span></header>"+
			"<main class=\"rows books\">"+rows.String()+"</main></section>")
}

// tally's second clause is in a .more span the stylesheet drops on a phone: the
// server does not know the screen's width.
func tally(books []Book) string {
	notes := 0
	for _, book := range books {
		notes += book.Notes
	}
	return plural(len(books), "notebook") + "<span class=\"more\"> · " + plural(notes, "note") + "</span>"
}

// bookRow is two links side by side rather than nested: the row goes to the
// listing, the chip to the network screen. With no remote the chip is plain
// words.
func bookRow(book Book) string {
	at := escape(book.Name)
	// A dot for `noda notebook ls`'s `*`, with words for a screen reader.
	mark := ""
	if book.Active {
		mark = "<span class=\"mark\" aria-hidden=\"true\"></span><span class=\"sr\">Active — </span>"
	}

	// .holds, not .when: a test helper reads stamps by .when.
	facts := []string{"<span class=\"holds\">" + plural(book.Notes, "note") + "</span>"}
	if book.Files > 0 {
		// Classed so a phone can drop it: three facts do not fit beside the
		// chip at 390px.
		facts = append(facts, "<span class=\"holds files\">"+plural(book.Files, "file")+"</span>")
	}
	if book.Uncommitted > 0 {
		facts = append(facts, "<span class=\"holds\">"+strconv.Itoa(book.Uncommitted)+" uncommitted</span>")
	}

	aside := "<span class=\"aside\">no remote</span>"
	if book.Drift != nil {
		aside = "<a class=\"aside\" href=\"/nb/" + at + "/status\" aria-label=\"Status: " + escape(*book.Drift) + "\">" +
			"<span class=\"pill\"><svg viewBox=\"0 0 24 24\" aria-hidden=\"true\">" + iconSync + "</svg>" +
			"<span>" + escape(*book.Drift) + "</span></span></a>"
	}

	return "<div class=\"row split book\">" +
		"<a class=\"most\" href=\"/nb/" + at + "\">" +
		"<span class=\"name\">" + mark + escape(book.Name) + "</span>" +
		"<div class=\"under\">" + strings.Join(facts, separator) + "</div>" +
		"<span class=\"stamp\">" + escape(book.Last) + "</span></a>" + aside + "</div>"
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

// Listing is what the listing's index pane is drawn from. Drift is the chip
// linking to the network screen.
type Listing struct {
	Book  string
	Rows  []Row
	Asked Asked
	Order Order
	Drift string
}

// ListingPage is a notebook's notes, narrowed by whatever was typed. front is
// the front page beside them, when the notebook has one.
func ListingPage(list Listing, front *string) string {
	// "indexed": the rows are in the markup, as the script marks a note route
	// once it has fetched them. Beside is for the note a picked row swaps into
	// the reading pane.
	return scripted(listingTitle(list.Book), "split at-list indexed",
		[]Asset{AssetListing, AssetPanes, AssetBeside, AssetStamps},
		listingPanes(list, front)+notebookBar(list.Book, atNotes))
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
	var body strings.Builder
	for _, row := range list.Rows {
		if row.Shown {
			shown++
		}
		body.WriteString(listingRow(list.Book, row, list.Asked.Terms))
	}
	body.WriteString(emptySentence(list.Book, list.Asked, total, shown))

	counted := strconv.Itoa(total)
	if list.Asked.Typed != "" {
		counted = fmt.Sprintf("%d of %d", shown, total)
	}
	return indexPane(indexParts{
		book: list.Book, asked: list.Asked, order: &list.Order,
		counted: counted, drift: list.Drift, rows: body.String(),
	})
}

// listingRow is one note's row in the index pane.
func listingRow(book string, row Row, terms []string) string {
	// The pin mark goes last, after the tags.
	pin := ""
	if row.Pinned {
		pin = "<span class=\"pin\">" + style.PinMark + "</span>"
	}
	under := joinPieces(when(row.Stamp), tagLine(row.Tags), pin)

	hidden := ""
	if !row.Shown {
		hidden = " hidden"
	}
	// The day is written twice, here beside the id and in under, and the
	// stylesheet shows one, as the server does not know the screen's width.
	dayBesideID := ""
	if row.Stamp != nil {
		dayBesideID = "<span class=\"day\">" + escape(day(*row.Stamp)) + "</span>"
	}
	// A hidden row's title is unmarked, as the script would leave it.
	title := escape(row.Title)
	if row.Shown {
		title = highlight(row.Title, terms)
	}
	return "<a class=\"row\"" + hidden + " href=\"/nb/" + escape(book) + "/n/" + escape(row.ID) + "\">" +
		"<div class=\"ident\"><span class=\"id\">" + escape(row.ID) + "</span>" + dayBesideID + "</div>" +
		"<div class=\"title\">" + title + "</div>" +
		"<div class=\"under\">" + under + "</div></a>"
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
		"<header class=\"topbar\">" + back("/", "the notebooks") + "<span class=\"here\">" + escape(pane.book) + "</span>" +
		driftChip(pane.book, pane.drift) +
		"<span class=\"count\">" + pane.counted + "</span></header>" +
		"<form class=\"searchbar\" method=\"get\" action=\"/nb/" + escape(pane.book) + "\">" +
		"<input type=\"search\" name=\"q\" value=\"" + escape(pane.asked.Typed) + "\" " +
		"placeholder=\"tag:work OR tag:q3 budget\" " +
		"autocomplete=\"off\" autocapitalize=\"off\" spellcheck=\"false\" " +
		"enterkeyhint=\"search\" aria-label=\"Search this notebook\">" +
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
// drops the reversal.
func sortbar(book string, asked Asked, order Order) string {
	var out strings.Builder
	out.WriteString("<nav class=\"sortbar\" aria-label=\"Order\">" +
		"<span class=\"lab\"><svg viewBox=\"0 0 24 24\" aria-hidden=\"true\">" + iconOrder + "</svg></span>")
	for _, sort := range cmd.AllSorts {
		here := sort == order.Sort
		next := Order{Sort: sort, Reversed: here && !order.Reversed}
		current, label, arrow := "", "Order by "+sort.Name(), ""
		if here {
			current = " aria-current=\"true\""
			label = orderedLabel(sort, order.Reversed)
			arrow = "<svg viewBox=\"0 0 24 24\" aria-hidden=\"true\">" + arrowFor(order.Reversed) + "</svg>"
		}
		out.WriteString("<a href=\"/nb/" + escape(book) + escape(next.asked(asked.Typed)) + "\"" + current +
			" aria-label=\"" + escape(label) + "\"><span class=\"pill\">" + escape(sort.Name()) + arrow + "</span></a>")
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
func frontPane(book string, front *string) string {
	if front == nil {
		return "<section class=\"pane read\"><header class=\"topbar\">" +
			"<span class=\"here lead\">Reading</span></header>" +
			"<main><div class=\"empty\"><b>Pick a note</b>Its text opens here. " +
			"Narrow the list with a search, or press + to write a new one." +
			"</div></main></section>"
	}
	return "<section class=\"pane read\"><header class=\"topbar\">" +
		"<span class=\"here lead\">" + escape(book) + "</span>" +
		"<span class=\"count mono\">README.md</span></header>" +
		"<main class=\"note\"><div class=\"body\">" + *front + "</div></main></section>"
}

// hint is unhidden by the script only when its answer and the server's can
// differ: a bare word or `text:` reads the body, which is not on the page.
func hint() string {
	return "<p class=\"hint\" hidden>Filtered by title and tag — press ⏎ to search the text.</p>"
}

// Files is one row per file: size, type and how many notes point at it (zero
// being `doctor --links`' orphan).
func Files(book string, held []Held) string {
	// Columns (.cols) only when there are rows: the empty sentence poured into
	// columns would arrive as fragments with a rule through them.
	laid := "rows"
	body := "<div class=\"empty\"><b>No files yet</b>Run <code>noda file add diagram.png</code> in a terminal to put one here.</div>"
	if len(held) > 0 {
		laid, body = "rows cols wide", fileRows(book, held)
	}
	return shell("Files — "+book+" — noda", "",
		"<section class=\"pane\">"+
			"<header class=\"topbar\">"+back("/nb/"+escape(book), book)+"<span class=\"here\">Files</span>"+
			"<span class=\"count\">"+strconv.Itoa(len(held))+"</span></header>"+
			"<main class=\""+laid+"\">"+body+"</main></section>"+notebookBar(book, atFiles))
}

func fileRows(book string, held []Held) string {
	var out strings.Builder
	for _, file := range held {
		under := size(file.Size) + separator + escape(file.Kind)
		// Side by side, since links cannot nest: the row goes to the file, the
		// count to its backlinks. Zero is not a link.
		asks := "<span class=\"aside\">nothing links to it</span>"
		if file.Used > 0 {
			asks = "<a class=\"aside\" href=\"/nb/" + escape(book) + "/f/" + escape(file.Name) + "/backlinks\">in " +
				plural(file.Used, "note") + "</a>"
		}
		out.WriteString("<div class=\"row split\">" +
			"<a class=\"most\" href=\"/nb/" + escape(book) + "/f/" + escape(file.Name) + "\"><div class=\"title mono\">" + escape(file.Name) + "</div>" +
			"<div class=\"under\">" + under + "</div></a>" + asks + "</div>")
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
func Tags(book string, tallies []Tally) string {
	// Columns only when there are rows, as in Files.
	laid := "rows"
	body := "<div class=\"empty\"><b>No tags yet</b>Tags come from a note's frontmatter. Open a note and press Tags to add one.</div>"
	if len(tallies) > 0 {
		var out strings.Builder
		for _, tally := range tallies {
			out.WriteString("<a class=\"row\" href=\"/nb/" + escape(book) + "?q=" + escape(encoded(query.Scoped(tally.Tag))) + "\">" +
				"<div class=\"name tags\">" + escape(tally.Tag) + "</div>" +
				"<div class=\"under\"><span class=\"when\">" + plural(tally.Notes, "note") + "</span></div></a>")
		}
		laid, body = "rows cols", out.String()
	}
	return shell("Tags — "+book+" — noda", "",
		"<section class=\"pane\">"+
			"<header class=\"topbar\">"+back("/nb/"+escape(book), book)+"<span class=\"here\">Tags</span>"+
			"<span class=\"count\">"+strconv.Itoa(len(tallies))+"</span></header>"+
			"<main class=\""+laid+"\">"+body+"</main></section>"+notebookBar(book, atTags))
}

// Todo is everything in the notebook that is not done, in the order `noda todo`
// prints.
//
// No box can be ticked here: an item has no stable address (line numbers move,
// text collides) and giving it an id would make the file noda-only. The row
// goes to the note instead.
func Todo(book string, tasks []Task) string {
	body := "<div class=\"empty\"><b>Nothing to do</b>An unticked <code>- [ ]</code> in any note " +
		"turns up here, with a <code>due:2026-08-20</code> if you give it one.</div>"
	if len(tasks) > 0 {
		body = taskRows(book, tasks)
	}
	return shell("Todo — "+book+" — noda", "",
		"<section class=\"pane\">"+
			"<header class=\"topbar\">"+back("/nb/"+escape(book), book)+"<span class=\"here\">Todo</span>"+
			"<span class=\"count\">"+strconv.Itoa(len(tasks))+"</span></header>"+
			"<main class=\"rows\">"+body+"</main></section>"+notebookBar(book, atTodo))
}

func taskRows(book string, tasks []Task) string {
	var out strings.Builder
	for _, task := range tasks {
		due := ""
		switch {
		case task.Due != nil && task.Overdue:
			due = "<span class=\"overdue\">" + escape(*task.Due) + "</span>"
		case task.Due != nil:
			due = "<span class=\"when\">" + escape(*task.Due) + "</span>"
		}
		under := joinPieces(due, "<span class=\"in\">"+escape(task.Title)+"</span>")
		out.WriteString("<a class=\"row\" href=\"/nb/" + escape(book) + "/n/" + escape(task.ID) + "\"><div class=\"title\">" +
			escape(task.Text) + "</div><div class=\"under\">" + under + "</div></a>")
	}
	return out.String()
}

// Backlinks is what links to a note, or to one of the notebook's files. Inbound
// only: what a note points at is already in the note.
func Backlinks(book string, subject Subject, rows []Row) string {
	return shell("Links to "+subject.What+" — noda", "",
		"<section class=\"pane\">"+
			"<header class=\"topbar\">"+back(subject.At, subject.What)+"<span class=\"here\">Backlinks</span>"+
			"<span class=\"count\">"+strconv.Itoa(len(rows))+"</span></header>"+
			BacklinksRows(book, subject, rows)+"</section>")
}

// BacklinksRows is the part of Backlinks the beside script fetches, per note
// read on a wide screen, to rebuild the rows in the margin.
func BacklinksRows(book string, subject Subject, rows []Row) string {
	var body strings.Builder
	if len(rows) == 0 {
		body.WriteString("<div class=\"empty\"><b>Nothing links here</b>No note in " + escape(book) +
			" points at " + escape(subject.What) + ".</div>")
	}
	for _, row := range rows {
		body.WriteString("<a class=\"row\" href=\"/nb/" + escape(book) + "/n/" + escape(row.ID) + "\"><div class=\"title\">" +
			escape(row.Title) + "</div><div class=\"under\">" + tagLine(row.Tags) + "</div></a>")
	}
	class := "subject"
	if subject.Mono {
		class = "mono"
	}
	return "<main class=\"rows\"><p class=\"said\">What links to <span class=\"" + class + "\">" +
		escape(subject.What) + "</span></p>" + body.String() + "</main>"
}

// NotePage is one note, and, on a wide enough screen, the listing it came from.
// drift is for the index pane's bar.
func NotePage(book string, reading Reading, drift string) string {
	// No "indexed": the index pane is sent empty, since a listing is ~290 bytes
	// a note (half a megabyte at two thousand) and none of it is drawn below
	// 1024px. The panes script fetches it where the column shows. No order
	// either: 450 bytes of a control over no rows.
	return scripted(noteTitle(reading), "split at-note",
		[]Asset{AssetPanes, AssetBeside, AssetStamps},
		indexPane(indexParts{book: book, drift: drift})+readPane(book, reading)+notebookBar(book, atNotes))
}

// NotePane is the same note, for a reader who already has the page around it:
// all a swap uses, the rest (48 of 52 KB, measured) being on screen already. No
// drift, as the swap keeps the index pane, so this costs one file read and no
// refs.
func NotePane(book string, reading Reading) string {
	return titled(noteTitle(reading)) + readPane(book, reading)
}

func noteTitle(reading Reading) string { return reading.Title + " — noda" }

// readPane is shared by NotePage and NotePane, so the swap is part of the page.
func readPane(book string, reading Reading) string {
	at := "/nb/" + escape(book) + "/n/" + escape(reading.ID)
	meta := joinPieces(tagLine(reading.Tags), stamp("created", reading.Created), stamp("updated", reading.Updated))
	// The box, not the answer: backlinks walk every note (+8% on `ls`,
	// measured) for a column no screen under 1440px draws. The beside script
	// fills it where it shows; scriptless readers have the Links button.
	beside := "<aside class=\"beside\" hidden><div class=\"pane-head\">Backlinks</div>" +
		"<div class=\"answer\"></div></aside>"
	return "<section class=\"pane read\">" +
		"<header class=\"topbar\">" + back("/nb/"+escape(book), book) +
		// The note's title: the index pane beside it already names the notebook.
		"<span class=\"here\">" + escape(reading.Title) + "</span></header>" +
		"<main class=\"note\">" +
		"<div class=\"note-head\"><h1>" + escape(reading.Title) + "</h1>" +
		"<div class=\"filename\"><span class=\"id\">" + escape(reading.ID) + "</span>" +
		"<span class=\"slug\">-" + escape(reading.Slug) + "</span><span class=\"ext\">.md</span></div>" +
		"<div class=\"note-meta\">" + meta + "</div></div>" +
		"<div class=\"body\">" + reading.Rendered + "</div>" +
		beside + "</main>" + readBar(at, reading.Pinned) + "</section>"
}

// readBar is what can be done to a note. Things to do to the note, so none is
// marked current. Delete is safe on the bar because /delete is a confirmation
// page. Pin is labelled with what a press does, and is the one item that writes
// without a page in between: it is undone by pressing it again.
func readBar(at string, pinned bool) string {
	pin := barItem{iconPin, "Pin", postTo(at+"/pin", "pin"), markPlain}
	if pinned {
		pin = barItem{iconPin, "Unpin", postTo(at+"/unpin", "pin"), markPlain}
	}
	return actionBar([]barItem{
		{iconEdit, "Edit", goTo(at + "/edit"), markPlain},
		{iconTags, "Tags", goTo(at + "/tags"), markPlain},
		{iconRename, "Rename", goTo(at + "/rename"), markPlain},
		{iconLinks, "Links", goTo(at + "/backlinks"), markPlain},
		pin,
		{iconTrash, "Delete", goTo(at + "/delete"), markDanger},
	})
}
