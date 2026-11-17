package version

import "testing"

func TestInjectedVersionWins(t *testing.T) {
	t.Cleanup(func() { version = "" })
	version = "1.2.3"
	if got := String(); got != "1.2.3" {
		t.Fatalf("got %q", got)
	}
}

func TestInjectedDevIsTreatedAsUnset(t *testing.T) {
	t.Cleanup(func() { version = "" })
	version = "dev"
	got := String()
	if got == "" {
		t.Fatal("empty version")
	}
}

// A test binary carries no VCS stamp, so the fallback is "dev".
func TestWithoutAStampTheVersionIsDev(t *testing.T) {
	t.Cleanup(func() { version = "" })
	version = ""
	if got := String(); got != "dev" {
		t.Fatalf("got %q, want dev", got)
	}
}
