package main

import (
	"errors"
	"io"
	"strings"
	"syscall"

	"github.com/henry40408/noda/internal/style"
)

// printOutput writes a command's answer, adding a trailing newline only when
// needed. Colour is kept only where wanted, so a redirected `noda show` writes
// the file byte for byte.
func printOutput(w io.Writer, text string, color bool) error {
	if text == "" {
		return nil
	}
	// NUL-separated output (`ls -q0`, for `xargs -0`) is machine-made: the
	// escape stripping would eat it and a newline after the last terminator
	// would become part of the next record.
	if strings.Contains(text, "\x00") {
		_, err := io.WriteString(w, text)
		return err
	}
	if !color {
		text = style.Strip(text)
	}
	if !strings.HasSuffix(text, "\n") {
		text += "\n"
	}
	_, err := io.WriteString(w, text)
	return err
}

// isBrokenPipe is `noda ls | head -1`: the reader left, which is not an error.
func isBrokenPipe(err error) bool { return errors.Is(err, syscall.EPIPE) }
