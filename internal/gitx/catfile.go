package gitx

import (
	"bufio"
	"bytes"
	"context"
	"errors"
	"fmt"
	"io"
	"os/exec"
	"strconv"
	"strings"

	"github.com/henry40408/noda/internal/remote"
)

// Batch is one `git cat-file --batch` process, for reading many objects
// without a process each. It is for the span of a single operation: the caller
// closes it when done, and it is never shared.
type Batch struct {
	command *exec.Cmd
	stdin   io.WriteCloser
	stdout  *bufio.Reader
	stderr  *bytes.Buffer
	// broken is set by the first failed read: the stream's position is then
	// unknown, and every later read would be answered with the wrong object.
	broken error
	// finished and waitErr keep Wait to a single call.
	finished bool
	waitErr  error
}

// finish ends the process once. Wait also completes the copy into stderr, so
// the buffer is safe to read only afterwards.
func (b *Batch) finish() error {
	if b.finished {
		return b.waitErr
	}
	b.finished = true
	_ = b.stdin.Close()
	b.waitErr = b.command.Wait()
	return b.waitErr
}

// fail ends the batch's usefulness and explains why, with what git said.
func (b *Batch) fail(err error) error {
	_ = b.finish()
	reason := reasonLine(remote.RedactText(b.stderr.String()))
	if reason != "" {
		err = fmt.Errorf("%w: %s", err, reason)
	}
	b.broken = err
	return err
}

// OpenBatch starts the process in dir.
func (g *Git) OpenBatch(ctx context.Context, dir string) (*Batch, error) {
	if dir == "" {
		return nil, errors.New("running git without a directory")
	}
	args := append(globalArgs(dir, false), "cat-file", "--batch")
	command := exec.CommandContext(ctx, g.path, args...)
	command.Env = g.environment(dir, true, nil)
	stopGently(command)
	stdin, err := command.StdinPipe()
	if err != nil {
		return nil, err
	}
	stdout, err := command.StdoutPipe()
	if err != nil {
		return nil, err
	}
	stderr := &bytes.Buffer{}
	command.Stderr = stderr
	if err := command.Start(); err != nil {
		return nil, fmt.Errorf("running git cat-file: %w", err)
	}
	return &Batch{command: command, stdin: stdin, stdout: bufio.NewReader(stdout), stderr: stderr}, nil
}

// Read fetches one object by any revision spec (`HEAD:note.md`, an oid). The
// bool is false when git has no such object.
func (b *Batch) Read(spec string) (kind string, data []byte, ok bool, err error) {
	if b.broken != nil {
		return "", nil, false, fmt.Errorf("git cat-file is no longer usable: %w", b.broken)
	}
	if strings.ContainsAny(spec, "\n\r") {
		return "", nil, false, fmt.Errorf("a revision spec cannot hold a line break: %q", spec)
	}
	if _, err := io.WriteString(b.stdin, spec+"\n"); err != nil {
		return "", nil, false, b.fail(fmt.Errorf("writing to git cat-file: %w", err))
	}
	header, err := b.stdout.ReadString('\n')
	if err != nil {
		return "", nil, false, b.fail(fmt.Errorf("reading git cat-file: %w", err))
	}
	header = strings.TrimSpace(header)
	// A spec may hold spaces (`HEAD:my file.md`), so a missing object is told
	// by the end of the line.
	if strings.HasSuffix(header, " missing") || strings.HasSuffix(header, " ambiguous") {
		return "", nil, false, nil
	}
	fields := strings.Fields(header)
	if len(fields) != 3 {
		return "", nil, false, b.fail(fmt.Errorf("unexpected git cat-file header %q", header))
	}
	size, err := strconv.Atoi(fields[2])
	if err != nil {
		return "", nil, false, b.fail(fmt.Errorf("unexpected git cat-file size in %q", header))
	}
	// The object is followed by one newline of its own.
	data = make([]byte, size+1)
	if _, err := io.ReadFull(b.stdout, data); err != nil {
		return "", nil, false, b.fail(fmt.Errorf("reading git cat-file: %w", err))
	}
	return fields[1], data[:size], true, nil
}

// Close ends the process.
func (b *Batch) Close() error {
	err := b.finish()
	var exit *exec.ExitError
	if errors.As(err, &exit) {
		return nil
	}
	return err
}
