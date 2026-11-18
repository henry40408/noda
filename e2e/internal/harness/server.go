// Package harness starts the server under test and the browser that drives it.
//
// The fixture notebook is built by running the binary (`init`, then `add`)
// rather than committed, so a change to what `add` writes reaches these tests.
// Nothing here asserts on an id: ids are minted, so features name notes by
// title.
package harness

import (
	"fmt"
	"net"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
	"time"
)

// Notebook is the name of the notebook the features read.
const Notebook = "default"

const startupTimeout = time.Minute

// Workspace is a directory tree the binary is pointed at through the XDG
// variables, so the developer's real notebooks and git config stay untouched.
type Workspace struct {
	Binary string
	Root   string
}

// Run executes noda against the workspace, returning stdout.
func (w Workspace) Run(args ...string) (string, error) {
	command := exec.Command(w.Binary, args...)
	command.Env = w.env()
	output, err := command.Output()
	if err != nil {
		return "", commandError(w.Binary, args, err)
	}
	return string(output), nil
}

// env is all four XDG directories, state included: the active-notebook pointer
// lives there. The git config is empty, so a machine that signs its commits
// does not send every commit here to gpg.
func (w Workspace) env() []string {
	return append(os.Environ(),
		"HOME="+filepath.Join(w.Root, "home"),
		"XDG_CONFIG_HOME="+filepath.Join(w.Root, "config"),
		"XDG_DATA_HOME="+filepath.Join(w.Root, "data"),
		"XDG_STATE_HOME="+filepath.Join(w.Root, "state"),
		"XDG_CACHE_HOME="+filepath.Join(w.Root, "cache"),
		"GIT_CONFIG_GLOBAL="+os.DevNull,
		"GIT_CONFIG_NOSYSTEM=1",
	)
}

func (w Workspace) notebookDir() string {
	return filepath.Join(w.Root, "data", "noda", "notebooks", Notebook)
}

func (w Workspace) remoteDir() string { return filepath.Join(w.Root, "origin.git") }

// WriteConfig writes the config every workspace starts with.
func (w Workspace) WriteConfig() error {
	dir := filepath.Join(w.Root, "config", "noda")
	if err := os.MkdirAll(dir, 0o755); err != nil {
		return err
	}
	return os.WriteFile(filepath.Join(dir, "config.toml"), []byte("sign = false\n"), 0o644)
}

// NoteFile is the filename `noda path` answers for a note.
func (w Workspace) NoteFile(key string) (string, error) {
	path, err := w.Run("path", key)
	if err != nil {
		return "", err
	}
	return filepath.Base(strings.TrimSpace(path)), nil
}

// Server is a running `noda web` and its workspace.
type Server struct {
	Workspace
	BaseURL string
	command *exec.Cmd
	head    string
}

// EnsureBinary builds the binary into a directory of its own. The build cache
// makes it cheap, and it cannot run a stale binary.
func EnsureBinary() (string, error) {
	if path := os.Getenv("E2E_NODA"); path != "" {
		return path, nil
	}
	dir, err := os.MkdirTemp("", "noda-e2e-bin-")
	if err != nil {
		return "", err
	}
	binary := filepath.Join(dir, "noda")
	build := exec.Command("go", "build", "-mod=vendor", "-o", binary, "./cmd/noda")
	build.Dir = repositoryRoot()
	if output, err := build.CombinedOutput(); err != nil {
		return "", fmt.Errorf("building noda: %w\n%s", err, output)
	}
	return binary, nil
}

func repositoryRoot() string {
	_, file, _, _ := runtime.Caller(0)
	return filepath.Join(filepath.Dir(file), "..", "..", "..")
}

// StartServer seeds a fresh workspace with seed, then serves it on a free port.
func StartServer(binary string, seed func(Workspace) error) (*Server, error) {
	root, err := os.MkdirTemp("", "noda-e2e-")
	if err != nil {
		return nil, err
	}
	server := &Server{Workspace: Workspace{Binary: binary, Root: root}}
	if err := server.start(seed); err != nil {
		server.Stop()
		return nil, err
	}
	return server, nil
}

func (s *Server) start(seed func(Workspace) error) error {
	if err := s.WriteConfig(); err != nil {
		return err
	}
	if err := seed(s.Workspace); err != nil {
		return err
	}
	head, err := git(s.notebookDir(), "rev-parse", "HEAD")
	if err != nil {
		return err
	}
	s.head = strings.TrimSpace(head)

	address, err := freeAddress()
	if err != nil {
		return err
	}
	s.BaseURL = "http://" + address
	return s.spawn(address)
}

func (s *Server) spawn(address string) error {
	s.command = exec.Command(s.Binary, "web", "--listen", address)
	s.command.Env = s.env()
	// Inherited, so a refusal to start shows in the test output.
	s.command.Stdout = os.Stdout
	s.command.Stderr = os.Stderr
	if err := s.command.Start(); err != nil {
		return fmt.Errorf("spawning %s web: %w", s.Binary, err)
	}
	return waitUntilListening(address)
}

// restart replaces the web process on the same address. The server keeps what
// its last errand said until the next one begins, so without a restart the
// next scenario would read the previous scenario's "Synced" as its own.
func (s *Server) restart() error {
	_ = s.command.Process.Kill()
	_ = s.command.Wait()
	return s.spawn(strings.TrimPrefix(s.BaseURL, "http://"))
}

// Stop kills the server and removes its workspace.
func (s *Server) Stop() {
	if s.command != nil && s.command.Process != nil {
		_ = s.command.Process.Kill()
		_ = s.command.Wait()
	}
	_ = os.RemoveAll(s.Root)
}

// Reset restores the fixture notebook and remote; call it after every
// scenario. The suite is wrong without it: scenarios share one notebook and
// some write to it.
func (s *Server) Reset() error {
	notebook := s.notebookDir()
	if _, err := git(notebook, "reset", "--hard", s.head); err != nil {
		return err
	}
	// Untracked files, e.g. a note left by a step that failed part way.
	if _, err := git(notebook, "clean", "-fd"); err != nil {
		return err
	}
	// The remote is fixture too: a sync leaves commits on the bare repository
	// and tracking refs, and "never synced" would then describe the last scenario.
	refs, err := git(notebook, "for-each-ref", "--format=%(refname)", "refs/remotes")
	if err != nil {
		return err
	}
	for _, ref := range strings.Fields(refs) {
		if _, err := git(notebook, "update-ref", "-d", ref); err != nil {
			return err
		}
	}
	if err := s.BareRemote(); err != nil {
		return err
	}
	return s.restart()
}

// BareRemote makes a fresh bare repository standing in for a git host.
func (s *Server) BareRemote() error { return makeBareRemote(s.remoteDir()) }

func makeBareRemote(path string) error {
	if err := os.RemoveAll(path); err != nil {
		return err
	}
	if err := os.MkdirAll(path, 0o755); err != nil {
		return err
	}
	_, err := git("", "init", "--bare", "-q", path)
	return err
}

func git(repo string, args ...string) (string, error) {
	if repo != "" {
		args = append([]string{"-C", repo}, args...)
	}
	command := exec.Command("git", args...)
	command.Env = append(os.Environ(), "GIT_CONFIG_GLOBAL="+os.DevNull, "GIT_CONFIG_NOSYSTEM=1")
	output, err := command.Output()
	if err != nil {
		return "", commandError("git", args, err)
	}
	return string(output), nil
}

func commandError(name string, args []string, err error) error {
	if exitError, ok := err.(*exec.ExitError); ok {
		return fmt.Errorf("%s %v failed: %s", name, args, exitError.Stderr)
	}
	return fmt.Errorf("running %s %v: %w", name, args, err)
}

func freeAddress() (string, error) {
	listener, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		return "", err
	}
	defer listener.Close()
	return listener.Addr().String(), nil
}

func waitUntilListening(address string) error {
	deadline := time.Now().Add(startupTimeout)
	for time.Now().Before(deadline) {
		connection, err := net.DialTimeout("tcp", address, time.Second)
		if err == nil {
			connection.Close()
			return nil
		}
		time.Sleep(50 * time.Millisecond)
	}
	return fmt.Errorf("noda web did not start listening on %s within %v", address, startupTimeout)
}

// SeedFixture writes the five notes the features read, and a remote that is
// set but never synced. One body holds markup and one tag has a space, because
// both come out of a real `noda import tiddlywiki`.
func SeedFixture(w Workspace) error {
	if _, err := w.Run("init"); err != nil {
		return err
	}
	// Two open todos and a done one. The absurd dates keep "overdue"
	// independent of when the suite runs.
	steps := [][]string{
		{"add", "Budget review", "-c", "the q3 budget is late\n\n" +
			"- [ ] chase the marketing line due:2000-01-01\n" +
			"- [ ] send the draft to Ana due:2999-12-31\n" +
			"- [x] pull the ledger export\n", "--tag", "work"},
		{"add", "Meeting notes", "-c", "the budget, again", "--tag", "work", "--tag", "24.04 Dark patterns"},
	}
	for _, step := range steps {
		if _, err := w.Run(step...); err != nil {
			return err
		}
	}
	// The link targets the meeting notes, not the budget: the filename is in
	// the body, so `…-budget-review.md` would make this note match
	// searching.feature's search for "budget".
	notes, err := w.NoteFile("meeting-notes")
	if err != nil {
		return err
	}
	if _, err := w.Run("add", "Reading list", "-c", "a book, and [the notes]("+notes+")"); err != nil {
		return err
	}
	// A file and a note pointing at it, for the files screen's backlink count.
	png := filepath.Join(w.Root, "rack.png")
	if err := os.WriteFile(png, []byte("\x89PNG\r\n\x1a\nnot really"), 0o644); err != nil {
		return err
	}
	if _, err := w.Run("file", "add", png); err != nil {
		return err
	}
	if _, err := w.Run("add", "Markup import", "-c", "a <b>bold</b> here, and ![the rack](rack.png)",
		"--tag", "ops"); err != nil {
		return err
	}
	// A remote set but never synced: a state the listing's chip must show, and
	// the starting point for the sync scenarios.
	if err := makeBareRemote(w.remoteDir()); err != nil {
		return err
	}
	_, err = w.Run("remote", "set", w.remoteDir())
	return err
}
