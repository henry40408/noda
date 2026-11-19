// Package tui is the terminal browser. app.go holds what a browsing session
// holds and how one keystroke changes it.
//
// Nothing here opens a file, a repository or a terminal: a key goes in, the
// state moves, and anything needing the outside world comes back as an
// Action, so the whole interaction is testable with no terminal.
//
// Keys that change a note ask a cmd command to do it and show the line it
// would have printed; what a change means is written once, in cmd.
//
// A session is a stack of screens, the notes at the bottom and never popped.
// Each keeps its own cursor, query and scroll, so going back lands where you
// left.
//
// The notes are held in memory for the session, paying once the body reads
// `noda search` pays per query.
package tui

import (
	"path/filepath"
	"slices"
	"sort"
	"strings"
	"unicode"

	"github.com/henry40408/noda/internal/cmd"
	"github.com/henry40408/noda/internal/note"
	"github.com/henry40408/noda/internal/notebook"
	"github.com/henry40408/noda/internal/style"
	"github.com/henry40408/noda/internal/todo"
	"github.com/henry40408/noda/internal/tui/ui"
)

// ActionKind says which of the things the runtime has to do an Action is.
type ActionKind uint8

const (
	ActQuit ActionKind = iota
	// ActReload re-reads the notebook: the answer to another process writing,
	// rather than a file watcher.
	ActReload
	// ActEdit is the one action needing the terminal handed back, to $EDITOR.
	ActEdit
	// ActAdd leaves the title to the body when HasTitle is false, as `add`
	// does. It has no Touch: `add` has none.
	ActAdd
	// ActRetitle is `noda mv`: a new title, and the slug follows it.
	ActRetitle
	// ActTag is `noda tag`, with the +tag / -tag changes as typed.
	ActTag
	// ActPin is `noda pin` / `noda unpin`, one kind because the key is a toggle.
	ActPin
	// ActRemove is `noda rm`, once confirmed.
	ActRemove
	// ActSend is every change in the queue, in one commit via cmd.Bulk.
	ActSend
	// ActRestore is `noda restore`: a new commit, so nothing is rewritten and
	// nothing asked.
	ActRestore
	// ActOpen opens a note the prompt named. The runtime resolves the key
	// through Notebook.Resolve, so there is one answer to what a prefix or
	// slug names.
	ActOpen
	// ActShow is a screen about a note named by key, resolved likewise.
	ActShow
	// ActUse is not a Run: the runtime builds a new session for the other
	// notebook.
	ActUse
	// ActRun is a command that reads or changes the notebook and answers with
	// a line.
	ActRun
)

// Action is something the runtime has to do that the state cannot do for
// itself. Which fields mean something depends on Kind.
//
// Changes name a command and its arguments, never an edit, since what a change
// means lives in cmd. A note is named by id rather than by row: the command
// reopens the notebook, and by then the listing may be stale.
type Action struct {
	Kind ActionKind
	// Key is the note (ActEdit, ActRetitle, ActTag, ActPin, ActRemove,
	// ActRestore, ActOpen, ActShow) or the notebook's name (ActUse).
	Key      string
	Touch    cmd.Touch
	Title    string
	HasTitle bool
	Changes  []string
	Pinned   bool
	Steps    []cmd.Step
	Rev      string
	Look     Look
	Run      Run
}

// ScopeKeys is how many tags get a digit key: 1-9, 0 being the way out. The
// long tail is what `/` is for.
const ScopeKeys = 9

// Look is a screen about one note, named before the note is known.
type Look uint8

const (
	LookLog Look = iota
	LookBlame
	LookBacklinks
)

// RunKind is one variant per command rather than a closure, so what the
// browser can ask for is a readable list and only the runtime knows how a
// call is made.
type RunKind uint8

const (
	RunStatus RunKind = iota
	// RunDoctor is reporting only: a keystroke should not rewrite a directory.
	RunDoctor
	RunReadme
	// RunSnapshot marks the notebook with Name; no name lists what has been
	// marked.
	RunSnapshot
	RunSync
	RunPush
	RunPull
)

// Run is a command the runtime executes and answers with a line.
type Run struct {
	Kind    RunKind
	Links   bool
	Times   bool
	Name    string
	HasName bool
}

// Working is a label for a slow command, drawn before it runs; otherwise the
// last frame stays up with no sign of anything happening. Empty for the rest.
func (a Action) Working() string {
	if a.Kind != ActRun {
		return ""
	}
	switch a.Run.Kind {
	case RunSync:
		return "syncing…"
	case RunPush:
		return "pushing…"
	case RunPull:
		return "pulling…"
	}
	return ""
}

// Message is a command's answer in its own words, not summarised by the
// browser. Failed gets a card; a success goes on the status line, since a
// reason has to be read and an acknowledgement only glanced at.
type Message struct {
	Text   string
	Failed bool
}

// Line is the first line, for the status bar.
func (m Message) Line() string {
	line, _, _ := strings.Cut(m.Text, "\n")
	return strings.TrimSuffix(line, "\r")
}

// Subject is what a backlinks screen is of: a note or a file, one walk either
// way, as `noda backlinks` takes either.
type Subject struct {
	File bool
	Name string
}

// ViewKind names a screen.
type ViewKind uint8

const (
	ViewNotes ViewKind = iota
	ViewNote
	// ViewTodo is every unticked box in the notebook, soonest due first.
	ViewTodo
	// ViewTags is every tag, and how many notes carry it.
	ViewTags
	// ViewFiles is what the notebook holds that is not a note.
	ViewFiles
	// ViewNotebooks is every notebook there is, and which one this is.
	ViewNotebooks
	// ViewDeleted is the notes history holds that the notebook no longer does.
	ViewDeleted
	// ViewDiff is what is uncommitted, or what the last commit did.
	ViewDiff
	// ViewLog is commits, newest first: one note's, or the whole notebook's.
	ViewLog
	// ViewBacklinks is what links to a note or to a file.
	ViewBacklinks
	// ViewBlame is which commit put each line of a note where it is.
	ViewBlame
)

// View is a screen. Name is a note's id for ViewNote, ViewBlame and ViewLog
// (empty for the notebook's whole log) and the subject's name for
// ViewBacklinks, where File says which kind. Notes are by id rather than row,
// so a view survives the listing being filtered, re-sorted or reloaded.
type View struct {
	Kind ViewKind
	Name string
	File bool
}

// Constructors for the views that carry a name.
func NoteView(id string) View { return View{Kind: ViewNote, Name: id} }

// LogView is one note's history, or the notebook's when id is empty.
func LogView(id string) View { return View{Kind: ViewLog, Name: id} }

func BlameView(id string) View { return View{Kind: ViewBlame, Name: id} }

func BacklinksView(subject Subject) View {
	return View{Kind: ViewBacklinks, Name: subject.Name, File: subject.File}
}

// Subject is what a backlinks view is of.
func (v View) Subject() Subject { return Subject{File: v.File, Name: v.Name} }

// Crumb is what the crumb trail calls it. Which note a screen is about is on
// the title band instead, or a deep stack's trail outgrows the screen.
func (v View) Crumb() string {
	switch v.Kind {
	case ViewNote:
		return v.Name
	case ViewTodo:
		return "todo"
	case ViewTags:
		return "tags"
	case ViewFiles:
		return "files"
	case ViewNotebooks:
		return "notebooks"
	case ViewDeleted:
		return "deleted"
	case ViewDiff:
		return "diff"
	case ViewLog:
		return "log"
	case ViewBacklinks:
		return "backlinks"
	case ViewBlame:
		return "blame"
	}
	return "notes"
}

// ContentKind says which field of Content is filled.
type ContentKind uint8

const (
	// ContentNote is a note as it is on disk, not rendered back from the parse.
	ContentNote ContentKind = iota
	ContentTodo
	ContentTags
	ContentBacklinks
	ContentLog
	ContentBlame
	ContentDeleted
	// ContentDiff is uncoloured; the drawing colours it.
	ContentDiff
)

// Content is what the top screen shows, once worked out.
//
// Only one is kept: a cache of a notebook another process writes to goes
// stale quietly. Some the session derives itself (App.derive); some need a
// repository only the runtime may open (App.Wanted).
type Content struct {
	Kind ContentKind
	// Text is a note's file or a diff.
	Text    string
	Tasks   []Task
	Tallies []Tally
	// Linking is indices into the session's notes: the ones that link to the
	// subject.
	Linking []int
	// Entries is the history, and Unpushed which commits the remote has not
	// seen (empty with no upstream, as `noda log`).
	Entries  []notebook.Entry
	Unpushed map[string]bool
	Blame    []notebook.BlameLine
	Gone     []notebook.Deleted
}

// Task is one unticked box, and the note carrying it.
type Task struct {
	// Note is an index, the list being rebuilt whenever the notes are.
	Note int
	Item todo.Item
}

// Tally is one tag, and how many notes carry it.
type Tally struct {
	Tag   string
	Notes int
}

// Mark has three states rather than a tick: on a mixed set, leaving a tag
// alone differs from giving it to all or taking it from all.
type Mark uint8

const (
	// MarkLeave means each note keeps what it has.
	MarkLeave Mark = iota
	MarkAdd
	MarkRemove
)

// Choice is a tag, how it stands with the notes aimed at, and what is to be
// done.
type Choice struct {
	Tag string
	// Held is how many of those notes already carry it.
	Held int
	// Notes is how many notes in the notebook carry it, as on the tags screen.
	Notes int
	Mark  Mark
}

// states are the marks that would change something: a tag every note carries
// can only be removed and one none carries only added.
func (c Choice) states(total int) []Mark {
	out := []Mark{MarkLeave}
	if c.Held < total {
		out = append(out, MarkAdd)
	}
	if c.Held > 0 {
		out = append(out, MarkRemove)
	}
	return out
}

// next is the next of them, for Tab.
func (c Choice) next(total int) Mark {
	states := c.states(total)
	for at, state := range states {
		if state == c.Mark {
			return states[(at+1)%len(states)]
		}
	}
	return states[0]
}

// Tick is `[x]` for what is true and `[+]`/`[-]` for what is being done.
// ASCII because `☑` is ambiguous-width and can push the row's end off screen.
func (c Choice) Tick(total int) string {
	switch {
	case c.Mark == MarkAdd:
		return "[+]"
	case c.Mark == MarkRemove:
		return "[-]"
	case total > 0 && c.Held == total:
		return "[x]"
	}
	return "[ ]"
}

// Proposal is the row for typed text that is not an existing tag. A new tag
// can be a typo, so it takes its own keystroke and shows its near neighbour.
type Proposal struct {
	// Tag is a tag that could be made.
	Tag string
	// Near is an existing one a typo away, with its note count; empty if none.
	Near      string
	NearNotes int
	// Refused is set instead when it could not be made, in note.ValidateTag's
	// own words.
	Refused string
}

// oneEditApart is whether two tags are one keystroke apart: equal ignoring
// case, or one character added, dropped, mistyped or swapped with its
// neighbour. Not plain edit distance, which counts a transposition, the
// commonest typo, as two.
func oneEditApart(left, right string) bool {
	l := []rune(strings.ToLower(left))
	r := []rune(strings.ToLower(right))
	if abs(len(l)-len(r)) > 1 {
		return false
	}
	head := 0
	for head < len(l) && head < len(r) && l[head] == r[head] {
		head++
	}
	tail := 0
	for tail < len(l)-head && tail < len(r)-head && l[len(l)-1-tail] == r[len(r)-1-tail] {
		tail++
	}
	dl, dr := len(l)-head-tail, len(r)-head-tail
	switch {
	case dl <= 1 && dr <= 1:
		return true
	case dl == 2 && dr == 2:
		return l[head] == r[head+1] && l[head+1] == r[head]
	}
	return false
}

func abs(n int) int {
	if n < 0 {
		return -n
	}
	return n
}

// NeedKind says what the runtime has to fetch.
type NeedKind uint8

const (
	NeedNote NeedKind = iota
	NeedLog
	NeedBlame
	NeedDeleted
	NeedDiff
)

// Need is something a screen needs that only the runtime can fetch.
type Need struct {
	Kind NeedKind
	// ID is the note (NeedNote, NeedBlame) or, for NeedLog, the note whose
	// history is wanted, empty for the notebook's.
	ID   string
	Path string
	Slug string
}

// Screen is what a screen is of, and everything about it a keystroke can move
// — per screen, so going back lands where you left.
type Screen struct {
	View View
	// Table is a listing's cursor and scroll offset.
	Table ui.TableState
	// Scroll is how far a screen of text has been scrolled.
	Scroll int
	// Search is split as a shell would; this is what `noda search` takes.
	Search Field
	// Terms are words to highlight in a title and body; not `tag:` or `id:`,
	// which match nothing in the prose.
	Terms []string
	// Error is why the query as typed is not a query yet.
	Error    string
	HasError bool
	// visible is indices into the session's notes that this screen's query
	// admits, in the same order. Empty on a screen that is not a listing.
	visible []int
}

func newScreen(view View, terms []string) *Screen {
	return &Screen{View: view, Table: ui.NewTableState(), Terms: terms}
}

// Mode is what the keyboard currently means.
type Mode uint8

const (
	ModeBrowse Mode = iota
	// ModeSearch narrows the list on every keystroke; Enter only returns to
	// the list.
	ModeSearch
	ModeHelp
	// ModeAsk is typing the one thing a change needs said in words; the App's
	// Asking says which prompt.
	ModeAsk
	// ModeCommand is typed on the query's line; only one can be open at a time.
	ModeCommand
	// ModeCommands is the command palette.
	ModeCommands
	// ModeConfirm is asked on screen: in raw mode, a command reading stdin
	// would steal keys. App.Confirming says what a `y` would agree to.
	ModeConfirm
	// ModeQueue is reading the queue and dropping from it.
	ModeQueue
	// ModeTagging is the tag picker: a card rather than a screen, not being a
	// place.
	ModeTagging
	// ModeAlert is a refusal, or an answer longer than a line. Dismissed by
	// anything.
	ModeAlert
)

// What a `y` would agree to.
type What uint8

const (
	// WhatDelete deletes the note under the cursor, now.
	WhatDelete What = iota
	// WhatSend is asked at the send, not the queueing: a queued delete can
	// still be dropped.
	WhatSend
	// WhatQuit quits with a non-empty queue, which is written down nowhere
	// else.
	WhatQuit
)

// Ask is the one thing a change needs said in words before it can be asked
// for.
type Ask uint8

const (
	// AskTitle is a title for a new note. $EDITOR opens on the body once it is
	// given.
	AskTitle Ask = iota
	// AskRetitle is a new title for the note under the cursor.
	AskRetitle
)

// Prompt is what the status line calls the field.
func (a Ask) Prompt() string {
	if a == AskTitle {
		return "new note"
	}
	return "retitle"
}

// Hint is what the prompt does not say, shown at the end of the line.
func (a Ask) Hint() string {
	if a == AskTitle {
		return "Enter alone takes the title from the body"
	}
	return ""
}

// Session is one read of the notebook, replaced whole on reload so no part
// describes an older notebook than the rest.
type Session struct {
	Status notebook.Status
	Notes  []notebook.NoteFile
	// Files is what the notebook holds that is not a note, by name.
	Files     []string
	Notebooks []string
	// Today is the local date, not UTC: east of Greenwich UTC is still
	// yesterday in the morning, when a todo list is read.
	Today string
}

// App is a browsing session.
type App struct {
	// Notebook is the active notebook's name, for the header.
	Notebook string
	// Root is the notebook's directory, for turning a note into a path.
	Root string
	// Status is as of the last load, with no network: the drift as
	// `noda status` has it.
	Status notebook.Status
	// notes is in the walk's order — by slug, as `noda ls` shows without
	// `--sort`.
	notes []notebook.NoteFile
	// files and notebooks are read with the notes, rather than opening a
	// repository when asked.
	files     []string
	notebooks []string
	// today is once per read rather than per frame.
	today string
	// stack is oldest first, never empty.
	stack []*Screen
	Mode  Mode
	// Asking says which prompt ModeAsk is; Confirming what ModeConfirm agrees
	// to.
	Asking     Ask
	Confirming What
	// Input is the prompt's field.
	Input Field
	// Message is what the last command that changed something had to say.
	Message *Message
	// Marks are the notes picked out to be changed together, by id.
	// Independent of the query, so marking some then searching for more keeps
	// the first lot.
	Marks map[string]bool
	// Queue is handed to cmd.Bulk unaltered, so nothing translates a change.
	Queue   []cmd.Step
	queueAt int
	// choices are the picker's rows: every tag in the tags screen's order,
	// then anything typed. Built when the picker opens, dropped when it
	// closes.
	choices []Choice
	// aimedAt are the notes the picker was opened on, fixed though the cursor
	// moves.
	aimedAt []string
	// tagsAt is over the rows on screen, not the choices, so it holds while
	// narrowed.
	tagsAt int
	// Touch is session-long, as a single key has no room for a flag; shown in
	// the header so it is not forgotten.
	Touch cmd.Touch
	// Sort and Reverse are `--sort` and `-r`, session-long for Touch's reason.
	Sort    cmd.Sort
	Reverse bool
	// Long is `ls -l`'s columns, in the same places.
	Long bool
	// CrumbsShown is whether the crumb trail takes a row.
	CrumbsShown bool
	// loaded is kept with its view so a screen never draws another screen's
	// content.
	loaded     *Content
	loadedView View
	// history is the prompt's history, never written to disk: a file of
	// everything typed into a notebook is a privacy question, not a
	// convenience. historyAt is -1 for the line being typed, where walking
	// forward past the end lands.
	history    []string
	historyAt  int
	commandsAt int
	// Working is what is being waited for, while it is being waited for.
	Working string
	// page is written back by the drawing, which alone knows the screen's
	// height.
	page int
}

// NewApp starts a session on the listing.
func NewApp(notebookName, root string, session Session) *App {
	listing := newScreen(View{Kind: ViewNotes}, nil)
	listing.visible = allIndices(len(session.Notes))
	app := &App{
		Notebook:    notebookName,
		Root:        root,
		Status:      session.Status,
		notes:       session.Notes,
		files:       session.Files,
		notebooks:   session.Notebooks,
		today:       session.Today,
		stack:       []*Screen{listing},
		Marks:       map[string]bool{},
		Touch:       cmd.TouchStamp,
		CrumbsShown: true,
		historyAt:   -1,
		page:        10,
	}
	app.selectRow(0)
	return app
}

func allIndices(n int) []int {
	all := make([]int, n)
	for i := range all {
		all[i] = i
	}
	return all
}

// top is the screen the keyboard is on.
func (a *App) top() *Screen { return a.stack[len(a.stack)-1] }

// listing is the bottom screen, never popped.
func (a *App) listing() *Screen { return a.stack[0] }

// View is the screen the keyboard is on.
func (a *App) View() View { return a.top().View }

// Crumbs is the trail from the listing to where the keyboard is, outermost
// first.
func (a *App) Crumbs() []string {
	crumbs := make([]string, len(a.stack))
	for i, screen := range a.stack {
		crumbs[i] = screen.View.Crumb()
	}
	return crumbs
}

// Depth is how deep the stack is. One is the listing alone.
func (a *App) Depth() int { return len(a.stack) }

// Search is the query the listing is narrowed by, as typed.
func (a *App) Search() string { return a.listing().Search.Text() }

// SearchBefore is the query up to the cursor, for placing the terminal's
// cursor.
func (a *App) SearchBefore() string { return a.listing().Search.Before() }

// Error is why the query as typed is not a query yet.
func (a *App) Error() (string, bool) {
	listing := a.listing()
	return listing.Error, listing.HasError
}

// Terms are inherited by a screen opened from a query, so a hit stays
// highlighted in the note it was found in.
func (a *App) Terms() []string { return a.top().Terms }

// Scroll is how far the note on screen has been scrolled.
func (a *App) Scroll() int { return a.top().Scroll }

func (a *App) open(view View) {
	terms := append([]string(nil), a.top().Terms...)
	a.stack = append(a.stack, newScreen(view, terms))
	a.settle()
}

// back is false when there was nothing to close, so Esc can mean what it
// means on the listing.
func (a *App) back() bool {
	if len(a.stack) <= 1 {
		return false
	}
	a.stack = a.stack[:len(a.stack)-1]
	a.settle()
	return true
}

// settle works out what the top screen shows, where the session can; called
// whenever the top screen changes or the notebook is re-read. The rest ask
// through Wanted and wait a frame.
func (a *App) settle() {
	if a.content() == nil {
		view := a.top().View
		if content := a.derive(view); content != nil {
			a.loaded, a.loadedView = content, view
		}
	}
	// Kept, not reset, since this runs on the way back too; clamped for a
	// list gone shorter.
	a.cursorTo(max(a.top().Table.Selected, 0))
}

// derive is what a screen shows, when the session already holds the answer.
// These parse every body, so they run when a screen opens, not per keystroke.
func (a *App) derive(view View) *Content {
	switch view.Kind {
	case ViewTodo:
		var tasks []Task
		for at, file := range a.notes {
			for _, item := range todo.Items(file.Note.Body) {
				tasks = append(tasks, Task{Note: at, Item: item})
			}
		}
		// todo.Compare, so this matches `noda todo`.
		sort.SliceStable(tasks, func(i, j int) bool {
			left, right := tasks[i], tasks[j]
			return todo.Compare(a.notes[left.Note].Slug, left.Item, a.notes[right.Note].Slug, right.Item) < 0
		})
		return &Content{Kind: ContentTodo, Tasks: tasks}
	case ViewTags:
		// Counted and ordered by notebook, so this matches the web's tags.
		var tallies []Tally
		for _, count := range notebook.TagTally(a.notes) {
			tallies = append(tallies, Tally{Tag: count.Tag, Notes: count.Notes})
		}
		return &Content{Kind: ContentTags, Tallies: tallies}
	case ViewBacklinks:
		// notebook decides what counts as a link.
		var found []int
		for at, file := range a.notes {
			linked := notebook.LinksToNote(file.Note, view.Name)
			if view.File {
				linked = notebook.LinksToFile(file.Note, view.Name)
			}
			if linked {
				found = append(found, at)
			}
		}
		return &Content{Kind: ContentBacklinks, Linking: found}
	}
	return nil
}

// content is what the top screen shows, when what is loaded was loaded for it.
func (a *App) content() *Content {
	if a.loaded != nil && a.loadedView == a.top().View {
		return a.loaded
	}
	return nil
}

// Wanted is what the top screen needs from the runtime; nil once supplied, so
// a repository is not opened per frame.
func (a *App) Wanted() *Need {
	if a.content() != nil {
		return nil
	}
	view := a.top().View
	switch view.Kind {
	case ViewNote:
		file := a.NoteOf(view.Name)
		if file == nil {
			return nil
		}
		return &Need{Kind: NeedNote, ID: file.ID, Path: filepath.Join(a.Root, note.FileName(file.ID, file.Slug))}
	case ViewBlame:
		file := a.NoteOf(view.Name)
		if file == nil {
			return nil
		}
		return &Need{Kind: NeedBlame, ID: file.ID, Slug: file.Slug}
	case ViewLog:
		return &Need{Kind: NeedLog, ID: view.Name}
	case ViewDeleted:
		return &Need{Kind: NeedDeleted}
	case ViewDiff:
		return &Need{Kind: NeedDiff}
	}
	return nil
}

// Supply is dropped if its screen is no longer on top: a long blame can be
// escaped from while it runs.
func (a *App) Supply(view View, content Content) {
	if a.top().View != view {
		return
	}
	a.loaded, a.loadedView = &content, view
	a.top().Scroll = 0
	a.cursorTo(max(a.top().Table.Selected, 0))
}

// GiveUp closes a screen the runtime could not fill, so the error card is not
// drawn over an empty screen.
func (a *App) GiveUp() { a.back() }

// NoteOf is the note with this id, or nil.
func (a *App) NoteOf(id string) *notebook.NoteFile {
	for i := range a.notes {
		if a.notes[i].ID == id {
			return &a.notes[i]
		}
	}
	return nil
}

// Replace swaps in a fresh read, keeping the query, and the cursor on its
// note if that is still there or else on its row (as after a delete).
func (a *App) Replace(session Session) {
	was := ""
	if file := a.atCursor(); file != nil {
		was = file.ID
	}
	row := a.listing().Table.Selected
	a.Status = session.Status
	a.notes = session.Notes
	a.files = session.Files
	a.notebooks = session.Notebooks
	a.today = session.Today
	// A read comes back in walk order; sorted before the query picks out
	// indices into it.
	a.arrange()
	a.refilter()
	if at := a.visiblePosition(was); was != "" && at >= 0 {
		a.selectRow(at)
	} else {
		a.selectRow(max(row, 0))
	}
	a.loaded = nil
	// A screen about a note or file that has gone comes off the stack.
	kept := a.stack[:0:0]
	for _, screen := range a.stack {
		if a.screenSurvives(screen.View) {
			kept = append(kept, screen)
		}
	}
	a.stack = kept
	// Marks on gone notes are dropped. The queue keeps its ids: bulk reports
	// one that has since gone.
	for id := range a.Marks {
		if a.NoteOf(id) == nil {
			delete(a.Marks, id)
		}
	}
	a.settle()
}

func (a *App) screenSurvives(view View) bool {
	switch view.Kind {
	case ViewNote, ViewBlame:
		return a.NoteOf(view.Name) != nil
	case ViewLog:
		return view.Name == "" || a.NoteOf(view.Name) != nil
	case ViewBacklinks:
		if view.File {
			return containsString(a.files, view.Name)
		}
		return a.NoteOf(view.Name) != nil
	}
	return true
}

func containsString(list []string, want string) bool {
	for _, item := range list {
		if item == want {
			return true
		}
	}
	return false
}

// visiblePosition is the listing row showing the note with this id, or -1.
func (a *App) visiblePosition(id string) int {
	for row, i := range a.listing().visible {
		if a.notes[i].ID == id {
			return row
		}
	}
	return -1
}

// Marked is whether a note is picked out.
func (a *App) Marked(id string) bool { return a.Marks[id] }

// markedKeys are in id order; ids being what a command takes and what survives
// a rename. Ordered, so a commit message does not depend on map order.
func (a *App) markedKeys() []string {
	keys := make([]string, 0, len(a.Marks))
	for id := range a.Marks {
		keys = append(keys, id)
	}
	slices.Sort(keys)
	return keys
}

// atCursor is the note the listing's cursor is on, whatever screen is on top
// of it.
func (a *App) atCursor() *notebook.NoteFile {
	listing := a.listing()
	at := listing.Table.Selected
	if at < 0 || at >= len(listing.visible) {
		return nil
	}
	return a.noteAt(listing.visible[at])
}

// Selected is the note the top screen is about, which lets `e`, `m`, `#` and
// Ctrl-d mean the same everywhere: the row on a screen of notes, the note on a
// screen about one, and nothing on a screen about the notebook.
func (a *App) Selected() *notebook.NoteFile {
	row := a.top().Table.Selected
	view := a.top().View
	switch view.Kind {
	case ViewNotes:
		return a.atCursor()
	case ViewNote, ViewBlame:
		return a.NoteOf(view.Name)
	case ViewLog:
		if view.Name == "" {
			return nil
		}
		return a.NoteOf(view.Name)
	case ViewTodo:
		tasks := a.Tasks()
		if row < 0 || row >= len(tasks) {
			return nil
		}
		return a.noteAt(tasks[row].Note)
	case ViewBacklinks:
		linking := a.Linking()
		if row < 0 || row >= len(linking) {
			return nil
		}
		return a.noteAt(linking[row])
	}
	return nil
}

// Tasks is the loaded content of the top screen, empty when it shows something
// else; likewise the accessors below.
func (a *App) Tasks() []Task {
	if c := a.content(); c != nil && c.Kind == ContentTodo {
		return c.Tasks
	}
	return nil
}

func (a *App) Tallies() []Tally {
	if c := a.content(); c != nil && c.Kind == ContentTags {
		return c.Tallies
	}
	return nil
}

func (a *App) Linking() []int {
	if c := a.content(); c != nil && c.Kind == ContentBacklinks {
		return c.Linking
	}
	return nil
}

func (a *App) Entries() []notebook.Entry {
	if c := a.content(); c != nil && c.Kind == ContentLog {
		return c.Entries
	}
	return nil
}

// IsUnpushed is false on every other screen and with no remote, leaving the
// margin blank.
func (a *App) IsUnpushed(commit string) bool {
	if c := a.content(); c != nil && c.Kind == ContentLog {
		return c.Unpushed[commit]
	}
	return false
}

func (a *App) Gone() []notebook.Deleted {
	if c := a.content(); c != nil && c.Kind == ContentDeleted {
		return c.Gone
	}
	return nil
}

func (a *App) Blamed() []notebook.BlameLine {
	if c := a.content(); c != nil && c.Kind == ContentBlame {
		return c.Blame
	}
	return nil
}

// Text is for the two screens that are a block of text rather than a list.
func (a *App) Text() (string, bool) {
	if c := a.content(); c != nil && (c.Kind == ContentNote || c.Kind == ContentDiff) {
		return c.Text, true
	}
	return "", false
}

// NoteAt is the note at an index into the session's notes, or nil.
func (a *App) NoteAt(at int) *notebook.NoteFile { return a.noteAt(at) }

func (a *App) noteAt(at int) *notebook.NoteFile {
	if at < 0 || at >= len(a.notes) {
		return nil
	}
	return &a.notes[at]
}

func (a *App) Files() []string     { return a.files }
func (a *App) Notebooks() []string { return a.notebooks }

// Today is for deciding which due dates have been missed.
func (a *App) Today() string { return a.today }

// rowsHere is -1 when the screen is a block of text the keys scroll. The one
// place that says which kind a screen is, so every key agrees.
func (a *App) rowsHere() int {
	switch a.top().View.Kind {
	case ViewNotes:
		return len(a.top().visible)
	case ViewTodo:
		return len(a.Tasks())
	case ViewTags:
		return len(a.Tallies())
	case ViewFiles:
		return len(a.files)
	case ViewNotebooks:
		return len(a.notebooks)
	case ViewDeleted:
		return len(a.Gone())
	case ViewBacklinks:
		return len(a.Linking())
	case ViewLog:
		return len(a.Entries())
	}
	return -1
}

// HasRows is false on a screen that is a block of text.
func (a *App) HasRows() bool { return a.rowsHere() >= 0 }

// Row is which row the cursor is on, for the screens that draw one.
func (a *App) Row() (int, bool) {
	row := a.top().Table.Selected
	return row, row >= 0
}

// Rows are the notes the listing's query admits, in listing order.
func (a *App) Rows() []notebook.NoteFile {
	listing := a.listing()
	rows := make([]notebook.NoteFile, 0, len(listing.visible))
	for _, i := range listing.visible {
		if file := a.noteAt(i); file != nil {
			rows = append(rows, *file)
		}
	}
	return rows
}

// Shown is how many notes the query admits.
func (a *App) Shown() int { return len(a.listing().visible) }

// Total is how many notes the notebook has.
func (a *App) Total() int { return len(a.notes) }

// TableState is the top screen's cursor and offset, for the drawing to move.
func (a *App) TableState() *ui.TableState { return &a.top().Table }

// SetPage tells the state how many rows a screen shows.
func (a *App) SetPage(rows int) { a.page = max(rows, 1) }

// IDs are the ids of every note, for the runtime to find a note just made by
// diffing.
func (a *App) IDs() []string {
	ids := make([]string, len(a.notes))
	for i, file := range a.notes {
		ids[i] = file.ID
	}
	return ids
}

// SelectID puts the listing's cursor on a note, e.g. one just made with `a`.
func (a *App) SelectID(id string) {
	if at := a.visiblePosition(id); at >= 0 {
		a.selectRow(at)
	}
}

// OpenNote opens a note the runtime resolved through Notebook.Resolve.
func (a *App) OpenNote(id string) { a.open(NoteView(id)) }

// LookAt opens a screen about a note the runtime has resolved.
func (a *App) LookAt(look Look, id string) {
	switch look {
	case LookLog:
		a.open(LogView(id))
	case LookBlame:
		a.open(BlameView(id))
	case LookBacklinks:
		a.open(BacklinksView(Subject{Name: id}))
	}
}

// Report is how a command the runtime ran went. A non-nil err is shown in
// full: e.g. a broken frontmatter error ends by saying where the file was
// left.
func (a *App) Report(text string, err error) {
	if err != nil {
		a.Mode = ModeAlert
		a.Message = &Message{Text: style.Strip(err.Error()), Failed: true}
		return
	}
	text = strings.TrimRightFunc(style.Strip(text), unicode.IsSpace)
	// One line goes on the status bar; more gets the card.
	if strings.Count(text, "\n") > 0 {
		a.Mode = ModeAlert
	}
	a.Message = &Message{Text: text}
}
