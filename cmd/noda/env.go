package main

import (
	"fmt"
	"strings"
)

// The variables noda reads. Everything else in the environment is for git.
const (
	envLogLevel  = "NODA_LOG_LEVEL"
	envLogFormat = "NODA_LOG_FORMAT"
)

// liveEnvVars lists the noda-specific variables that are read. A retired
// variable's replacement must be one of them.
var liveEnvVars = []string{envLogLevel, envLogFormat}

// legacyEnvVar pairs a retired environment variable with its replacement.
type legacyEnvVar struct{ old, replacement string }

// legacyEnvVars lists the retired variables. Update it whenever an environment
// variable is added, renamed or removed: a silently ignored one is worse than
// a startup failure.
var legacyEnvVars = []legacyEnvVar{
	// The Rust build filtered its web log with RUST_LOG; a global one would
	// otherwise be ignored without a word.
	{"RUST_LOG", envLogLevel},
}

// ensureNoLegacyEnvVars fails when a retired variable is set, even empty.
func ensureNoLegacyEnvVars(lookupEnv func(string) (string, bool)) error {
	var found []string
	for _, legacy := range legacyEnvVars {
		if _, ok := lookupEnv(legacy.old); ok {
			found = append(found, fmt.Sprintf("  %s -> %s", legacy.old, legacy.replacement))
		}
	}
	if len(found) == 0 {
		return nil
	}
	return fmt.Errorf("these environment variables no longer exist; "+
		"rename (or unset) them to continue:\n%s", strings.Join(found, "\n"))
}
