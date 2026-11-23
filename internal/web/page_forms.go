package web

import (
	"strconv"
	"strings"

	"github.com/henry40408/noda/internal/version"
)

// formParts is the parts of a form page: where the way back goes and what it is
// called, what to say and the form to say it beside.
type formParts struct {
	title     string
	backTo    string
	backLabel string
	said      string
	form      string
	// tall lets the form's text area take the height of the window, for a page
	// that is mostly the text being written.
	tall bool
	// watching adds the script that listens for the note moving under the form:
	// the three that carry a fingerprint.
	watching bool
}

// formPage is a bar with a way back, an optional line saying what went wrong,
// and the form. Both inside <main>, which the wide layout caps to a column;
// outside it a textarea would run the monitor's width.
//
// The way back has no key: Escape out of a form would take what was typed with
// it.
func formPage(page formParts) string {
	scripts := []Asset{AssetSending, AssetKeys}
	if page.watching {
		scripts = []Asset{AssetWatching, AssetSending, AssetKeys}
	}
	class := "pane form"
	if page.tall {
		class += " tall"
	}
	return scripted(page.title+" — noda", "solo", scripts,
		"<section class=\""+class+"\">"+
			"<header class=\"topbar\">"+back(page.backTo, page.backLabel, "")+"<h1 class=\"place\">"+escape(page.title)+"</h1></header>"+
			"<main>"+page.said+page.form+"</main></section>")
}

// refusal is why a change was refused, said where the change was typed.
func refusal(problem *string) string {
	if problem == nil {
		return ""
	}
	return "<p class=\"said bad\">" + escape(*problem) + "</p>"
}

// submitButton names what it does while it does it (data-busy), so the press is
// answered before the server is.
func submitButton(label, busy string) string {
	return "<button class=\"go\" type=\"submit\" data-busy=\"" + escape(busy) + "\">" + label + "</button>"
}

func cancelLink(href, label, extra string) string {
	return "<a class=\"button\" href=\"" + escape(href) + "\"" + extra + ">" + label + "</a>"
}

// Composing is a new note.
func Composing(book string, draft Draft, problem *string) string {
	at := "/nb/" + escape(book)
	return formPage(formParts{
		title: "New note", backTo: at, backLabel: book, said: refusal(problem), tall: true,
		form: "<form class=\"write\" method=\"post\" action=\"" + at + "/new\">" +
			"<div class=\"pair\"><div><label for=\"t\">Title</label>" +
			"<input id=\"t\" type=\"text\" name=\"title\" value=\"" + escape(draft.Title) + "\" " +
			"placeholder=\"Leave it empty to take the first line\"></div>" +
			"<div><label for=\"g\">Tags</label>" +
			"<input id=\"g\" type=\"text\" name=\"tags\" value=\"" + escape(draft.Tags) + "\" placeholder=\"work q3\"></div></div>" +
			"<div class=\"grow\"><label for=\"b\">Note</label>" +
			"<textarea id=\"b\" name=\"body\" autofocus>" + escape(draft.Body) + "</textarea></div>" +
			"<div class=\"buttons\">" + submitButton("Add note", "Adding…") + cancelLink(at, "Cancel", "") + "</div></form>",
	})
}

// Edit is what Editing draws. Was is the fingerprint the file had when the page
// was drawn, carried through the form so the write can tell whether anything
// happened since.
type Edit struct {
	Book    string
	About   About
	Body    string
	Was     string
	Problem *string
}

// editParts is what the three editor pages share: the fingerprint of the file
// they were drawn from and the field, which is the only thing that differs.
type editParts struct {
	book        string
	about       About
	fingerprint string
	said        string
	fields      string
}

func editForm(parts editParts) string {
	at := parts.about.at(parts.book)
	return formPage(formParts{
		title: "Edit", backTo: at, backLabel: parts.about.Title, said: parts.said, tall: true, watching: true,
		form: "<form class=\"write\" method=\"post\" action=\"" + at + "/edit\">" +
			"<input type=\"hidden\" name=\"fingerprint\" value=\"" + escape(parts.fingerprint) + "\">" + parts.fields +
			"<div class=\"buttons\">" + submitButton("Save", "Saving…") + cancelLink(at, "Cancel", "") +
			"<span class=\"filename\"><span class=\"id\">" + escape(parts.about.ID) + "</span>-" + escape(parts.about.Slug) + ".md</span></div></form>",
	})
}

// Editing is the body in a textarea, with the fingerprint it was drawn from.
func Editing(edit Edit) string {
	return editForm(editParts{
		book: edit.Book, about: edit.About, fingerprint: edit.Was, said: refusal(edit.Problem),
		fields: "<div class=\"grow\"><label for=\"b\">Text of the note — the title and the tags have their own screens</label>" +
			"<textarea id=\"b\" name=\"body\" autofocus>" + escape(edit.Body) + "</textarea></div>",
	})
}

// Clash is what Clashed shows: both versions, and the fingerprint of the one on
// disk.
type Clash struct {
	Book   string
	About  About
	Theirs string
	Mine   string
	Now    string
}

// Clashed is for a note that changed under the reader with no version to merge
// from (the base is not in the object database, e.g. never committed), so both
// are handed back whole; otherwise Conflicted shows the merge.
//
// What is on disk is shown read-only beside what they wrote, which stays
// editable: saving is "keep mine", and editing is combining the two.
func Clashed(clash Clash) string {
	return editForm(editParts{
		book: clash.Book, about: clash.About, fingerprint: clash.Now,
		said: "<p class=\"said bad\"><b>This note changed while you were writing.</b> Nothing has been overwritten.</p>",
		fields: "<div class=\"versus grow\"><div><label>Saved now</label><pre class=\"theirs\">" + escape(clash.Theirs) + "</pre></div>" +
			"<div><label for=\"b\">What you wrote</label>" +
			"<textarea id=\"b\" name=\"body\" autofocus>" + escape(clash.Mine) + "</textarea></div></div>",
	})
}

// Conflicted is for two edits that changed the same lines: the merge comes back
// to be settled in one field, and everything outside the markers is both edits
// combined.
func Conflicted(book string, about About, merged, now string) string {
	return editForm(editParts{
		book: book, about: about, fingerprint: now,
		said: "<p class=\"said bad\"><b>Someone else saved while you were writing.</b> " +
			"Both versions are here and nothing has been overwritten. Where the two " +
			"changed the same lines, they are wrapped in <code>&lt;&lt;&lt;&lt;&lt;&lt;&lt;</code> " +
			"markers — keep what the note should say and delete the rest.</p>",
		fields: "<div class=\"grow\"><label for=\"b\">Text of the note, with both versions</label>" +
			"<textarea id=\"b\" name=\"body\" autofocus>" + escape(merged) + "</textarea></div>",
	})
}

// Renaming is a new title.
func Renaming(book string, about About, title string, problem *string) string {
	at := about.at(book)
	return formPage(formParts{
		title: "Rename", backTo: at, backLabel: about.Title, said: refusal(problem),
		form: "<form class=\"write\" method=\"post\" action=\"" + at + "/rename\">" +
			"<div><label for=\"t\">Title</label>" +
			"<input id=\"t\" type=\"text\" name=\"title\" value=\"" + escape(title) + "\" autofocus>" +
			"<small class=\"hint\">The id <span class=\"id\">" + escape(about.ID) + "</span> never moves, so links and bookmarks survive. " +
			"The filename follows the title.</small></div>" +
			"<div class=\"buttons\">" + submitButton("Rename", "Renaming…") + cancelLink(at, "Cancel", "") + "</div></form>",
	})
}

// Tagging is a ticked box per tag the note has, and a field for new ones; the
// server works out the `+`s and `-`s. The field is cut by query.Split: a space
// separates, a quote holds one together, and the placeholder shows a quoted tag
// so it does not read as a one-tag field.
//
// Every tag is sent as `saw`, and again as `keep` if still ticked. An unticked
// box sends nothing, so without `saw` the server would diff against the file and
// remove a tag added since. One hidden field per tag, so a tag with a space
// needs no quoting.
func Tagging(book string, about About, tags []string, problem *string) string {
	at := about.at(book)
	held := "<p class=\"said\">This note has no tags yet.</p>"
	if len(tags) > 0 {
		var boxes strings.Builder
		for n, tag := range tags {
			id := "t" + strconv.Itoa(n)
			boxes.WriteString("<input type=\"hidden\" name=\"saw\" value=\"" + escape(tag) + "\">" +
				"<label class=\"tick\" for=\"" + id + "\">" +
				"<input id=\"" + id + "\" type=\"checkbox\" name=\"keep\" value=\"" + escape(tag) + "\" checked>" +
				"<span>" + escape(tag) + "</span></label>")
		}
		held = "<div><label>On this note — untick one to take it off</label><div class=\"ticks\">" + boxes.String() + "</div></div>"
	}
	return formPage(formParts{
		title: "Tags", backTo: at, backLabel: about.Title, said: refusal(problem),
		form: "<form class=\"write\" method=\"post\" action=\"" + at + "/tags\">" + held +
			"<div><label for=\"a\">Add tags</label>" +
			"<input id=\"a\" type=\"text\" name=\"add\" " +
			"placeholder=\"ops docs &quot;24.04 Dark patterns&quot;\">" +
			"<small class=\"hint\">Separated by spaces. Quote one that has a space in it.</small></div>" +
			"<div class=\"buttons\">" + submitButton("Save", "Saving…") + cancelLink(at, "Cancel", "") + "</div></form>",
	})
}

// Deleting says the note can be brought back, which git makes true: a warning
// that overstates the danger teaches people to click through warnings.
//
// The page opens with the focus on "Keep it" and Escape presses it, so Enter on
// arrival is the harmless answer. The file that would go is named, because a
// title alone does not say which of two notes that share one is meant.
func Deleting(book string, about About) string {
	at := about.at(book)
	return formPage(formParts{
		title: "Delete", backTo: at, backLabel: about.Title,
		// In the said slot, not the form: .said has its own padding and rule,
		// which a form would inset.
		said: "<p class=\"said ask\"><b>Delete " + escape(about.Title) + "?</b></p>" +
			"<p class=\"doomed filename\"><span class=\"id\">" + escape(about.ID) + "</span>-" + escape(about.Slug) + ".md</p>" +
			"<p class=\"said\">The file goes and the commit that removed it stays, so " +
			"<code>noda restore</code> brings it back with its id.</p>",
		form: "<form class=\"write\" method=\"post\" action=\"" + at + "/delete\">" +
			"<div class=\"buttons\">" + cancelLink(at, "Keep it", " autofocus data-key=\"Escape\"") +
			"<button class=\"danger\" type=\"submit\" data-busy=\"Deleting…\">Delete</button></div></form>",
	})
}

// Standing is `noda status`'s facts, already worked out by the caller.
type Standing struct {
	Branch string `json:"branch"`
	Notes  int    `json:"notes"`
	Files  int    `json:"files"`
	// Uncommitted is files differing from HEAD: what a sync would commit before
	// it pulls.
	Uncommitted int     `json:"uncommitted"`
	Remote      *string `json:"remote"`
	// Drift is how far apart the two are, in words: `2 to push, 1 to pull`.
	Drift string `json:"drift"`
	// Problems is one line per kind, as `doctor` would name them.
	Problems []string `json:"problems"`
}

// Errand is an errand as the page needs to say it: what it is, and how it went.
type Errand struct {
	// Doing is `Syncing` / `Pulling` / `Pushing`, for while it is happening.
	Doing string `json:"doing"`
	// Done is chosen by the caller alongside Failed.
	Done string `json:"done"`
	// Said is what it printed, or what went wrong. Nil while it is still going.
	Said    *string `json:"said"`
	Failed  bool    `json:"failed"`
	Seconds uint64  `json:"seconds"`
}

// againIn is read by both the refresh meta and the script's poll.
const againIn = 2

func working(errand *Errand) bool { return errand != nil && errand.Said == nil }

func refreshWhile(errand *Errand) int {
	if working(errand) {
		return againIn
	}
	return 0
}

// StandingPage is where the notebook stands, and the three ways to move it.
//
// A button POSTs to start the errand and redirects back, rather than holding a
// request open for as long as the network takes; the page is a GET, so a reload
// asks how it is going. While one runs the page refreshes itself.
func StandingPage(book string, standing Standing, errand *Errand) string {
	return dressed(pageParts{
		title: "Status — " + book + " — noda", againIn: refreshWhile(errand),
		scripts: []Asset{AssetStanding, AssetKeys},
		body: "<section class=\"pane\">" +
			"<header class=\"topbar\">" + back("/nb/"+escape(book), book, "Escape") + "<h1 class=\"place\">Status</h1></header>" +
			networkMain(book, standing, errand) + "</section>" +
			rail(book, standing.Drift, atStatus),
	})
}

// StandingMain is what the standing script polls every againIn seconds while an
// errand runs: the refresh meta, the <main>, and the rail's sync link, which
// would otherwise keep saying what was true before the errand. The script polls
// again only if the meta is there, so stopping stays the server's decision.
func StandingMain(book string, standing Standing, errand *Errand) string {
	return refresh(refreshWhile(errand)) + networkMain(book, standing, errand) + syncLink(book, standing.Drift, true)
}

// networkMain leads with the one thing the page is asked for: how far this
// notebook is from its remote. Each button says what it will do.
func networkMain(book string, standing Standing, errand *Errand) string {
	return "<main class=\"status\">" + errandSaid(errand) + lead(standing) +
		"<div class=\"abreast\">" + networkButtons(book, working(errand)) + "</div>" +
		"<dl class=\"facts\">" + facts(standing) + "</dl></main>"
}

func lead(standing Standing) string {
	drift := "No remote"
	if standing.Remote != nil {
		drift = standing.Drift
	}
	return "<div class=\"lead\"><b>" + escape(drift) + "</b><span>" + escape(changesWords(standing.Uncommitted)) + "</span></div>"
}

// facts is what `noda status` knows that the lead does not, a row each.
func facts(standing Standing) string {
	var rows strings.Builder
	row := func(name, value string, mono bool) {
		class := ""
		if mono {
			class = " class=\"mono\""
		}
		rows.WriteString("<div><dt>" + name + "</dt><dd" + class + ">" + escape(value) + "</dd></div>")
	}
	row("Branch", standing.Branch, true)
	if standing.Remote != nil {
		row("Remote", *standing.Remote, true)
	} else {
		// The remedy, since no screen here sets a remote.
		row("Remote", "none — set one with `noda remote set <url>`", false)
	}
	holds := plural(standing.Notes, "note")
	if standing.Files > 0 {
		holds += ", " + plural(standing.Files, "file")
	}
	row("Holds", holds, false)
	for _, problem := range standing.Problems {
		row("Problem", problem, false)
	}
	// Which build is answering, for bug reports against `noda web`.
	row("Version", version.String(), true)
	return rows.String()
}

// changesWords uses `noda status`'s words.
func changesWords(uncommitted int) string {
	switch uncommitted {
	case 0:
		return "clean"
	case 1:
		return "1 file uncommitted"
	}
	return strconv.Itoa(uncommitted) + " files uncommitted"
}

// errandSaid is how the errand is going, or how it went.
func errandSaid(errand *Errand) string {
	if errand == nil {
		return ""
	}
	if errand.Said == nil {
		// Seconds rather than a bar: nothing knows how long a fetch takes.
		return "<p class=\"said working\"><b>" + escape(errand.Doing) + "…</b> " + plural(int(errand.Seconds), "second") + "</p>"
	}
	class := ""
	if errand.Failed {
		class = " bad"
	}
	// Whole: what sync printed says what it did.
	return "<p class=\"said" + class + "\"><b>" + escape(errand.Done) + "</b><span class=\"outcome\">" + escape(*errand.Said) + "</span></p>"
}

// networkErrands are the three ways to move the notebook, and the words for what
// each does: a button that is only a verb leaves the reader to remember which
// of the three sends.
var networkErrands = []struct{ kind, label, doing, does string }{
	{"sync", "Sync", "Syncing", "Commit what changed, pull, then push."},
	{"pull", "Pull", "Pulling", "Bring in what the remote has."},
	{"push", "Push", "Pushing", "Send what is committed here."},
}

// networkButtons are the three errands, disabled while one runs. data-doing is
// the line the script draws on the press, before the server has answered.
func networkButtons(book string, busy bool) string {
	at := escape(book)
	var buttons strings.Builder
	for _, errand := range networkErrands {
		// Sync is accented by colour, so all three stay one row on a phone.
		class := ""
		if errand.kind == "sync" {
			class = " class=\"go\""
		}
		// The server already refuses a second press; disabling shows the first
		// landed.
		disabled := ""
		if busy {
			disabled = " disabled"
		}
		buttons.WriteString("<form method=\"post\" action=\"/nb/" + at + "/status/" + errand.kind + "\">" +
			"<button" + class + " type=\"submit\" data-doing=\"" + errand.doing + "\"" + disabled + ">" + errand.label + "</button>" +
			"<small>" + errand.does + "</small></form>")
	}
	return buttons.String()
}
