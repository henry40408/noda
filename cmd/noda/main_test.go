package main

import (
	"bytes"
	"errors"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"

	"github.com/henry40408/noda/internal/gitx/gittest"
)

// result is one in-process run.
type result struct {
	code           int
	stdout, stderr string
}

// sandbox is an isolated home: HOME and every XDG root point into a temp dir,
// so no test touches the developer's notebooks or git config.
type sandbox struct {
	env map[string]string
}

func newSandbox(t *testing.T) sandbox {
	t.Helper()
	env := gittest.Isolate(t)
	for _, name := range []string{"RUST_LOG", "NODA_LOG_LEVEL", "NODA_LOG_FORMAT", "NO_COLOR", "CLICOLOR_FORCE", "CLICOLOR", "VISUAL", "EDITOR", "TZ",
		"XDG_DATA_HOME", "XDG_STATE_HOME", "XDG_CACHE_HOME"} {
		delete(env, name)
	}
	// noda's own setting outranks git's, and turns signing off for every test.
	configHome := filepath.Join(env["HOME"], "cfg")
	env["XDG_CONFIG_HOME"] = configHome
	if err := os.MkdirAll(filepath.Join(configHome, "noda"), 0o777); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(configHome, "noda", "config.toml"), []byte("sign = false\n"), 0o666); err != nil {
		t.Fatal(err)
	}
	return sandbox{env: env}
}

func (s sandbox) run(args ...string) result {
	var stdout, stderr bytes.Buffer
	code := run(process{args: args, env: s.env, stdin: strings.NewReader(""), stdout: &stdout, stderr: &stderr})
	return result{code, stdout.String(), stderr.String()}
}

func (s sandbox) ok(t *testing.T, args ...string) string {
	t.Helper()
	got := s.run(args...)
	if got.code != 0 {
		t.Fatalf("noda %v: exit %d, stderr %q", args, got.code, got.stderr)
	}
	return got.stdout
}

func TestVersionFlagsPrintTheVersion(t *testing.T) {
	s := newSandbox(t)
	for _, flag := range []string{"--version", "-V"} {
		got := s.run(flag)
		if got.code != 0 || !strings.HasPrefix(got.stdout, "noda ") || !strings.HasSuffix(got.stdout, "\n") {
			t.Fatalf("%s: %+v", flag, got)
		}
	}
}

func TestHelpNeedsNeitherGitNorAHome(t *testing.T) {
	var stdout, stderr bytes.Buffer
	env := map[string]string{"PATH": ""}
	for _, args := range [][]string{{"--help"}, {"-h"}, {"help"}, {"help", "add"}, {"add", "--help"}, {"--version"}} {
		stdout.Reset()
		if code := run(process{args: args, env: env, stdout: &stdout, stderr: &stderr}); code != 0 || stdout.Len() == 0 {
			t.Fatalf("%v: exit %d, stdout %q, stderr %q", args, code, stdout.String(), stderr.String())
		}
	}
}

func TestACommandNeedsGit(t *testing.T) {
	var stdout, stderr bytes.Buffer
	code := run(process{args: []string{"ls"}, env: map[string]string{"HOME": t.TempDir(), "PATH": ""}, stdout: &stdout, stderr: &stderr})
	if code != 1 || !strings.HasPrefix(stderr.String(), "noda: noda needs git") {
		t.Fatalf("exit %d, stderr %q", code, stderr.String())
	}
}

func TestAnOldGitIsRefusedButVersionStillWorks(t *testing.T) {
	bin := t.TempDir()
	script := "#!/bin/sh\necho 'git version 2.38.0'\n"
	if err := os.WriteFile(filepath.Join(bin, "git"), []byte(script), 0o755); err != nil {
		t.Fatal(err)
	}
	var stdout, stderr bytes.Buffer
	env := map[string]string{"HOME": t.TempDir(), "PATH": bin}
	if code := run(process{args: []string{"ls"}, env: env, stdout: &stdout, stderr: &stderr}); code != 1 ||
		!strings.Contains(stderr.String(), "found 2.38.0") {
		t.Fatalf("exit %d, stderr %q", code, stderr.String())
	}
	stdout.Reset()
	if code := run(process{args: []string{"--version"}, env: env, stdout: &stdout, stderr: &stderr}); code != 0 {
		t.Fatalf("--version exit %d", code)
	}
}

func TestMissingHomeIsReported(t *testing.T) {
	s := newSandbox(t)
	delete(s.env, "HOME")
	delete(s.env, "XDG_CONFIG_HOME")
	if got := s.run("status"); got.code != 1 || !strings.Contains(got.stderr, "HOME is not set") {
		t.Fatalf("%+v", got)
	}
}

func TestLegacyEnvVarStopsEveryCommandIncludingVersionAndHelp(t *testing.T) {
	// Pinned on purpose: as in comics, the check comes before the arguments, so
	// a stale RUST_LOG cannot hide behind `--version`.
	for _, args := range [][]string{{"ls"}, {"--version"}, {"--help"}, {"help"}, {"bogus"}} {
		s := newSandbox(t)
		s.env["RUST_LOG"] = ""
		got := s.run(args...)
		want := "noda: these environment variables no longer exist; rename (or unset) them to continue:\n  RUST_LOG -> NODA_LOG_LEVEL\n"
		if got.code != 1 || got.stderr != want || got.stdout != "" {
			t.Fatalf("%v: %+v", args, got)
		}
	}
}

func TestUsageErrorsExitTwoWithClapsShape(t *testing.T) {
	s := newSandbox(t)
	got := s.run("show")
	want := "error: the following required arguments were not provided:\n  <NOTE>\n\nUsage: noda show <NOTE>\n\nFor more information, try '--help'.\n"
	if got.code != 2 || got.stderr != want {
		t.Fatalf("%+v", got)
	}
	if got := s.run(); got.code != 2 || got.stdout != "" || !strings.HasPrefix(got.stderr, "A git-native notebook for your terminal\n\nUsage: noda <COMMAND>") {
		t.Fatalf("no subcommand: %+v", got)
	}
}

func TestNoteLifecycleThroughTheCommandLine(t *testing.T) {
	s := newSandbox(t)
	s.ok(t, "init")
	added := s.ok(t, "add", "Meeting Notes", "--tag", "work", "-c", "- [ ] agenda\n")
	id := strings.Fields(added)[0]
	if ls := s.ok(t, "ls"); !strings.Contains(ls, id) || !strings.Contains(ls, "Meeting Notes") {
		t.Fatalf("ls:\n%s", ls)
	}
	if shown := s.ok(t, "show", id); !strings.Contains(shown, "- [ ] agenda") {
		t.Fatalf("show:\n%s", shown)
	}
	s.ok(t, "tag", id, "+q3", "-work")
	if ls := s.ok(t, "ls", "--tag", "q3", "-q"); ls != id+"\n" {
		t.Fatalf("ls -q: %q", ls)
	}
	if null := s.ok(t, "ls", "-q0"); null != id+"\x00" {
		t.Fatalf("ls -q0 is NUL-terminated and gets no newline: %q", null)
	}
	if found := s.ok(t, "search", "-tag:work", "agenda"); !strings.Contains(found, id) {
		t.Fatalf("search:\n%s", found)
	}
	s.ok(t, "mv", id, "Retitled")
	s.ok(t, "rm", id)
	if ls := s.ok(t, "ls", "--json"); !strings.Contains(ls, `"notes":[]`) {
		t.Fatalf("after rm: %s", ls)
	}
}

func TestAFlagMayFollowThePositionalsExceptAfterTags(t *testing.T) {
	s := newSandbox(t)
	s.ok(t, "init")
	id := strings.Fields(s.ok(t, "add", "Tagged", "-c", "x"))[0]
	s.ok(t, "tag", "--no-touch", id, "+a")
	// After the first tag every argument is a tag, as in the Rust build.
	if got := s.run("tag", id, "+b", "--no-touch"); got.code != 1 {
		t.Fatalf("`--no-touch` after the tags is read as a tag: %+v", got)
	}
	s.ok(t, "ls", "--long", "--sort=title", "-r")
	if got := s.run("ls", "--sort", "size"); got.code != 2 || !strings.Contains(got.stderr, "[possible values: created, updated, title]") {
		t.Fatalf("%+v", got)
	}
}

func TestAnEmptyValueIsAValueNotAnAbsentOne(t *testing.T) {
	s := newSandbox(t)
	s.ok(t, "init")
	s.ok(t, "add", "Alpha", "-c", "x", "--tag", "t")
	source := filepath.Join(t.TempDir(), "f.txt")
	if err := os.WriteFile(source, []byte("hi"), 0o666); err != nil {
		t.Fatal(err)
	}
	for _, c := range []struct {
		want string
		args []string
	}{
		{"invalid note reference:", []string{"path", ""}},
		{"invalid note reference:", []string{"log", ""}},
		{"invalid note reference:", []string{"diff", ""}},
		{"invalid notebook name:", []string{"ls", "--notebook="}},
		{"invalid notebook name:", []string{"deleted", "--notebook="}},
		{"invalid notebook name:", []string{"clone", "somewhere", ""}},
		{"a file needs a name", []string{"file", "add", "--as=", source}},
		{"cannot set empty URL", []string{"clone", "", "nb"}},
		{"cannot set empty URL", []string{"notebook", "add", "nb2", "--remote="}},
	} {
		got := s.run(c.args...)
		if got.code != 1 || !strings.Contains(got.stderr, c.want) || got.stdout != "" {
			t.Errorf("%v: %+v", c.args, got)
		}
	}
	// Rust created the notebook before it refused the remote.
	if got := s.ok(t, "notebook", "ls"); !strings.Contains(got, "nb2") {
		t.Errorf("notebook ls: %q", got)
	}
	if got := s.run("ls", "--tag="); got.code != 0 || got.stdout != "" {
		t.Errorf("--tag= lists nothing: %+v", got)
	}
	if got := s.ok(t, "ls", "--tag=", "--json"); !strings.Contains(got, `"notes":[]`) || !strings.Contains(got, `"files":[]`) {
		t.Errorf("--tag= --json: %s", got)
	}
}

func TestConfigThroughTheCommandLine(t *testing.T) {
	s := newSandbox(t)
	s.ok(t, "init")
	s.ok(t, "config", "author", "Ann <ann@example.com>")
	if got := s.ok(t, "config", "author"); !strings.Contains(got, "Ann") {
		t.Fatalf("get: %q", got)
	}
	s.ok(t, "config", "author", "--unset")
	if got := s.run("config", "--unset"); got.code != 2 {
		t.Fatalf("--unset needs a key: %+v", got)
	}
}

func TestPrintStripsColourOffATerminalAndKeepsItWhenForced(t *testing.T) {
	s := newSandbox(t)
	s.ok(t, "init")
	s.ok(t, "add", "Pretty", "-c", "x", "--tag", "t")
	if plain := s.ok(t, "ls"); strings.Contains(plain, "\x1b") {
		t.Fatalf("a pipe got escapes: %q", plain)
	}
	s.env["CLICOLOR_FORCE"] = "1"
	if forced := s.ok(t, "ls"); !strings.Contains(forced, "\x1b[") {
		t.Fatalf("forced colour is missing: %q", forced)
	}
	s.env["NO_COLOR"] = "1"
	if off := s.ok(t, "ls"); strings.Contains(off, "\x1b") {
		t.Fatalf("NO_COLOR is ignored: %q", off)
	}
}

func TestWebRefusesABadLogLevelAndIsNotYetServed(t *testing.T) {
	s := newSandbox(t)
	s.env["NODA_LOG_LEVEL"] = "loud"
	if got := s.run("web"); got.code != 1 || !strings.Contains(got.stderr, `unknown log level "loud"`) {
		t.Fatalf("%+v", got)
	}
	delete(s.env, "NODA_LOG_LEVEL")
	if got := s.run("web", "--log-format", "xml"); got.code != 2 {
		t.Fatalf("%+v", got)
	}
}

func TestEnvironmentSplitsOnTheFirstEquals(t *testing.T) {
	got := environment([]string{"A=b=c", "EMPTY=", "bare"})
	if got["A"] != "b=c" || got["EMPTY"] != "" || len(got) != 2 {
		t.Fatalf("%v", got)
	}
}

func TestOnlyEPIPEIsABrokenPipe(t *testing.T) {
	if isBrokenPipe(errors.New("other")) {
		t.Fatal("any error is not a broken pipe")
	}
}

// buildBinary is for the tests that need a real process: exit codes, signals
// and a closed stdout cannot be seen from run().
func buildBinary(t *testing.T) string {
	t.Helper()
	out := filepath.Join(t.TempDir(), "noda")
	build := exec.Command("go", "build", "-o", out, ".")
	if output, err := build.CombinedOutput(); err != nil {
		t.Fatalf("go build: %v\n%s", err, output)
	}
	return out
}

func TestBinaryExitsOneOnARetiredVariable(t *testing.T) {
	binary := buildBinary(t)
	command := exec.Command(binary, "--version")
	command.Env = append(withoutRustLog(os.Environ()), "RUST_LOG=")
	var stderr bytes.Buffer
	command.Stderr = &stderr
	err := command.Run()
	var exit *exec.ExitError
	if !errors.As(err, &exit) || exit.ExitCode() != 1 || !strings.Contains(stderr.String(), "  RUST_LOG -> NODA_LOG_LEVEL") {
		t.Fatalf("%v, stderr %q", err, stderr.String())
	}
}

func TestBinaryTreatsABrokenPipeAsSuccess(t *testing.T) {
	binary := buildBinary(t)
	read, write, err := os.Pipe()
	if err != nil {
		t.Fatal(err)
	}
	read.Close()
	command := exec.Command(binary, "--help")
	command.Env = withoutRustLog(os.Environ())
	command.Stdout = write
	var stderr bytes.Buffer
	command.Stderr = &stderr
	runErr := command.Run()
	write.Close()
	if runErr != nil || stderr.Len() != 0 {
		t.Fatalf("%v, stderr %q", runErr, stderr.String())
	}
}

// The editor is a child of the binary. If noda ignored SIGPIPE, the child
// would inherit that and `yes | head -1` would end with "Broken pipe".
func TestBinaryLeavesSIGPIPEAtItsDefaultForChildren(t *testing.T) {
	binary := buildBinary(t)
	s := newSandbox(t)
	s.ok(t, "init")
	editor := filepath.Join(t.TempDir(), "editor.sh")
	if err := os.WriteFile(editor, []byte("#!/bin/sh\nyes | head -1 >/dev/null\n"), 0o777); err != nil {
		t.Fatal(err)
	}
	s.env["EDITOR"] = editor
	command := exec.Command(binary, "add")
	for name, value := range s.env {
		command.Env = append(command.Env, name+"="+value)
	}
	var stderr bytes.Buffer
	command.Stderr = &stderr
	_ = command.Run() // the empty note is refused; only the editor's stderr matters
	if strings.Contains(stderr.String(), "Broken pipe") || !strings.Contains(stderr.String(), "aborted") {
		t.Fatalf("stderr %q", stderr.String())
	}
}

func withoutRustLog(environ []string) []string {
	kept := make([]string, 0, len(environ))
	for _, line := range environ {
		if !strings.HasPrefix(line, "RUST_LOG=") {
			kept = append(kept, line)
		}
	}
	return kept
}
