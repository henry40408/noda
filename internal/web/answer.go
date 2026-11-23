package web

import (
	"net/http"
	"strings"
)

// answer is what a handler decided, before it is HTTP.
type answer func(w http.ResponseWriter)

func pageOf(body string) answer {
	return func(w http.ResponseWriter) { writeHTML(w, http.StatusOK, body) }
}

// elsewhere is a 303, so a reload does not resend a form.
func elsewhere(to string) answer {
	return func(w http.ResponseWriter) {
		w.Header().Set("Location", to)
		w.WriteHeader(http.StatusSeeOther)
	}
}

func missing(heading, detail string) answer {
	return func(w http.ResponseWriter) { writeHTML(w, http.StatusNotFound, Failure(heading, detail)) }
}

func missingNotebook(book string) answer {
	return missing("No such notebook", "There is no notebook called "+book+" on this machine.")
}

func missingNote(book, key string) answer {
	return missing("No such note", "Nothing in "+book+" is called "+key+".")
}

// fragmentHeader is its own header rather than an Accept variant: the type is
// text/html either way, and this only says how much of it.
const fragmentHeader = "X-Noda-Fragment"

// writeHTML sets the headers every page carries.
func writeHTML(w http.ResponseWriter, status int, body string) {
	header := w.Header()
	header.Set("Content-Type", "text/html; charset=utf-8")
	// Or a cache could hand a fragment to the next reader.
	header.Set("Vary", fragmentHeader)
	// An address here carries a note id, so a link out must not leak it. Not
	// no-referrer: under it Fetch nulls a form's Origin, which the guard
	// refuses, so every write would be turned away.
	header.Set("Referrer-Policy", "same-origin")
	// no-cache means "revalidate", so back still uses the cache.
	header.Set("Cache-Control", "no-cache")
	w.WriteHeader(status)
	_, _ = w.Write([]byte(body))
}

// writePlain is no-store, so a cache cannot report a stopped server as running.
func writePlain(w http.ResponseWriter, status int, body string) {
	header := w.Header()
	header.Set("Content-Type", "text/plain; charset=utf-8")
	header.Set("Cache-Control", "no-store")
	header.Set("X-Content-Type-Options", "nosniff")
	w.WriteHeader(status)
	_, _ = w.Write([]byte(body))
}

// fragment names the part of a page a script's fetch will use.
type fragment string

const (
	fragmentRead   fragment = "read"
	fragmentIndex  fragment = "index"
	fragmentScreen fragment = "screen"
	fragmentNews   fragment = "news"
	// fragmentSent is a write that wants only the address it went to. See told.
	fragmentSent fragment = "sent"
)

// told answers a script's write with where it went instead of a 303. A fetch
// that follows the 303 makes the server render the page it lands on, and then
// the script navigates there and the server renders it again; a note's page
// carries a scan of the whole notebook for its backlinks.
type told struct{ http.ResponseWriter }

func (t told) WriteHeader(status int) {
	if status == http.StatusSeeOther {
		header := t.Header()
		header.Set("X-Noda-Location", header.Get("Location"))
		header.Del("Location")
		status = http.StatusNoContent
	}
	t.ResponseWriter.WriteHeader(status)
}

// wanted is asked per part, since each route can send only its own. The whole
// page is always a correct answer.
func (f fragment) wanted(r *http.Request) bool {
	return r.Header.Get(fragmentHeader) == string(f)
}

// attachment is a file the notebook holds, for download.
type attachment struct {
	bytes  []byte
	kind   string
	inline bool
	name   string
}

func (a attachment) answer() answer {
	return func(w http.ResponseWriter) {
		disposition := "attachment"
		if a.inline {
			disposition = "inline"
		}
		header := w.Header()
		header.Set("Content-Type", a.kind)
		header.Set("X-Content-Type-Options", "nosniff")
		// Not a bare filename=: a notebook's file may be réunion.pdf.
		header.Set("Content-Disposition", disposition+"; filename*=UTF-8''"+encoded(a.name))
		// Belt and braces for any format that turns out able to run a script,
		// as SVG does.
		header.Set("Content-Security-Policy", "default-src 'none'; sandbox")
		_, _ = w.Write(a.bytes)
	}
}

// holding is a file's content type, and whether it may be shown inline.
//
// An allow-list: attachments share the pages' origin, so anything inline that
// can carry a script (SVG) runs as this site. Anything unlisted is an
// octet-stream download.
func holding(name string) (kind string, inline bool) {
	extension := ""
	if at := strings.LastIndexByte(name, '.'); at >= 0 {
		extension = lowerASCIIString(name[at+1:])
	}
	switch extension {
	case "png":
		return "image/png", true
	case "jpg", "jpeg":
		return "image/jpeg", true
	case "gif":
		return "image/gif", true
	case "webp":
		return "image/webp", true
	case "avif":
		return "image/avif", true
	case "txt", "md", "csv", "log":
		return "text/plain; charset=utf-8", true
	case "pdf":
		return "application/pdf", false
	case "svg":
		return "image/svg+xml", false
	}
	return "application/octet-stream", false
}

// lowerASCIIString folds ASCII only, as the allow-list above is ASCII.
func lowerASCIIString(text string) string {
	folded := []byte(text)
	for i := range folded {
		folded[i] = lowerASCII(folded[i])
	}
	return string(folded)
}
