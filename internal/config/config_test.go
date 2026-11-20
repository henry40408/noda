package config

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"testing"

	"github.com/henry40408/noda/internal/paths"
)

type editOp struct{ verb, key, value string }

func set(key, value string) editOp { return editOp{"set", key, value} }
func unset(key string) editOp      { return editOp{"unset", key, ""} }

// parseErrorResult stands for a TOML parse error: its wording is the TOML
// library's, so only the path and the hint around it are compared.
const parseErrorResult = "<parse error>"

const unknownColor = "unknown setting: color — noda knows editor, author, notebook, sign"

type editCase struct {
	name        string
	noFile      bool // the config file does not exist before the first op
	initial     string
	ops         []editOp
	want        string
	wantNoFile  bool // no op may have created the file
	wantResults []string
}

func runEdits(t *testing.T, c editCase) (string, bool, []string) {
	t.Helper()
	p := paths.Rooted(t.TempDir())
	file := filepath.Join(p.ConfigDir(), "config.toml")
	if !c.noFile {
		if err := os.MkdirAll(p.ConfigDir(), 0o755); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(file, []byte(c.initial), 0o644); err != nil {
			t.Fatal(err)
		}
	}
	var results []string
	for _, op := range c.ops {
		results = append(results, runEdit(p, op))
	}
	raw, err := os.ReadFile(file)
	if errors.Is(err, os.ErrNotExist) {
		return "", false, results
	}
	if err != nil {
		t.Fatal(err)
	}
	return string(raw), true, results
}

func runEdit(p paths.Paths, op editOp) string {
	config, err := Load(p)
	if err != nil {
		return describeLoadError(p, err)
	}
	if op.verb == "set" {
		if err := config.Set(op.key, op.value); err != nil {
			return "err: " + err.Error()
		}
		return "set"
	}
	removed, err := config.Unset(op.key)
	if err != nil {
		return "err: " + err.Error()
	}
	return fmt.Sprintf("removed=%v", removed)
}

func describeLoadError(p paths.Paths, err error) string {
	const hint = "\nfix it by hand, or start over with `noda config --edit`"
	text := err.Error()
	if strings.HasPrefix(text, p.ConfigDir()+"/config.toml: ") && strings.HasSuffix(text, hint) {
		return parseErrorResult
	}
	return "load error: " + text
}

func TestConfigEdits(t *testing.T) {
	cases := []editCase{
		{name: "a first set creates the file", noFile: true, ops: []editOp{set("editor", "vi")},
			want: "editor = \"vi\"\n", wantResults: []string{"set"}},
		{name: "author and sign are written in their own types", noFile: true,
			ops:  []editOp{set("author", "A B <a@b.c>"), set("sign", "true")},
			want: "author = \"A B <a@b.c>\"\nsign = true\n", wantResults: []string{"set", "set"}},
		{name: "a bad sign is refused and creates no file", noFile: true,
			ops: []editOp{set("sign", "no"), set("sign", "Yes"), set("sign", "off"), set("sign", "1"), set("sign", "0")}, wantNoFile: true,
			wantResults: []string{
				"err: sign must be `true` or `false`, not `no`", "err: sign must be `true` or `false`, not `Yes`",
				"err: sign must be `true` or `false`, not `off`", "err: sign must be `true` or `false`, not `1`",
				"err: sign must be `true` or `false`, not `0`",
			}},
		{name: "an author without an email is refused", noFile: true, ops: []editOp{set("author", "nobody")}, wantNoFile: true,
			wantResults: []string{"err: author must be `Name <email>`, not `nobody`"}},
		{name: "an unknown key is refused by set and unset", noFile: true,
			ops: []editOp{set("color", "red"), unset("color")}, wantNoFile: true,
			wantResults: []string{"err: " + unknownColor, "err: " + unknownColor}},
		{name: "unsetting an absent key reports false and creates no file", noFile: true,
			ops: []editOp{unset("editor")}, wantNoFile: true, wantResults: []string{"removed=false"}},
		{name: "a second key is added after the first", noFile: true,
			ops:  []editOp{set("editor", "vi"), set("notebook", "work")},
			want: "editor = \"vi\"\nnotebook = \"work\"\n", wantResults: []string{"set", "set"}},
		{name: "unsetting the only key leaves an empty file", noFile: true,
			ops: []editOp{set("editor", "vi"), unset("editor")}, want: "", wantResults: []string{"set", "removed=true"}},
		{name: "a value with quotes and a backslash is a literal string", noFile: true,
			ops: []editOp{set("editor", `vi "x" \ y`)}, want: "editor = 'vi \"x\" \\ y'\n", wantResults: []string{"set"}},
		{name: "unicode is written as is", noFile: true, ops: []editOp{set("editor", "編輯器")},
			want: "editor = \"編輯器\"\n", wantResults: []string{"set"}},
		{name: "a newline makes a multi-line string", noFile: true, ops: []editOp{set("editor", "a\nb")},
			want: "editor = \"\"\"\na\nb\"\"\"\n", wantResults: []string{"set"}},
		{name: "a tab is escaped", noFile: true, ops: []editOp{set("editor", "a\tb")},
			want: "editor = \"a\\tb\"\n", wantResults: []string{"set"}},
		{name: "an empty value is written", noFile: true, ops: []editOp{set("editor", "")},
			want: "editor = \"\"\n", wantResults: []string{"set"}},
		{name: "the starter template keeps its comments around a set", initial: template, ops: []editOp{set("editor", "nvim"), set("sign", "true")},
			want: template + "editor = \"nvim\"\nsign = true\n", wantResults: []string{"set", "set"}},
		{name: "setting a key twice replaces the first value", initial: template, ops: []editOp{set("editor", "nvim"), set("editor", "vim")},
			want: template + "editor = \"vim\"\n", wantResults: []string{"set", "set"}},
		{name: "unsetting the only key drops the template's comments too", initial: template,
			ops: []editOp{set("editor", "nvim"), unset("editor")}, want: "", wantResults: []string{"set", "removed=true"}},
		{name: "unsetting a key the template only mentions in a comment changes nothing", initial: template,
			ops: []editOp{unset("editor")}, want: template, wantResults: []string{"removed=false"}},
		{name: "a replaced value drops its trailing comment", initial: "editor = \"vi\" # my editor\n",
			ops: []editOp{set("editor", "nvim")}, want: "editor = \"nvim\"\n", wantResults: []string{"set"}},
		{name: "a replaced value keeps the key's indent and normalises the spacing", initial: "  editor   =   \"vi\"\n",
			ops: []editOp{set("editor", "nvim")}, want: "  editor   = \"nvim\"\n", wantResults: []string{"set"}},
		{name: "a single-quoted value is replaced", initial: "editor = 'vi'\n",
			ops: []editOp{set("editor", "nvim")}, want: "editor = \"nvim\"\n", wantResults: []string{"set"}},
		{name: "a new key goes above the first table", initial: "editor = \"vi\"\n\n[other]\nx = 1\n",
			ops: []editOp{set("notebook", "work")}, want: "editor = \"vi\"\nnotebook = \"work\"\n\n[other]\nx = 1\n", wantResults: []string{"set"}},
		{name: "a new key goes above a table that has the same key", initial: "[git]\neditor = \"x\"\n",
			ops: []editOp{set("editor", "vi")}, want: "editor = \"vi\"\n[git]\neditor = \"x\"\n", wantResults: []string{"set"}},
		{name: "a new key goes after an inline table", initial: "other = { a = 1 }\n",
			ops: []editOp{set("editor", "vi")}, want: "other = { a = 1 }\neditor = \"vi\"\n", wantResults: []string{"set"}},
		{name: "a header comment is kept", initial: "# my config\neditor = \"vi\"\n",
			ops: []editOp{set("sign", "true")}, want: "# my config\neditor = \"vi\"\nsign = true\n", wantResults: []string{"set"}},
		{name: "unknown keys are left alone", initial: "color = \"red\"\neditor = \"vi\"\n",
			ops: []editOp{set("notebook", "w")}, want: "color = \"red\"\neditor = \"vi\"\nnotebook = \"w\"\n", wantResults: []string{"set"}},
		{name: "blank lines around the settings are kept", initial: "\n\neditor = \"vi\"\n\n\n",
			ops: []editOp{set("notebook", "w")}, want: "\n\neditor = \"vi\"\nnotebook = \"w\"\n\n\n", wantResults: []string{"set"}},
		{name: "a missing final newline is supplied", initial: "editor = \"vi\"",
			ops: []editOp{set("notebook", "w")}, want: "editor = \"vi\"\nnotebook = \"w\"\n", wantResults: []string{"set"}},
		{name: "CRLF line endings become LF", initial: "editor = \"vi\"\r\n",
			ops: []editOp{set("notebook", "w")}, want: "editor = \"vi\"\nnotebook = \"w\"\n", wantResults: []string{"set"}},
		{name: "an empty file takes the first key", initial: "", ops: []editOp{set("editor", "vi")},
			want: "editor = \"vi\"\n", wantResults: []string{"set"}},
		{name: "a comment-only file keeps its comment", initial: "# just a comment",
			ops: []editOp{set("editor", "vi")}, want: "# just a comment\neditor = \"vi\"\n", wantResults: []string{"set"}},
		{name: "a string sign becomes a boolean", initial: "sign = \"true\"\n",
			ops: []editOp{set("sign", "false")}, want: "sign = false\n", wantResults: []string{"set"}},
		{name: "an array value is replaced by a string", initial: "editor = [\"a\"]\n",
			ops: []editOp{set("editor", "vi")}, want: "editor = \"vi\"\n", wantResults: []string{"set"}},
		{name: "unsetting the first of two keys keeps the second", initial: "editor = \"vi\"\nnotebook = \"w\"\n",
			ops: []editOp{unset("editor")}, want: "notebook = \"w\"\n", wantResults: []string{"removed=true"}},
		{name: "unsetting the last of two keys keeps the first", initial: "editor = \"vi\"\nnotebook = \"w\"\n",
			ops: []editOp{unset("notebook")}, want: "editor = \"vi\"\n", wantResults: []string{"removed=true"}},
		{name: "unsetting a key takes its leading comment with it", initial: "# about editor\neditor = \"vi\"\n",
			ops: []editOp{unset("editor")}, want: "", wantResults: []string{"removed=true"}},
		{name: "unsetting a key takes its trailing comment with it", initial: "editor = \"vi\" # c\n",
			ops: []editOp{unset("editor")}, want: "", wantResults: []string{"removed=true"}},
		{name: "a missing value is a parse error and the file is untouched", initial: "editor = \n",
			ops: []editOp{set("editor", "vi")}, want: "editor = \n", wantResults: []string{parseErrorResult}},
		{name: "an unterminated string is a parse error", initial: "editor = \"vi\n",
			ops: []editOp{unset("editor")}, want: "editor = \"vi\n", wantResults: []string{parseErrorResult}},
		{name: "a duplicate key is a parse error", initial: "editor = \"a\"\neditor = \"b\"\n",
			ops: []editOp{set("editor", "c")}, want: "editor = \"a\"\neditor = \"b\"\n", wantResults: []string{parseErrorResult}},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			file, exists, results := runEdits(t, c)
			if exists == c.wantNoFile {
				t.Errorf("file exists = %v, want %v", exists, !c.wantNoFile)
			}
			if file != c.want {
				t.Errorf("file\n got  %q\n want %q", file, c.want)
			}
			if !reflect.DeepEqual(results, c.wantResults) {
				t.Errorf("results\n got  %q\n want %q", results, c.wantResults)
			}
		})
	}
}

func TestTheStarterConfigIsWrittenOnceAndChangesNothing(t *testing.T) {
	p := paths.Rooted(t.TempDir())
	wrote, err := WriteTemplate(p)
	if err != nil || !wrote {
		t.Fatalf("%v %v", wrote, err)
	}
	if wrote, _ = WriteTemplate(p); wrote {
		t.Fatal("an existing file is left alone")
	}
	config, err := Load(p)
	if err != nil {
		t.Fatal(err)
	}
	if _, ok := config.Get("editor"); ok {
		t.Fatal("the template sets nothing")
	}
	if _, set := config.Sign(); set {
		t.Fatal("unset means ask git")
	}
}

func TestTheConfigFileOutranksTheEnvironment(t *testing.T) {
	chosen, source := Editor("nvim", "code -w", "vi")
	if chosen != "nvim" || source != SourceFile {
		t.Fatal(chosen, source)
	}
}

func TestVisualOutranksEditorAndBothOutrankTheFallback(t *testing.T) {
	cases := []struct {
		visual, editor, want string
		source               Source
	}{
		{"code -w", "vi", "code -w", SourceEnvironment},
		{"", "emacs", "emacs", SourceEnvironment},
		{"", "", "vi", SourceDefault},
	}
	for _, c := range cases {
		if chosen, source := Editor("", c.visual, c.editor); chosen != c.want || source != c.source {
			t.Errorf("%+v: %q %v", c, chosen, source)
		}
	}
}

func TestASettingThatIsPresentButBlankDoesNotCount(t *testing.T) {
	if chosen, source := Editor("  ", "   ", ""); chosen != "vi" || source != SourceDefault {
		t.Fatal(chosen, source)
	}
}

func TestSourceLabels(t *testing.T) {
	want := map[Source]string{
		SourceFile: "config.toml", SourceEnvironment: "environment", SourceGit: "git config", SourceDefault: "built-in",
	}
	for source, label := range want {
		if source.Label() != label {
			t.Errorf("%v: %s", source, source.Label())
		}
	}
}

func TestAnAuthorNeedsBothHalves(t *testing.T) {
	name, email, ok := AuthorParts("Heng-Yi Wu <me@henry40408.com>")
	if !ok || name != "Heng-Yi Wu" || email != "me@henry40408.com" {
		t.Fatal(name, email, ok)
	}
	name, email, ok = AuthorParts("  Spaced  < e@x >  ")
	if !ok || name != "Spaced" || email != "e@x" {
		t.Fatal(name, email, ok)
	}
	for _, bad := range []string{"no email", "<only@email>", "Name <unclosed", "A <b<c>"} {
		if _, _, ok := AuthorParts(bad); ok {
			t.Errorf("%q", bad)
		}
	}
}

func TestSignIsABooleanHoweverItWasWritten(t *testing.T) {
	for text, want := range map[string]bool{"true": true, " false ": false} {
		if got, ok := parseBool(text); !ok || got != want {
			t.Errorf("%q", text)
		}
	}
	for _, bad := range []string{"yes", "1", ""} {
		if _, ok := parseBool(bad); ok {
			t.Errorf("%q", bad)
		}
	}
	cases := map[string][2]bool{
		"sign = true": {true, true}, "sign = false": {false, true}, `sign = "true"`: {true, true},
		// Unset means "ask git", not "off".
		`editor = "vi"`: {false, false}, "sign = 3": {false, false},
	}
	for text, want := range cases {
		config := &Config{text: text}
		if err := config.parse(); err != nil {
			t.Fatal(err)
		}
		if value, set := config.Sign(); [2]bool{value, set} != want {
			t.Errorf("%s: %v %v", text, value, set)
		}
	}
}

func TestUnknownSettingsAreNamedRatherThanIgnored(t *testing.T) {
	err := ValidateKey("edito")
	if err == nil || !strings.Contains(err.Error(), "editor, author, notebook") {
		t.Fatal(err)
	}
	for _, key := range Keys {
		if err := ValidateKey(key); err != nil {
			t.Fatal(err)
		}
	}
}

// A value with a quote and a backslash or double quote used to be written as a
// single-line literal string, which ended at the quote: config.toml was then
// unparseable and every later Load failed.
func TestAValueWithASingleQuoteStaysParseable(t *testing.T) {
	for _, value := range []string{`it's "x"`, `a'b\c`, `'`, "it's\nmulti \"line\""} {
		p := paths.Rooted(t.TempDir())
		config, err := Load(p)
		if err != nil {
			t.Fatal(err)
		}
		if err := config.Set("editor", value); err != nil {
			t.Fatalf("%q: %v", value, err)
		}
		reloaded, err := Load(p)
		if err != nil {
			t.Fatalf("%q: the file no longer loads: %v", value, err)
		}
		if got, _ := reloaded.Get("editor"); got != value {
			t.Fatalf("%q came back as %q", value, got)
		}
	}
}

func TestAKeyThatIsATableIsNotOverwrittenSilently(t *testing.T) {
	p := paths.Rooted(t.TempDir())
	if err := os.MkdirAll(p.ConfigDir(), 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(p.ConfigDir(), "config.toml"), []byte("[editor]\nx = 1\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	config, err := Load(p)
	if err != nil {
		t.Fatal(err)
	}
	// Written above the table, the key would make the file invalid TOML.
	if err := config.Set("editor", "vi"); err == nil {
		t.Fatal("a table and a key cannot share a name")
	}
}

func TestAWrittenSettingIsReadBackFromTheSameConfig(t *testing.T) {
	config, err := Load(paths.Rooted(t.TempDir()))
	if err != nil {
		t.Fatal(err)
	}
	if err := config.Set("editor", "vi"); err != nil {
		t.Fatal(err)
	}
	if got, _ := config.Get("editor"); got != "vi" {
		t.Fatalf("got %q after Set", got)
	}
	if removed, err := config.Unset("editor"); err != nil || !removed {
		t.Fatalf("%v %v", removed, err)
	}
	if _, set := config.Get("editor"); set {
		t.Fatal("still set after Unset")
	}
}

func TestAStringIsWrittenLiteralOnlyWhenALiteralCanCarryIt(t *testing.T) {
	cases := map[string]string{
		"a\\b\nc":  "'''\na\\b\nc'''",
		"a\\\tb":   "'a\\\tb'",
		"a\\ b":    `'a\ b'`,
		"a\\\x1fb": `"a\\\u001Fb"`,
		"a\\\x7fb": `"a\\\u007Fb"`,
	}
	for value, want := range cases {
		if got := tomlString(value); got != want {
			t.Errorf("%q: got %q, want %q", value, got, want)
		}
	}
}
