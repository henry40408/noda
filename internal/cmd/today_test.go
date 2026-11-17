package cmd

import (
	"testing"
	"time"

	"github.com/henry40408/noda/internal/gitx"
	"github.com/henry40408/noda/internal/gitx/gittest"
)

func envWithTZ(t *testing.T, tz string) Env {
	t.Helper()
	gittest.Require(t)
	env := gittest.Isolate(t)
	env["TZ"] = tz
	git, err := gitx.New(gitx.Options{Env: env})
	if err != nil {
		t.Fatal(err)
	}
	return Env{Git: git}
}

// TestTodayReadsTZFromTheInjectedEnvironment pins that the process's own TZ is
// never consulted: the process is set to a zone that is never the one asked for.
func TestTodayReadsTZFromTheInjectedEnvironment(t *testing.T) {
	t.Setenv("TZ", "Pacific/Pago_Pago")
	for _, zone := range []string{"Pacific/Kiritimati", "UTC", "Pacific/Pago_Pago"} {
		location, err := time.LoadLocation(zone)
		if err != nil {
			t.Fatal(err)
		}
		before := time.Now().In(location).Format("2006-01-02")
		got := Today(envWithTZ(t, zone))
		after := time.Now().In(location).Format("2006-01-02")
		if got != before && got != after {
			t.Fatalf("%s: Today is %s, want %s", zone, got, before)
		}
	}
}
