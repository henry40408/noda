package clock

import (
	"strings"
	"time"
	// The runtime image may have no zoneinfo (a slim image does not install
	// tzdata), and `TZ=Asia/Taipei` must still work.
	_ "time/tzdata"
)

// Location resolves a TZ value the way the C library does, which is where the
// Rust version got every commit time it printed. set tells an unset TZ (the
// system's zone) from an empty one (UTC).
//
// A value is tried as an IANA name, then as a POSIX string such as `CST-8`.
// One that is neither gives UTC, as the C library does.
func Location(tz string, set bool) *time.Location {
	if !set {
		return time.Local
	}
	tz = strings.TrimPrefix(tz, ":")
	if tz == "" {
		return time.UTC
	}
	if location, err := time.LoadLocation(tz); err == nil {
		return location
	}
	if location, ok := locationFromPOSIX(tz); ok {
		return location
	}
	return time.UTC
}

// LocalNow is the current time in the zone Location resolves.
func LocalNow(tz string, set bool) time.Time {
	return time.Now().In(Location(tz, set))
}
