package tui

import (
	"fmt"
	"strings"
	"unicode/utf8"

	"github.com/henry40408/noda/internal/cmd"
	"github.com/henry40408/noda/internal/style"
	"github.com/henry40408/noda/internal/tui/ui"
)

// The bands every screen shares (header, title, crumbs, status) and the card
// drawn over them, kept here so no screen's furniture can move.
//
// The current screen's keys are in the header rather than behind `?`, which
// also keeps the help card short enough for a small terminal.

// InfoRows is the header's height when the terminal has room for it.
const InfoRows = 5

// roomForInfo is the terminal height below which the header is one line.
const roomForInfo = 20

const menuRows = 5

// labelWidth is one wider than `Notebook:`, so the longest label still gets a
// gap.
const labelWidth = 10

const menuGap = 2

const gap = "  "

const keyGap = 2

type keyPair struct{ key, what string }

type column [menuRows]keyPair

// blank pads a column out to five. Nothing is drawn.
var blank = keyPair{}

// viewKeys come last on every screen, being the first to drop: each is also a
// `:` command.
var viewKeys = column{{"t", "todo"}, {"l", "log"}, {"b", "backlinks"}, {"B", "blame"}, blank}

// listingKeys are column-major: read down each column of five.
var listingKeys = []column{
	{{"enter", "read"}, {"/", "filter"}, {":", "command"}, {"ctrl-a", "commands"}, {"?", "keys"}},
	{{"e", "edit"}, {"a", "new"}, {"m", "retitle"}, {"#", "tags"}, {"ctrl-d", "delete"}},
	{{"space", "mark"}, {"*", "mark shown"}, {"Q", "queue"}, {"T", "keep updated"}, {"q", "quit"}},
	// Listing-only view keys, after the others because columns drop from the
	// right and each of these can also be found on the `?` card.
	{{"S", "sort"}, {"R", "reverse"}, {"ctrl-w", "wide"}, {"1-9", "tag, 0 all"}, {"ctrl-g", "crumbs"}},
	viewKeys,
}

// noteKeys has `:` first and leaves `g`/`G` off: columns drop from the right,
// and `:` cannot be looked up while `G` is on the card.
var noteKeys = []column{
	{{"esc", "back"}, {"j/k", "scroll"}, {":", "command"}, {"ctrl-a", "commands"}, {"?", "keys"}},
	{{"e", "edit"}, {"m", "retitle"}, {"#", "tags"}, {"ctrl-d", "delete"}, {"T", "keep updated"}},
	{{"r", "reload"}, {"q", "quit"}, blank, blank, blank},
	viewKeys,
}

var changingKeys = column{{"e", "edit"}, {"m", "retitle"}, {"#", "tags"}, {"ctrl-d", "delete"}, {"T", "keep updated"}}

// rowsKeys are the keys of a screen of rows, labelled with what `enter` does
// there. An empty label is where `enter` does nothing: the notebook's log has
// no one note to restore.
func rowsKeys(enter string) []column {
	// The first column is never dropped, so it holds what cannot be looked up.
	// `ctrl-f/b` is left off: its width would push out the note-changing keys.
	if enter == "" {
		return []column{
			{{"j/k", "move"}, {"g/G", "first / last"}, {"esc", "back"}, {":", "command"}, {"?", "keys"}},
			{{"ctrl-a", "commands"}, {"r", "reload"}, {"q", "quit"}, blank, blank},
		}
	}
	return []column{
		{{"enter", enter}, {"j/k", "move"}, {"esc", "back"}, {":", "command"}, {"?", "keys"}},
		{{"g/G", "first / last"}, {"ctrl-a", "commands"}, {"r", "reload"}, {"q", "quit"}, blank},
	}
}

// keysFor is a screen's grid. A screen whose rows are notes also gets the
// note-changing keys, ahead of the view keys, which have `:` names to fall
// back on.
func keysFor(view View) []column {
	var keys []column
	switch view.Kind {
	case ViewNotes:
		return append([]column(nil), listingKeys...)
	case ViewNote:
		return append([]column(nil), noteKeys...)
	// Short, because this shape has four columns to fit.
	case ViewTodo, ViewBacklinks:
		keys = append(rowsKeys("read it"), changingKeys)
	case ViewTags:
		keys = rowsKeys("filter by it")
	case ViewFiles:
		keys = rowsKeys("what links here")
	case ViewNotebooks:
		keys = rowsKeys("switch to it")
	// The ellipsis: `enter` writes the command rather than running it.
	case ViewDeleted:
		keys = rowsKeys("restore it…")
	case ViewLog:
		if view.Name != "" {
			keys = rowsKeys("restore it…")
		} else {
			keys = rowsKeys("")
		}
	case ViewBlame, ViewDiff:
		keys = []column{
			{{"j/k", "scroll"}, {"g/G", "top / end"}, {"esc", "back"}, {":", "command"}, {"?", "keys"}},
			{{"ctrl-a", "commands"}, {"r", "reload"}, {"q", "quit"}, blank, blank},
		}
	}
	return append(keys, viewKeys)
}

// headerRows is one line on a short terminal.
func headerRows(height int) int {
	if height >= roomForInfo {
		return InfoRows
	}
	return 1
}

func drawHeader(f *ui.Frame, area ui.Rect, app *App) {
	if area.Height < InfoRows {
		f.Line(area, ui.LineOf(compact(app)...))
		return
	}
	mark := wordmark()
	parts := ui.Layout{Pieces: []ui.Constraint{
		ui.Length(infoWidth(app)), ui.Fill(1), ui.Length(mark.Width()),
	}}.Split(area)
	f.Paragraph(parts[0], ui.Paragraph{Lines: info(app)})
	f.Paragraph(parts[1], ui.Paragraph{Lines: menu(app, parts[1].Width)})
	f.Line(parts[2], mark)
}

// info is always the same five labels in the same order, so each stays where
// the eye learned it.
func info(app *App) []ui.Line {
	muted := theme(style.Muted)
	branch := []ui.Span{ui.Raw(app.Status.Branch)}
	if drift := app.Status.Drift; drift != nil && (drift.Ahead > 0 || drift.Behind > 0) {
		branch = append(branch, ui.Styled(fmt.Sprintf("  ↑%d ↓%d", drift.Ahead, drift.Behind), muted))
	}

	counts := []ui.Span{ui.Raw(plural(app.Total(), "note"))}
	if app.Status.Files > 0 {
		counts = append(counts, ui.Styled("  "+plural(app.Status.Files, "file"), muted))
	}

	changes := ui.Styled("none", muted)
	if n := app.Status.Uncommitted; n > 0 {
		changes = ui.Styled(fmt.Sprintf("%d uncommitted", n), theme(style.Match))
	}

	remote := ui.Styled("none", muted)
	if app.Status.HasRemote {
		remote = ui.Raw(app.Status.Remote)
	}

	return []ui.Line{
		labelled("Notebook", ui.Styled(app.Notebook, bold)),
		labelled("Branch", branch...),
		labelled("Remote", remote),
		labelled("Notes", counts...),
		labelled("Changes", changes),
	}
}

// session is the keyed-in state that changes what the next key does. It is on
// the title band, not the header, since in the header its width pushed out the
// key grid.
func session(app *App) []ui.Span {
	var spans []ui.Span
	add := func(text string, s ui.Style) {
		if len(spans) > 0 {
			spans = append(spans, ui.Styled("  ", theme(style.Muted)))
		}
		spans = append(spans, ui.Styled(text, s))
	}
	if app.Touch == cmd.TouchKeep {
		add("keeping updated", theme(style.Match))
	}
	if len(app.Marks) > 0 {
		add(plural(len(app.Marks), "mark"), theme(style.Match))
	}
	if len(app.Queue) > 0 {
		add(fmt.Sprintf("%d queued", len(app.Queue)), theme(style.Tags))
	}
	return spans
}

func labelled(label string, value ...ui.Span) ui.Line {
	name := padRight(label+":", labelWidth)
	return ui.LineOf(append([]ui.Span{ui.Styled(name, theme(style.Muted))}, value...)...)
}

func infoWidth(app *App) int {
	widest := 0
	for _, line := range info(app) {
		widest = max(widest, line.Width())
	}
	return widest + menuGap
}

// menu drops columns from the right rather than squeezing them until words
// are cut.
func menu(app *App, width int) []ui.Line {
	columns := keysFor(app.View())

	var widths []int
	room := width
	for _, col := range columns {
		wanted := 0
		for _, pair := range col {
			if pair.key != "" {
				wanted = max(wanted, runeCount(pair.key)+2+keyGap+runeCount(pair.what))
			}
		}
		wanted += menuGap
		if wanted > room {
			break
		}
		room -= wanted
		widths = append(widths, wanted)
	}

	lines := make([]ui.Line, menuRows)
	for row := range menuRows {
		var spans []ui.Span
		for at, wanted := range widths {
			pair := columns[at][row]
			// A blank still takes its width, or later columns on this row slide
			// left.
			if pair.key == "" {
				spans = append(spans, ui.Raw(strings.Repeat(" ", wanted)))
				continue
			}
			named := "<" + pair.key + ">"
			// Padding after the description, or it sits against the next key
			// and reads as that key's.
			used := runeCount(named) + keyGap + runeCount(pair.what)
			spans = append(spans, ui.Styled(named, theme(style.ID)))
			spans = append(spans, ui.Raw(strings.Repeat(" ", keyGap)+pair.what+strings.Repeat(" ", max(0, wanted-used))))
		}
		lines[row] = ui.LineOf(spans...)
	}
	return lines
}

// wordmark is the name only; no version, whose columns the key grid needs more.
func wordmark() ui.Line {
	return ui.LineOf(ui.Styled("noda", bold)).Right()
}

// compact is the header on a short terminal: the facts on one line, no keys.
func compact(app *App) []ui.Span {
	spans := []ui.Span{
		ui.Styled(app.Notebook, bold),
		ui.Styled(fmt.Sprintf("  (%s)  ", app.Status.Branch), theme(style.Muted)),
		ui.Raw(plural(app.Total(), "note")),
	}
	if n := app.Status.Uncommitted; n > 0 {
		spans = append(spans, ui.Styled(fmt.Sprintf("  %d uncommitted", n), theme(style.Match)))
	}
	return spans
}

// looking is the `--sort`, `-r` and `-l` settings, when not the defaults:
// those keys rearrange rows and otherwise leave no trace.
func looking(app *App) string {
	var said []string
	if app.Sort != cmd.SortSlug || app.Reverse {
		said = append(said, "by "+app.Sort.Name())
	}
	if app.Reverse {
		said = append(said, "reversed")
	}
	if app.Long {
		said = append(said, "wide")
	}
	return strings.Join(said, " ")
}

// banner is name, scope, count in that order on every screen, split by gaps.
func banner(name, scope string, count int) []ui.Span {
	spans := []ui.Span{ui.Styled(name, bold)}
	if scope != "" {
		spans = append(spans, ui.Styled(gap+scope, theme(style.Muted)))
	}
	if count >= 0 {
		spans = append(spans, ui.Styled(fmt.Sprintf("%s%d", gap, count), theme(style.Muted)))
	}
	return spans
}

// titleSpans is a listing's filter and count, or a note's id then title, as
// `noda ls` writes them.
func titleSpans(app *App) []ui.Span {
	titled := func(id string) ui.Span {
		if file := app.NoteOf(id); file != nil {
			return ui.Raw(file.Note.Title)
		}
		return ui.Raw("")
	}
	view := app.View()
	switch view.Kind {
	case ViewNotes:
		scope := app.Search()
		if scope == "" {
			scope = "all"
		}
		spans := banner("Notes", scope, app.Shown())
		if said := looking(app); said != "" {
			spans = append(spans, ui.Styled(gap+said, theme(style.Muted)))
		}
		return spans
	case ViewNote:
		return append(banner("Note", view.Name, -1), ui.Raw(gap), titled(view.Name))
	case ViewTodo:
		return banner("Todo", "", len(app.Tasks()))
	case ViewTags:
		return banner("Tags", "", len(app.Tallies()))
	case ViewFiles:
		return banner("Files", "", len(app.Files()))
	case ViewNotebooks:
		return banner("Notebooks", app.Notebook, len(app.Notebooks()))
	case ViewDeleted:
		return banner("Deleted", "", len(app.Gone()))
	case ViewDiff:
		return banner("Diff", "", -1)
	// The id is here rather than in the crumbs, which would grow too wide.
	case ViewLog:
		scope := view.Name
		if scope == "" {
			scope = app.Notebook
		}
		spans := banner("Log", scope, len(app.Entries()))
		if view.Name != "" {
			spans = append(spans, ui.Raw(gap), titled(view.Name))
		}
		return spans
	case ViewBacklinks:
		return banner("Backlinks", view.Name, len(app.Linking()))
	case ViewBlame:
		return append(banner("Blame", view.Name, -1), ui.Raw(gap), titled(view.Name))
	}
	return nil
}

func drawTitle(f *ui.Frame, area ui.Rect, app *App) {
	spans := titleSpans(app)
	// Ruled to the far end, so the band reads as the top of the body.
	held := session(app)
	used := ui.LineOf(spans...).Width()
	wanted := ui.LineOf(held...).Width()
	if wanted > 0 {
		wanted++
	}
	if rule := area.Width - (used + wanted + 1); rule > 0 {
		spans = append(spans, ui.Styled(" "+strings.Repeat("─", rule), theme(style.Muted)))
	}
	if len(held) > 0 {
		spans = append(spans, ui.Raw(" "))
		spans = append(spans, held...)
	}
	f.Line(area, ui.LineOf(spans...))
}

// drawCrumbs is shown even with one crumb, so how far `esc` goes back is
// never a guess.
func drawCrumbs(f *ui.Frame, area ui.Rect, app *App) {
	depth := app.Depth()
	var spans []ui.Span
	for at, crumb := range app.Crumbs() {
		s := theme(style.Muted)
		if at+1 == depth {
			s = reversed
		}
		spans = append(spans, ui.Styled(" "+crumb+" ", s), ui.Raw(" "))
	}
	f.Line(area, ui.LineOf(spans...))
}

// statusLeft is the line for typing or for the last command's answer, and
// where the cursor belongs in it (-1 for nowhere).
func statusLeft(app *App) (line ui.Line, cursor int) {
	muted := theme(style.Muted)
	switch {
	case app.Working != "":
		return ui.LineOf(ui.Styled(app.Working, muted)), -1
	// Measured in columns, not characters (CJK is two wide), over the text
	// left of the cursor.
	case app.Mode == ModeCommand:
		width := 1 + ui.Raw(app.Input.Before()).Width()
		return ui.LineOf(ui.Styled(":", muted), ui.Raw(app.Input.Text())), width
	case app.Mode == ModeSearch:
		width := 1 + ui.Raw(app.SearchBefore()).Width()
		return ui.LineOf(ui.Styled("/", muted), ui.Raw(app.Search())), width
	case app.Mode == ModeAsk:
		label := ui.Styled(app.Asking.Prompt()+"  ", muted)
		width := label.Width() + ui.Raw(app.Input.Before()).Width()
		return ui.LineOf(label, ui.Raw(app.Input.Text())), width
	// Cleared by the next key. A failure uses the invalid-query colour: both
	// are a sentence that never reached the notebook.
	case app.Message != nil:
		s := ui.Style{}
		if app.Message.Failed {
			s = theme(style.Invalid)
		}
		return ui.LineOf(ui.Styled(app.Message.Line(), s)), -1
	// Kept after typing ends: it is why the listing is short.
	case app.Search() != "":
		return ui.LineOf(ui.Styled("/", muted), ui.Styled(app.Search(), muted)), -1
	}
	return ui.Line{}, -1
}

// statusRight is an error first, else a prompt's hint.
func statusRight(app *App) ui.Line {
	if message, bad := app.Error(); bad {
		return ui.LineOf(ui.Styled(message, theme(style.Invalid)))
	}
	if app.Mode == ModeAsk && app.Asking.Hint() != "" {
		return ui.LineOf(ui.Styled(app.Asking.Hint(), theme(style.Muted)))
	}
	return ui.Line{}
}

// drawStatus is the one line for typing or for the last command's answer. It
// returns where the cursor belongs, or -1.
func drawStatus(f *ui.Frame, area ui.Rect, app *App) int {
	left, cursor := statusLeft(app)
	// Drawn on a frame of its own, before the slow action starts.
	if app.Working != "" {
		f.Line(area, left)
		return -1
	}
	right := statusRight(app)
	parts := ui.Layout{Pieces: []ui.Constraint{ui.Fill(1), ui.Length(right.Width())}}.Split(area)
	f.Line(parts[0], left)
	f.Line(parts[1], right)
	if cursor < 0 {
		return -1
	}
	return area.X + cursor
}

// plural is `1 note` / `3 notes`.
func plural(n int, thing string) string {
	if n == 1 {
		return "1 " + thing
	}
	return fmt.Sprintf("%d %ss", n, thing)
}

// card is what a card says, and the colour it says it in.
type card struct {
	title  string
	lines  []ui.Line
	border ui.Style
}

// draw makes the card as wide as its widest line in terminal columns, clamped
// to the screen; what does not fit wraps rather than cuts.
func (c card) draw(f *ui.Frame, area ui.Rect) {
	widest := 0
	for _, line := range c.lines {
		widest = max(widest, line.Width())
	}
	width := min(max(2+widest, runeCount(c.title)+2), area.Width)
	// After the clamp, so a line too wide gets the rows it wraps onto.
	inner := max(width-2, 1)
	rows := 0
	for _, line := range c.lines {
		rows += max((line.Width()+inner-1)/inner, 1)
	}
	height := min(rows+2, area.Height)
	area = ui.Layout{Flex: ui.FlexCenter, Pieces: []ui.Constraint{ui.Length(width)}}.Split(area)[0]
	area = ui.Layout{Direction: ui.Vertical, Flex: ui.FlexCenter, Pieces: []ui.Constraint{ui.Length(height)}}.Split(area)[0]

	f.Clear(area)
	f.Paragraph(area, ui.Paragraph{
		Lines: c.lines,
		Wrap:  true,
		Block: ui.Block{
			Bordered: true,
			// In the border's colour, so the title is quieter than the body.
			Title:       ui.Styled(c.title, c.border.WithAdd(ui.Bold)),
			BorderStyle: c.border,
		},
	})
}

func runeCount(text string) int { return utf8.RuneCountInString(text) }

// padRight pads to a number of characters, as Rust's `{:<n}` does.
func padRight(text string, chars int) string {
	return text + strings.Repeat(" ", max(0, chars-runeCount(text)))
}

// padLeft pads to a number of characters, as Rust's `{:>n}` does.
func padLeft(text string, chars int) string {
	return strings.Repeat(" ", max(0, chars-runeCount(text))) + text
}
