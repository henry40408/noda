// Package config is `config.toml`: the four settings noda keeps, and where each
// value comes from.
//
// Often edited by hand, so it is written back through a TOML editor that keeps
// comments and layout.
package config

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"slices"
	"strings"

	"github.com/pelletier/go-toml/v2"

	"github.com/henry40408/noda/internal/paths"
)

// Keys are the only settings: anything else is refused by name, because a typo
// silently doing nothing is worse.
var Keys = []string{"editor", "author", "notebook", "sign"}

// DefaultNotebook is the notebook used when none is active and the one
// `noda init` creates.
const DefaultNotebook = "default"

// template is entirely commented out, so the defaults still apply.
const template = "# noda configuration. Every setting is optional; the defaults are shown.\n" +
	"\n" +
	"# Editor for `noda add` and `noda edit`.\n" +
	"# Overrides $VISUAL and $EDITOR, the way git's core.editor does.\n" +
	"# editor = \"nvim\"\n" +
	"\n" +
	"# Who commits. Falls back to your git config, then to noda <noda@localhost>.\n" +
	"# author = \"Your Name <you@example.com>\"\n" +
	"\n" +
	"# The notebook to fall back to when no notebook is active, and the one\n" +
	"# `noda init` creates.\n" +
	"# notebook = \"default\"\n" +
	"\n" +
	"# Whether commits are GPG-signed. Falls back to git's commit.gpgsign.\n" +
	"# Set it here to sign your notes without signing everything else, or to\n" +
	"# leave them unsigned when the rest of your commits are signed.\n" +
	"# sign = true\n"

// Config is the loaded file.
type Config struct {
	path   string
	text   string
	values map[string]any
}

func configPath(p paths.Paths) string {
	return filepath.Join(p.ConfigDir(), "config.toml")
}

// Load reads the file. An absent config is a valid config, not an error.
func Load(p paths.Paths) (*Config, error) {
	path := configPath(p)
	raw, err := os.ReadFile(path)
	if errors.Is(err, os.ErrNotExist) {
		return &Config{path: path, values: map[string]any{}}, nil
	}
	if err != nil {
		return nil, err
	}
	config := &Config{path: path, text: string(raw)}
	if err := config.parse(); err != nil {
		return nil, fmt.Errorf("%s: %s\nfix it by hand, or start over with `noda config --edit`", path, describe(err))
	}
	return config, nil
}

func describe(err error) string {
	var decode *toml.DecodeError
	if errors.As(err, &decode) {
		return decode.String()
	}
	return err.Error()
}

// parse reads the text through the full TOML decoder, which also refuses what
// the editor's scanner lets through (a duplicate key).
func (c *Config) parse() error {
	values := map[string]any{}
	if err := toml.Unmarshal([]byte(c.text), &values); err != nil {
		return err
	}
	c.values = values
	return nil
}

// Get is a string setting; false when it is absent or not a string.
func (c *Config) Get(key string) (string, bool) {
	text, ok := c.values[key].(string)
	return text, ok
}

// Sign is nil-like (false, false) when the file leaves it to git's
// `commit.gpgsign`. A quoted `sign = "true"` is accepted too.
func (c *Config) Sign() (value, set bool) {
	switch v := c.values["sign"].(type) {
	case bool:
		return v, true
	case string:
		return parseBool(v)
	}
	return false, false
}

// Set writes one setting and saves the file.
func (c *Config) Set(key, text string) error {
	if err := ValidateKey(key); err != nil {
		return err
	}
	if key == "author" {
		if _, _, ok := AuthorParts(text); !ok {
			return fmt.Errorf("author must be `Name <email>`, not `%s`", text)
		}
	}
	value := tomlString(text)
	if key == "sign" {
		on, ok := parseBool(text)
		if !ok {
			return fmt.Errorf("sign must be `true` or `false`, not `%s`", text)
		}
		value = fmt.Sprint(on)
	}
	updated, err := setKey(c.text, key, value)
	if err != nil {
		return err
	}
	return c.save(updated)
}

// Unset removes a setting and reports whether it was there.
func (c *Config) Unset(key string) (bool, error) {
	if err := ValidateKey(key); err != nil {
		return false, err
	}
	updated, removed, err := removeKey(c.text, key)
	if err != nil || !removed {
		return false, err
	}
	return true, c.save(updated)
}

func (c *Config) save(text string) error {
	if err := os.MkdirAll(filepath.Dir(c.path), 0o755); err != nil {
		return err
	}
	if err := os.WriteFile(c.path, []byte(text), 0o644); err != nil {
		return err
	}
	c.text = text
	return c.parse()
}

// WriteTemplate writes the starter config unless a file exists, and returns
// whether it wrote one.
func WriteTemplate(p paths.Paths) (bool, error) {
	path := configPath(p)
	if _, err := os.Stat(path); err == nil {
		return false, nil
	}
	if err := os.MkdirAll(p.ConfigDir(), 0o755); err != nil {
		return false, err
	}
	return true, os.WriteFile(path, []byte(template), 0o644)
}

// ValidateKey refuses a setting noda does not have.
func ValidateKey(key string) error {
	if slices.Contains(Keys, key) {
		return nil
	}
	return fmt.Errorf("unknown setting: %s — noda knows %s", key, strings.Join(Keys, ", "))
}

// Source says where a value came from, so `noda config` can answer "why is it
// using vi?".
type Source int

const (
	SourceFile Source = iota
	SourceEnvironment
	SourceGit
	SourceDefault
)

// Label is the name `noda config` prints.
func (s Source) Label() string {
	switch s {
	case SourceFile:
		return "config.toml"
	case SourceEnvironment:
		return "environment"
	case SourceGit:
		return "git config"
	}
	return "built-in"
}

// Editor picks the editor command. The config wins over $VISUAL and $EDITOR, as
// git's core.editor does; a blank value does not count.
func Editor(configured, visual, editor string) (string, Source) {
	if value := strings.TrimSpace(configured); value != "" {
		return value, SourceFile
	}
	for _, fromEnvironment := range []string{visual, editor} {
		if value := strings.TrimSpace(fromEnvironment); value != "" {
			return value, SourceEnvironment
		}
	}
	return "vi", SourceDefault
}

// parseBool is not git's `yes`/`on`/`1`, which TOML would read differently.
func parseBool(text string) (value, ok bool) {
	switch strings.TrimSpace(text) {
	case "true":
		return true, true
	case "false":
		return false, true
	}
	return false, false
}

// AuthorParts splits `Name <email>`; both halves must be non-empty.
func AuthorParts(text string) (name, email string, ok bool) {
	name, rest, found := strings.Cut(strings.TrimSpace(text), "<")
	if !found {
		return "", "", false
	}
	email, found = strings.CutSuffix(rest, ">")
	if !found {
		return "", "", false
	}
	email = strings.TrimSpace(email)
	name = strings.TrimSpace(name)
	if name == "" || email == "" || strings.Contains(email, "<") {
		return "", "", false
	}
	return name, email, true
}
