package web

import (
	"strings"
	"testing"
)

func some(text string) *string { return &text }

func TestAnAddressIsAdmittedWithoutBeingAskedFor(t *testing.T) {
	for _, host := range []string{"127.0.0.1:8080", "localhost:8080", "localhost", "192.168.1.4:8080", "[::1]:8080"} {
		if err := NewGuard(nil).Admits(host, nil); err != nil {
			t.Errorf("%s refused: %v", host, err)
		}
	}
}

// DNS rebinding: only the name gives it away.
func TestANameNobodyAskedForIsRefusedEvenWhenTheOriginAgrees(t *testing.T) {
	err := NewGuard(nil).Admits("evil.example", some("http://evil.example"))
	if err == nil || !strings.Contains(err.Error(), "--allow-host evil.example") {
		t.Errorf("got %v", err)
	}
}

func TestANameThatWasAskedForIsAdmitted(t *testing.T) {
	guard := NewGuard([]string{"noda.tail1234.ts.net"})
	if err := guard.Admits("noda.tail1234.ts.net", some("https://noda.tail1234.ts.net")); err != nil {
		t.Error(err)
	}
}

func TestAnotherSiteCannotReachIn(t *testing.T) {
	err := NewGuard(nil).Admits("127.0.0.1:8080", some("https://elsewhere.example"))
	if err == nil || !strings.Contains(err.Error(), "elsewhere.example") {
		t.Errorf("got %v", err)
	}
}

func TestADifferentPortIsADifferentSite(t *testing.T) {
	if NewGuard(nil).Admits("127.0.0.1:8080", some("http://127.0.0.1:8081")) == nil {
		t.Error("admitted")
	}
}

func TestAnOrdinaryNavigationCarriesNoOriginAndIsFine(t *testing.T) {
	if err := NewGuard(nil).Admits("127.0.0.1:8080", nil); err != nil {
		t.Error(err)
	}
}

func TestAnOpaqueOriginIsNamedRatherThanLumpedIn(t *testing.T) {
	err := NewGuard(nil).Admits("127.0.0.1:8080", some("null"))
	if err == nil || !strings.Contains(err.Error(), "opaque") {
		t.Errorf("got %v", err)
	}
}

func TestARequestWithNoHostIsRefused(t *testing.T) {
	if NewGuard(nil).Admits("", nil) == nil {
		t.Error("admitted")
	}
}

func TestAHostNameIsNotFoldedBeyondASCII(t *testing.T) {
	// U+212A KELVIN SIGN folds to k under Unicode rules.
	guard := NewGuard([]string{"nod.example"})
	if guard.Admits("Kod.example", nil) == nil {
		t.Error("a look-alike name was admitted")
	}
}
