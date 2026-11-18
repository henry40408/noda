// Package version reports the build version.
package version

import "runtime/debug"

// version is set at link time:
//
//	-ldflags "-X github.com/henry40408/noda/internal/version.version=$GIT_VERSION"
//
// where GIT_VERSION is `git describe --tags --always --match 'v[0-9]*'` with
// the leading "v" removed. Docker builds have no .git directory, so they pass
// it in.
var version string

// shortRevisionLength is how many characters of a commit id a bare revision
// keeps, the same width `git describe --always` uses.
const shortRevisionLength = 7

// String returns the injected version, else the VCS revision Go stamps into the
// binary, else "dev". An injected "dev" counts as unset.
func String() string {
	if version != "" && version != "dev" {
		return version
	}
	return stampedRevision()
}

func stampedRevision() string {
	info, ok := debug.ReadBuildInfo()
	if !ok {
		return "dev"
	}
	for _, setting := range info.Settings {
		if setting.Key == "vcs.revision" && len(setting.Value) >= shortRevisionLength {
			return setting.Value[:shortRevisionLength]
		}
	}
	return "dev"
}
