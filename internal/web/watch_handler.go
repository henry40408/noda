package web

import (
	"fmt"
	"net/http"
)

// watch sends the note's fingerprint each time it changes, never the content,
// which is somebody's prose; the page compares it with its form's.
//
// No keep-alives: a proxy timing out an idle stream costs one reconnect, which
// EventSource makes by itself.
func (s *Server) watch(w http.ResponseWriter, r *http.Request) {
	at, err := s.aim(r, "/watch")
	if err != nil {
		LogFailed(s.logger, err.Error())
		w.WriteHeader(http.StatusInternalServerError)
		return
	}
	if at.other != nil {
		// Most likely a race with a delete; nobody reads this page.
		w.WriteHeader(http.StatusNotFound)
		return
	}
	path := at.opened.NotePath(at.id, at.slug)
	now, err := blobID(path)
	if err != nil {
		LogFailed(s.logger, err.Error())
		w.WriteHeader(http.StatusInternalServerError)
		return
	}
	changes := s.watching.Subscribe(r.PathValue("book"), at.id, path, now, r.Context().Done())

	w.Header().Set("Content-Type", "text/event-stream")
	w.Header().Set("Cache-Control", "no-cache")
	flush := http.NewResponseController(w).Flush
	_ = flush()
	for hash := range changes {
		if _, err := fmt.Fprintf(w, "data: %s\n\n", hash); err != nil {
			return
		}
		if flush() != nil {
			return
		}
	}
}
