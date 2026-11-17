package tui

import (
	"fmt"
	"strings"

	"github.com/henry40408/noda/internal/cmd"
	"github.com/henry40408/noda/internal/query"
)

type edge uint8

const (
	edgeFirst edge = iota
	edgeLast
)

// OnKey applies a keystroke. A nil result means the state moved and nothing
// else.
func (a *App) OnKey(key Key) *Action {
	// Quits from anywhere without asking about the queue, as the terminal
	// would; `q` is the key that asks.
	if key.Ctrl && key.Code == CodeChar && key.Char == 'c' {
		return &Action{Kind: ActQuit}
	}
	switch a.Mode {
	case ModeHelp, ModeAlert:
		// Any key dismisses either card.
		a.Mode = ModeBrowse
		a.Message = nil
		return nil
	case ModeSearch:
		return a.searching(key)
	case ModeAsk:
		return a.asking(key)
	case ModeCommand:
		return a.commanding(key)
	case ModeCommands:
		return a.listingCommands(key)
	case ModeConfirm:
		return a.confirming(key)
	case ModeQueue:
		return a.queueing(key)
	case ModeTagging:
		return a.picking(key)
	}
	return a.browsing(key)
}

func isChar(key Key, c rune) bool { return key.Code == CodeChar && key.Char == c }

func (a *App) browsing(key Key) *Action {
	a.Message = nil
	if key.Ctrl {
		return a.chord(key)
	}
	if key.Code == CodeChar {
		if action, handled := a.everywhere(key.Char); handled {
			return action
		}
	}
	// A list is walked and a page scrolled.
	switch {
	case a.top().View.Kind == ViewNotes:
		return a.onListing(key)
	case a.HasRows():
		return a.onRows(key)
	}
	return a.onReading(key)
}

// everywhere answers the keys that mean the same on every screen.
func (a *App) everywhere(c rune) (*Action, bool) {
	switch c {
	case 'q':
		return a.leaving(), true
	case 'r':
		return &Action{Kind: ActReload}, true
	case '?':
		a.Mode = ModeHelp
	// Subcommands without a key of their own are typed.
	case ':':
		a.Mode = ModeCommand
		a.Input.Clear()
		a.historyAt = -1
	case 'T':
		if a.Touch == cmd.TouchStamp {
			a.Touch = cmd.TouchKeep
		} else {
			a.Touch = cmd.TouchStamp
		}
	// Each aims at whatever the screen is about.
	case 'e':
		file := a.Selected()
		if file == nil {
			return nil, true
		}
		return &Action{Kind: ActEdit, Key: file.ID, Touch: a.Touch}, true
	case 'a':
		a.ask(AskTitle, "")
	case 'm':
		file := a.Selected()
		if file != nil {
			a.ask(AskRetitle, file.Note.Title)
		}
	// A toggle: the row shows which way it will go.
	case 'p':
		file := a.Selected()
		if file == nil {
			a.refuse("no note on screen — open one first")
			return nil, true
		}
		return &Action{Kind: ActPin, Key: file.ID, Pinned: !file.Note.IsPinned(), Touch: a.Touch}, true
	case '#':
		keys := a.markedKeys()
		if len(keys) == 0 {
			file := a.Selected()
			if file == nil {
				return nil, true
			}
			keys = []string{file.ID}
		}
		a.pick(keys)
	case 'Q':
		a.Mode = ModeQueue
		a.queueAt = min(a.queueAt, max(len(a.Queue)-1, 0))
	// Screens about the note in front of you, plus the todo list; the other
	// screens are typed.
	case 't':
		a.open(View{Kind: ViewTodo})
	// The notebook's or one note's, by what the screen is about, as `:log`.
	case 'l':
		a.open(LogView(a.about()))
	case 'b':
		if subject, ok := a.linkable(); ok {
			a.open(BacklinksView(subject))
		}
	case 'B':
		if id, ok := a.aimed(""); ok {
			a.open(BlameView(id))
		}
	// `d` used to delete; it points at the new key rather than quietly
	// meaning something else.
	case 'd':
		a.Message = &Message{Text: "delete is Ctrl-d"}
	default:
		// The nine commonest tags, as numbered on the tags screen, `0` the way
		// out. From any screen: it returns to the listing.
		if c >= '0' && c <= '9' {
			a.scopeNth(int(c - '0'))
			return nil, true
		}
		return nil, false
	}
	return nil, true
}

// chord handles Ctrl keys. Delete is one, being the key that cannot be taken
// back.
func (a *App) chord(key Key) *Action {
	half := max(a.page, 2) / 2
	if key.Code != CodeChar {
		return nil
	}
	switch key.Char {
	case 'f':
		a.step(half)
	case 'b':
		a.step(-half)
	case 'd':
		return a.delete()
	case 'a':
		a.Mode = ModeCommands
		a.Input.Clear()
		a.commandsAt = 0
	// Only the listing has a long form.
	case 'w':
		if a.top().View.Kind == ViewNotes {
			a.Long = !a.Long
		}
	case 'g':
		a.CrumbsShown = !a.CrumbsShown
	}
	return nil
}

// delete is the marked notes, or the one on the screen.
func (a *App) delete() *Action {
	if len(a.Marks) == 0 {
		if a.Selected() == nil {
			return nil
		}
		a.Mode = ModeConfirm
		a.Confirming = WhatDelete
		return nil
	}
	// Queued unasked; the confirmation comes at the send.
	a.enqueue(cmd.Step{Keys: a.markedKeys(), Change: cmd.Change{Remove: true}})
	return nil
}

// walk handles the movement keys every screen shares and says whether the key
// was one.
func (a *App) walk(key Key) bool {
	switch {
	case isChar(key, 'j'), key.Code == CodeDown:
		a.step(1)
	case isChar(key, 'k'), key.Code == CodeUp:
		a.step(-1)
	case key.Code == CodePageDown:
		a.step(a.page)
	case key.Code == CodePageUp:
		a.step(-a.page)
	case isChar(key, 'g'), key.Code == CodeHome:
		a.jump(edgeFirst)
	case isChar(key, 'G'), key.Code == CodeEnd:
		a.jump(edgeLast)
	default:
		return false
	}
	return true
}

// onListing is the listing: moving through it, narrowing it, marking it, and
// opening what the cursor is on.
func (a *App) onListing(key Key) *Action {
	if a.walk(key) {
		return nil
	}
	switch {
	case isChar(key, '/'):
		a.Mode = ModeSearch
	// `--sort` and `-r`; shifted, being about the listing and `r` taken.
	case isChar(key, 'S'):
		a.Sort = a.Sort.Next()
		a.reorder()
	case isChar(key, 'R'):
		a.Reverse = !a.Reverse
		a.reorder()
	// A screen of its own, not a split, so it is read at full width.
	case key.Code == CodeEnter:
		if file := a.Selected(); file != nil {
			a.open(NoteView(file.ID))
		}
	// `Space` marks the one under the cursor, `*` everything shown.
	case isChar(key, ' '):
		if file := a.Selected(); file != nil {
			if a.Marks[file.ID] {
				delete(a.Marks, file.ID)
			} else {
				a.Marks[file.ID] = true
			}
		}
	case isChar(key, '*'):
		a.markShown()
	// Clears the query first, then the marks, a query being cheaper to retype.
	// Never the queue.
	case key.Code == CodeEsc:
		if a.top().Search.IsEmpty() {
			clear(a.Marks)
		} else {
			a.top().Search.Clear()
			a.refilter()
		}
	}
	return nil
}

// onReading is a note, a patch, a blame: scrolling it and closing it again.
func (a *App) onReading(key Key) *Action {
	if a.walk(key) {
		return nil
	}
	if key.Code == CodeEsc {
		a.back()
	}
	return nil
}

// onRows is the other lists: walking, closing, and what the row is for.
func (a *App) onRows(key Key) *Action {
	if a.walk(key) {
		return nil
	}
	switch key.Code {
	case CodeEnter:
		return a.chose()
	case CodeEsc:
		a.back()
	}
	return nil
}

// chose is Enter on a row: a note opens, another screen opens, and anything
// irreversible goes onto the prompt rather than running.
func (a *App) chose() *Action {
	at := a.top().Table.Selected
	if at < 0 {
		return nil
	}
	view := a.top().View
	switch view.Kind {
	case ViewTodo, ViewBacklinks:
		if file := a.Selected(); file != nil {
			a.open(NoteView(file.ID))
		}
	// Narrows the listing, as `1`-`9` do.
	case ViewTags:
		if tallies := a.Tallies(); at < len(tallies) {
			a.scope(tallies[at].Tag)
		}
	case ViewFiles:
		if at < len(a.files) {
			a.open(BacklinksView(Subject{File: true, Name: a.files[at]}))
		}
	case ViewNotebooks:
		if at < len(a.notebooks) {
			return a.switchTo(a.notebooks[at])
		}
	case ViewDeleted:
		if gone := a.Gone(); at < len(gone) {
			a.compose(fmt.Sprintf("restore %s %s", gone[at].ID, gone[at].RestoreFromShort()))
		}
	// A commit in one note's history is a revision to restore; the notebook's
	// log names no note.
	case ViewLog:
		if entries := a.Entries(); view.Name != "" && at < len(entries) {
			a.compose(fmt.Sprintf("restore %s %s", view.Name, entries[at].ShortID()))
		}
	}
	return nil
}

// compose puts a command on the prompt, written out but not run.
func (a *App) compose(line string) {
	a.Mode = ModeCommand
	a.Input.Set(line)
	a.historyAt = -1
}

// arrange is cmd.SortNotes, so the orders are `--sort`'s; reversed after.
func (a *App) arrange() {
	cmd.SortNotes(a.notes, a.Sort)
	if a.Reverse {
		for i, j := 0, len(a.notes)-1; i < j; i, j = i+1, j-1 {
			a.notes[i], a.notes[j] = a.notes[j], a.notes[i]
		}
	}
}

// reorder re-sorts, keeping the cursor on the note rather than the row.
func (a *App) reorder() {
	was := ""
	if file := a.atCursor(); file != nil {
		was = file.ID
	}
	a.arrange()
	a.refilter()
	if was != "" {
		a.SelectID(was)
	}
	// Derived screens hold indices, and the notes have just moved.
	a.loaded = nil
	a.settle()
}

// scopeNth narrows to the tags screen's nth tag; 0 clears the query.
func (a *App) scopeNth(nth int) {
	if nth == 0 {
		for a.back() {
		}
		a.top().Search.Clear()
		a.refilter()
		return
	}
	content := a.derive(View{Kind: ViewTags})
	if content == nil || nth > len(content.Tallies) {
		return
	}
	a.scope(content.Tallies[nth-1].Tag)
}

// scope writes the query, quotes and all, as the web does.
func (a *App) scope(tag string) {
	for a.back() {
	}
	a.top().Search.Set(query.Scoped(tag))
	a.refilter()
}

// switchTo is refused while anything is queued: an entry's ids mean nothing,
// or the wrong thing, in another notebook.
func (a *App) switchTo(name string) *Action {
	if name == a.Notebook {
		a.refuse(fmt.Sprintf("`%s` is the notebook you are in", name))
		return nil
	}
	if len(a.Queue) > 0 {
		a.refuse(fmt.Sprintf("%d queued against `%s` — send it or drop it first (Q)", len(a.Queue), a.Notebook))
		return nil
	}
	return &Action{Kind: ActUse, Key: name}
}

// markShown marks everything shown, or unmarks it when it is all marked. Notes
// the query hides are untouched.
func (a *App) markShown() {
	shown := a.Rows()
	all := true
	for _, file := range shown {
		all = all && a.Marks[file.ID]
	}
	for _, file := range shown {
		if all {
			delete(a.Marks, file.ID)
		} else {
			a.Marks[file.ID] = true
		}
	}
}

// enqueue checks here rather than at the send, so a bad tag is refused where it
// was typed.
func (a *App) enqueue(step cmd.Step) {
	if err := cmd.Check(step.Change); err != nil {
		a.Report("", err)
		return
	}
	a.Queue = append(a.Queue, step)
}

func (a *App) queueing(key Key) *Action {
	switch {
	case isChar(key, 'j'), key.Code == CodeDown:
		a.queueAt = min(a.queueAt+1, max(len(a.Queue)-1, 0))
	case isChar(key, 'k'), key.Code == CodeUp:
		a.queueAt = max(a.queueAt-1, 0)
	// A plain `d` is safe here: nothing in this card deletes a note.
	case isChar(key, 'd'), isChar(key, 'x'), key.Code == CodeBackspace:
		if a.queueAt < len(a.Queue) {
			a.Queue = append(a.Queue[:a.queueAt], a.Queue[a.queueAt+1:]...)
			a.queueAt = min(a.queueAt, max(len(a.Queue)-1, 0))
		}
	case key.Code == CodeEnter:
		return a.send()
	case key.Code == CodeEsc, isChar(key, 'q'), isChar(key, 'Q'):
		a.Mode = ModeBrowse
	}
	return nil
}

// send sends the queue, once anything irreversible in it has been agreed to.
func (a *App) send() *Action {
	if len(a.Queue) == 0 {
		a.Mode = ModeBrowse
		return nil
	}
	// Asked only for a deletion: a confirmation shown every time is not read.
	if a.queueRemoves() {
		a.Mode = ModeConfirm
		a.Confirming = WhatSend
		return nil
	}
	a.Mode = ModeBrowse
	return &Action{Kind: ActSend, Steps: a.copyQueue()}
}

func (a *App) queueRemoves() bool {
	for _, step := range a.Queue {
		if step.Change.Remove {
			return true
		}
	}
	return false
}

func (a *App) copyQueue() []cmd.Step { return append([]cmd.Step(nil), a.Queue...) }

// QueuedNotes is how many distinct notes the queue touches.
func (a *App) QueuedNotes() int {
	notes := map[string]bool{}
	for _, step := range a.Queue {
		for _, key := range step.Keys {
			notes[key] = true
		}
	}
	return len(notes)
}

// QueuedDeletions is how many of the queued changes would delete something.
func (a *App) QueuedDeletions() int {
	total := 0
	for _, step := range a.Queue {
		if step.Change.Remove {
			total += len(step.Keys)
		}
	}
	return total
}

// QueueAt is which entry the queue view has its cursor on.
func (a *App) QueueAt() int { return a.queueAt }

// Sent is called once the queue has been carried out; clears it and the marks.
func (a *App) Sent() {
	a.Queue = nil
	a.queueAt = 0
	clear(a.Marks)
}

func (a *App) searching(key Key) *Action {
	// The field handles editing (and knows a chord is not a character); what it
	// leaves unbound falls through.
	switch a.top().Search.Key(key) {
	case Typed:
		a.refilter()
		return nil
	case Moved:
		return nil
	}
	switch {
	case key.Code == CodeEnter:
		a.Mode = ModeBrowse
	// Clears the query, unlike Enter.
	case key.Code == CodeEsc:
		a.top().Search.Clear()
		a.refilter()
		a.Mode = ModeBrowse
	// The listing can be walked while typing. Ctrl-N/P walk it too, as in other
	// narrowing lists, not readline's history.
	case key.Code == CodeDown, key.Ctrl && isChar(key, 'n'):
		a.step(1)
	case key.Code == CodeUp, key.Ctrl && isChar(key, 'p'):
		a.step(-1)
	}
	return nil
}

// ask opens the prompt holding start, e.g. the current title for a retitle.
func (a *App) ask(what Ask, start string) {
	a.Mode = ModeAsk
	a.Asking = what
	a.Input.Set(start)
}

func (a *App) asking(key Key) *Action {
	if a.Input.Key(key) != NoEdit {
		return nil
	}
	switch key.Code {
	case CodeEnter:
		return a.answered()
	case CodeEsc:
		a.Mode = ModeBrowse
		a.Input.Clear()
	}
	return nil
}

// answered treats an empty answer as a way out, not an error — except for a
// new note, where it is `noda add` with no title.
func (a *App) answered() *Action {
	answer := strings.TrimSpace(a.Input.Text())
	what := a.Asking
	a.Mode = ModeBrowse
	a.Input.Clear()
	if what == AskTitle {
		return &Action{Kind: ActAdd, Title: answer, HasTitle: answer != ""}
	}
	if answer == "" {
		return nil
	}
	file := a.Selected()
	if file == nil {
		return nil
	}
	return &Action{Kind: ActRetitle, Key: file.ID, Title: answer, Touch: a.Touch}
}

// leaving quits, or asks first if anything is queued.
func (a *App) leaving() *Action {
	if len(a.Queue) == 0 {
		return &Action{Kind: ActQuit}
	}
	a.Mode = ModeConfirm
	a.Confirming = WhatQuit
	return nil
}

// confirming: `y` agrees; any other key cancels.
func (a *App) confirming(key Key) *Action {
	a.Mode = ModeBrowse
	if !isChar(key, 'y') && !isChar(key, 'Y') {
		return nil
	}
	switch a.Confirming {
	case WhatDelete:
		if file := a.Selected(); file != nil {
			return &Action{Kind: ActRemove, Key: file.ID}
		}
		return nil
	case WhatSend:
		return &Action{Kind: ActSend, Steps: a.copyQueue()}
	}
	return &Action{Kind: ActQuit}
}

// refuse puts a refusal on the status line, not a card: cards are for what a
// command said when it ran, and this never ran.
func (a *App) refuse(text string) {
	a.Message = &Message{Text: text, Failed: true}
}

// step moves the cursor, or scrolls the page, by delta rows.
func (a *App) step(delta int) {
	rows := a.rowsHere()
	if rows < 0 {
		a.top().Scroll = scrolled(a.top().Scroll, delta, a.readingHeight())
		return
	}
	at := a.top().Table.Selected
	if at < 0 {
		return
	}
	a.cursorTo(max(min(at+delta, max(rows-1, 0)), 0))
}

func (a *App) jump(to edge) {
	rows := a.rowsHere()
	if rows < 0 {
		if to == edgeFirst {
			a.top().Scroll = 0
		} else {
			a.top().Scroll = a.readingHeight()
		}
		return
	}
	if to == edgeFirst {
		a.cursorTo(0)
		return
	}
	a.cursorTo(max(rows-1, 0))
}

// readingHeight is the furthest scroll: the last line at the top of the
// screen. Counted before wrapping, which under-shoots on long lines — better
// than scrolling into blank space.
func (a *App) readingHeight() int {
	lines := 0
	if c := a.content(); c != nil {
		switch c.Kind {
		case ContentNote, ContentDiff:
			lines = countLines(c.Text)
		case ContentBlame:
			lines = len(c.Blame)
		}
	}
	return max(lines-1, 0)
}

// countLines counts the way Rust's str::lines does: a trailing newline does
// not start another line.
func countLines(text string) int {
	if text == "" {
		return 0
	}
	return strings.Count(strings.TrimSuffix(text, "\n"), "\n") + 1
}

// scrolled is a scroll offset moved by delta and kept inside [0, max].
func scrolled(from, delta, limit int) int {
	return min(max(from+delta, 0), limit)
}

// cursorTo is clamped to the screen's rows; an empty list has no cursor at all.
func (a *App) cursorTo(at int) {
	rows := max(a.rowsHere(), 0)
	screen := a.top()
	if rows == 0 {
		screen.Table.Selected = -1
		return
	}
	screen.Table.Selected = min(at, rows-1)
}

func (a *App) selectRow(at int) {
	listing := a.listing()
	if len(listing.visible) == 0 {
		listing.Table.Selected = -1
		return
	}
	listing.Table.Selected = min(at, len(listing.visible)-1)
}

// refilter reruns the query over every note — whole, not incremental, since a
// keystroke can widen a query too, and it is well under a frame.
//
// Split as a shell would, since query.Parse takes shell arguments and no shell
// stands in front of this field; otherwise `tag:"12.34 foo bar"` would be three
// terms.
func (a *App) refilter() {
	listing := a.listing()
	tokens := query.Split(listing.Search.Text())
	if len(tokens) == 0 {
		listing.HasError, listing.Error = false, ""
		listing.Terms = nil
		listing.visible = allIndices(len(a.notes))
		a.selectRow(0)
		return
	}
	parsed, err := query.Parse(tokens)
	// Half a query is normal while typing: say so and keep the last good result
	// rather than emptying the list.
	if err != nil {
		listing.HasError, listing.Error = true, err.Error()
		return
	}
	visible := []int{}
	for at, file := range a.notes {
		if parsed.Matches(file.ID, file.Note) {
			visible = append(visible, at)
		}
	}
	listing.HasError, listing.Error = false, ""
	listing.Terms = parsed.ExcerptTerms()
	listing.visible = visible
	a.selectRow(0)
}
