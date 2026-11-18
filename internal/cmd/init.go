package cmd

import (
	"context"
	"fmt"
	"path/filepath"
	"strings"

	"github.com/henry40408/noda/internal/config"
	"github.com/henry40408/noda/internal/gitx"
	"github.com/henry40408/noda/internal/notebook"
	"github.com/henry40408/noda/internal/style"
)

const fallbackAuthor = "noda <noda@localhost>"

type setting struct {
	key, value string
	source     config.Source
}

// Init is safe to run more than once.
func Init(e Env) (string, error) {
	if err := e.Paths.CreateDirs(); err != nil {
		return "", err
	}
	var lines []string

	// Commented-out defaults change nothing but show what can be set.
	wrote, err := config.WriteTemplate(e.Paths)
	if err != nil {
		return "", err
	}
	if wrote {
		lines = append(lines, "wrote "+e.configPath())
	}

	settings, err := config.Load(e.Paths)
	if err != nil {
		return "", err
	}
	name, ok := settings.Get("notebook")
	if !ok {
		name = config.DefaultNotebook
	}
	if notebook.Exists(e.Paths, name) {
		lines = append(lines, fmt.Sprintf("notebook `%s` already exists", name))
	} else {
		created, err := notebook.Create(e.Git, e.Paths, name)
		if err != nil {
			return "", err
		}
		lines = append(lines, fmt.Sprintf("created notebook `%s` at %s", name, created.Path))
	}
	if _, missing := e.Paths.ActiveNotebook(); missing != nil {
		if err := e.Paths.SetActiveNotebook(name); err != nil {
			return "", err
		}
		lines = append(lines, "active notebook: "+name)
	}
	return strings.Join(lines, "\n"), nil
}

func (e Env) configPath() string {
	return filepath.Join(e.Paths.ConfigDir(), "config.toml")
}

// ConfigShow is every setting with where its value came from.
func ConfigShow(e Env) (string, error) {
	settings, err := config.Load(e.Paths)
	if err != nil {
		return "", err
	}
	rows := e.effective(settings)

	keyWidth, valueWidth := 0, 0
	for _, row := range rows {
		keyWidth = max(keyWidth, DisplayWidth(row.key))
		valueWidth = max(valueWidth, DisplayWidth(row.value))
	}
	var out strings.Builder
	for _, row := range rows {
		fmt.Fprintf(&out, "%s  %s  %s\n", pad(row.key, keyWidth), pad(row.value, valueWidth),
			style.Muted.Paint("("+row.source.Label()+")"))
	}
	return out.String(), nil
}

// ConfigGet is one setting's effective value, unadorned for a script.
func ConfigGet(e Env, key string) (string, error) {
	if err := config.ValidateKey(key); err != nil {
		return "", err
	}
	settings, err := config.Load(e.Paths)
	if err != nil {
		return "", err
	}
	row, _ := e.settingFor(settings, key)
	return row.value, nil
}

func ConfigSet(e Env, key, value string) (string, error) {
	settings, err := config.Load(e.Paths)
	if err != nil {
		return "", err
	}
	if err := settings.Set(key, value); err != nil {
		return "", err
	}
	return key + "  " + value, nil
}

func ConfigUnset(e Env, key string) (string, error) {
	settings, err := config.Load(e.Paths)
	if err != nil {
		return "", err
	}
	removed, err := settings.Unset(key)
	if err != nil {
		return "", err
	}
	if !removed {
		return key + "  (was not set)", nil
	}
	now, found := e.settingFor(settings, key)
	if !found {
		return key + "  unset", nil
	}
	return fmt.Sprintf("%s  %s  (now from %s)", key, now.value, now.source.Label()), nil
}

// ConfigEdit writes the starter template first if the file is missing.
func ConfigEdit(e Env) (string, error) {
	if _, err := config.WriteTemplate(e.Paths); err != nil {
		return "", err
	}
	path := e.configPath()
	if err := e.runEditor(e.configuredEditor(), path); err != nil {
		return "", err
	}
	// Surface a typo now, not at the next unrelated command.
	if _, err := config.Load(e.Paths); err != nil {
		return "", err
	}
	return path, nil
}

func (e Env) settingFor(settings *config.Config, key string) (setting, bool) {
	for _, row := range e.effective(settings) {
		if row.key == key {
			return row, true
		}
	}
	return setting{}, false
}

func (e Env) effective(settings *config.Config) []setting {
	configured, _ := settings.Get("editor")
	visual, _ := e.Git.Getenv("VISUAL")
	editor, _ := e.Git.Getenv("EDITOR")
	editorValue, editorSource := config.Editor(configured, visual, editor)

	author, authorSource := e.author(settings)

	notebookValue, notebookSource := config.DefaultNotebook, config.SourceDefault
	if name, ok := settings.Get("notebook"); ok {
		notebookValue, notebookSource = name, config.SourceFile
	}
	sign, signSource := e.sign(settings)
	return []setting{
		{"editor", editorValue, editorSource},
		{"author", author, authorSource},
		{"notebook", notebookValue, notebookSource},
		{"sign", fmt.Sprint(sign), signSource},
	}
}

// sign falls back to the user's git config, not a notebook's: this answers for
// every notebook.
func (e Env) sign(settings *config.Config) (bool, config.Source) {
	if on, set := settings.Sign(); set {
		return on, config.SourceFile
	}
	values, err := gitx.LoadConfig(context.Background(), e.Git, "")
	if err != nil {
		return false, config.SourceDefault
	}
	if on, ok := values.Bool("commit.gpgsign"); ok {
		return on, config.SourceGit
	}
	return false, config.SourceDefault
}

// author is the identity commits are made under, and where it came from.
func (e Env) author(settings *config.Config) (string, config.Source) {
	if author, ok := settings.Get("author"); ok {
		return author, config.SourceFile
	}
	// In the order git itself would ask.
	if active, err := e.openActive(); err == nil {
		if author, ok := active.GitAuthor(); ok {
			return author, config.SourceGit
		}
	}
	values, err := gitx.LoadConfig(context.Background(), e.Git, "")
	if err == nil {
		name, hasName := values.Get("user.name")
		email, hasEmail := values.Get("user.email")
		if hasName && hasEmail {
			return fmt.Sprintf("%s <%s>", name, email), config.SourceGit
		}
	}
	return fallbackAuthor, config.SourceDefault
}
