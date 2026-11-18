package main

import (
	"errors"

	"github.com/henry40408/noda/internal/cmd"
	"github.com/henry40408/noda/internal/notebook"
)

// An empty value (`path ""`, `--notebook=`) is a value, not an absent one. The
// commands below take "" to mean "not given", so each place the Rust build
// refused or answered an empty value differently is handled here, in the order
// the Rust build checked things.

// neverATag matches no note: a tag is never blank, so `ls --tag=` lists
// nothing, as it did in Rust.
const neverATag = " "

// givenEmpty says the flag or argument was on the command line with no text.
func (p parsed) givenEmpty(name string) bool {
	if values, ok := p.flags[name]; ok {
		return values[len(values)-1] == ""
	}
	values := p.args[name]
	return len(values) > 0 && values[0] == ""
}

// refuseEmptyNoteReference runs next unless the argument was "". The active
// notebook is checked first, as in Rust.
func refuseEmptyNoteReference(name string, next handler) handler {
	return func(e cmd.Env, line parsed) (string, error) {
		if !line.givenEmpty(name) {
			return next(e, line)
		}
		if _, err := notebook.ActiveName(e.Paths); err != nil {
			return "", err
		}
		return "", errors.New("invalid note reference: ")
	}
}

// refuseEmptyNotebook runs next unless `--notebook` was "", which Rust
// refused before looking at any notebook.
func refuseEmptyNotebook(next handler) handler {
	return func(e cmd.Env, line parsed) (string, error) {
		if line.givenEmpty("notebook") {
			return "", notebook.ValidateName("")
		}
		return next(e, line)
	}
}

func runFileAdd(e cmd.Env, line parsed) (string, error) {
	if line.givenEmpty("as") {
		if _, err := notebook.ActiveName(e.Paths); err != nil {
			return "", err
		}
		return "", errors.New("a file needs a name")
	}
	return cmd.FileAdd(e, line.args["paths"], line.value("as"))
}

// runNotebookAdd creates the notebook before refusing an empty `--remote`:
// Rust did, and left the notebook behind.
func runNotebookAdd(e cmd.Env, line parsed) (string, error) {
	output, err := cmd.NotebookAdd(e, line.arg("name"), line.value("remote"))
	if err != nil {
		return "", err
	}
	if line.givenEmpty("remote") {
		return "", errors.New("cannot set empty URL")
	}
	return output, nil
}

func runClone(e cmd.Env, line parsed) (string, error) {
	hasName := len(line.args["name"]) > 0
	if hasName && line.givenEmpty("url") {
		return "", errors.New("cannot set empty URL")
	}
	if line.givenEmpty("name") {
		return "", notebook.ValidateName("")
	}
	return cmd.Clone(e, line.arg("url"), line.arg("name"))
}
