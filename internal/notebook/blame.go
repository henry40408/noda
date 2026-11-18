package notebook

import (
	"fmt"
	"os"
	"strconv"
	"strings"

	"github.com/henry40408/noda/internal/gitx"
	"github.com/henry40408/noda/internal/note"
)

// BlameLine is one line of a note's body and the commit that put it there.
type BlameLine struct {
	// Commit is empty for a line that is on disk but not committed.
	Commit string
	// Seconds and OffsetMinutes are both zero for an uncommitted line.
	Seconds       int64
	OffsetMinutes int
	Text          string
}

// ShortCommit is abbreviated, or git's `0000000` for an uncommitted line.
func (b BlameLine) ShortCommit() string { return Short(b.Commit) }

// commitTime is a commit's committer time, from `git blame --porcelain`.
type commitTime struct {
	seconds int64
	offset  int
}

// Blame is which commit put each line of a note's body where it is.
//
// Body only: `updated` changes on every edit, so the frontmatter is noise. The
// working file is blamed, so a line edited and not yet committed is nobody's.
// The file is named as HEAD knows it, so a retitle that is not committed yet
// still has its history.
func (n *Notebook) Blame(id, slug string) ([]BlameLine, error) {
	raw, err := os.ReadFile(n.NotePath(id, slug))
	if err != nil {
		return nil, err
	}
	text := string(raw)
	lines := splitLines(text)
	owners := make([]string, len(lines))
	times := map[string]commitTime{}

	entry, tracked, err := n.noteEntry("HEAD", id)
	if err != nil {
		return nil, err
	}
	if tracked {
		if owners, times, err = n.blameLines(entry.name, n.NotePath(id, slug), len(lines)); err != nil {
			return nil, err
		}
	}

	var blamed []BlameLine
	for i := bodyStart(text); i < len(lines); i++ {
		line := BlameLine{Commit: owners[i], Text: lines[i]}
		if when, ok := times[owners[i]]; ok {
			line.Seconds, line.OffsetMinutes = when.seconds, when.offset
		}
		blamed = append(blamed, line)
	}
	return blamed, nil
}

// blameLines runs `git blame --porcelain` on the working file under the name
// HEAD holds it by. It returns each line's commit ("" for one not committed)
// and each commit's time.
//
// No revision is named: git before 2.41 refuses `--contents` together with one,
// and without one it blames the working file as a commit on top of HEAD.
func (n *Notebook) blameLines(headName, workFile string, count int) ([]string, map[string]commitTime, error) {
	result, err := n.run(gitx.Cmd{
		Args:     []string{"blame", "--porcelain", "--no-textconv", "--contents", workFile, "--", headName},
		ReadOnly: true,
	})
	if err != nil {
		return nil, nil, err
	}
	return parseBlame(string(result.Stdout), count)
}

// parseBlame reads porcelain output. Each line of the file has a header
// `<oid> <original line> <final line> [<group size>]`, then, the first time an
// oid appears, its metadata, then the line itself after a tab.
func parseBlame(output string, count int) ([]string, map[string]commitTime, error) {
	owners := make([]string, count)
	times := map[string]commitTime{}
	var current string
	var pending commitTime
	for _, line := range strings.Split(output, "\n") {
		switch {
		case strings.HasPrefix(line, "\t"):
		case isBlameHeader(line):
			fields := strings.Fields(line)
			final, err := strconv.Atoi(fields[2])
			if err != nil || final < 1 || final > count {
				return nil, nil, fmt.Errorf("unexpected git blame header %q", line)
			}
			current = fields[0]
			if isZeroOID(current) {
				current = ""
			}
			owners[final-1] = current
		case strings.HasPrefix(line, "committer-time "):
			seconds, err := strconv.ParseInt(strings.TrimPrefix(line, "committer-time "), 10, 64)
			if err != nil {
				return nil, nil, fmt.Errorf("unexpected git blame line %q", line)
			}
			pending = commitTime{seconds: seconds}
		case strings.HasPrefix(line, "committer-tz "):
			_, offset, err := gitx.ParseRawDate("0 " + strings.TrimPrefix(line, "committer-tz "))
			if err != nil {
				return nil, nil, err
			}
			pending.offset = offset
			if current != "" {
				times[current] = pending
			}
		}
	}
	return owners, times, nil
}

func isBlameHeader(line string) bool {
	fields := strings.Fields(line)
	return len(fields) >= 3 && len(fields[0]) >= 40 && isHex(fields[0])
}

func isHex(text string) bool {
	for _, r := range text {
		if !strings.ContainsRune("0123456789abcdef", r) {
			return false
		}
	}
	return text != ""
}

func isZeroOID(oid string) bool { return strings.Trim(oid, "0") == "" }

// splitLines is Rust's `str::lines`: the last line needs no newline, and a
// `\r` before the `\n` is not part of the line.
func splitLines(text string) []string {
	if text == "" {
		return nil
	}
	lines := strings.Split(text, "\n")
	if lines[len(lines)-1] == "" {
		lines = lines[:len(lines)-1]
	}
	for i, line := range lines {
		lines[i] = strings.TrimSuffix(line, "\r")
	}
	return lines
}

// bodyStart is the first line past the frontmatter and the blank line after
// it; zero when there is no frontmatter.
func bodyStart(text string) int {
	_, body, ok := note.SplitFrontmatter(text)
	if !ok {
		return 0
	}
	// As Parse trims.
	body = strings.TrimLeft(body, "\n")
	return len(splitLines(text[:len(text)-len(body)]))
}
