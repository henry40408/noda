package tui

import (
	"fmt"
	"strings"

	"github.com/henry40408/noda/internal/clock"
	"github.com/henry40408/noda/internal/cmd"
	"github.com/henry40408/noda/internal/notebook"
	"github.com/henry40408/noda/internal/style"
	"github.com/henry40408/noda/internal/tui/ui"
)

// Drawing one frame: the body band and the cards; the other bands are in
// frame.go.
//
// The listing uses `noda ls`'s row, so a note is named the same everywhere. A
// note is drawn as `noda show` prints it, with search matches picked out as
// `noda search` does.

// keyColumn is the widest key cell on the help card, so the descriptions line
// up.
const keyColumn = 22

// markSymbol goes in front of the id, the same width marked or not, so columns
// never shift.
const (
	markSymbol = "• "
	unmarked   = "  "
)

// columnGap and titleFloor: the title keeps that width however long the tags.
const (
	columnGap  = 2
	titleFloor = 10
)

const shortCommit = 7

// markedCommit is wider by the unpushed mark, which only the log shows.
const markedCommit = shortCommit + 2

// barColumn is the scrollbar's column, reserved even when no bar is drawn so
// columns do not shift when a list overflows.
const barColumn = 1

// cursorBar is a half block pointing at the row, not a place in the text. The
// space keeps it from reading as part of a commit hash.
const cursorBar = "▌ "

// gutter is the cursor bar's width, taken off every row measurement.
const gutter = 2

const headingRows = 1

// helpKeys is the `?` card: what the header's key grid leaves out or may drop.
// Kept short enough to fit a 24-row terminal.
var helpKeys = [][2]string{
	{"j / k, ↓ / ↑", "move · scroll"},
	{"ctrl-f / ctrl-b, g / G", "half a screen · first / last"},
	{"enter, esc", "open it · back out of it"},
	{"/", "filter: tag:work OR tag:q3 budget"},
	{":, ctrl-a", "run a command · the list of what it takes"},
	{"space, *, Q", "mark · mark all shown · the queue"},
	// `p` is only here: every column of the header's grid is full.
	{"e, a, p", "edit in $EDITOR · new note · pin, and unpin"},
	{"m, #", "retitle · tags: a box each, tab chooses"},
	{"ctrl-d, T", "delete (after a y) · leave updated alone"},
	{"t, l, b, B", "todo · log · backlinks · blame"},
	{"S, R, ctrl-w, 1-9", "sort · reverse · wide row · a tag (0 = all)"},
	{"r, ctrl-g, q / ctrl-c", "read again · crumbs · quit"},
	{"while typing", "readline: ctrl-a/e/w/u/k/y, alt-b/f"},
}

// Draw paints one frame of the session. It tells the state how many rows fit,
// so a half-screen jump lands where the screen says.
func Draw(f *ui.Frame, app *App) {
	area := f.Area()
	crumbRows := 0
	if app.CrumbsShown {
		crumbRows = 1
	}
	// Hidden crumbs get zero rows, so the body gets them.
	parts := ui.Layout{Direction: ui.Vertical, Pieces: []ui.Constraint{
		ui.Length(headerRows(area.Height)), ui.Length(1), ui.Min(0), ui.Length(crumbRows), ui.Length(1),
	}}.Split(area)
	header, title, body, crumbs, status := parts[0], parts[1], parts[2], parts[3], parts[4]

	// Less the heading row, or a half-screen jump lands a row too far.
	if app.HasRows() {
		app.SetPage(max(0, body.Height-headingRows))
	} else {
		app.SetPage(body.Height)
	}
	drawHeader(f, header, app)
	drawTitle(f, title, app)
	drawBody(f, body, app)
	if app.CrumbsShown {
		drawCrumbs(f, crumbs, app)
	}
	if x := drawStatus(f, status, app); x >= 0 {
		f.Cursor = &ui.Point{X: x, Y: status.Y}
	}
	drawCard(f, area, app)
}

// drawCard draws at most one card, matching the mode.
func drawCard(f *ui.Frame, area ui.Rect, app *App) {
	switch app.Mode {
	case ModeHelp:
		drawHelp(f, area)
	case ModeCommands:
		drawCommands(f, area, app)
	case ModeConfirm:
		drawConfirm(f, area, app)
	case ModeQueue:
		drawQueue(f, area, app)
	case ModeTagging:
		drawTagging(f, area, app)
	case ModeAlert:
		drawAlert(f, area, app)
	}
}

func lessTheBar(area ui.Rect) (content, bar ui.Rect) {
	parts := ui.Layout{Pieces: []ui.Constraint{ui.Fill(1), ui.Length(barColumn)}}.Split(area)
	return parts[0], parts[1]
}

// scroll is how far through total rows the shown ones have got, at being the
// first.
type scroll struct{ total, shown, at int }

// drawScrollbar draws only on overflow, and without end arrows, which would
// cost two rows.
func drawScrollbar(f *ui.Frame, area ui.Rect, s scroll) {
	if s.total <= s.shown || area.Height == 0 {
		return
	}
	f.Scrollbar(area, ui.Scrollbar{Track: "│", TrackStyle: theme(style.TagsPunct), Thumb: "█"},
		ui.ScrollbarState{ContentLength: s.total - s.shown, Viewport: s.shown, Position: s.at})
}

// sheet is the one table shape for every screen of rows. The cursor is a bar
// and bold rather than reversed, which would invert the id's and tags' colours.
func sheet(rows []ui.Row, widths []ui.Constraint, headings []string) ui.Table {
	cells := make([]ui.Line, len(headings))
	for i, name := range headings {
		cells[i] = ui.LineOf(ui.Styled(name, theme(style.Column)))
	}
	return ui.Table{
		Rows:            rows,
		Header:          &ui.Row{Cells: cells},
		Widths:          widths,
		ColumnSpacing:   columnGap,
		HighlightStyle:  bold,
		HighlightSymbol: ui.Styled(cursorBar, theme(style.Cursor)),
	}
}

// underMark is a heading indented past the mark, which is inside the cell.
func underMark(name string) string { return unmarked + name }

// drawBody draws a list with a cursor or a page to scroll, as the state
// decided; deciding again here could disagree with what `j` does.
func drawBody(f *ui.Frame, area ui.Rect, app *App) {
	switch app.View().Kind {
	case ViewNotes:
		drawListing(f, area, app)
	case ViewNote:
		drawNote(f, area, app)
	case ViewTodo:
		drawRows(f, area, app, todoRows(app), "nothing to do")
	case ViewTags:
		drawRows(f, area, app, tagRows(app), "no tags yet")
	case ViewFiles:
		drawRows(f, area, app, fileRows(app), "this notebook holds nothing but notes")
	case ViewNotebooks:
		drawRows(f, area, app, notebookRows(app), "no notebooks")
	case ViewDeleted:
		drawRows(f, area, app, deletedRows(app), "nothing has been deleted")
	case ViewBacklinks:
		drawRows(f, area, app, backlinkRows(app), "nothing links here")
	case ViewLog:
		drawRows(f, area, app, logRows(app), "no commits")
	case ViewBlame:
		drawBlame(f, area, app)
	case ViewDiff:
		drawDiff(f, area, app)
	}
}

// drawNothing says why a screen is empty in its own words, not "no results".
func drawNothing(f *ui.Frame, area ui.Rect, said string) {
	f.Paragraph(area, ui.Paragraph{
		Lines: []ui.Line{ui.LineOf(ui.Styled(said, theme(style.Muted)))},
		// Aligned with where a list's rows start.
		Block: ui.Block{Padding: ui.Padding{Left: gutter, Right: barColumn}},
	})
}

func drawListing(f *ui.Frame, area ui.Rect, app *App) {
	if app.Shown() == 0 {
		if app.Total() == 0 {
			drawNothing(f, area, "this notebook has no notes yet")
		} else {
			drawNothing(f, area, "nothing matches")
		}
		return
	}
	area, bar := lessTheBar(area)
	rows := app.Rows()

	idWidth := 0
	tagsWidest := 0
	pinnedHere := false
	for _, file := range rows {
		idWidth = max(idWidth, runeCount(file.ID))
		tagsWidest = max(tagsWidest, runeCount(tagList(file.Note.Tags)))
		pinnedHere = pinnedHere || file.Note.IsPinned()
	}
	idWidth += runeCount(markSymbol)
	// The row's real width, less the cursor bar's gutter (the scrollbar's
	// column is already gone).
	inner := max(0, area.Width-gutter)

	// `-l`'s columns are dropped whole from the right while the title is below
	// its floor: the id and title name a note, the rest is density.
	extra := longColumns(app, rows)
	widths := make([]int, len(extra))
	for i, col := range extra {
		for _, value := range col.values {
			widths[i] = max(widths[i], cmd.DisplayWidth(value))
		}
	}
	// Only a listing holding a pin gets the pin column.
	pinWidth := 0
	if pinnedHere {
		pinWidth = runeCount(style.PinMark) + columnGap
	}
	spent := func() int {
		total := (len(widths)+2)*columnGap + idWidth + titleFloor + pinWidth
		for _, width := range widths {
			total += width
		}
		return total
	}
	for len(widths) > 0 && spent() > inner {
		widths = widths[:len(widths)-1]
		extra = extra[:len(extra)-1]
	}

	// As wide as the longest tag list, but only what the title's floor leaves:
	// an imported tag may be a sentence, and a note is found by its title.
	tagWidth := min(tagsWidest, max(0, inner-spent()))

	constraints := []ui.Constraint{ui.Length(idWidth), ui.Fill(1)}
	for _, width := range widths {
		constraints = append(constraints, ui.Length(width))
	}
	constraints = append(constraints, ui.Length(tagWidth))
	if pinnedHere {
		constraints = append(constraints, ui.Length(runeCount(style.PinMark)))
	}

	// Headings are needed with `-l`: `created` and `updated` look alike.
	names := []string{underMark("ID"), "TITLE"}
	for _, col := range extra {
		names = append(names, strings.ToUpper(col.name))
	}
	names = append(names, "TAGS")
	if pinnedHere {
		names = append(names, strings.ToUpper(style.PinMark))
	}

	table := sheet(listingRows(app, rows, extra, pinnedHere), constraints, names)
	state := app.TableState()
	f.Table(area, table, state)
	rowsBar := rowsArea(bar)
	drawScrollbar(f, rowsBar, scroll{total: len(rows), shown: rowsBar.Height, at: state.Offset})
}

// listingRows is one row per note on screen, with whatever `-l` and a pin add
// after the title.
func listingRows(app *App, files []notebook.NoteFile, extra []longColumn, pinnedHere bool) []ui.Row {
	terms := app.Terms()
	muted := theme(style.Muted)
	rows := make([]ui.Row, len(files))
	for at, file := range files {
		mark := unmarked
		if app.Marked(file.ID) {
			mark = markSymbol
		}
		cells := []ui.Line{
			ui.LineOf(ui.Styled(mark, theme(style.Match)), ui.Styled(file.ID, theme(style.ID))),
			// Uncoloured, as in `noda ls`: it is where the eye lands.
			marked(file.Note.Title, terms, ui.Style{}),
		}
		// `-l` extends the row, in the CLI's order and colours.
		for _, col := range extra {
			s := muted
			if col.name == "slug" {
				s = theme(style.Slug)
			}
			cells = append(cells, ui.LineOf(ui.Styled(col.values[at], s)))
		}
		cells = append(cells, tagsLine(file.Note.Tags))
		if pinnedHere {
			pin := ui.Raw("")
			if file.Note.IsPinned() {
				pin = ui.Styled(style.PinMark, theme(style.Pin))
			}
			cells = append(cells, ui.LineOf(pin))
		}
		rows[at] = ui.Row{Cells: cells}
	}
	return rows
}

type longColumn struct {
	name   string
	values []string
}

// longColumns is what `-l` adds, one column of values per heading.
func longColumns(app *App, files []notebook.NoteFile) []longColumn {
	if !app.Long {
		return nil
	}
	pick := func(name string, of func(notebook.NoteFile) string) longColumn {
		values := make([]string, len(files))
		for i, file := range files {
			values[i] = of(file)
		}
		return longColumn{name, values}
	}
	return []longColumn{
		pick("slug", func(file notebook.NoteFile) string { return file.Slug }),
		pick("created", func(file notebook.NoteFile) string { return stamp(file.Note.Created) }),
		pick("updated", func(file notebook.NoteFile) string { return stamp(file.Note.Updated) }),
	}
}

// rowsArea lines the bar up with the rows, not the heading.
func rowsArea(area ui.Rect) ui.Rect {
	parts := ui.Layout{Direction: ui.Vertical, Pieces: []ui.Constraint{ui.Length(headingRows), ui.Fill(1)}}.Split(area)
	return parts[1]
}

// stamp is `noda ls -l`'s dash for a missing time, rather than a blank cell.
func stamp(value *string) string {
	if value == nil {
		return "-"
	}
	return *value
}

// tableData is a table of rows, one screen's worth.
type tableData struct {
	names  []string
	widths []ui.Constraint
	rows   []ui.Row
}

func drawRows(f *ui.Frame, area ui.Rect, app *App, sheetOf tableData, empty string) {
	if len(sheetOf.rows) == 0 {
		drawNothing(f, area, empty)
		return
	}
	area, bar := lessTheBar(area)
	state := app.TableState()
	f.Table(area, sheet(sheetOf.rows, sheetOf.widths, sheetOf.names), state)
	rowsBar := rowsArea(bar)
	drawScrollbar(f, rowsBar, scroll{total: len(sheetOf.rows), shown: rowsBar.Height, at: state.Offset})
}

func cell(text string, s ui.Style) ui.Line { return ui.LineOf(ui.Styled(text, s)) }

// todoRows colours only an overdue date. Never truncated, as in `noda todo`.
func todoRows(app *App) tableData {
	muted := theme(style.Muted)
	var rows []ui.Row
	idWidth, slugWidth := 0, 0
	for _, task := range app.Tasks() {
		file := app.NoteAt(task.Note)
		if file == nil {
			continue
		}
		idWidth = max(idWidth, cmd.DisplayWidth(file.ID))
		slugWidth = max(slugWidth, cmd.DisplayWidth(file.Slug))
		due := ui.Raw("")
		if task.Item.Due != nil {
			due = ui.Styled(*task.Item.Due, muted)
			if task.Item.Overdue(app.Today()) {
				due = ui.Styled(*task.Item.Due, theme(style.Overdue))
			}
		}
		rows = append(rows, ui.Row{Cells: []ui.Line{
			cell(file.ID, theme(style.ID)),
			cell(file.Slug, theme(style.Slug)),
			ui.LineOf(due),
			ui.LineRaw(task.Item.Text),
		}})
	}
	return tableData{
		names: []string{"ID", "SLUG", "DUE", "TASK"},
		widths: []ui.Constraint{
			ui.Length(idWidth), ui.Length(slugWidth), ui.Length(clock.DateWidth), ui.Fill(1),
		},
		rows: rows,
	}
}

// tagRows is every tag, commonest first, with its note count.
func tagRows(app *App) tableData {
	width := 0
	var rows []ui.Row
	for at, tally := range app.Tallies() {
		width = max(width, cmd.DisplayWidth(tally.Tag))
		// The digit key that filters the listing by this tag.
		key := ""
		if at < ScopeKeys {
			key = fmt.Sprint(at + 1)
		}
		rows = append(rows, ui.Row{Cells: []ui.Line{
			cell(key, theme(style.ID)),
			cell(tally.Tag, theme(style.Tags)),
			cell(plural(tally.Notes, "note"), theme(style.Muted)),
		}})
	}
	return tableData{
		// No heading fits a one-column cell.
		names:  []string{"", "TAG", "NOTES"},
		widths: []ui.Constraint{ui.Length(1), ui.Length(width), ui.Fill(1)},
		rows:   rows,
	}
}

func fileRows(app *App) tableData {
	rows := make([]ui.Row, len(app.Files()))
	for i, name := range app.Files() {
		rows[i] = ui.Row{Cells: []ui.Line{ui.LineRaw(name)}}
	}
	return tableData{names: []string{"FILE"}, widths: []ui.Constraint{ui.Fill(1)}, rows: rows}
}

// notebookRows has the listing's mark on the current notebook.
func notebookRows(app *App) tableData {
	rows := make([]ui.Row, len(app.Notebooks()))
	for i, name := range app.Notebooks() {
		mark := unmarked
		if name == app.Notebook {
			mark = markSymbol
		}
		rows[i] = ui.Row{Cells: []ui.Line{ui.LineOf(ui.Styled(mark, theme(style.Match)), ui.Raw(name))}}
	}
	return tableData{
		names:  []string{underMark("NOTEBOOK")},
		widths: []ui.Constraint{ui.Fill(1)},
		rows:   rows,
	}
}

// deletedRows shows the revision `restore` needs, the commit before the
// deletion, as `noda deleted` shows it.
func deletedRows(app *App) tableData {
	muted := theme(style.Muted)
	ids, slugs := 0, 0
	var rows []ui.Row
	for _, gone := range app.Gone() {
		ids = max(ids, cmd.DisplayWidth(gone.ID))
		slugs = max(slugs, cmd.DisplayWidth(gone.Slug))
		rows = append(rows, ui.Row{Cells: []ui.Line{
			cell(gone.ID, theme(style.ID)),
			cell(gone.Slug, theme(style.Slug)),
			cell(clock.FormatTime(gone.RemovedAt, gone.OffsetMinutes), muted),
			cell(gone.RestoreFromShort(), theme(style.Commit)),
			ui.LineRaw(gone.Title),
		}})
	}
	return tableData{
		// `FROM`: it is the commit before the deletion, in seven columns.
		names: []string{"ID", "SLUG", "DELETED", "FROM", "TITLE"},
		widths: []ui.Constraint{
			ui.Length(ids), ui.Length(slugs), ui.Length(clock.TimeWidth), ui.Length(shortCommit), ui.Fill(1),
		},
		rows: rows,
	}
}

// backlinkRows is `noda ls`'s row, since each result is a note.
func backlinkRows(app *App) tableData {
	ids, tagsWidest := 0, 0
	var rows []ui.Row
	for _, at := range app.Linking() {
		file := app.NoteAt(at)
		if file == nil {
			continue
		}
		ids = max(ids, cmd.DisplayWidth(file.ID))
		tagsWidest = max(tagsWidest, runeCount(tagList(file.Note.Tags)))
		rows = append(rows, ui.Row{Cells: []ui.Line{
			cell(file.ID, theme(style.ID)),
			marked(file.Note.Title, app.Terms(), ui.Style{}),
			tagsLine(file.Note.Tags),
		}})
	}
	return tableData{
		names:  []string{"ID", "TITLE", "TAGS"},
		widths: []ui.Constraint{ui.Length(ids), ui.Fill(1), ui.Length(tagsWidest)},
		rows:   rows,
	}
}

// logRows is commits, newest first, in `noda log`'s columns.
func logRows(app *App) tableData {
	muted := theme(style.Muted)
	var rows []ui.Row
	for _, entry := range app.Entries() {
		// Inside the commit column, where `noda log` puts it, rather than a
		// column that is blank on most rows.
		mark := " "
		if app.IsUnpushed(entry.ID) {
			mark = cmd.Unpushed
		}
		rows = append(rows, ui.Row{Cells: []ui.Line{
			ui.LineOf(ui.Styled(mark+" ", muted), ui.Styled(entry.ShortID(), theme(style.Commit))),
			cell(clock.FormatTime(entry.Seconds, entry.OffsetMinutes), muted),
			ui.LineRaw(entry.Summary),
		}})
	}
	return tableData{
		// Indented over the ids, not the arrows.
		names:  []string{"  COMMIT", "WHEN", "SUMMARY"},
		widths: []ui.Constraint{ui.Length(markedCommit), ui.Length(clock.TimeWidth), ui.Fill(1)},
		rows:   rows,
	}
}

// drawBlame is a page, not a list. Not wrapped, so the commit and time columns
// line up.
func drawBlame(f *ui.Frame, area ui.Rect, app *App) {
	muted := theme(style.Muted)
	var lines []ui.Line
	for _, line := range app.Blamed() {
		when := padRight("not committed", clock.TimeWidth)
		if line.Commit != "" {
			when = clock.FormatTime(line.Seconds, line.OffsetMinutes)
		}
		lines = append(lines, ui.LineOf(
			ui.Styled(line.ShortCommit(), theme(style.Commit)),
			ui.Raw("  "),
			ui.Styled(when, muted),
			ui.Raw("  "),
			ui.Raw(line.Text),
		))
	}
	drawPage(f, area, page{lines: lines, scroll: app.Scroll()})
}

// page is the lines of a page, how far down they are scrolled, and whether
// long ones wrap.
type page struct {
	lines  []ui.Line
	scroll int
	wrap   bool
}

// drawPage draws a bar that counts source lines, which `j` moves by, not
// wrapped rows: an approximate bar beats one that disagrees with the key.
func drawPage(f *ui.Frame, area ui.Rect, p page) {
	area, bar := lessTheBar(area)
	f.Paragraph(area, ui.Paragraph{
		Lines: p.lines,
		Wrap:  p.wrap,
		// The gutter, so text starts where rows do.
		Block:  ui.Block{Padding: ui.Padding{Left: gutter}},
		Scroll: p.scroll,
	})
	drawScrollbar(f, bar, scroll{total: len(p.lines), shown: bar.Height, at: p.scroll})
}

// drawDiff colours by each line's prefix; `fetch` strips `cmd.Diff`'s escapes
// rather than parsing them.
func drawDiff(f *ui.Frame, area ui.Rect, app *App) {
	patch, ok := app.Text()
	if !ok {
		return
	}
	if strings.TrimSpace(patch) == "" {
		drawNothing(f, area, "nothing has changed since the last commit")
		return
	}
	var lines []ui.Line
	for _, line := range splitLines(patch) {
		lines = append(lines, ui.LineOf(ui.Styled(line, diffStyle(line))))
	}
	// Unwrapped: a wrapped `+` line reads as two additions.
	drawPage(f, area, page{lines: lines, scroll: app.Scroll()})
}

func diffStyle(line string) ui.Style {
	switch {
	case strings.HasPrefix(line, "+++"), strings.HasPrefix(line, "---"):
		return theme(style.Heading)
	case strings.HasPrefix(line, "+"):
		return theme(style.Added)
	case strings.HasPrefix(line, "-"):
		return theme(style.Removed)
	case strings.HasPrefix(line, "@@"):
		return theme(style.Hunk)
	case strings.HasPrefix(line, "diff "), strings.HasPrefix(line, "index "):
		return theme(style.Heading)
	}
	return ui.Style{}
}

func drawNote(f *ui.Frame, area ui.Rect, app *App) {
	text, ok := app.Text()
	if !ok {
		return
	}
	// Wrapped, so prose needs no sideways scrolling.
	drawPage(f, area, page{lines: noteLines(text, app.Terms()), scroll: app.Scroll(), wrap: true})
}

func drawHelp(f *ui.Frame, area ui.Rect) {
	lines := make([]ui.Line, len(helpKeys))
	for i, entry := range helpKeys {
		lines[i] = ui.LineOf(ui.Styled(padLeft(entry[0], keyColumn)+"  ", theme(style.ID)), ui.Raw(entry[1]))
	}
	card{title: " keys ", lines: lines, border: theme(style.Muted)}.draw(f, area)
}

// drawCommands cuts to what the terminal holds, keeps the cursor in view and
// counts the rest in the footer, which would otherwise run off the bottom.
func drawCommands(f *ui.Frame, area ui.Rect, app *App) {
	muted := theme(style.Muted)
	shown := MatchingCommands(app.Input.Text())
	width := 0
	for _, spec := range shown {
		width = max(width, runeCount(spec.Usage()))
	}

	// Less two of border, a blank and the footer. One row per command, which
	// holds only because descriptions are cut rather than wrapped.
	room := max(area.Height-4, 1)
	first := max(app.CommandsAt()-(room-1), 0)
	told := max(0, area.Width-(2+width+2))
	var lines []ui.Line
	if len(shown) == 0 {
		lines = append(lines, ui.LineOf(ui.Styled("nothing goes by that", muted)))
	}
	for at := first; at < min(len(shown), first+room); at++ {
		usage := theme(style.ID)
		if at == app.CommandsAt() {
			usage = reversed
		}
		lines = append(lines, ui.LineOf(
			ui.Styled(padRight(shown[at].Usage(), width), usage),
			ui.Styled("  "+cut(shown[at].What, told), muted),
		))
	}

	footer := "type to narrow       enter  put it on the prompt       esc  back"
	if more := max(len(shown)-(first+room), 0); more > 0 {
		footer = fmt.Sprintf("enter  put it on the prompt       esc  back       %d more", more)
	}
	lines = append(lines, ui.Line{}, ui.LineOf(ui.Styled(footer, muted)))

	title := " commands "
	if !app.Input.IsEmpty() {
		title = fmt.Sprintf(" commands: %s ", app.Input.Text())
	}
	card{title: title, lines: lines, border: muted}.draw(f, area)
}

// drawConfirm asks on screen: here a delete is one chord, and in raw mode a
// command reading stdin would steal the browser's keystrokes.
func drawConfirm(f *ui.Frame, area ui.Rect, app *App) {
	muted := theme(style.Muted)
	queued := func() ui.Line {
		return ui.LineRaw(fmt.Sprintf("%s over %s", plural(len(app.Queue), "change"), plural(app.QueuedNotes(), "note")))
	}
	var title, aside, keys string
	var subject ui.Line
	switch app.Confirming {
	case WhatDelete:
		file := app.Selected()
		if file == nil {
			return
		}
		title, keys = " delete this note? ", "y  delete       any other key  keep it"
		aside = "the commit that removes it stays, so git revert brings it back"
		subject = ui.LineOf(ui.Styled(file.ID, theme(style.ID)), ui.Raw("  "), ui.Raw(file.Note.Title))
	// Deletions counted apart: they are why the question is asked.
	case WhatSend:
		title, keys = " send the queue? ", "y  send it       any other key  back to the queue"
		aside = fmt.Sprintf("%s to be deleted — the commit stays, so git revert brings them back",
			plural(app.QueuedDeletions(), "note"))
		subject = queued()
	case WhatQuit:
		title, keys = " leave the queue behind? ", "y  quit anyway       any other key  stay"
		aside = "none of it has happened, and none of it is written down anywhere"
		subject = queued()
	}
	lines := []ui.Line{
		subject,
		{},
		ui.LineOf(ui.Styled(aside, muted)),
		{},
		ui.LineOf(ui.Styled(keys, muted)),
	}
	card{title: title, lines: lines, border: muted}.draw(f, area)
}

// drawQueue gives each line as the sentence the commit message will use.
func drawQueue(f *ui.Frame, area ui.Rect, app *App) {
	muted := theme(style.Muted)
	var lines []ui.Line
	if len(app.Queue) == 0 {
		lines = append(lines, ui.LineOf(ui.Styled("nothing queued — mark some notes, then # or ctrl-d", muted)))
	}
	for at, step := range app.Queue {
		s := ui.Style{}
		if at == app.QueueAt() {
			s = reversed
		}
		lines = append(lines, ui.LineOf(ui.Styled(step.Describe(), s)))
	}
	lines = append(lines, ui.Line{}, ui.LineOf(ui.Styled(
		"enter  send, in one commit       d  drop this one       esc  back", muted)))
	card{title: " queued ", lines: lines, border: muted}.draw(f, area)
}

// drawTagging has a box per notebook tag, in the tags screen's order. The
// count is the tag's note total for one note, or how many of the marked set
// hold it.
func drawTagging(f *ui.Frame, area ui.Rect, app *App) {
	muted := theme(style.Muted)
	total := app.PickingNotes()
	here := app.TagsAt()
	shown := app.ShownTags()
	proposal := app.Proposal()

	// In terminal columns, not characters, so the counts line up past CJK.
	width := 0
	for _, at := range shown {
		if at < len(app.Choices()) {
			width = max(width, cmd.DisplayWidth(app.Choices()[at].Tag))
		}
	}
	if proposal != nil && proposal.Refused == "" {
		width = max(width, cmd.DisplayWidth(proposal.Tag))
	}

	var rows []ui.Line
	for row, at := range shown {
		if at < len(app.Choices()) {
			rows = append(rows, chosen(app.Choices()[at], total, width, row == here))
		}
	}
	if proposal != nil {
		rows = append(rows, proposed(*proposal, width, len(shown) == here))
	}

	// Less two of border, a blank and the footer.
	room := max(area.Height-4, 1)
	first := max(here-(room-1), 0)
	var lines []ui.Line
	if len(rows) == 0 {
		lines = append(lines, ui.LineOf(ui.Styled("no tags yet — type one to make it", muted)))
	} else {
		lines = append(lines, rows[min(first, len(rows)):min(len(rows), first+room)]...)
	}

	// With marks the change is queued, so "apply" would overpromise.
	doing := "apply"
	if len(app.Marks) > 0 {
		doing = "queue it"
	}
	more := max(app.PickerRows()-(first+room), 0)
	footer := fmt.Sprintf("type to narrow      tab  choose      enter  %s      esc  back", doing)
	if more > 0 {
		footer = fmt.Sprintf("tab  choose      enter  %s      esc  back      %d more", doing, more)
	}
	lines = append(lines, ui.Line{}, ui.LineOf(ui.Styled(footer, muted)))

	about := plural(total, "note")
	if file := app.PickingNote(); file != nil {
		about = file.Slug
	}
	title := fmt.Sprintf(" tags: %s ", about)
	if !app.Input.IsEmpty() {
		title = fmt.Sprintf(" tags: %s / %s ", about, app.Input.Text())
	}
	card{title: title, lines: lines, border: muted}.draw(f, area)
}

// chosen is a tag no note carries — one just made on this card — saying `new`,
// not `0 notes`.
func chosen(choice Choice, total, width int, here bool) ui.Line {
	count := plural(choice.Notes, "note")
	switch {
	case choice.Notes == 0:
		count = "new"
	case total > 1:
		count = fmt.Sprintf("%d of %d", choice.Held, total)
	}
	return ui.LineOf(
		ui.Styled(choice.Tick(total)+" ", boxStyle(choice, total)),
		ui.Styled(padded(choice.Tag, width), nameStyle(here)),
		ui.Styled("  "+count, theme(style.Muted)),
	)
}

// proposed is the row offering a typed tag the notebook lacks, unticked until
// chosen.
func proposed(proposal Proposal, width int, here bool) ui.Line {
	// In `cmd`'s words: shown so `Tab` doing nothing is explained.
	if proposal.Refused != "" {
		return ui.LineOf(ui.Styled("    "+proposal.Refused, theme(style.Invalid)))
	}
	spans := []ui.Span{
		ui.Styled("[ ] ", theme(style.Muted)),
		ui.Styled(padded(proposal.Tag, width), nameStyle(here)),
		ui.Styled("  new", theme(style.Muted)),
	}
	// A near-miss of an existing tag is nearly always a misspelling.
	if proposal.Near != "" {
		spans = append(spans, ui.Styled(
			fmt.Sprintf(" — close to %s, %s", proposal.Near, plural(proposal.NearNotes, "note")),
			theme(style.Match)))
	}
	return ui.LineOf(spans...)
}

// padded pads in terminal columns, not characters.
func padded(name string, width int) string {
	return name + strings.Repeat(" ", max(0, width-cmd.DisplayWidth(name)))
}

func nameStyle(here bool) ui.Style {
	if here {
		return reversed
	}
	return theme(style.Tags)
}

// boxStyle is the diff colours for a change, the tags' colour for a tick,
// muted otherwise.
func boxStyle(choice Choice, total int) ui.Style {
	switch {
	case choice.Mark == MarkAdd:
		return theme(style.Added)
	case choice.Mark == MarkRemove:
		return theme(style.Removed)
	case total > 0 && choice.Held == total:
		return theme(style.Tags)
	}
	return theme(style.Muted)
}

// drawAlert is a card for a multi-line answer the status line would cut, such
// as `bulk`'s list of what it could not do.
func drawAlert(f *ui.Frame, area ui.Rect, app *App) {
	if app.Message == nil {
		return
	}
	var lines []ui.Line
	for _, line := range splitLines(app.Message.Text) {
		lines = append(lines, ui.LineRaw(line))
	}
	title, border := " done ", theme(style.Muted)
	if app.Message.Failed {
		title, border = " no ", theme(style.Invalid)
	}
	card{title: title, lines: lines, border: border}.draw(f, area)
}

// cut keeps whole characters, not bytes.
func cut(text string, room int) string {
	runes := []rune(text)
	if len(runes) <= room {
		return text
	}
	kept := string(runes[:max(room-1, 0)])
	return strings.TrimRight(kept, " \t\n\r") + "…"
}

// tagList is the uncoloured tag list, built from the same pieces as the row so
// the column width cannot drift from it.
func tagList(tags []string) string {
	var text strings.Builder
	for _, piece := range style.TagPieces(tags) {
		text.WriteString(piece.Text)
	}
	return text.String()
}

func tagsLine(tags []string) ui.Line {
	var spans []ui.Span
	for _, piece := range style.TagPieces(tags) {
		spans = append(spans, ui.Styled(piece.Text, theme(piece.Style)))
	}
	return ui.LineOf(spans...)
}

// splitLines is Rust's `str::lines`: no empty last line, and a `\r` before the
// `\n` goes too.
func splitLines(text string) []string {
	if text == "" {
		return nil
	}
	lines := strings.Split(strings.TrimSuffix(text, "\n"), "\n")
	for i, line := range lines {
		lines[i] = strings.TrimSuffix(line, "\r")
	}
	return lines
}

// noteLines dims the frontmatter and marks search terms in the body.
func noteLines(text string, terms []string) []ui.Line {
	muted := theme(style.Muted)
	front, body := splitFrontmatter(text)
	var out []ui.Line
	for _, line := range splitLines(front) {
		out = append(out, ui.LineOf(ui.Styled(line, muted)))
	}
	for _, line := range splitLines(body) {
		out = append(out, marked(line, terms, ui.Style{}))
	}
	return out
}

// splitFrontmatter judges as `cmd.dim_frontmatter` does: no closed block,
// nothing dimmed.
func splitFrontmatter(text string) (front, body string) {
	rest, ok := strings.CutPrefix(text, "---\n")
	if !ok {
		return "", text
	}
	end := strings.Index(rest, "\n---\n")
	if end < 0 {
		return "", text
	}
	cutAt := len("---\n") + end + len("\n---\n")
	return text[:cutAt], text[cutAt:]
}

// marked lets the earliest match win an overlap and resumes the search after
// it, so a line is walked once.
func marked(text string, terms []string, base ui.Style) ui.Line {
	var spans []ui.Span
	at := 0
	for at < len(text) {
		start, end, found := nextMatch(text[at:], terms)
		if !found {
			break
		}
		start, end = at+start, at+end
		if start > at {
			spans = append(spans, ui.Styled(text[at:start], base))
		}
		spans = append(spans, ui.Styled(text[start:end], theme(style.Match)))
		at = end
	}
	if at < len(text) || len(spans) == 0 {
		spans = append(spans, ui.Styled(text[at:], base))
	}
	return ui.LineOf(spans...)
}

// nextMatch is the earliest match of any term; an empty term matches nothing.
func nextMatch(text string, terms []string) (start, end int, found bool) {
	for _, term := range terms {
		if term == "" {
			continue
		}
		s, e, ok := cmd.FindIgnoringCase(text, term)
		if ok && (!found || s < start) {
			start, end, found = s, e, true
		}
	}
	return start, end, found
}
