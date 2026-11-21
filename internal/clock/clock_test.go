package clock

import (
	"math"
	"testing"
	"time"
)

func TestFormatTimeTable(t *testing.T) {
	cases := []struct {
		name    string
		seconds int64
		offset  int
		want    string
	}{
		{"epoch", 0, 0, "1970-01-01 00:00"},
		{"seconds are truncated, not rounded", 59, 0, "1970-01-01 00:00"},
		{"a full minute ticks over", 60, 0, "1970-01-01 00:01"},
		{"one second before the epoch is the previous minute", -1, 0, "1969-12-31 23:59"},
		{"a whole day before the epoch", -86400, 0, "1969-12-31 00:00"},
		{"a negative second inside a day floors to that day", -86399, 0, "1969-12-31 00:00"},
		{"last second of the first day", 86399, 0, "1970-01-01 23:59"},
		{"first second of the second day", 86400, 0, "1970-01-02 00:00"},
		{"2000 is a leap year (divisible by 400)", 951782400, 0, "2000-02-29 00:00"},
		{"day after the 2000 leap day", 951868800, 0, "2000-03-01 00:00"},
		{"2100 is not a leap year", 4107542400, 0, "2100-03-01 00:00"},
		{"1900 is not a leap year", -2203891200, 0, "1900-03-01 00:00"},
		{"2024 leap day starts", 1709164800, 0, "2024-02-29 00:00"},
		{"2024 leap day ends", 1709251199, 0, "2024-02-29 23:59"},
		{"new year starts the next year", 1735689600, 0, "2025-01-01 00:00"},
		{"a day before new year is still the old year", 1735603200, 0, "2024-12-31 00:00"},
		{"a typical time after the epoch", 1234567890, 0, "2009-02-13 23:31"},
		{"a time well past 2030", 2000000000, 0, "2033-05-18 03:33"},
		{"a time before the epoch", -1000000000, 0, "1938-04-24 22:13"},
		{"positive offset moves forward", 1700000000, 480, "2023-11-15 06:13"},
		{"negative offset moves back", 1700000000, -480, "2023-11-14 14:13"},
		{"half-hour offset", 1700000000, 330, "2023-11-15 03:43"},
		{"negative half-hour offset", 1700000000, -570, "2023-11-14 12:43"},
		{"offset not a multiple of 15 minutes", 1700000000, 765, "2023-11-15 10:58"},
		{"largest offset carries into the next day", 1700000000, 1439, "2023-11-15 22:12"},
		{"smallest offset reaches two days back", 1700000000, -1439, "2023-11-13 22:14"},
		{"a one hour offset crosses midnight backwards", -3600, 60, "1970-01-01 00:00"},
		{"year zero starts at its first second", -62167219200, 0, "0000-01-01 00:00"},
		{"one second before year zero is out of range", -62167219201, 0, "---------- --:--"},
		{"an offset can bring year zero back in range", -62167219201, 1, "0000-01-01 00:00"},
		{"an offset can push year zero out of range", -62167219200, -1, "---------- --:--"},
		{"last second of year 9999", 253402300799, 0, "9999-12-31 23:59"},
		{"first second after year 9999 is out of range", 253402300800, 0, "---------- --:--"},
		{"an offset can bring year 9999 back in range", 253402300800, -1, "9999-12-31 23:59"},
		{"an offset can push year 9999 out of range", 253402300799, 1, "---------- --:--"},
		{"max int64 saturates to out of range", math.MaxInt64, 0, "---------- --:--"},
		{"min int64 saturates to out of range", math.MinInt64, 0, "---------- --:--"},
		{"max int64 plus an offset does not wrap", math.MaxInt64, 1439, "---------- --:--"},
		{"min int64 minus an offset does not wrap", math.MinInt64, -1439, "---------- --:--"},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			if got := FormatTime(c.seconds, c.offset); got != c.want {
				t.Errorf("FormatTime(%d, %d) = %q, want %q", c.seconds, c.offset, got, c.want)
			}
		})
	}
}

func TestTheOutputIsAlwaysAsWideAsTimeWidth(t *testing.T) {
	for _, seconds := range []int64{math.MinInt64, math.MaxInt64, -1 << 62, 1 << 62, 0} {
		for _, offset := range []int{-1440, 0, 1440, math.MaxInt32, math.MinInt32} {
			if got := FormatTime(seconds, offset); len(got) != TimeWidth {
				t.Errorf("%d %d: %q", seconds, offset, got)
			}
		}
	}
}

func TestTheRangeEndsAtYearZeroAndYear9999(t *testing.T) {
	cases := []struct {
		seconds int64
		want    string
	}{
		{-719_528 * 86_400, "0000-01-01 00:00"},
		{-719_528*86_400 - 1, "---------- --:--"},
		{2_932_897*86_400 - 1, "9999-12-31 23:59"},
		{2_932_897 * 86_400, "---------- --:--"},
	}
	for _, c := range cases {
		if got := FormatTime(c.seconds, 0); got != c.want {
			t.Errorf("%d: got %q, want %q", c.seconds, got, c.want)
		}
	}
}

func TestTheOffsetMovesTheLocalDate(t *testing.T) {
	if got := FormatTime(0, 8*60); got != "1970-01-01 08:00" {
		t.Errorf("got %q", got)
	}
	if got := FormatTime(0, -5*60-30); got != "1969-12-31 18:30" {
		t.Errorf("got %q", got)
	}
}

func TestEveryDayLandsOnARealCalendarDate(t *testing.T) {
	start := time.Date(1900, 1, 1, 0, 0, 0, 0, time.UTC)
	for day := range 366 * 400 {
		date := start.AddDate(0, 0, day)
		want := date.Format("2006-01-02") + " 00:00"
		if got := FormatTime(date.Unix(), 0); got != want {
			t.Fatalf("%s: got %q", want, got)
		}
	}
}

func offsetAt(t *testing.T, location *time.Location, when time.Time) (string, int) {
	t.Helper()
	name, offset := when.In(location).Zone()
	return name, offset
}

func TestLocationReadsIANANamesAndPOSIXStrings(t *testing.T) {
	winter := time.Date(2025, 1, 15, 12, 0, 0, 0, time.UTC)
	summer := time.Date(2025, 7, 15, 12, 0, 0, 0, time.UTC)
	cases := []struct {
		tz           string
		winterOffset int
		summerOffset int
	}{
		{"Asia/Taipei", 8 * 3600, 8 * 3600},
		{":Asia/Taipei", 8 * 3600, 8 * 3600},
		{"CST-8", 8 * 3600, 8 * 3600},
		{"UTC0", 0, 0},
		{"<+0530>-5:30", 5*3600 + 1800, 5*3600 + 1800},
		{"EST5EDT", -5 * 3600, -4 * 3600},
		{"EST5EDT,M3.2.0,M11.1.0", -5 * 3600, -4 * 3600},
		{"CET-1CEST,M3.5.0,M10.5.0/3", 3600, 7200},
		{"AEST-10AEDT,M10.1.0,M4.1.0/3", 11 * 3600, 10 * 3600},
		{"PST8PDT7", -8 * 3600, -7 * 3600},
		{"", 0, 0},
		{"garbage!", 0, 0},
		{"CST", 0, 0},
		{"CST-25", 0, 0},
	}
	for _, c := range cases {
		location := Location(c.tz, true)
		if _, got := offsetAt(t, location, winter); got != c.winterOffset {
			t.Errorf("%q winter: got %d, want %d", c.tz, got, c.winterOffset)
		}
		if _, got := offsetAt(t, location, summer); got != c.summerOffset {
			t.Errorf("%q summer: got %d, want %d", c.tz, got, c.summerOffset)
		}
	}
}

func TestPOSIXZonesKeepTheirAbbreviations(t *testing.T) {
	summer := time.Date(2025, 7, 15, 12, 0, 0, 0, time.UTC)
	if name, _ := offsetAt(t, Location("CET-1CEST,M3.5.0,M10.5.0/3", true), summer); name != "CEST" {
		t.Errorf("got %q", name)
	}
	if name, _ := offsetAt(t, Location("<+0530>-5:30", true), summer); name != "+0530" {
		t.Errorf("got %q", name)
	}
}

func TestAnUnsetTZIsTheSystemZone(t *testing.T) {
	if Location("ignored", false) != time.Local {
		t.Fatal("an unset TZ must use the system zone")
	}
}

func TestLocalNowIsNow(t *testing.T) {
	before := time.Now()
	got := LocalNow("CST-8", true)
	if got.Before(before) || got.Sub(before) > time.Minute {
		t.Fatalf("got %v", got)
	}
	if _, offset := got.Zone(); offset != 8*3600 {
		t.Fatalf("offset %d", offset)
	}
}

// Before the one transition, which lies far in the past, the standard offset
// applies: a mistake in reading it shows only there, because the footer rule
// decides every ordinary time.
func TestBeforeTheFirstTransitionTheStandardOffsetApplies(t *testing.T) {
	beginning := time.Unix(-1<<60, 0)
	cases := []struct {
		tz   string
		want int
	}{
		{"CST-8", 8 * 3600},
		{"EST5", -5 * 3600},
		{"<+0530>-5:30", 5*3600 + 1800},
		{"XXX-8:30:15", 8*3600 + 30*60 + 15},
		{"XXX-24", 24 * 3600},
		{"XXX-0:59", 59 * 60},
	}
	for _, c := range cases {
		if _, got := offsetAt(t, Location(c.tz, true), beginning); got != c.want {
			t.Errorf("%q: got %d, want %d", c.tz, got, c.want)
		}
	}
}

func TestAnOffsetPartPastItsLimitIsNotPOSIX(t *testing.T) {
	now := time.Date(2025, 7, 15, 12, 0, 0, 0, time.UTC)
	for _, tz := range []string{"XXX-0:60", "XXX-8:60", "XXX-25"} {
		if _, got := offsetAt(t, Location(tz, true), now); got != 0 {
			t.Errorf("%q: got %d, want the UTC fallback", tz, got)
		}
	}
}
