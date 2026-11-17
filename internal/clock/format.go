// Package clock formats commit times and finds the local time zone.
package clock

import (
	"fmt"
	"math"
)

// TimeWidth is the width of FormatTime's output.
const TimeWidth = len("0000-00-00 00:00")

// DateWidth is the width of a `due:` date, for padding rows that have none.
const DateWidth = len("0000-00-00")

// timePlaceholder is what FormatTime prints for an instant outside years
// 0000-9999.
const timePlaceholder = "---------- --:--"

const (
	secondsPerDay = 86_400
	// 0000-01-01 is day -719,528; 9999-12-31 ends on day 2,932,896.
	minLocal = -719_528 * secondsPerDay
	maxLocal = 2_932_897*secondsPerDay - 1
)

// FormatTime prints an instant in the commit's own zone, as git does. It is
// absolute rather than "3 days ago": it sorts and needs no frozen clock to
// test.
//
// Timestamps come from commits, which a cloned repository does not vouch for.
// The offset is added with saturation (a wrapped sum would be a plausible but
// wrong date; a saturated one is simply out of range), and anything outside
// 0000-01-01..=9999-12-31 renders as a placeholder, so the output is always
// TimeWidth wide and a `[:DateWidth]` slice cannot panic.
func FormatTime(seconds int64, offsetMinutes int) string {
	local := saturatingAdd(seconds, int64(offsetMinutes)*60)
	if local < minLocal || local > maxLocal {
		return timePlaceholder
	}
	year, month, day := civilFromDays(floorDiv(local, secondsPerDay))
	timeOfDay := floorMod(local, secondsPerDay)
	return fmt.Sprintf("%04d-%02d-%02d %02d:%02d",
		year, month, day, timeOfDay/3600, (timeOfDay%3600)/60)
}

func saturatingAdd(a, b int64) int64 {
	switch {
	case b > 0 && a > math.MaxInt64-b:
		return math.MaxInt64
	case b < 0 && a < math.MinInt64-b:
		return math.MinInt64
	}
	return a + b
}

func floorDiv(a, b int64) int64 {
	q := a / b
	if a%b != 0 && (a < 0) != (b < 0) {
		q--
	}
	return q
}

func floorMod(a, b int64) int64 {
	return a - floorDiv(a, b)*b
}

// civilFromDays is Howard Hinnant's algorithm: a few lines beat a date
// dependency, and the standard library's time package would clamp the years the
// placeholder handles.
func civilFromDays(days int64) (year int64, month, day int64) {
	// The epoch is at 0000-03-01: the leap day lands at the year's end and
	// every era is exactly 146,097 days.
	shifted := days + 719_468
	era := floorDiv(shifted, 146_097)
	dayOfEra := floorMod(shifted, 146_097)
	yearOfEra := (dayOfEra - dayOfEra/1_460 + dayOfEra/36_524 - dayOfEra/146_096) / 365
	dayOfYear := dayOfEra - (365*yearOfEra + yearOfEra/4 - yearOfEra/100)
	monthPosition := (5*dayOfYear + 2) / 153
	day = dayOfYear - (153*monthPosition+2)/5 + 1
	if monthPosition < 10 {
		month = monthPosition + 3
	} else {
		month = monthPosition - 9
	}
	year = yearOfEra + era*400
	if month <= 2 {
		year++
	}
	return year, month, day
}
