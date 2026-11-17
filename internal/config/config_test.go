package config

import (
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"testing"

	"github.com/henry40408/noda/internal/paths"
)

type goldenCase struct {
	Name    string     `json:"name"`
	Initial *string    `json:"initial"`
	Ops     [][]string `json:"ops"`
}

type goldenExpected struct {
	Name string `json:"name"`
	Out  struct {
		File    *string          `json:"file"`
		Results []map[string]any `json:"results"`
	} `json:"out"`
}

func readJSON(t *testing.T, path string, into any) {
	t.Helper()
	raw, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	if err := json.Unmarshal(raw, into); err != nil {
		t.Fatal(err)
	}
}

func runOps(t *testing.T, c goldenCase) (*string, []map[string]any) {
	root := t.TempDir()
	p := paths.Rooted(root)
	file := filepath.Join(p.ConfigDir(), "config.toml")
	if c.Initial != nil {
		text := *c.Initial
		if text == "@template" {
			text = template
		}
		if err := os.MkdirAll(p.ConfigDir(), 0o755); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(file, []byte(text), 0o644); err != nil {
			t.Fatal(err)
		}
	}
	var results []map[string]any
	for _, op := range c.Ops {
		results = append(results, runOp(p, op))
	}
	raw, err := os.ReadFile(file)
	if errors.Is(err, os.ErrNotExist) {
		return nil, results
	}
	if err != nil {
		t.Fatal(err)
	}
	text := string(raw)
	return &text, results
}

func runOp(p paths.Paths, op []string) map[string]any {
	config, err := Load(p)
	if err != nil {
		return map[string]any{"err": strings.ReplaceAll(err.Error(), p.ConfigDir(), "<ROOT>/config/noda")}
	}
	if op[0] == "set" {
		if err := config.Set(op[1], op[2]); err != nil {
			return map[string]any{"err": err.Error()}
		}
		return map[string]any{"ok": "set"}
	}
	removed, err := config.Unset(op[1])
	if err != nil {
		return map[string]any{"err": err.Error()}
	}
	return map[string]any{"ok": removed}
}

func TestConfigEditsMatchTheRustGolden(t *testing.T) {
	var cases []goldenCase
	var expected []goldenExpected
	readJSON(t, "../../testdata/golden/config/cases.json", &cases)
	readJSON(t, "../../testdata/golden/config/expected.json", &expected)
	if len(cases) != len(expected) || len(cases) != 49 {
		t.Fatalf("%d cases, %d expected", len(cases), len(expected))
	}
	for i, c := range cases {
		if expected[i].Name != c.Name {
			t.Fatalf("case %d is %s, expected %s", i, c.Name, expected[i].Name)
		}
		file, results := runOps(t, c)
		if !reflect.DeepEqual(file, expected[i].Out.File) {
			t.Errorf("%s: file\n got  %q\n want %q", c.Name, deref(file), deref(expected[i].Out.File))
		}
		if len(results) != len(expected[i].Out.Results) {
			t.Fatalf("%s: %d results", c.Name, len(results))
		}
		for j, got := range results {
			assertResult(t, c.Name, got, expected[i].Out.Results[j])
		}
	}
}

func deref(text *string) string {
	if text == nil {
		return "<absent>"
	}
	return *text
}

// A parse error's wording is the TOML library's, so only its frame is compared.
func assertResult(t *testing.T, name string, got, want map[string]any) {
	t.Helper()
	wantErr, isErr := want["err"].(string)
	if !isErr || !strings.Contains(wantErr, "TOML parse error") {
		if !reflect.DeepEqual(got, want) {
			t.Errorf("%s: got %v, want %v", name, got, want)
		}
		return
	}
	gotErr, _ := got["err"].(string)
	const suffix = "\nfix it by hand, or start over with `noda config --edit`"
	prefix := "<ROOT>/config/noda/config.toml: "
	if !strings.HasPrefix(gotErr, prefix) || !strings.HasSuffix(gotErr, suffix) {
		t.Errorf("%s: error %q lacks the path and the hint", name, gotErr)
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
