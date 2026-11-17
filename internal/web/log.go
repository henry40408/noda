package web

import (
	"io"
	"log/slog"
	"net/http"
	"strings"
	"time"
)

// What the server says about itself while it runs.
//
// The log goes to stderr, because stdout carries the address to open: the
// command's answer, and where the test harnesses read the port from.
//
// The default level (info) is quiet: a healthy server logs nothing per request;
// debug turns that on. The startup lines are not events, since they are already
// the answer on stdout.

// SlowRequest is how long a request may take before it is a warning rather
// than a debug line. The most expensive page is tens of milliseconds at two
// thousand notes, so a second means something is wrong.
const SlowRequest = time.Second

// Unmatched is the route of a request that matched nothing; never its path.
const Unmatched = "<unmatched>"

// LogFormat is a person or a program.
type LogFormat string

const (
	LogText LogFormat = "text" // for a person, at a terminal
	LogJSON LogFormat = "json" // one JSON object per line, for something that collects them
)

// NewLogger writes to w at level and above.
func NewLogger(w io.Writer, level slog.Level, format LogFormat) *slog.Logger {
	options := &slog.HandlerOptions{Level: level}
	if format == LogJSON {
		return slog.New(slog.NewJSONHandler(w, options))
	}
	return slog.New(slog.NewTextHandler(w, options))
}

// Timed is layered outermost, so it also sees requests the guard refuses.
func Timed(logger *slog.Logger, next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		started := time.Now()
		recorder := &statusRecorder{ResponseWriter: w, status: http.StatusOK}
		next.ServeHTTP(recorder, r)
		// The mux sets Pattern on the request it was handed, so it is read
		// after the handler ran.
		logFinished(logger, finished{
			method: r.Method, route: routeOf(r.Pattern), status: recorder.status, elapsed: time.Since(started),
		})
	})
}

// routeOf is the matched route template without its method, or Unmatched.
func routeOf(pattern string) string {
	if pattern == "" {
		return Unmatched
	}
	if _, path, found := strings.Cut(pattern, " "); found {
		pattern = path
	}
	if pattern == "/{$}" {
		return "/"
	}
	return pattern
}

type finished struct {
	method  string
	route   string
	status  int
	elapsed time.Duration
}

// logFinished says that a request finished.
//
// The route is the matched template, never the path: ids, filenames and query
// strings are somebody's notes and words, and a template is also what one
// aggregates on. An unmatched path is labelled, not repeated.
//
// elapsed is for a terminal, elapsed_ms for totalling under the json format.
func logFinished(logger *slog.Logger, request finished) {
	elapsedMS := float64(request.elapsed) / float64(time.Millisecond)
	attributes := []any{
		"event", "http.request",
		"method", request.method,
		"route", request.route,
		"status", request.status,
		"elapsed", request.elapsed,
		"elapsed_ms", elapsedMS,
	}
	if request.elapsed >= SlowRequest {
		attributes[1] = "http.slow_request"
		attributes = append(attributes, "threshold_ms", float64(SlowRequest)/float64(time.Millisecond))
		logger.Warn("request took longer than the slow-request threshold", attributes...)
		return
	}
	logger.Debug("request finished", attributes...)
}

// LogRefused is the one event worth an alert: a refusal is a misconfiguration
// or the rebinding attack the guard exists for.
//
// Host and Origin are what was decided on. Being attacker-controlled, they go
// in attributes, which the handler quotes and escapes, not the message.
func LogRefused(logger *slog.Logger, host string, origin *string, why string) {
	if host == "" {
		host = "<none>"
	}
	from := "<none>"
	if origin != nil {
		from = *origin
	}
	logger.Warn("the guard turned a request away", "event", "http.refused", "host", host, "origin", from, "why", why)
}

// LogFailed records what a 500 page only showed. noda's own text, so it may go
// in the message.
func LogFailed(logger *slog.Logger, problem string) {
	logger.Error("the request could not be answered", "event", "http.failed", "error", problem)
}

// statusRecorder remembers the status for the log and passes Flush through, so
// a stream still streams.
type statusRecorder struct {
	http.ResponseWriter
	status int
}

func (s *statusRecorder) WriteHeader(status int) {
	s.status = status
	s.ResponseWriter.WriteHeader(status)
}

func (s *statusRecorder) Flush() {
	if flusher, ok := s.ResponseWriter.(http.Flusher); ok {
		flusher.Flush()
	}
}

func (s *statusRecorder) Unwrap() http.ResponseWriter { return s.ResponseWriter }
