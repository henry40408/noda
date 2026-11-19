package main

import (
	"bufio"
	"io"
	"net"
	"net/http"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"sync"
	"syscall"
	"testing"
	"time"
)

var (
	builtOnce sync.Once
	builtPath string
	builtErr  error
)

// coverDir is where a `-cover` binary writes its profile; empty means the
// binary is built plain. `go test -coverprofile` does not see an exec'd binary.
var coverDir = os.Getenv("NODA_COVERDIR")

func buildArguments(output string) []string {
	arguments := []string{"build", "-mod=vendor"}
	if coverDir != "" {
		arguments = append(arguments, "-cover", "-coverpkg=github.com/henry40408/noda/...")
	}
	return append(arguments, "-o", output, ".")
}

// withCoverage tells a `-cover` binary where to write; without GOCOVERDIR it
// would warn on stderr, which the tests read.
func withCoverage(env []string) []string {
	if coverDir == "" {
		return env
	}
	return append(env, "GOCOVERDIR="+coverDir)
}

// theBinary is built once: these tests are about a real process and its
// signals, which a call into run cannot show.
func theBinary(t *testing.T) string {
	t.Helper()
	builtOnce.Do(func() {
		dir, err := os.MkdirTemp("", "noda-bin-")
		if err != nil {
			builtErr = err
			return
		}
		if coverDir != "" {
			if err := os.MkdirAll(coverDir, 0o755); err != nil {
				builtErr = err
				return
			}
		}
		builtPath = filepath.Join(dir, "noda")
		build := exec.Command("go", buildArguments(builtPath)...)
		if out, err := build.CombinedOutput(); err != nil {
			builtErr = &buildFailure{err: err, out: string(out)}
		}
	})
	if builtErr != nil {
		t.Fatal(builtErr)
	}
	return builtPath
}

type buildFailure struct {
	err error
	out string
}

func (b *buildFailure) Error() string { return b.err.Error() + "\n" + b.out }

// serverProcess is `noda web` on a free port, up when it has said where.
type serverProcess struct {
	command *exec.Cmd
	address string
	lines   chan string
}

func startServer(t *testing.T, args ...string) *serverProcess {
	t.Helper()
	box := newSandbox(t)
	box.ok(t, "init")
	command := exec.Command(theBinary(t), append([]string{"web", "--listen", "127.0.0.1:0"}, args...)...)
	for name, value := range box.env {
		command.Env = append(command.Env, name+"="+value)
	}
	command.Env = withCoverage(command.Env)
	stdout, err := command.StdoutPipe()
	if err != nil {
		t.Fatal(err)
	}
	if err := command.Start(); err != nil {
		t.Fatal(err)
	}
	process := &serverProcess{command: command, lines: make(chan string, 32)}
	t.Cleanup(func() { _ = command.Process.Kill(); _ = command.Wait() })
	go func() {
		scanner := bufio.NewScanner(stdout)
		for scanner.Scan() {
			process.lines <- scanner.Text()
		}
		close(process.lines)
	}()
	first := process.next(t)
	const prefix = "noda is at http://"
	if !strings.HasPrefix(first, prefix) {
		t.Fatalf("the first line was %q", first)
	}
	process.address = strings.TrimPrefix(first, prefix)
	return process
}

func (p *serverProcess) next(t *testing.T) string {
	t.Helper()
	select {
	case line := <-p.lines:
		return line
	case <-time.After(20 * time.Second):
		t.Fatal("the server said nothing")
		return ""
	}
}

func (p *serverProcess) stop(t *testing.T, signal syscall.Signal) (string, error) {
	t.Helper()
	if err := p.command.Process.Signal(signal); err != nil {
		t.Fatal(err)
	}
	var said strings.Builder
	finished := make(chan error, 1)
	go func() {
		for line := range p.lines {
			said.WriteString(line + "\n")
		}
		finished <- p.command.Wait()
	}()
	select {
	case err := <-finished:
		return said.String(), err
	case <-time.After(20 * time.Second):
		t.Fatal("it did not stop")
		return "", nil
	}
}

func (p *serverProcess) healthy(t *testing.T) {
	t.Helper()
	response, err := http.Get("http://" + p.address + "/health")
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = response.Body.Close() }()
	if body, _ := io.ReadAll(response.Body); response.StatusCode != 200 || string(body) != "ok\n" {
		t.Fatalf("%d %q", response.StatusCode, body)
	}
}

func TestASupervisorCanStopItAndItNamesTheSignal(t *testing.T) {
	for signal, name := range map[syscall.Signal]string{syscall.SIGTERM: "SIGTERM", syscall.SIGINT: "SIGINT"} {
		server := startServer(t)
		server.healthy(t)
		said, err := server.stop(t, signal)
		if err != nil || !strings.Contains(said, name) {
			t.Errorf("%s: %v %q", name, err, said)
		}
		if _, err := net.DialTimeout("tcp", server.address, time.Second); err == nil {
			t.Errorf("%s: still accepting connections", name)
		}
	}
}

func TestAnIdleConnectionDoesNotHoldItOpen(t *testing.T) {
	server := startServer(t)
	kept, err := net.Dial("tcp", server.address)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = kept.Close() }()
	if _, err := kept.Write([]byte("GET /health HTTP/1.1\r\nHost: " + server.address + "\r\n\r\n")); err != nil {
		t.Fatal(err)
	}
	if line, _ := bufio.NewReader(kept).ReadString('\n'); !strings.Contains(line, "200") {
		t.Fatalf("%q", line)
	}
	silent, err := net.Dial("tcp", server.address)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = silent.Close() }()
	if _, err := server.stop(t, syscall.SIGTERM); err != nil {
		t.Fatal(err)
	}
}

func TestAnAddressThatCannotBeBoundIsNamedAndFails(t *testing.T) {
	taken, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = taken.Close() }()
	box := newSandbox(t)
	box.ok(t, "init")
	got := box.run("web", "--listen", taken.Addr().String())
	if got.code == 0 || !strings.Contains(got.stderr, "could not listen on "+taken.Addr().String()) {
		t.Fatalf("%d %q", got.code, got.stderr)
	}
}

func TestAWideAddressWarnsThatThereIsNoPassword(t *testing.T) {
	box := newSandbox(t)
	box.ok(t, "init")
	command := exec.Command(theBinary(t), "web", "--listen", "0.0.0.0:0")
	for name, value := range box.env {
		command.Env = append(command.Env, name+"="+value)
	}
	command.Env = withCoverage(command.Env)
	stdout, err := command.StdoutPipe()
	if err != nil {
		t.Fatal(err)
	}
	if err := command.Start(); err != nil {
		t.Fatal(err)
	}
	defer func() { _ = command.Process.Kill(); _ = command.Wait() }()
	reader := bufio.NewReader(stdout)
	first, _ := reader.ReadString('\n')
	second, _ := reader.ReadString('\n')
	if !strings.HasPrefix(first, "noda is at http://") || !strings.Contains(second, "there is no password on it") {
		t.Fatalf("%q %q", first, second)
	}
}
