package web

import (
	"bufio"
	"bytes"
	"compress/gzip"
	"errors"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"
)

func TestAFileNamedOnlyByItsExtensionIsStillTyped(t *testing.T) {
	kind, inline := holding(".png")
	if kind != "image/png" || !inline {
		t.Errorf("got %q inline=%v", kind, inline)
	}
}

func TestOnlyAPdfAddressIsEmbeddedAsOne(t *testing.T) {
	for url, want := range map[string]bool{
		"/nb/b/f/a.pdf":                    true,
		"/nb/b/f/a.PDF":                    true,
		"https://example.com/a.pdf?x=1":    true,
		"https://example.com/a.pdf#p=2":    true,
		"/nb/b/f/a.pdf.png":                false,
		"/nb/b/f/pdf":                      false,
		"https://example.com/a.png?x=.pdf": false,
		"https://example.com/a.png#.pdf":   false,
		"":                                 false,
	} {
		if got := namesPDF(url); got != want {
			t.Errorf("namesPDF(%q) = %v, want %v", url, got, want)
		}
	}
}

func TestTheFirstCharacterOfEachSafeSetIsSafe(t *testing.T) {
	if !isHrefSafe('!') {
		t.Error("'!' is href safe")
	}
	if !isUnreserved('-') {
		t.Error("'-' is unreserved")
	}
	if !isPathSafe('-') {
		t.Error("'-' is path safe")
	}
}

func TestHexAndCaseHelpersAgreeWithTheirDefinitionOnEveryByte(t *testing.T) {
	for i := 0; i < 256; i++ {
		b := byte(i)
		wantHex := strings.IndexByte("0123456789abcdefABCDEF", b) >= 0
		if isHex(b) != wantHex {
			t.Errorf("isHex(%q) = %v", b, !wantHex)
		}
		wantLower := b
		if b >= 'A' && b <= 'Z' {
			wantLower = b + 32
		}
		if lowerASCII(b) != wantLower {
			t.Errorf("lowerASCII(%q) = %q", b, lowerASCII(b))
		}
	}
}

func TestIsSpaceCoversTheWholeUnicodeSpaceRange(t *testing.T) {
	for r, want := range map[rune]bool{
		0x1FFF: false, 0x2000: true, 0x2005: true, 0x200A: true, 0x200B: false,
	} {
		if isSpace(r) != want {
			t.Errorf("isSpace(%U) = %v", r, !want)
		}
	}
}

func TestBareURLsEdges(t *testing.T) {
	for _, text := range []string{"xhttps://a.b", "http:// foo", "http://.", "http://"} {
		if got := bareURLs(text); len(got) != 0 {
			t.Errorf("bareURLs(%q) = %v", text, got)
		}
	}
	text := "https://en.example.org/A_(b)"
	got := bareURLs(text)
	if len(got) != 1 || got[0] != [2]int{0, len(text)} {
		t.Errorf("balanced parenthesis dropped: %v", got)
	}
}

func TestAnAddressInsideALinkTextIsNotLinkedAgain(t *testing.T) {
	out := Body("[see https://example.org](https://example.com)", AroundOf("work", nil))
	if n := strings.Count(out, "<a "); n != 1 {
		t.Errorf("want one anchor, got %d in %q", n, out)
	}
}

func TestAPanicIsAnErrorPageButAnAbortIsNot(t *testing.T) {
	web := serving(t).web
	serve := func(lost any) *httptest.ResponseRecorder {
		rec := httptest.NewRecorder()
		web.answering(func(*http.Request) (answer, error) { panic(lost) }).
			ServeHTTP(rec, httptest.NewRequest("GET", "/", nil))
		return rec
	}
	if got := serve(errors.New("boom")); got.Code != http.StatusInternalServerError {
		t.Errorf("panic status %d", got.Code)
	}
	if got := serve(http.ErrAbortHandler); got.Code != http.StatusOK || got.Body.Len() != 0 {
		t.Errorf("abort wrote status %d body %q", got.Code, got.Body.String())
	}
}

func TestAWatchKeepsSendingAfterTheFirstChange(t *testing.T) {
	site := serving(t)
	id := site.idOf("budget-review")
	response, err := site.client.Get(site.server.URL + "/nb/default/n/" + id + "/watch")
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = response.Body.Close() }()
	reader := bufio.NewReader(response.Body)
	for _, hash := range []string{"first", "second"} {
		site.web.watching.tell(watchKey{"default", id}, hash)
		line, err := reader.ReadString('\n')
		if err != nil || line != "data: "+hash+"\n" {
			t.Fatalf("want %q, got %q (%v)", hash, line, err)
		}
		_, _ = reader.ReadString('\n')
	}
}

func TestOnlyOneErrandRunsPerNotebook(t *testing.T) {
	errands := NewErrands()
	if !errands.Begin("a", JobPull) {
		t.Fatal("first Begin refused")
	}
	if errands.Begin("a", JobPush) {
		t.Fatal("second Begin accepted while running")
	}
	errands.Finish("a", Outcome{})
	if !errands.Begin("a", JobPush) {
		t.Fatal("Begin refused after Finish")
	}
}

func TestAReportTimesARunningErrandAndFreezesAFinishedOne(t *testing.T) {
	errands := NewErrands()
	errands.Begin("a", JobPull)
	time.Sleep(20 * time.Millisecond)
	running, _ := errands.Report("a")
	if running.Took < 20*time.Millisecond {
		t.Errorf("running took %v", running.Took)
	}
	errands.Finish("a", Outcome{})
	first, _ := errands.Report("a")
	time.Sleep(20 * time.Millisecond)
	second, _ := errands.Report("a")
	if first.Took != second.Took || first.Took == 0 {
		t.Errorf("finished took %v then %v", first.Took, second.Took)
	}
}

func TestQualityOfGzipDecidesAcceptance(t *testing.T) {
	for header, want := range map[string]bool{
		"gzip;q=0.5": true, "gzip;q=abc": false, "gzip;q=0": false, "gzip": true,
	} {
		if got := acceptsGzip([]string{header}); got != want {
			t.Errorf("acceptsGzip(%q) = %v", header, got)
		}
	}
}

// gzipDuring runs handler behind Gzip and hands it the recorder so it can
// look at what has reached the client so far.
func gzipDuring(handler func(w http.ResponseWriter, rec *httptest.ResponseRecorder)) *httptest.ResponseRecorder {
	rec := httptest.NewRecorder()
	request := httptest.NewRequest("GET", "/", nil)
	request.Header.Set("Accept-Encoding", "gzip")
	Gzip(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) { handler(w, rec) })).ServeHTTP(rec, request)
	return rec
}

func TestAStatusOfAnUncompressibleBodyIsPassedAtOnce(t *testing.T) {
	gzipDuring(func(w http.ResponseWriter, rec *httptest.ResponseRecorder) {
		w.Header().Set("Content-Type", "image/png")
		w.WriteHeader(http.StatusNotFound)
		if rec.Code != http.StatusNotFound {
			t.Errorf("status not passed on, got %d", rec.Code)
		}
	})
}

func TestTheMinimumLengthDecidesWhenCompressionStarts(t *testing.T) {
	gzipDuring(func(w http.ResponseWriter, rec *httptest.ResponseRecorder) {
		w.Header().Set("Content-Type", "text/plain")
		_, _ = w.Write(bytes.Repeat([]byte("a"), gzipMinimum))
		if rec.Header().Get("Content-Encoding") != "gzip" {
			t.Error("a body of exactly the minimum was not compressed")
		}
	})
	short := gzipDuring(func(w http.ResponseWriter, _ *httptest.ResponseRecorder) {
		w.Header().Set("Content-Type", "text/plain")
		_, _ = w.Write(bytes.Repeat([]byte("a"), gzipMinimum-1))
	})
	if short.Header().Get("Content-Encoding") != "" {
		t.Error("a short body was compressed")
	}
}

func TestAFlushCompressesWhatIsStillUndecided(t *testing.T) {
	gzipDuring(func(w http.ResponseWriter, rec *httptest.ResponseRecorder) {
		w.Header().Set("Content-Type", "text/plain")
		_, _ = w.Write([]byte("hi"))
		w.(http.Flusher).Flush()
		if rec.Header().Get("Content-Encoding") != "gzip" {
			t.Error("flush did not start compression")
		}
		w.(http.Flusher).Flush()
	})
}

func TestADeclaredLengthIsComparedWithTheMinimum(t *testing.T) {
	for length, want := range map[string]string{"32": "gzip", "31": "", "1000": "gzip"} {
		got := gzipDuring(func(w http.ResponseWriter, _ *httptest.ResponseRecorder) {
			w.Header().Set("Content-Type", "text/plain")
			w.Header().Set("Content-Length", length)
			_, _ = w.Write(bytes.Repeat([]byte("a"), 40))
		})
		if got.Header().Get("Content-Encoding") != want {
			t.Errorf("Content-Length %s: encoding %q", length, got.Header().Get("Content-Encoding"))
		}
	}
}

func TestFlushPendingWritesOnlyOnceTheStateIsDecided(t *testing.T) {
	undecided := httptest.NewRecorder()
	waiting := &gzipWriter{ResponseWriter: undecided, state: gzipUndecided, pending: []byte("abc")}
	if err := waiting.flushPending(); err != nil || string(waiting.pending) != "abc" || undecided.Body.Len() != 0 {
		t.Errorf("undecided: pending %q body %q", waiting.pending, undecided.Body.String())
	}

	passing := httptest.NewRecorder()
	_ = (&gzipWriter{ResponseWriter: passing, state: gzipPassing, pending: []byte("abc")}).flushPending()
	if passing.Body.String() != "abc" {
		t.Errorf("passing wrote %q", passing.Body.String())
	}

	compressed := httptest.NewRecorder()
	zipping := &gzipWriter{ResponseWriter: compressed, state: gzipCompressing, pending: []byte("abc")}
	zipping.zip = gzip.NewWriter(compressed)
	_ = zipping.flushPending()
	if err := zipping.zip.Close(); err != nil {
		t.Fatal(err)
	}
	reader, err := gzip.NewReader(compressed.Body)
	if err != nil {
		t.Fatal(err)
	}
	if body, _ := io.ReadAll(reader); string(body) != "abc" {
		t.Errorf("compressing wrote %q", body)
	}
}
