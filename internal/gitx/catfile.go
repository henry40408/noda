package gitx

import (
	"bufio"
	"context"
	"errors"
	"fmt"
	"io"
	"os/exec"
	"strconv"
	"strings"
)

// Batch is one `git cat-file --batch` process, for reading many objects
// without a process each. It is for the span of a single operation: the caller
// closes it when done, and it is never shared.
type Batch struct {
	command *exec.Cmd
	stdin   io.WriteCloser
	stdout  *bufio.Reader
}

// OpenBatch starts the process in dir.
func (g *Git) OpenBatch(ctx context.Context, dir string) (*Batch, error) {
	args := append(globalArgs(dir), "cat-file", "--batch")
	command := exec.CommandContext(ctx, g.path, args...)
	command.Env = sanitizedEnv(g.env, true, nil)
	stdin, err := command.StdinPipe()
	if err != nil {
		return nil, err
	}
	stdout, err := command.StdoutPipe()
	if err != nil {
		return nil, err
	}
	if err := command.Start(); err != nil {
		return nil, fmt.Errorf("running git cat-file: %w", err)
	}
	return &Batch{command: command, stdin: stdin, stdout: bufio.NewReader(stdout)}, nil
}

// Read fetches one object by any revision spec (`HEAD:note.md`, an oid). The
// bool is false when git has no such object.
func (b *Batch) Read(spec string) (kind string, data []byte, ok bool, err error) {
	if strings.ContainsAny(spec, "\n\r") {
		return "", nil, false, fmt.Errorf("a revision spec cannot hold a line break: %q", spec)
	}
	if _, err := io.WriteString(b.stdin, spec+"\n"); err != nil {
		return "", nil, false, err
	}
	header, err := b.stdout.ReadString('\n')
	if err != nil {
		return "", nil, false, fmt.Errorf("reading git cat-file: %w", err)
	}
	fields := strings.Fields(header)
	if len(fields) == 2 && (fields[1] == "missing" || fields[1] == "ambiguous") {
		return "", nil, false, nil
	}
	if len(fields) != 3 {
		return "", nil, false, fmt.Errorf("unexpected git cat-file header %q", strings.TrimSpace(header))
	}
	size, err := strconv.Atoi(fields[2])
	if err != nil {
		return "", nil, false, fmt.Errorf("unexpected git cat-file size in %q", strings.TrimSpace(header))
	}
	// The object is followed by one newline of its own.
	data = make([]byte, size+1)
	if _, err := io.ReadFull(b.stdout, data); err != nil {
		return "", nil, false, fmt.Errorf("reading git cat-file: %w", err)
	}
	return fields[1], data[:size], true, nil
}

// Close ends the process.
func (b *Batch) Close() error {
	_ = b.stdin.Close()
	err := b.command.Wait()
	var exit *exec.ExitError
	if errors.As(err, &exit) {
		return nil
	}
	return err
}
