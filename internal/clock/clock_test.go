package clock

import (
	"encoding/json"
	"math"
	"os"
	"testing"
	"time"
)

func TestFormatTimeMatchesTheRustGolden(t *testing.T) {
	var cases []struct {
		Name    string `json:"name"`
		Seconds int64  `json:"seconds"`
		Offset  int    `json:"offset"`
	}
	var expected []struct {
		Name string `json:"name"`
		Out  string `json:"out"`
	}
	readJSON(t, "../../testdata/golden/clock/cases.json", &cases)
	readJSON(t, "../../testdata/golden/clock/expected.json", &expected)
	if len(cases) != len(expected) || len(cases) < 30 {
		t.Fatalf("%d cases, %d expected", len(cases), len(expected))
	}
	for i, c := range cases {
		if expected[i].Name != c.Name {
			t.Fatalf("case %d is %s, expected %s", i, c.Name, expected[i].Name)
		}
		if got := FormatTime(c.Seconds, c.Offset); got != expected[i].Out {
			t.Errorf("%s (%d, %d): got %q, want %q", c.Name, c.Seconds, c.Offset, got, expected[i].Out)
		}
	}
}

func readJSON(t *testing.T, path string, into any) {
	t.Helper()
	raw, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	if err := json.Unmarshal(raw, into); err != nil {
		t.Fatal(err)
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
