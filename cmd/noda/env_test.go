package main

import (
	"log/slog"
	"slices"
	"strings"
	"testing"
)

func lookupIn(env map[string]string) func(string) (string, bool) {
	return func(name string) (string, bool) {
		value, ok := env[name]
		return value, ok
	}
}

func TestLegacyEnvVarAbortsStartup(t *testing.T) {
	for _, value := range []string{"", "debug"} {
		err := ensureNoLegacyEnvVars(lookupIn(map[string]string{"RUST_LOG": value}))
		want := "these environment variables no longer exist; rename (or unset) them to continue:\n  RUST_LOG -> NODA_LOG_LEVEL"
		if err == nil || err.Error() != want {
			t.Fatalf("RUST_LOG=%q: %v", value, err)
		}
	}
	if err := ensureNoLegacyEnvVars(lookupIn(map[string]string{"NODA_LOG_LEVEL": "debug"})); err != nil {
		t.Fatalf("a live variable is fine: %v", err)
	}
}

func TestLegacyEnvVarsAreReportedInOrder(t *testing.T) {
	saved := legacyEnvVars
	t.Cleanup(func() { legacyEnvVars = saved })
	legacyEnvVars = []legacyEnvVar{{"B_OLD", "B_NEW"}, {"A_OLD", "A_NEW"}, {"C_OLD", "C_NEW"}}

	err := ensureNoLegacyEnvVars(lookupIn(map[string]string{"A_OLD": "1", "B_OLD": "1"}))
	if err == nil || !strings.HasSuffix(err.Error(), ":\n  B_OLD -> B_NEW\n  A_OLD -> A_NEW") {
		t.Fatalf("got %v", err)
	}
}

func TestEveryLegacyVariableNamesALiveReplacement(t *testing.T) {
	for _, legacy := range legacyEnvVars {
		if !slices.Contains(liveEnvVars, legacy.replacement) {
			t.Errorf("%s points at %s, which noda does not read", legacy.old, legacy.replacement)
		}
		if slices.Contains(liveEnvVars, legacy.old) {
			t.Errorf("%s is both retired and read", legacy.old)
		}
	}
}

func TestParseLogLevel(t *testing.T) {
	cases := map[string]slog.Level{
		"": slog.LevelInfo, "info": slog.LevelInfo, "debug": slog.LevelDebug,
		"warn": slog.LevelWarn, "error": slog.LevelError,
	}
	for text, want := range cases {
		if got, err := parseLogLevel(text); err != nil || got != want {
			t.Errorf("%q: %v, %v", text, got, err)
		}
	}
	for _, bad := range []string{"INFO", "trace", "warning", "noda=debug"} {
		_, err := parseLogLevel(bad)
		if err == nil || err.Error() != `unknown log level "`+bad+`" (want debug, info, warn or error)` {
			t.Errorf("%q: %v", bad, err)
		}
	}
}
