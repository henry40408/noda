package tui

import (
	"fmt"
	"slices"
	"strings"
	"unicode"

	"github.com/henry40408/noda/internal/cmd"
	"github.com/henry40408/noda/internal/note"
	"github.com/henry40408/noda/internal/notebook"
	"github.com/henry40408/noda/internal/query"
)

// pick opens the tag picker over the given notes.
//
// It asks which tags the notes should end up with, not which `+`/`-` to apply,
// so a `-` aimed at a misspelling, which silently removes nothing, cannot
// happen. Tab chooses, not Space, since a tag may contain a space.
func (a *App) pick(keys []string) {
	var tallies []Tally
	if content := a.derive(View{Kind: ViewTags}); content != nil {
		tallies = content.Tallies
	}
	held := func(tag string) int {
		count := 0
		for _, key := range keys {
			if file := a.NoteOf(key); file != nil && slices.Contains(file.Note.Tags, tag) {
				count++
			}
		}
		return count
	}
	a.choices = a.choices[:0:0]
	for _, tally := range tallies {
		a.choices = append(a.choices, Choice{Tag: tally.Tag, Held: held(tally.Tag), Notes: tally.Notes})
	}
	a.aimedAt = keys
	a.tagsAt = 0
	a.Input.Clear()
	a.Mode = ModeTagging
}

func (a *App) pickerDown() { a.tagsAt = min(a.tagsAt+1, max(a.PickerRows()-1, 0)) }
func (a *App) pickerUp()   { a.tagsAt = max(a.tagsAt-1, 0) }

func (a *App) picking(key Key) *Action {
	switch {
	case key.Code == CodeTab:
		a.choose()
		return nil
	case key.Code == CodeEnter:
		return a.tagged()
	case key.Code == CodeEsc:
		a.shut()
		return nil
	// Not `j`/`k`: every letter goes into the filter.
	case key.Code == CodeDown, key.Ctrl && isChar(key, 'n'):
		a.pickerDown()
		return nil
	case key.Code == CodeUp, key.Ctrl && isChar(key, 'p'):
		a.pickerUp()
		return nil
	}
	// Typing and erasing only; the card shows no cursor to move. A new filter is
	// a new list, so the cursor goes to the top.
	if a.Input.Erasing(key) != NoEdit {
		a.tagsAt = 0
	}
	return nil
}

// shut closes the picker, changing nothing.
func (a *App) shut() {
	a.Mode = ModeBrowse
	a.Input.Clear()
	a.choices = nil
	a.aimedAt = nil
	a.tagsAt = 0
}

// choose walks the tag under the cursor through the states that would change
// something, or makes the one that is not there.
func (a *App) choose() {
	if shown := a.ShownTags(); a.tagsAt < len(shown) {
		choice := &a.choices[shown[a.tagsAt]]
		choice.Mark = choice.next(len(a.aimedAt))
		return
	}
	// A new tag joins the choices; it still matches the filter, so the cursor
	// stays on it.
	proposal := a.Proposal()
	if proposal == nil || proposal.Refused != "" {
		return
	}
	a.choices = append(a.choices, Choice{Tag: proposal.Tag, Mark: MarkAdd})
}

// tagged is the only place `+`/`-` changes are written; nothing splits them
// back, so a tag with a space survives as one string.
func (a *App) tagged() *Action {
	var changes []string
	for _, choice := range a.choices {
		switch choice.Mark {
		case MarkAdd:
			changes = append(changes, "+"+choice.Tag)
		case MarkRemove:
			changes = append(changes, "-"+choice.Tag)
		}
	}
	keys := a.aimedAt
	a.aimedAt = nil
	a.shut()
	// Nothing chosen is a way out, not an error.
	if len(changes) == 0 {
		return nil
	}
	// A marked set queues; a single note runs now.
	if len(a.Marks) == 0 {
		if len(keys) == 0 {
			return nil
		}
		return &Action{Kind: ActTag, Key: keys[0], Changes: changes, Touch: a.Touch}
	}
	a.enqueue(cmd.Step{Keys: keys, Change: cmd.Change{Tags: changes, Touch: a.Touch}})
	return nil
}

// ShownTags are indices of the choices the filter admits, commonest first,
// typed ones last.
func (a *App) ShownTags() []int {
	filter := strings.ToLower(strings.TrimSpace(a.Input.Text()))
	var shown []int
	for at, choice := range a.choices {
		if filter == "" || strings.Contains(strings.ToLower(choice.Tag), filter) {
			shown = append(shown, at)
		}
	}
	return shown
}

// Proposal is the row after the last tag, when what has been typed is not one
// of them; nil otherwise.
func (a *App) Proposal() *Proposal {
	typed := strings.TrimSpace(a.Input.Text())
	if typed == "" {
		return nil
	}
	for _, choice := range a.choices {
		if choice.Tag == typed {
			return nil
		}
	}
	// Refused where typed, by the check the command itself would fail on.
	if err := note.ValidateTag(typed); err != nil {
		return &Proposal{Refused: err.Error()}
	}
	proposal := &Proposal{Tag: typed}
	proposal.Near, proposal.NearNotes = a.nearest(typed)
	return proposal
}

// nearest is the commonest existing tag a keystroke away, the likeliest one
// meant. Tags made in this picker are not candidates.
func (a *App) nearest(tag string) (string, int) {
	best, notes := "", 0
	for _, choice := range a.choices {
		if choice.Notes > 0 && oneEditApart(choice.Tag, tag) && choice.Notes >= notes {
			best, notes = choice.Tag, choice.Notes
		}
	}
	return best, notes
}

// PickerRows is how many rows the picker shows.
func (a *App) PickerRows() int {
	rows := len(a.ShownTags())
	if a.Proposal() != nil {
		rows++
	}
	return rows
}

func (a *App) Choices() []Choice { return a.choices }
func (a *App) TagsAt() int       { return a.tagsAt }

// PickingNotes is what a row's `12/40` is out of.
func (a *App) PickingNotes() int { return len(a.aimedAt) }

// PickingNote is the one note, when the picker is not over a marked set.
func (a *App) PickingNote() *notebook.NoteFile {
	if len(a.aimedAt) != 1 || len(a.Marks) != 0 {
		return nil
	}
	return a.NoteOf(a.aimedAt[0])
}

func (a *App) commanding(key Key) *Action {
	if a.Input.Key(key) != NoEdit {
		return nil
	}
	switch {
	case key.Code == CodeEnter:
		line := a.Input.Take()
		a.Mode = ModeBrowse
		a.historyAt = -1
		return a.run(line)
	case key.Code == CodeEsc:
		a.Mode = ModeBrowse
		a.Input.Clear()
		a.historyAt = -1
	// History, as a shell has it, under both readline spellings.
	case key.Code == CodeUp, key.Ctrl && isChar(key, 'p'):
		a.recall(true)
	case key.Code == CodeDown, key.Ctrl && isChar(key, 'n'):
		a.recall(false)
	}
	return nil
}

// recall walks the history. Forward past the newest entry gives an empty line,
// so something new can be typed without clearing the field.
func (a *App) recall(back bool) {
	if len(a.history) == 0 {
		return
	}
	last := len(a.history) - 1
	switch {
	case back && a.historyAt < 0:
		a.historyAt = last
	case back:
		a.historyAt = max(a.historyAt-1, 0)
	case a.historyAt < 0:
	case a.historyAt >= last:
		a.historyAt = -1
	default:
		a.historyAt++
	}
	if a.historyAt < 0 {
		a.Input.Set("")
		return
	}
	a.Input.Set(a.history[a.historyAt])
}

// run runs a `:` line. The rest of the line is kept as typed, not rebuilt from
// tokens, which would break `tag:"12.34 foo bar"`.
func (a *App) run(line string) *Action {
	line = strings.TrimSpace(line)
	if line == "" {
		return nil
	}
	a.remember(line)
	name, rest := line, ""
	if at := strings.IndexFunc(line, unicode.IsSpace); at >= 0 {
		name, rest = line[:at], strings.TrimLeftFunc(line[at:], unicode.IsSpace)
	}
	spec, ok := FindCommand(name)
	if !ok {
		a.refuse(fmt.Sprintf("no command `%s` — Ctrl-a lists them", name))
		return nil
	}
	args := query.Split(rest)
	first := ""
	if len(args) > 0 {
		first = args[0]
	}

	switch spec.Name {
	case "quit":
		return a.leaving()
	case "reload":
		return &Action{Kind: ActReload}
	case "keys":
		a.Mode = ModeHelp
	// Back to the listing, narrowed on the way if a query came too.
	case "notes":
		for a.back() {
		}
		a.top().Search.Set(rest)
		a.refilter()
	case "open":
		if len(args) == 0 {
			return a.wants(spec)
		}
		return &Action{Kind: ActOpen, Key: first}
	case "edit":
		key, ok := a.aimed(first)
		if !ok {
			return nil
		}
		return &Action{Kind: ActEdit, Key: key, Touch: a.Touch}
	case "add":
		return &Action{Kind: ActAdd, Title: rest, HasTitle: rest != ""}
	// A title's first word cannot be told from a key, so this retitles the note
	// on screen.
	case "mv":
		if rest == "" {
			return a.wants(spec)
		}
		key, ok := a.aimed("")
		if !ok {
			return nil
		}
		return &Action{Kind: ActRetitle, Key: key, Title: rest, Touch: a.Touch}
	case "tag":
		return a.runTag(spec, args)
	case "pin", "unpin":
		key, ok := a.aimed(first)
		if !ok {
			return nil
		}
		return &Action{Kind: ActPin, Key: key, Pinned: spec.Name == "pin", Touch: a.Touch}
	case "rm":
		return a.delete()
	case "restore":
		if len(args) < 2 {
			return a.wants(spec)
		}
		return &Action{Kind: ActRestore, Key: args[0], Rev: args[1], Touch: a.Touch}
	case "use":
		if len(args) == 0 {
			return a.wants(spec)
		}
		return a.switchTo(first)
	case "todo":
		a.open(View{Kind: ViewTodo})
	case "tags":
		a.open(View{Kind: ViewTags})
	case "files":
		a.open(View{Kind: ViewFiles})
	case "notebooks":
		a.open(View{Kind: ViewNotebooks})
	case "deleted":
		a.open(View{Kind: ViewDeleted})
	case "diff":
		a.open(View{Kind: ViewDiff})
	// A named key goes to the runtime to be resolved, as `open` does.
	case "log":
		if len(args) > 0 {
			return &Action{Kind: ActShow, Key: first, Look: LookLog}
		}
		a.open(LogView(a.about()))
	case "blame":
		if len(args) > 0 {
			return &Action{Kind: ActShow, Key: first, Look: LookBlame}
		}
		if id, ok := a.aimed(""); ok {
			a.open(BlameView(id))
		}
	// A named key is a note; a file's backlinks come from the files screen,
	// which has already said which kind it is.
	case "backlinks":
		if len(args) > 0 {
			return &Action{Kind: ActShow, Key: first, Look: LookBacklinks}
		}
		if subject, ok := a.linkable(); ok {
			a.open(BacklinksView(subject))
		}
	case "status":
		return &Action{Kind: ActRun, Run: Run{Kind: RunStatus}}
	case "doctor":
		return a.runDoctor(spec, args)
	case "snapshot":
		return &Action{Kind: ActRun, Run: Run{Kind: RunSnapshot, Name: first, HasName: len(args) > 0}}
	case "readme":
		return &Action{Kind: ActRun, Run: Run{Kind: RunReadme}}
	case "sync":
		return &Action{Kind: ActRun, Run: Run{Kind: RunSync}}
	case "push":
		return &Action{Kind: ActRun, Run: Run{Kind: RunPush}}
	case "pull":
		return &Action{Kind: ActRun, Run: Run{Kind: RunPull}}
	}
	return nil
}

func (a *App) runTag(spec Spec, args []string) *Action {
	// A key cannot begin with `+` or `-`, so one that does is a change rather
	// than a note.
	var key string
	var changes []string
	var ok bool
	if len(args) > 0 && !strings.HasPrefix(args[0], "+") && !strings.HasPrefix(args[0], "-") {
		key, ok = a.aimed(args[0])
		changes = args[1:]
	} else {
		key, ok = a.aimed("")
		changes = args
	}
	if !ok {
		return nil
	}
	if len(changes) == 0 {
		return a.wants(spec)
	}
	return &Action{Kind: ActTag, Key: key, Changes: changes, Touch: a.Touch}
}

func (a *App) runDoctor(spec Spec, args []string) *Action {
	run := Run{Kind: RunDoctor}
	for _, arg := range args {
		switch arg {
		case "--links":
			run.Links = true
		case "--times":
			run.Times = true
		default:
			return a.wants(spec)
		}
	}
	return &Action{Kind: ActRun, Run: run}
}

// aimed is the note a command is aimed at: the one it named, or the one the
// screen is about.
func (a *App) aimed(given string) (string, bool) {
	if given != "" {
		return given, true
	}
	file := a.Selected()
	if file == nil {
		a.refuse("no note on screen — name one, or open one first")
		return "", false
	}
	return file.ID, true
}

// about is the note the screen is about, if any — for `log`, the one command
// that takes a note or the whole notebook. Empty means the whole notebook.
func (a *App) about() string {
	view := a.top().View
	switch view.Kind {
	case ViewNote, ViewBlame, ViewLog:
		return view.Name
	case ViewBacklinks:
		if !view.File {
			return view.Name
		}
	}
	return ""
}

// linkable is the file under the cursor on the files screen, otherwise the
// aimed note.
func (a *App) linkable() (Subject, bool) {
	if a.top().View.Kind == ViewFiles {
		if at := a.top().Table.Selected; at >= 0 && at < len(a.files) {
			return Subject{File: true, Name: a.files[at]}, true
		}
	}
	id, ok := a.aimed("")
	return Subject{Name: id}, ok
}

// wants refuses with the command's usage.
func (a *App) wants(spec Spec) *Action {
	a.refuse(fmt.Sprintf("%s — %s", spec.Usage(), spec.What))
	return nil
}

func (a *App) remember(line string) {
	if len(a.history) > 0 && a.history[len(a.history)-1] == line {
		return
	}
	a.history = append(a.history, line)
}

// listingCommands is the command palette, narrowed as you type.
func (a *App) listingCommands(key Key) *Action {
	// Typing and erasing only; see Field.Erasing.
	if a.Input.Erasing(key) != NoEdit {
		a.commandsAt = 0
		return nil
	}
	matching := MatchingCommands(a.Input.Text())
	shown := len(matching)
	switch {
	case key.Code == CodeEsc:
		a.Mode = ModeBrowse
		a.Input.Clear()
	case key.Code == CodeDown, key.Ctrl && isChar(key, 'n'):
		a.commandsAt = min(a.commandsAt+1, max(shown-1, 0))
	case key.Code == CodeUp, key.Ctrl && isChar(key, 'p'):
		a.commandsAt = max(a.commandsAt-1, 0)
	// Onto the prompt rather than run: most take arguments, and `push` should
	// not go off by landing on it.
	case key.Code == CodeEnter:
		if a.commandsAt >= shown {
			a.Mode = ModeBrowse
			a.Input.Clear()
			return nil
		}
		spec := matching[a.commandsAt]
		if spec.Takes == "" {
			a.Input.Set(spec.Name)
		} else {
			a.Input.Set(spec.Name + " ")
		}
		a.Mode = ModeCommand
		a.historyAt = -1
	}
	return nil
}

func (a *App) CommandsAt() int { return a.commandsAt }
