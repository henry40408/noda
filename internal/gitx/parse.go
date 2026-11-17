package gitx

import (
	"bytes"
	"fmt"
	"strconv"
	"strings"
)

// SplitNUL splits git's `-z` output into its records. The terminator after the
// last record gives no empty one.
func SplitNUL(output []byte) []string {
	if len(output) == 0 {
		return nil
	}
	output = bytes.TrimSuffix(output, []byte{0})
	parts := bytes.Split(output, []byte{0})
	records := make([]string, len(parts))
	for i, part := range parts {
		records[i] = string(part)
	}
	return records
}

// ParseRawDate reads `--date=raw`, `<unix seconds> <±hhmm>`, into the seconds
// and the zone's offset in minutes.
func ParseRawDate(text string) (seconds int64, offsetMinutes int, err error) {
	fields := strings.Fields(text)
	if len(fields) != 2 || len(fields[1]) != 5 {
		return 0, 0, fmt.Errorf("unexpected git date %q", text)
	}
	seconds, err = strconv.ParseInt(fields[0], 10, 64)
	if err != nil {
		return 0, 0, fmt.Errorf("unexpected git date %q", text)
	}
	hours, errHours := strconv.Atoi(fields[1][1:3])
	minutes, errMinutes := strconv.Atoi(fields[1][3:])
	if errHours != nil || errMinutes != nil || (fields[1][0] != '+' && fields[1][0] != '-') {
		return 0, 0, fmt.Errorf("unexpected git date %q", text)
	}
	offsetMinutes = hours*60 + minutes
	if fields[1][0] == '-' {
		offsetMinutes = -offsetMinutes
	}
	return seconds, offsetMinutes, nil
}

// CountStatus is the number of paths in `git status --porcelain=v2 -z`. A
// rename record carries its old path as a record of its own, which is not
// another path.
func CountStatus(output []byte) int {
	count := 0
	records := SplitNUL(output)
	for i := 0; i < len(records); i++ {
		record := records[i]
		if record == "" {
			continue
		}
		count++
		if strings.HasPrefix(record, "2 ") {
			i++
		}
	}
	return count
}

// ParseLeftRight reads `rev-list --left-right --count`: `<left>\t<right>`.
func ParseLeftRight(output []byte) (left, right int, err error) {
	fields := strings.Fields(string(output))
	if len(fields) != 2 {
		return 0, 0, fmt.Errorf("unexpected git rev-list count %q", strings.TrimSpace(string(output)))
	}
	left, errLeft := strconv.Atoi(fields[0])
	right, errRight := strconv.Atoi(fields[1])
	if errLeft != nil || errRight != nil {
		return 0, 0, fmt.Errorf("unexpected git rev-list count %q", strings.TrimSpace(string(output)))
	}
	return left, right, nil
}
