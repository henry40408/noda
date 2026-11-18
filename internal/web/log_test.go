package web

import (
	"bytes"
	"log/slog"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"
)

func capture(level slog.Level, format LogFormat) (*slog.Logger, *bytes.Buffer) {
	var out bytes.Buffer
	return NewLogger(&out, level, format), &out
}

func TestAFinishedRequestSaysWhatItWasAndHowLongItTook(t *testing.T) {
	logger, out := capture(slog.LevelDebug, LogText)
	logFinished(logger, finished{"GET", "/nb/{book}", 200, 12 * time.Millisecond})
	for _, want := range []string{"event=http.request", "level=DEBUG", "method=GET", "status=200", "elapsed_ms=12"} {
		if !strings.Contains(out.String(), want) {
			t.Errorf("missing %q in %s", want, out)
		}
	}
}

func TestANormalRequestIsQuietAtTheDefaultLevel(t *testing.T) {
	logger, out := capture(slog.LevelInfo, LogText)
	logFinished(logger, finished{"GET", "/", 200, time.Millisecond})
	if out.Len() != 0 {
		t.Errorf("logged %s", out)
	}
}

func TestARequestAtTheThresholdIsAWarningInstead(t *testing.T) {
	logger, out := capture(slog.LevelInfo, LogText)
	logFinished(logger, finished{"GET", "/", 200, SlowRequest})
	for _, want := range []string{"event=http.slow_request", "level=WARN", "threshold_ms=1000"} {
		if !strings.Contains(out.String(), want) {
			t.Errorf("missing %q in %s", want, out)
		}
	}
}

func TestARequestJustUnderItStaysAtDebug(t *testing.T) {
	logger, out := capture(slog.LevelDebug, LogText)
	logFinished(logger, finished{"GET", "/", 200, 999 * time.Millisecond})
	if !strings.Contains(out.String(), "event=http.request") || strings.Contains(out.String(), "slow_request") {
		t.Errorf("got %s", out)
	}
}

func TestARefusalSaysWhatItWasGivenAndIsLoggedByDefault(t *testing.T) {
	logger, out := capture(slog.LevelInfo, LogText)
	LogRefused(logger, "evil.example", nil, "the Host is not allowed")
	for _, want := range []string{"event=http.refused", "level=WARN", "host=evil.example", "origin=<none>"} {
		if !strings.Contains(out.String(), want) {
			t.Errorf("missing %q in %s", want, out)
		}
	}
}

func TestAFailureRecordsWhatThePageOnlyShowed(t *testing.T) {
	logger, out := capture(slog.LevelError, LogText)
	LogFailed(logger, "could not open the notebook")
	if !strings.Contains(out.String(), "event=http.failed") || !strings.Contains(out.String(), "could not open the notebook") {
		t.Errorf("got %s", out)
	}
}

func TestTheJSONFormatIsOneObjectPerLine(t *testing.T) {
	logger, out := capture(slog.LevelDebug, LogJSON)
	logFinished(logger, finished{"GET", "/", 200, time.Millisecond})
	if !strings.HasPrefix(out.String(), "{") || !strings.Contains(out.String(), `"event":"http.request"`) {
		t.Errorf("got %s", out)
	}
}

func TestANoteIsLoggedAsItsRouteAndNeverAsItsAddress(t *testing.T) {
	logger, out := capture(slog.LevelDebug, LogText)
	mux := http.NewServeMux()
	mux.HandleFunc("GET /nb/{book}/n/{key}", func(w http.ResponseWriter, _ *http.Request) {})
	mux.HandleFunc("GET /{$}", func(w http.ResponseWriter, _ *http.Request) {})
	handler := Timed(logger, mux)
	for _, path := range []string{"/nb/work/n/k3f9m2p1", "/", "/no/such/k3f9m2p1"} {
		handler.ServeHTTP(httptest.NewRecorder(), httptest.NewRequest("GET", path, nil))
	}
	log := out.String()
	for _, want := range []string{`route=/nb/{book}/n/{key}`, `route=/ `, `route=<unmatched>`} {
		if !strings.Contains(log, want) {
			t.Errorf("missing %q in %s", want, log)
		}
	}
	if strings.Contains(log, "k3f9m2p1") {
		t.Errorf("an address leaked into the log: %s", log)
	}
}
