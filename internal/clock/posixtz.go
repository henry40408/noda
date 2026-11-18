package clock

import (
	"encoding/binary"
	"regexp"
	"strconv"
	"strings"
	"time"
)

// The POSIX TZ grammar: `STDoffset[DST[offset][,start[/time],end[/time]]]`.
// The standard library can evaluate such a rule (it does for the footer of a
// zoneinfo file) but offers no way to parse one from a string, so
// locationFromPOSIX validates the string here and hands it over inside a
// minimal zoneinfo file.
var posixTZ = regexp.MustCompile(
	`^(<[A-Za-z0-9+-]{3,}>|[A-Za-z]{3,})([+-]?\d{1,2}(?::\d{2}(?::\d{2})?)?)` +
		`(?:(?:<[A-Za-z0-9+-]{3,}>|[A-Za-z]{3,})(?:[+-]?\d{1,2}(?::\d{2}(?::\d{2})?)?)?` +
		`(?:,` + ruleDate + `(?:/` + ruleTime + `)?,` + ruleDate + `(?:/` + ruleTime + `)?)?)?$`)

const (
	ruleDate = `(?:M\d{1,2}\.[1-5]\.[0-6]|J\d{1,3}|\d{1,3})`
	ruleTime = `[+-]?\d{1,3}(?::\d{2}(?::\d{2})?)?`
	// usRule is what a DST zone without a rule uses, as the C library does.
	usRule = ",M3.2.0,M11.1.0"
)

// locationFromPOSIX turns a POSIX TZ string such as `CST-8` or
// `EST5EDT,M3.2.0,M11.1.0` into a location. It reports false when the string is
// not valid POSIX.
func locationFromPOSIX(tz string) (*time.Location, bool) {
	match := posixTZ.FindStringSubmatch(tz)
	if match == nil {
		return nil, false
	}
	offset, ok := posixOffsetSeconds(match[2])
	if !ok {
		return nil, false
	}
	rule := tz
	if hasDST(tz, match) && !strings.Contains(tz, ",") {
		rule += usRule
	}
	name := strings.Trim(match[1], "<>")
	location, err := time.LoadLocationFromTZData(name, zoneinfo(name, -offset, rule))
	if err != nil {
		return nil, false
	}
	return location, true
}

// hasDST reports whether anything follows the standard offset.
func hasDST(tz string, match []string) bool {
	return len(tz) > len(match[1])+len(match[2])
}

// posixOffsetSeconds reads `[+-]hh[:mm[:ss]]`. The hour limit is the C
// library's (24), not the grammar's two digits.
func posixOffsetSeconds(text string) (int, bool) {
	sign := 1
	if strings.HasPrefix(text, "-") {
		sign = -1
	}
	text = strings.TrimLeft(text, "+-")
	seconds := 0
	unit := 3600
	for i, part := range strings.Split(text, ":") {
		n, err := strconv.Atoi(part)
		if err != nil || (i > 0 && n > 59) || (i == 0 && n > 24) {
			return 0, false
		}
		seconds += n * unit
		unit /= 60
	}
	return sign * seconds, true
}

// zoneinfo builds a version 2 zoneinfo file whose one transition lies far in
// the past, so every time of interest is decided by the footer rule.
func zoneinfo(name string, utcOffset int, footer string) []byte {
	var veryEarly int64 = -1 << 59
	abbreviation := append([]byte(name), 0)

	var out []byte
	header := func(version byte, transitions, types, chars uint32) {
		out = append(out, "TZif"...)
		out = append(out, version)
		out = append(out, make([]byte, 15)...)
		for _, count := range []uint32{0, 0, 0, transitions, types, chars} {
			out = binary.BigEndian.AppendUint32(out, count)
		}
	}
	zoneType := func() {
		out = binary.BigEndian.AppendUint32(out, uint32(int32(utcOffset)))
		out = append(out, 0, 0)
	}

	// The version 1 block is skipped by readers but must be well formed.
	header('2', 0, 1, uint32(len(abbreviation)))
	zoneType()
	out = append(out, abbreviation...)

	header('2', 1, 1, uint32(len(abbreviation)))
	out = binary.BigEndian.AppendUint64(out, uint64(veryEarly))
	out = append(out, 0)
	zoneType()
	out = append(out, abbreviation...)
	return append(out, "\n"+footer+"\n"...)
}
