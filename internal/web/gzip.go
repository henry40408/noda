package web

import (
	"compress/gzip"
	"net/http"
	"strconv"
	"strings"
)

// gzipMinimum is tower-http's DefaultPredicate threshold: a body shorter than
// this is not worth a gzip header.
const gzipMinimum = 32

// Gzip compresses what a reader asked to have compressed, except what is
// already compressed (images, PDFs, unknown binary), a stream that must arrive
// as it is written (text/event-stream) and bodies under gzipMinimum bytes.
func Gzip(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if !acceptsGzip(r.Header.Values("Accept-Encoding")) || r.Method == http.MethodHead {
			next.ServeHTTP(w, r)
			return
		}
		compressing := &gzipWriter{ResponseWriter: w, status: http.StatusOK}
		defer compressing.finish()
		next.ServeHTTP(compressing, r)
	})
}

// acceptsGzip reads Accept-Encoding for gzip with a quality above zero.
func acceptsGzip(values []string) bool {
	for _, value := range values {
		for _, item := range strings.Split(value, ",") {
			name, parameters, _ := strings.Cut(strings.TrimSpace(item), ";")
			name = strings.TrimSpace(name)
			if !strings.EqualFold(name, "gzip") && !strings.EqualFold(name, "x-gzip") {
				continue
			}
			return qualityOf(parameters) > 0
		}
	}
	return false
}

func qualityOf(parameters string) float64 {
	for _, parameter := range strings.Split(parameters, ";") {
		key, value, _ := strings.Cut(strings.TrimSpace(parameter), "=")
		if !strings.EqualFold(strings.TrimSpace(key), "q") {
			continue
		}
		quality, err := strconv.ParseFloat(strings.TrimSpace(value), 64)
		if err != nil {
			return 0
		}
		return quality
	}
	return 1
}

type gzipState int

const (
	gzipUndecided gzipState = iota
	gzipPassing
	gzipCompressing
)

// gzipWriter holds the first bytes back until it knows whether the body is
// long enough to be worth compressing.
type gzipWriter struct {
	http.ResponseWriter
	status  int
	wrote   bool
	state   gzipState
	pending []byte
	zip     *gzip.Writer
}

func (g *gzipWriter) Unwrap() http.ResponseWriter { return g.ResponseWriter }

func (g *gzipWriter) WriteHeader(status int) {
	if g.wrote {
		return
	}
	g.wrote = true
	g.status = status
	if g.state == gzipUndecided && !g.mayCompress() {
		g.pass()
	}
}

func (g *gzipWriter) Write(data []byte) (int, error) {
	if !g.wrote {
		g.WriteHeader(http.StatusOK)
	}
	switch g.state {
	case gzipPassing:
		return g.ResponseWriter.Write(data)
	case gzipCompressing:
		return g.zip.Write(data)
	}
	g.pending = append(g.pending, data...)
	if len(g.pending) >= gzipMinimum {
		g.compress()
		return len(data), g.flushPending()
	}
	return len(data), nil
}

// Flush decides in favour of compressing: a reader who flushes wants the bytes
// now, and a stream that must stay plain was settled by its content type.
func (g *gzipWriter) Flush() {
	if !g.wrote {
		g.WriteHeader(http.StatusOK)
	}
	if g.state == gzipUndecided {
		g.compress()
		_ = g.flushPending()
	}
	if g.state == gzipCompressing {
		_ = g.zip.Flush()
	}
	if flusher, ok := g.ResponseWriter.(http.Flusher); ok {
		flusher.Flush()
	}
}

// finish ends the body: a short one goes out as it is.
func (g *gzipWriter) finish() {
	if !g.wrote {
		return
	}
	switch g.state {
	case gzipUndecided:
		g.pass()
		_, _ = g.ResponseWriter.Write(g.pending)
	case gzipCompressing:
		_ = g.zip.Close()
	}
}

func (g *gzipWriter) mayCompress() bool {
	header := g.Header()
	if header.Get("Content-Encoding") != "" || header.Get("Content-Range") != "" {
		return false
	}
	if g.status < 200 || g.status == http.StatusNoContent || g.status == http.StatusNotModified {
		return false
	}
	if length, err := strconv.Atoi(header.Get("Content-Length")); err == nil && length < gzipMinimum {
		return false
	}
	return compressibleType(header.Get("Content-Type"))
}

// compressibleType is false for what is already compressed. SVG is text, and
// stays compressible as in tower-http.
func compressibleType(contentType string) bool {
	kind, _, _ := strings.Cut(contentType, ";")
	kind = strings.ToLower(strings.TrimSpace(kind))
	switch {
	case kind == "image/svg+xml":
		return true
	case strings.HasPrefix(kind, "image/"),
		kind == "text/event-stream",
		kind == "application/pdf",
		kind == "application/octet-stream":
		return false
	}
	return true
}

func (g *gzipWriter) pass() {
	g.state = gzipPassing
	g.ResponseWriter.WriteHeader(g.status)
}

func (g *gzipWriter) compress() {
	if !g.mayCompress() {
		g.pass()
		return
	}
	header := g.Header()
	header.Del("Content-Length")
	header.Set("Content-Encoding", "gzip")
	if !strings.Contains(strings.ToLower(strings.Join(header.Values("Vary"), ",")), "accept-encoding") {
		header.Add("Vary", "Accept-Encoding")
	}
	g.state = gzipCompressing
	g.ResponseWriter.WriteHeader(g.status)
	g.zip = gzip.NewWriter(g.ResponseWriter)
}

func (g *gzipWriter) flushPending() error {
	if g.state != gzipPassing && g.state != gzipCompressing {
		return nil
	}
	pending := g.pending
	g.pending = nil
	if len(pending) == 0 {
		return nil
	}
	if g.state == gzipPassing {
		_, err := g.ResponseWriter.Write(pending)
		return err
	}
	_, err := g.zip.Write(pending)
	return err
}
