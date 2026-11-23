package web

// The HTML, and nothing else.
//
// Every function takes what a page is about and returns a string; nothing opens
// a repository, binds a socket or knows what a request is, so a page is tested
// without either.
//
// Machinery (ids, filenames, tags, stamps) is set in monospace and the reader's
// own words in a text face: the line `style` draws with colour.
//
// No JavaScript required: the search field is a form and every row is a link.
// Filtering as you type is an enhancement, never a replacement. A key is the same:
// `data-key` is written on a control that is already on the page, so the script
// only presses what the markup offers.

import (
	"strconv"
	"strings"

	"github.com/henry40408/noda/internal/version"
)

// Book is `noda status` compressed to a row.
type Book struct {
	Name        string  `json:"name"`
	Notes       int     `json:"notes"`
	Files       int     `json:"files"`
	Uncommitted int     `json:"uncommitted"`
	Drift       *string `json:"drift"` // nil means no remote
	Active      bool    `json:"active"`
	Last        string  `json:"last"`
}

// Row is `ls -l`'s row minus the slug and the created stamp, which are on the
// note's own page.
type Row struct {
	ID    string   `json:"id"`
	Title string   `json:"title"`
	Tags  []string `json:"tags"`
	// Stamp is the one the listing is sorted by: a column of days in no order
	// beside a sorted list reads as a broken sort.
	Stamp *string `json:"stamp"`
	// Shown is false for a row a query excludes: it arrives hidden rather than
	// left out, because a script cannot put back a row the server never sent.
	Shown bool `json:"shown"`
	// Pinned explains a row above one with a newer stamp.
	Pinned bool `json:"pinned"`
}

// Held is a file the notebook holds that is not a note.
type Held struct {
	Name string `json:"name"`
	Size uint64 `json:"size"`
	// Kind is the type the download is served with.
	Kind string `json:"kind"`
	// Used is how many notes link to it; zero is `doctor --links`' orphan.
	Used int `json:"used"`
}

// Tally is one tag, and how many notes carry it.
type Tally struct {
	Tag   string `json:"tag"`
	Notes int    `json:"notes"`
}

// Task is a todo item, named by its note's title; the id is the address.
type Task struct {
	ID    string `json:"id"`
	Title string `json:"title"`
	// Text is the item's words with the `due:` term already lifted out.
	Text    string  `json:"text"`
	Due     *string `json:"due"`
	Overdue bool    `json:"overdue"`
}

// Subject is what a backlinks page is about: a note, or one of the notebook's
// files.
type Subject struct {
	What string `json:"what"`
	// At is where it is, for the way back.
	At string `json:"at"`
	// Mono is set for a filename; a title is never monospace.
	Mono bool `json:"mono"`
}

// Reading is a note, as its own page shows it.
type Reading struct {
	ID      string   `json:"id"`
	Slug    string   `json:"slug"`
	Title   string   `json:"title"`
	Tags    []string `json:"tags"`
	Created *string  `json:"created"`
	Updated *string  `json:"updated"`
	// Rendered is already HTML, from Body: the one string these pages write
	// out unescaped, hence not called body.
	Rendered string `json:"rendered"`
	Pinned   bool   `json:"pinned"`
	// LinkedFrom is the notes that link here, sent with the note: it is part of
	// what the note is, and a margin that waits for a second request would be
	// empty on first sight.
	LinkedFrom []Row `json:"linkedFrom"`
}

// day is the day a stamp names: `2026-08-15`; anything not a date comes back as
// is.
//
// Not the minute: stamps are UTC, and a time with the `Z` cut off to save room
// looks local and is wrong, while converting would put the server's zone into
// the note. The full stamp is on the note's own page.
func day(value string) string {
	if len(value) >= 10 && value[4] == '-' && value[7] == '-' {
		return value[:10]
	}
	return value
}

// highlight is text, escaped, with every run matching one of terms in <mark>.
//
// Matched before escaping and escaped as the pieces are cut: escaping first
// would search `&amp;` for `&`.
func highlight(text string, terms []string) string {
	if len(terms) == 0 {
		return escape(text)
	}
	haystack := strings.ToLower(text)
	if len(haystack) != len(text) {
		// A case mapping that changes a character's width would put the
		// match offsets in the wrong place.
		return escape(text)
	}
	var out strings.Builder
	at := 0
	for at < len(text) {
		found, length, ok := earliestLongest(haystack[at:], terms)
		if !ok {
			break
		}
		start := at + found
		out.WriteString(escape(text[at:start]))
		out.WriteString("<mark>" + escape(text[start:start+length]) + "</mark>")
		at = start + length
	}
	out.WriteString(escape(text[at:]))
	return out.String()
}

// earliestLongest finds the earliest match, then the longest, so overlapping
// terms mark one run rather than nesting.
func earliestLongest(haystack string, terms []string) (start, length int, ok bool) {
	for _, term := range terms {
		if term == "" {
			continue
		}
		at := strings.Index(haystack, term)
		if at < 0 {
			continue
		}
		if !ok || at < start || (at == start && len(term) > length) {
			start, length, ok = at, len(term), true
		}
	}
	return start, length, ok
}

// tagLine is a tag list, one <span> a tag: the listing script reads them back
// one at a time, and a tag needs no separator the stylesheet cannot draw.
func tagLine(tags []string) string {
	if len(tags) == 0 {
		return ""
	}
	var out strings.Builder
	out.WriteString("<span class=\"tags\">")
	for _, tag := range tags {
		out.WriteString("<span>" + escape(tag) + "</span>")
	}
	out.WriteString("</span>")
	return out.String()
}

// pageParts is what dressed wraps a page in.
type pageParts struct {
	title   string
	app     string
	againIn int // seconds; zero means no refresh
	scripts []Asset
	body    string
}

// shell is what every page is wrapped in. app is the layout's classes (root,
// solo, split, at-list/at-note, indexed), each a fact about the route, decided by
// its handler rather than worked out again from the markup.
func shell(title, app, body string) string {
	return dressed(pageParts{title: title, app: app, body: body})
}

// scripted links the scripts a page uses and no others. They are deferred:
// they read the rows, so they run after parsing but download during it.
func scripted(title, app string, scripts []Asset, body string) string {
	return dressed(pageParts{title: title, app: app, scripts: scripts, body: body})
}

// dressed is the shell, plus an optional <meta http-equiv="refresh">: how the
// scriptless network screen comes back for news.
//
// The referrer meta keeps an address (somebody's note id) from leaving: the
// server sends the same as a header a proxy may strip, and this copy also
// covers images a note embeds from elsewhere. same-origin rather than
// no-referrer, which would null a form post's Origin that the guard needs.
func dressed(page pageParts) string {
	var enhancement strings.Builder
	for _, asset := range page.scripts {
		enhancement.WriteString(asset.Tag())
	}
	classes := "app"
	if page.app != "" {
		classes = "app " + page.app
	}
	return "<!doctype html>\n<html lang=\"en\">\n<head>\n" +
		"<meta charset=\"utf-8\">\n" +
		"<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n" +
		"<meta name=\"referrer\" content=\"same-origin\">\n" +
		refresh(page.againIn) + titled(page.title) + "\n" +
		AssetIcon.Tag() + "\n" +
		AssetStyle.Tag() + enhancement.String() + "\n</head>\n<body>\n" +
		"<div class=\"" + classes + "\">\n" + page.body + "</div>\n</body>\n</html>\n"
}

// titled is shared by dressed and the fragments: a swap renames the tab and the
// polling screen steers by the refresh, so a fragment leads with both.
func titled(title string) string {
	return "<title>" + escape(title) + "</title>"
}

func refresh(againIn int) string {
	if againIn == 0 {
		return ""
	}
	return "<meta http-equiv=\"refresh\" content=\"" + strconv.Itoa(againIn) + "\">\n"
}

// Draft is kept as typed, so a refused write hands the reader's words back
// unchanged.
type Draft struct {
	Title string `json:"title"`
	Tags  string `json:"tags"`
	Body  string `json:"body"`
}

// About is which note a form is about. Title is the note's own, not what was
// typed into the form: the way back is named after it.
type About struct {
	ID    string `json:"id"`
	Slug  string `json:"slug"`
	Title string `json:"title"`
}

func (a About) at(book string) string {
	return "/nb/" + escape(book) + "/n/" + escape(a.ID)
}

// keyed writes the shortcut a control answers to. The script only presses a
// control that is on the page, so a key can never do what the page cannot, and
// what a key does without the script is what the control does. does names a key
// whose control has no words of its own (a notebook's name is not "Notebooks").
func keyed(key, does string) string {
	out := ""
	if key != "" {
		out += " data-key=\"" + escape(key) + "\""
	}
	if does != "" {
		out += " data-does=\"" + escape(does) + "\""
	}
	return out
}

// mark is one enum rather than two flags, which would allow "here" and
// "danger" at once.
type mark int

const (
	markPlain  mark = iota
	markHere        // the screen the bar is being drawn on
	markDanger      // the one item that cannot be undone by doing it again
)

func markAt(here bool) mark {
	if here {
		return markHere
	}
	return markPlain
}

// action is what an item on the bar does when it is pressed.
//
// A write is a POST, never a link: the guard has nothing to check when another
// site loads a link of ours as an image. The form sits beside the button, tied
// by form=, so the button stays the bar's flex item.
type action struct {
	href   string // a link, when postTo is empty
	postTo string
	formID string
}

func goTo(href string) action { return action{href: href} }

func postTo(to, formID string) action { return action{postTo: to, formID: formID} }

// barItem is one control on a bar. then is what the label becomes once the
// press has landed, for a control that flips (Pin, Unpin): the script shows
// the server's own word before the server has answered.
type barItem struct {
	icon, label string
	act         action
	mark        mark
	key         string
	then        string
}

func actionBar(items []barItem, label string) string {
	var out strings.Builder
	out.WriteString("<nav class=\"actionbar\" aria-label=\"" + escape(label) + "\">")
	for _, item := range items {
		open, closing := "<a href=\""+escape(item.act.href)+"\"", "</a>"
		if item.act.postTo != "" {
			open = "<form method=\"post\" action=\"" + escape(item.act.postTo) + "\" id=\"" + item.act.formID + "\" hidden></form>" +
				"<button type=\"submit\" form=\"" + item.act.formID + "\""
			closing = "</button>"
		}
		then := ""
		if item.then != "" {
			then = " data-then=\"" + escape(item.then) + "\""
		}
		out.WriteString(open + markAttribute(item.mark) + keyed(item.key, "") + then + ">" +
			svg(item.icon) + "<span>" + item.label + "</span>" + closing)
	}
	out.WriteString("</nav>")
	return out.String()
}

// markAttribute is aria-current so a screen reader says "current page"; danger
// is only a class, as the word Delete already says it.
func markAttribute(m mark) string {
	switch m {
	case markHere:
		return " aria-current=\"page\""
	case markDanger:
		return " class=\"danger\""
	}
	return ""
}

const (
	iconNew = "<path d=\"M12 4.5v15M4.5 12h15\"/>"
	// A page with a folded corner: a notebook is a directory of files.
	iconNotes  = "<path d=\"M5.5 4.5h9L19 9v10.5h-13.5z\"/><path d=\"M14.5 4.5V9H19\"/><path d=\"M9 13h6\"/><path d=\"M9 16.5h4\"/>"
	iconEdit   = "<path d=\"M4 20h4L19 9l-4-4L4 16z\"/>"
	iconTags   = "<path d=\"M4 4h7l9 9-7 7-9-9z\"/><circle cx=\"8\" cy=\"8\" r=\"1.4\"/>"
	iconRename = "<path d=\"M4 7V5h16v2\"/><path d=\"M12 5v14\"/><path d=\"M9 19h6\"/>"
	iconTodo   = "<rect x=\"4\" y=\"4\" width=\"16\" height=\"16\" rx=\"3.5\"/><path d=\"M8.5 12.2l2.6 2.6 4.6-5.4\"/>"
	iconFiles  = "<path d=\"M18.5 10.5 11 18a4 4 0 0 1-5.7-5.7l7.8-7.8a2.6 2.6 0 0 1 3.7 3.7l-7.7 7.7a1.2 1.2 0 0 1-1.7-1.7l7.1-7.1\"/>"
	// A thumbtack, for both Pin and Unpin: the label says which way.
	iconPin = "<path d=\"M9 3h6l-1 5.5 3.5 3v1.5h-11V11.5l3.5-3z\"/><path d=\"M12 13v8\"/>"
	// Same stroke as the rest: the warning is the colour alone.
	iconTrash = "<path d=\"M5 7h14\"/><path d=\"M9.5 7V4.5h5V7\"/><path d=\"M6.5 7l1 12.5h9L17.5 7\"/><path d=\"M10 10.5v6\"/><path d=\"M14 10.5v6\"/>"
	// Two arrows passing, not a cloud: a remote is often the reader's own machine.
	iconSync = "<path d=\"M7 9l5-5 5 5\"/><path d=\"M12 4v10\"/><path d=\"M17 15l-5 5-5-5\"/><path d=\"M12 20V10\"/>"
	// Down is `--sort`'s order, up is `-r`. Not ascending/descending: `updated`
	// runs newest-first and `title` A-to-Z.
	iconDownwards = "<path d=\"M12 5v14\"/><path d=\"M6 13l6 6 6-6\"/>"
	iconUpwards   = "<path d=\"M12 19V5\"/><path d=\"M6 11l6-6 6 6\"/>"
	// Inline SVG rather than `‹`, whose weight and position the reader's font
	// would decide.
	iconBack = "<svg viewBox=\"0 0 24 24\" aria-hidden=\"true\"><path d=\"M15 4.5 7.5 12 15 19.5\"/></svg>"
)

// svg is an icon's paths in the one frame every icon is drawn in.
func svg(paths string) string {
	return "<svg viewBox=\"0 0 24 24\" aria-hidden=\"true\">" + paths + "</svg>"
}

// driftPill is the way to the network screen, showing its answer. The link is
// the 48px touch target; the label repeats the words because a narrow screen
// may ellipsize the text. extra is whatever else the caller writes on the link.
func driftPill(book, drift, extra string) string {
	return "<a class=\"drift\" href=\"/nb/" + escape(book) + "/status\" aria-label=\"Status: " + escape(drift) + "\"" + extra + ">" +
		svg(iconSync) + "<span>" + escape(drift) + "</span></a>"
}

// screen is which of the notebook's screens is being drawn, so the rail can say
// so.
type screen int

const (
	atNotes screen = iota
	atTags
	atTodo
	atFiles
	atStatus // not on the bar, reached from the sync link: no item is marked
)

// rail says where you are: which notebook, which of its screens, how far it has
// drifted from its remote. From 640px it is the left column; on a phone the same
// element is the dock along the bottom. It is last in the markup, so Tab
// reaches the page before it.
//
// The wrapper sticks, not the bar inside it: on a short page the bar sits under
// the last row, and a button pinned to the window would float below.
func rail(book, drift string, here screen) string {
	at := "/nb/" + escape(book)
	ver := escape(version.String())
	return "<div class=\"foot rail\">" +
		"<a class=\"book " + hueClass(book) + "\" href=\"/\" aria-label=\"All notebooks\"" + keyed("gh", "Notebooks") + ">" +
		"<span class=\"name\">" + escape(book) + "</span><span class=\"all\">All notebooks</span></a>" +
		"<a class=\"fab\" href=\"" + at + "/new\"" + keyed("n", "") + ">" + svg(iconNew) + "<span>New note</span></a>" +
		actionBar([]barItem{
			{icon: iconNotes, label: "Notes", act: goTo(at), mark: markAt(here == atNotes), key: "gn"},
			{icon: iconTags, label: "Tags", act: goTo(at + "/tags"), mark: markAt(here == atTags), key: "gt"},
			{icon: iconTodo, label: "Todo", act: goTo(at + "/todo"), mark: markAt(here == atTodo), key: "gd"},
			{icon: iconFiles, label: "Files", act: goTo(at + "/files"), mark: markAt(here == atFiles), key: "gf"},
		}, "Notebook") +
		syncLink(book, drift, here == atStatus) +
		"<span class=\"ver\" title=\"noda " + ver + "\">" + ver + "</span></div>"
}

// syncLink is the rail's word for the notebook's drift. The network screen's
// poll sends it again alone, so the rail is not left saying what was true
// before the errand.
func syncLink(book, drift string, current bool) string {
	here := ""
	if current {
		here = " aria-current=\"page\""
	}
	return "<a class=\"sync\" href=\"/nb/" + escape(book) + "/status\"" + here +
		" aria-label=\"Status: " + escape(drift) + "\"" + keyed("gs", "Status") + ">" +
		svg(iconSync) + "<span>" + escape(drift) + "</span></a>"
}

// back is the way up one step, a breadcrumb with words. key is what presses it:
// Escape on a page that only reads, none on a form, where Escape would take
// what was typed with it.
func back(href, label, key string) string {
	does := ""
	if key == "gh" {
		// A place rather than a step, so it is named; Escape needs no name.
		does = "Notebooks"
	}
	return "<a class=\"back\" href=\"" + escape(href) + "\" aria-label=\"Back to " + escape(label) + "\"" + keyed(key, does) + ">" +
		iconBack + "<span>" + escape(label) + "</span></a>"
}

// when is a listing's stamp: the day, and nothing that could be read as a clock.
func when(updated *string) string {
	if updated == nil {
		return ""
	}
	return "<time class=\"when\" datetime=\"" + escape(*updated) + "\">" + escape(day(*updated)) + "</time>"
}

// fullTime is a note page's stamp, written whole with its offset so it cannot be
// misread; data-clock tells the stamps script it has room for a time of day,
// which a listing's does not.
func fullTime(value *string) string {
	if value == nil {
		return ""
	}
	shown := escape(*value)
	return "<time datetime=\"" + shown + "\" data-clock>" + shown + "</time>"
}

// plural is `1 note` / `2 notes`. English only, like every other string noda
// prints.
func plural(count int, thing string) string {
	return strconv.Itoa(count) + " " + noun(count, thing)
}

// noun is the word alone, for a count that is written apart from it.
func noun(count int, thing string) string {
	if count == 1 {
		return thing
	}
	return thing + "s"
}

// Failure is a page for a request that cannot be answered. It links no script:
// nothing on it needs one.
func Failure(heading, detail string) string {
	return shell(heading+" — noda", "root solo",
		"<section class=\"pane\"><header class=\"topbar\">"+back("/", "Notebooks", "")+"</header>"+
			"<main><div class=\"empty\"><b>"+escape(heading)+"</b>"+escape(detail)+"</div></main></section>")
}
