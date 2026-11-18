package tui

import (
	"errors"
	"os"
	"slices"

	"github.com/henry40408/noda/internal/cmd"
	"github.com/henry40408/noda/internal/notebook"
	"github.com/henry40408/noda/internal/style"
)

// Only this file and run.go open a repository, read a file, run a command or
// touch a terminal; the rest is tested without one. Notes are changed only
// through cmd (`e` runs `noda edit`, `ctrl-d` runs `noda rm`), and the status
// line shows what that command returned.

// Load reads every note, bodies included, once, to be held for the session.
func Load(env cmd.Env) (*App, error) {
	active, err := openActive(env)
	if err != nil {
		return nil, err
	}
	session, err := readSession(env, active)
	if err != nil {
		return nil, err
	}
	return NewApp(active.Name, active.Path, session), nil
}

// readSession uses Inventory rather than a notes walk: one walk yields the
// notes and the files screen.
func readSession(env cmd.Env, active *notebook.Notebook) (Session, error) {
	status, err := active.Status()
	if err != nil {
		return Session{}, err
	}
	notes, files, err := active.Inventory()
	if err != nil {
		return Session{}, err
	}
	notebooks, err := notebook.List(env.Paths)
	if err != nil {
		return Session{}, err
	}
	return Session{Status: status, Notes: notes, Files: files, Notebooks: notebooks, Today: cmd.Today(env)}, nil
}

// Reload is what `r` asks for. There is no file watching, so the screen never
// shifts under a reader.
func Reload(env cmd.Env, app *App) error {
	active, err := openActive(env)
	if err != nil {
		return err
	}
	session, err := readSession(env, active)
	if err != nil {
		return err
	}
	app.Replace(session)
	return nil
}

// Refresh fetches what a newly opened screen shows. App.Wanted is nil once the
// screen has it, so an ordinary frame opens no repository.
//
// A note is read from disk, as `noda show` does, not re-rendered from memory.
// A failed fetch closes the screen and says why. It is exported so tests that
// draw a screen can take this step too.
func Refresh(env cmd.Env, app *App) {
	need := app.Wanted()
	if need == nil {
		return
	}
	// Captured before the fetch, so a slow result cannot land on another
	// screen.
	asked := app.View()

	// Needs no repository; a read error is shown in place of the note rather
	// than closing the screen.
	if need.Kind == NeedNote {
		text, err := os.ReadFile(need.Path)
		if err != nil {
			app.Supply(asked, Content{Kind: ContentNote, Text: err.Error() + "\n"})
			return
		}
		app.Supply(asked, Content{Kind: ContentNote, Text: string(text)})
		return
	}

	content, err := fetch(env, *need)
	if err != nil {
		app.GiveUp()
		app.Report("", err)
		return
	}
	app.Supply(asked, content)
}

func fetch(env cmd.Env, need Need) (Content, error) {
	active, err := openActive(env)
	if err != nil {
		return Content{}, err
	}
	switch need.Kind {
	case NeedLog:
		return fetchLog(active, need.ID)
	case NeedBlame:
		lines, err := active.Blame(need.ID, need.Slug)
		return Content{Kind: ContentBlame, Blame: lines}, err
	case NeedDeleted:
		gone, err := active.Deleted()
		return Content{Kind: ContentDeleted, Gone: gone}, err
	case NeedDiff:
		// `cmd.Diff` stripped of its pipe colours; the drawing colours it. Always
		// the working tree's diff: a screen takes no flags.
		patch, err := cmd.Diff(env, "", false)
		return Content{Kind: ContentDiff, Text: style.Strip(patch)}, err
	}
	return Content{}, errors.New("a note's file is read without the repository")
}

// fetchLog needs no network: unpushed is two local refs.
func fetchLog(active *notebook.Notebook, id string) (Content, error) {
	entries, err := active.Log(id, 0)
	if err != nil {
		return Content{}, err
	}
	branch, err := active.Branch()
	if err != nil {
		return Content{}, err
	}
	unpushed, err := active.Unpushed(branch)
	if err != nil {
		return Content{}, err
	}
	pushed := make(map[string]bool, len(unpushed))
	for commit := range unpushed {
		pushed[commit] = true
	}
	return Content{Kind: ContentLog, Entries: entries, Unpushed: pushed}, nil
}

// Foreground hands the terminal to $EDITOR for the length of run, then takes
// it back.
type Foreground func(run func()) error

// Perform maps an action to the cmd call the shell would make; nothing else is
// decided here. app may be replaced wholesale (a change of notebook).
func Perform(env cmd.Env, foreground Foreground, app *App, action Action) error {
	// Diffed afterwards to find the new note, rather than parsing `add`'s
	// prose.
	var before []string
	if action.Kind == ActAdd {
		before = app.IDs()
	}

	switch action.Kind {
	case ActQuit, ActReload:
		return nil
	case ActOpen:
		return performOpen(env, app, action)
	case ActShow:
		return performShow(env, app, action)
	case ActUse:
		return performUse(env, app, action)
	}

	text, err := runAction(env, foreground, app, action)
	if err != nil && errors.Is(err, errTerminal) {
		return err
	}
	app.Report(text, err)

	if err := Reload(env, app); err != nil {
		return err
	}
	if before != nil {
		for _, id := range app.IDs() {
			if !slices.Contains(before, id) {
				app.SelectID(id)
				break
			}
		}
	}
	return nil
}

// errTerminal marks a failure to hand the terminal over or take it back, which
// ends the session rather than being reported on the status line.
var errTerminal = errors.New("terminal")

// performOpen reloads first: a note written from another window is exactly
// what somebody opens by name.
func performOpen(env cmd.Env, app *App, action Action) error {
	active, err := openActive(env)
	if err != nil {
		return err
	}
	resolved, err := active.Resolve(action.Key)
	if err != nil {
		app.Report("", err)
		return nil
	}
	if err := Reload(env, app); err != nil {
		return err
	}
	app.OpenNote(resolved.ID)
	return nil
}

func performShow(env cmd.Env, app *App, action Action) error {
	active, err := openActive(env)
	if err != nil {
		return err
	}
	resolved, err := active.Resolve(action.Key)
	if err != nil {
		app.Report("", err)
		return nil
	}
	app.LookAt(action.Look, resolved.ID)
	return nil
}

// performUse builds the session fresh: Reload keeps exactly the state that
// must not survive a change of notebook.
func performUse(env cmd.Env, app *App, action Action) error {
	said, err := cmd.Use(env, action.Key)
	if err != nil {
		app.Report("", err)
		return nil
	}
	fresh, err := Load(env)
	if err != nil {
		return err
	}
	*app = *fresh
	app.Report(said, nil)
	return nil
}

func runAction(env cmd.Env, foreground Foreground, app *App, action Action) (string, error) {
	switch action.Kind {
	case ActRun:
		return runCommand(env, action.Run)
	case ActEdit:
		return inTheForeground(foreground, func() (string, error) { return cmd.Edit(env, action.Key, action.Touch) })
	case ActAdd:
		options := cmd.AddOptions{}
		if action.HasTitle {
			options.Title = &action.Title
		}
		return inTheForeground(foreground, func() (string, error) { return cmd.Add(env, options) })
	// No `--update-links`: it edits notes nobody is looking at.
	case ActRetitle:
		return cmd.Mv(env, cmd.Rename{Key: action.Key, NewTitle: action.Title, Touch: action.Touch})
	case ActTag:
		return cmd.Tag(env, action.Key, action.Changes, action.Touch)
	case ActPin:
		return cmd.Pin(env, action.Key, action.Pinned, action.Touch)
	case ActRemove:
		return cmd.Rm(env, action.Key)
	case ActRestore:
		return cmd.Restore(env, action.Key, action.Rev, action.Touch)
	// One commit for the whole queue, through the same code as the keys above.
	case ActSend:
		sent, err := cmd.Bulk(env, action.Steps)
		// A refused queue is kept.
		if err == nil {
			app.Sent()
		}
		return sent, err
	}
	return "", errors.New("an action nobody performs")
}

func runCommand(env cmd.Env, run Run) (string, error) {
	switch run.Kind {
	// Dry run: a keystroke should not rewrite the notebook.
	case RunDoctor:
		return cmd.Doctor(env, cmd.DoctorOptions{DryRun: true, Links: run.Links, Times: run.Times})
	case RunStatus:
		return cmd.Status(env)
	case RunReadme:
		return cmd.Readme(env, false)
	case RunSnapshot:
		if run.HasName {
			return cmd.Snapshot(env, run.Name, "")
		}
		return cmd.SnapshotLs(env)
	case RunSync:
		return cmd.Sync(env)
	case RunPush:
		return cmd.Push(env)
	case RunPull:
		return cmd.Pull(env)
	}
	return "", errors.New("a command nobody runs")
}

// inTheForeground runs one cmd call with the terminal handed over. A failure
// of the hand-over itself is not the command's answer.
func inTheForeground(foreground Foreground, run func() (string, error)) (string, error) {
	var text string
	var err error
	if handed := foreground(func() { text, err = run() }); handed != nil {
		return "", errors.Join(errTerminal, handed)
	}
	return text, err
}

func openActive(env cmd.Env) (*notebook.Notebook, error) {
	return notebook.OpenActive(env.Git, env.Paths)
}
