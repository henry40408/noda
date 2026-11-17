package main

import (
	"errors"
	"fmt"
	"strings"

	"github.com/henry40408/noda/internal/cmd"
)

// handler runs one command and returns what to print.
type handler func(env cmd.Env, line parsed) (string, error)

// notYet names the commands whose Go port is a later step.
func notYet(name string) handler {
	return func(cmd.Env, parsed) (string, error) {
		return "", fmt.Errorf("`noda %s` is not available in this build yet", name)
	}
}

func handlers() map[string]handler {
	return map[string]handler{
		"init":   func(e cmd.Env, _ parsed) (string, error) { return cmd.Init(e) },
		"readme": func(e cmd.Env, l parsed) (string, error) { return cmd.Readme(e, l.has("force")) },
		"add":    runAdd,
		"ls":     refuseEmptyNotebook(runLs),
		"show":   func(e cmd.Env, l parsed) (string, error) { return cmd.Show(e, l.arg("note")) },
		"edit": func(e cmd.Env, l parsed) (string, error) {
			return cmd.Edit(e, l.arg("note"), touchOf(l))
		},
		"mv": func(e cmd.Env, l parsed) (string, error) {
			return cmd.Mv(e, cmd.Rename{Key: l.arg("note"), NewTitle: l.arg("new_title"), UpdateLinks: l.has("update-links"), Touch: touchOf(l)})
		},
		"tag": func(e cmd.Env, l parsed) (string, error) {
			return cmd.Tag(e, l.arg("note"), l.args["changes"], touchOf(l))
		},
		"pin":    func(e cmd.Env, l parsed) (string, error) { return cmd.Pin(e, l.arg("note"), true, touchOf(l)) },
		"unpin":  func(e cmd.Env, l parsed) (string, error) { return cmd.Pin(e, l.arg("note"), false, touchOf(l)) },
		"rm":     func(e cmd.Env, l parsed) (string, error) { return cmd.Rm(e, l.arg("note")) },
		"status": func(e cmd.Env, _ parsed) (string, error) { return cmd.Status(e) },
		"doctor": func(e cmd.Env, l parsed) (string, error) {
			return cmd.Doctor(e, cmd.DoctorOptions{DryRun: l.has("dry-run"), Links: l.has("links"), Times: l.has("times")})
		},
		"path": refuseEmptyNoteReference("key", func(e cmd.Env, l parsed) (string, error) {
			return cmd.Path(e, l.arg("key"))
		}),
		"search": func(e cmd.Env, l parsed) (string, error) { return cmd.Search(e, l.args["query"]) },
		"tui":    notYet("tui"),
		"web":    runWeb,
		"log":    refuseEmptyNoteReference("note", runLog),
		"backlinks": func(e cmd.Env, l parsed) (string, error) {
			return cmd.Backlinks(e, l.arg("key"), formatOf(l))
		},
		"todo":  func(e cmd.Env, l parsed) (string, error) { return cmd.Todo(e, l.has("json")) },
		"blame": func(e cmd.Env, l parsed) (string, error) { return cmd.Blame(e, l.arg("note")) },
		"diff": refuseEmptyNoteReference("note", func(e cmd.Env, l parsed) (string, error) {
			return cmd.Diff(e, l.arg("note"), l.has("remote"))
		}),
		"deleted": refuseEmptyNotebook(func(e cmd.Env, l parsed) (string, error) {
			return cmd.Deleted(e, l.value("notebook"), l.has("json"))
		}),
		"snapshot": runSnapshot,
		"restore": func(e cmd.Env, l parsed) (string, error) {
			return cmd.Restore(e, l.arg("note"), l.arg("commit"), touchOf(l))
		},
		"config": runConfig,
		"import tiddlywiki": func(e cmd.Env, l parsed) (string, error) {
			return cmd.ImportTiddlywiki(e, l.args["files"], !l.has("no-convert"))
		},
		"file add": runFileAdd,
		"file mv": func(e cmd.Env, l parsed) (string, error) {
			return cmd.FileMv(e, l.arg("old"), l.arg("new"), l.has("update-links"))
		},
		"file rm":      func(e cmd.Env, l parsed) (string, error) { return cmd.FileRm(e, l.arg("name")) },
		"notebook add": runNotebookAdd,
		"notebook ls":  func(e cmd.Env, _ parsed) (string, error) { return cmd.NotebookLs(e) },
		"notebook rm": func(e cmd.Env, l parsed) (string, error) {
			return cmd.NotebookRm(e, l.arg("name"), l.has("force"))
		},
		"notebook rename":  func(e cmd.Env, l parsed) (string, error) { return cmd.NotebookRename(e, l.arg("old"), l.arg("new")) },
		"notebook current": func(e cmd.Env, _ parsed) (string, error) { return cmd.NotebookCurrent(e) },
		"clone":            runClone,
		"remote set":       func(e cmd.Env, l parsed) (string, error) { return cmd.RemoteSet(e, l.arg("url")) },
		"remote show":      func(e cmd.Env, _ parsed) (string, error) { return cmd.RemoteShow(e) },
		"sync":             func(e cmd.Env, _ parsed) (string, error) { return cmd.Sync(e) },
		"push":             func(e cmd.Env, _ parsed) (string, error) { return cmd.Push(e) },
		"pull":             func(e cmd.Env, _ parsed) (string, error) { return cmd.Pull(e) },
		"use":              func(e cmd.Env, l parsed) (string, error) { return cmd.Use(e, l.arg("name")) },
	}
}

func (p process) dispatch(line parsed) error {
	run, ok := handlers()[strings.Join(line.path, " ")]
	if !ok {
		return errors.New("no such command: " + strings.Join(line.path, " "))
	}
	// Checked for `web` alone: no other command has a log to level. The value
	// is read here and nowhere below, so the web package never touches the
	// environment.
	if line.path[0] == "web" {
		if _, err := parseLogLevel(p.env[envLogLevel]); err != nil {
			return err
		}
	}
	env, err := p.environment()
	if err != nil {
		return err
	}
	output, err := run(env, line)
	if err != nil {
		return err
	}
	return p.print(output)
}

func touchOf(line parsed) cmd.Touch {
	if line.has("no-touch") {
		return cmd.TouchKeep
	}
	return cmd.TouchStamp
}

func formatOf(line parsed) cmd.Format {
	switch {
	case line.has("json"):
		return cmd.FormatJSON
	case line.has("quiet"):
		return cmd.FormatQuiet
	}
	return cmd.FormatTable
}

func runAdd(e cmd.Env, line parsed) (string, error) {
	options := cmd.AddOptions{Tags: line.flags["tag"]}
	if len(line.args["title"]) > 0 {
		title := line.arg("title")
		options.Title = &title
	}
	if line.has("content") {
		content := line.value("content")
		options.Content = &content
	}
	return cmd.Add(e, options)
}

func runLs(e cmd.Env, line parsed) (string, error) {
	options := cmd.List{
		Notebook: line.value("notebook"),
		Tag:      tagOf(line),
		Long:     line.has("long"),
		Reverse:  line.has("reverse"),
		Null:     line.has("null"),
		Format:   formatOf(line),
		Sort:     sortOf(line.value("sort")),
	}
	switch {
	case line.has("notes-only"):
		options.Only = cmd.OnlyNotes
	case line.has("files-only"):
		options.Only = cmd.OnlyFiles
	}
	return cmd.Ls(e, options)
}

func tagOf(line parsed) string {
	if line.givenEmpty("tag") {
		return neverATag
	}
	return line.value("tag")
}

func sortOf(name string) cmd.Sort {
	switch name {
	case "created":
		return cmd.SortCreated
	case "updated":
		return cmd.SortUpdated
	case "title":
		return cmd.SortTitle
	}
	return cmd.SortSlug
}

func runLog(e cmd.Env, line parsed) (string, error) {
	limit := 0
	if line.has("max-count") {
		text := line.value("max-count")
		count, err := parseCount(text)
		if err != nil {
			return "", usageError{
				message: fmt.Sprintf("invalid value '%s' for '--max-count <COUNT>': %s", text, err),
				usage:   usageLine(append([]string{"noda"}, line.path...), line.spec),
			}
		}
		// Rust showed one commit for 0, where a limit of 0 would mean none.
		limit = max(count, 1)
	}
	return cmd.Log(e, line.arg("note"), limit)
}

func runSnapshot(e cmd.Env, line parsed) (string, error) {
	if len(line.args["name"]) == 0 {
		return cmd.SnapshotLs(e)
	}
	return cmd.Snapshot(e, line.arg("name"), line.value("message"))
}

// runConfig picks among the five things `config` does, by what it was given.
func runConfig(e cmd.Env, line parsed) (string, error) {
	key, hasKey := line.arg("key"), len(line.args["key"]) > 0
	switch {
	case line.has("edit"):
		return cmd.ConfigEdit(e)
	case hasKey && line.has("unset"):
		return cmd.ConfigUnset(e, key)
	case hasKey && len(line.args["value"]) > 0:
		return cmd.ConfigSet(e, key, line.arg("value"))
	case hasKey:
		return cmd.ConfigGet(e, key)
	}
	return cmd.ConfigShow(e)
}

func runWeb(_ cmd.Env, _ parsed) (string, error) {
	return "", errors.New("`noda web` is not available in this build yet")
}
