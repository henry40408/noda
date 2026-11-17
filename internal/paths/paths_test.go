package paths

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func lookupOf(env map[string]string) func(string) (string, bool) {
	return func(key string) (string, bool) {
		v, ok := env[key]
		return v, ok
	}
}

func TestOnlyAnAbsoluteXDGValueIsUsed(t *testing.T) {
	home := "/home/someone"
	fallback := "/home/someone/.config/noda"
	for _, value := range []string{"relative/config", ""} {
		if got := resolve(value, home, ".config"); got != fallback {
			t.Errorf("%q: got %q", value, got)
		}
	}
	if got := resolve("/elsewhere", home, ".config"); got != "/elsewhere/noda" {
		t.Errorf("got %q", got)
	}
}

func TestNewReadsEachRootFromItsVariable(t *testing.T) {
	p, err := New(lookupOf(map[string]string{
		"HOME":            "/home/someone",
		"XDG_DATA_HOME":   "/srv/data",
		"XDG_STATE_HOME":  "relative",
		"XDG_CONFIG_HOME": "",
	}))
	if err != nil {
		t.Fatal(err)
	}
	checks := map[string][2]string{
		"config":    {p.ConfigDir(), "/home/someone/.config/noda"},
		"notebooks": {p.NotebooksDir(), "/srv/data/noda/notebooks"},
		"active":    {p.ActiveFile(), "/home/someone/.local/state/noda/active"},
		"cache":     {p.CacheDir(), "/home/someone/.cache/noda"},
		"notebook":  {p.NotebookDir("work"), "/srv/data/noda/notebooks/work"},
	}
	for name, c := range checks {
		if c[0] != c[1] {
			t.Errorf("%s: got %q, want %q", name, c[0], c[1])
		}
	}
}

func TestNewNeedsHome(t *testing.T) {
	for _, env := range []map[string]string{{}, {"HOME": ""}} {
		if _, err := New(lookupOf(env)); err == nil || err.Error() != "HOME is not set" {
			t.Errorf("%v: got %v", env, err)
		}
	}
}

func TestRootedKeepsTheFourRolesApart(t *testing.T) {
	p := Rooted("/tmp/x")
	want := map[string]string{
		p.NotebooksDir(): "/tmp/x/data/noda/notebooks",
		p.ActiveFile():   "/tmp/x/state/noda/active",
		p.ConfigDir():    "/tmp/x/config/noda",
		p.CacheDir():     "/tmp/x/cache/noda",
	}
	for got, expected := range want {
		if filepath.ToSlash(got) != expected {
			t.Errorf("got %q, want %q", got, expected)
		}
	}
}

func TestTheActivePointerRoundTrips(t *testing.T) {
	p := Rooted(t.TempDir())
	if _, err := p.ActiveNotebook(); err == nil || !strings.Contains(err.Error(), "run `noda init` first") {
		t.Fatalf("missing pointer: %v", err)
	}
	if err := p.SetActiveNotebook("work"); err != nil {
		t.Fatal(err)
	}
	if got, err := p.ActiveNotebook(); err != nil || got != "work" {
		t.Fatalf("got %q, %v", got, err)
	}
	raw, _ := os.ReadFile(p.ActiveFile())
	if string(raw) != "work\n" {
		t.Fatalf("file holds %q", raw)
	}
}

func TestABlankPointerIsAsGoodAsNone(t *testing.T) {
	p := Rooted(t.TempDir())
	if err := p.SetActiveNotebook("  "); err != nil {
		t.Fatal(err)
	}
	if _, err := p.ActiveNotebook(); err == nil || !strings.Contains(err.Error(), "no active notebook") {
		t.Fatalf("got %v", err)
	}
}

func TestAnUnreadablePointerNamesTheFileOnce(t *testing.T) {
	p := Rooted(t.TempDir())
	// A directory in the pointer's place cannot be read as a file.
	if err := os.MkdirAll(p.ActiveFile(), 0o777); err != nil {
		t.Fatal(err)
	}
	_, err := p.ActiveNotebook()
	if err == nil || strings.Count(err.Error(), p.ActiveFile()) != 1 {
		t.Fatalf("got %v", err)
	}
}

func TestCreateDirsMakesEveryRoot(t *testing.T) {
	p := Rooted(t.TempDir())
	if err := p.CreateDirs(); err != nil {
		t.Fatal(err)
	}
	for _, dir := range []string{p.ConfigDir(), p.CacheDir(), p.NotebooksDir(), filepath.Dir(p.ActiveFile())} {
		if info, err := os.Stat(dir); err != nil || !info.IsDir() {
			t.Errorf("%s missing: %v", dir, err)
		}
	}
}
