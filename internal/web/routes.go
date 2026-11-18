package web

import (
	"errors"
	"net/http"
)

var errNotFinished = errors.New("the request did not finish")

// Handler is the whole site. From the outside in: the log, compression, then
// per route the guard and the handler. /health alone skips the guard.
func (s *Server) Handler() http.Handler {
	mux := http.NewServeMux()
	// Inside the guard, so a refused page cannot load its assets either.
	s.route(mux, "GET /a/{file}", s.heldAsset)
	s.route(mux, "GET /{$}", s.front)
	s.route(mux, "GET /nb/{book}", s.listing)
	s.route(mux, "GET /nb/{book}/files", s.files)
	s.route(mux, "GET /nb/{book}/tags", s.tags)
	s.route(mux, "GET /nb/{book}/todo", s.todo)
	s.route(mux, "GET /nb/{book}/status", s.status)
	// POST for the errands, so a reload is a question, not a second push.
	s.route(mux, "POST /nb/{book}/status/{errand}", s.errand)
	// One segment, not a wildcard, so no path is assembled from pieces.
	s.route(mux, "GET /nb/{book}/f/{name}", s.heldFile)
	s.route(mux, "GET /nb/{book}/f/{name}/backlinks", s.fileBacklinks)
	s.route(mux, "GET /nb/{book}/n/{key}", s.reading)
	s.route(mux, "GET /nb/{book}/n/{key}/backlinks", s.noteBacklinks)
	s.addWriteRoutes(mux)
	// Outside the guard. See health.
	mux.HandleFunc("GET /health", s.health)

	// Inside the log, so the log times compression too.
	return Timed(s.logger, Gzip(s.withFallback(mux)))
}

// route registers a handler behind the guard, so a route added later is
// covered without anybody remembering.
func (s *Server) route(mux *http.ServeMux, pattern string, handler func(*http.Request) (answer, error)) {
	mux.Handle(pattern, s.guarded(s.answering(handler)))
}

// withFallback gives what matched nothing axum's answers: an empty 404, and a
// 405 that says which methods would have worked. The guard still comes first.
func (s *Server) withFallback(mux *http.ServeMux) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		handler, pattern := mux.Handler(r)
		if pattern != "" {
			mux.ServeHTTP(w, r)
			return
		}
		s.guarded(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			handler.ServeHTTP(&bareRefusal{ResponseWriter: w}, r)
		})).ServeHTTP(w, r)
	})
}

// bareRefusal turns the mux's own 404 and 405 pages into empty ones.
type bareRefusal struct {
	http.ResponseWriter
	decided bool
	bare    bool
}

func (b *bareRefusal) WriteHeader(status int) {
	if b.decided {
		return
	}
	b.decided = true
	if status != http.StatusNotFound && status != http.StatusMethodNotAllowed {
		b.ResponseWriter.WriteHeader(status)
		return
	}
	b.bare = true
	header := b.Header()
	allow := header.Values("Allow")
	for name := range header {
		header.Del(name)
	}
	for _, value := range allow {
		header.Add("Allow", value)
	}
	b.ResponseWriter.WriteHeader(status)
}

func (b *bareRefusal) Write(data []byte) (int, error) {
	if !b.decided {
		b.WriteHeader(http.StatusOK)
	}
	if b.bare {
		return len(data), nil
	}
	return b.ResponseWriter.Write(data)
}

// guarded asks the guard first. Host and Origin are read as the guard
// documents them: an absent Origin is not an empty one.
func (s *Server) guarded(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		host := r.Host
		var origin *string
		if values := r.Header.Values("Origin"); len(values) > 0 {
			origin = &values[0]
		}
		if err := s.guard.Admits(host, origin); err != nil {
			LogRefused(s.logger, host, origin, err.Error())
			writeHTML(w, http.StatusForbidden, Failure("Not answered", err.Error()))
			return
		}
		next.ServeHTTP(w, r)
	})
}

// answering turns a handler's answer, its error or its panic into a response.
func (s *Server) answering(handler func(*http.Request) (answer, error)) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		defer func() {
			if lost := recover(); lost != nil && lost != http.ErrAbortHandler {
				LogFailed(s.logger, errNotFinished.Error())
				writeHTML(w, http.StatusInternalServerError, Failure("Something went wrong", errNotFinished.Error()))
			}
		}()
		result, err := handler(r)
		if err != nil {
			LogFailed(s.logger, err.Error())
			writeHTML(w, http.StatusInternalServerError, Failure("Something went wrong", err.Error()))
			return
		}
		result(w)
	})
}

// health says whether this process can still answer.
//
// Outside the guard: a probe's Host is arbitrary, and a 403 would report a
// healthy server dead; it discloses only that something is listening. It opens
// no notebook: a broken repository is not what a restart fixes.
func (s *Server) health(w http.ResponseWriter, _ *http.Request) {
	writePlain(w, http.StatusOK, "ok\n")
}

// heldAsset is the stylesheet or one of the scripts. FindAsset matches the name
// against what this build embedded, so no path is ever joined or read.
//
// Immutable for a year: the address carries a hash of the bytes. Pages are
// no-cache to match.
func (s *Server) heldAsset(r *http.Request) (answer, error) {
	asset, ok := FindAsset(r.PathValue("file"))
	if !ok {
		return func(w http.ResponseWriter) { writePlain(w, http.StatusNotFound, "no such asset\n") }, nil
	}
	return func(w http.ResponseWriter) {
		header := w.Header()
		header.Set("Content-Type", asset.Kind)
		header.Set("Cache-Control", "public, max-age=31536000, immutable")
		header.Set("X-Content-Type-Options", "nosniff")
		_, _ = w.Write([]byte(asset.Body))
	}, nil
}
