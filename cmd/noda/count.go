package main

import (
	"errors"
	"math"
	"strconv"
	"strings"
)

// maxCount is the most `log -n` passes on: git takes an int.
const maxCount = math.MaxInt32

// parseCount reads a `-n` value the way Rust parsed a usize: an optional `+`,
// digits, and the reasons for refusing worded as Rust's. 0 is kept as 0.
func parseCount(text string) (int, error) {
	if text == "" {
		return 0, errors.New("cannot parse integer from empty string")
	}
	digits := strings.TrimPrefix(text, "+")
	count, err := strconv.ParseUint(digits, 10, 64)
	if errors.Is(err, strconv.ErrRange) {
		return 0, errors.New("number too large to fit in target type")
	}
	if err != nil {
		return 0, errors.New("invalid digit found in string")
	}
	return int(min(count, maxCount)), nil
}
