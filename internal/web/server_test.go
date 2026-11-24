package web

import (
	"bytes"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/henry40408/noda/internal/cmd"
	"github.com/henry40408/noda/internal/config"
	"github.com/henry40408/noda/internal/gitx"
	"github.com/henry40408/noda/internal/gitx/gittest"
	"github.com/henry40408/noda/internal/paths"
)

// site is the handler on a real socket, driven by requests written by hand:
// the guard reads Host and Origin, which a rebinding attack lies about.
type site struct {
	t      *testing.T
	env    cmd.Env
	server *httptest.Server
	web    *Server
	log    *bytes.Buffer
	client *http.Client
}

type reply struct {
	status int
	header http.Header
	body   string
}

func (r reply) says(text string) bool { return strings.Contains(r.body, text) }

func str(text string) *string { return &text }

// aNotebook is four notes, a few files, and a note embedding a file, with
// signing off before anything commits.
func aNotebook(t *testing.T) cmd.Env {
	t.Helper()
	env := cmd.Env{Paths: paths.Rooted(t.TempDir()), Git: gittest.Require(t)}
	if err := os.MkdirAll(env.Paths.ConfigDir(), 0o777); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(env.Paths.ConfigDir(), "config.toml"), []byte("sign = false\n"), 0o666); err != nil {
		t.Fatal(err)
	}
	if _, err := cmd.Init(env); err != nil {
		t.Fatal(err)
	}
	for _, note := range []struct {
		title, body string
		tags        []string
	}{
		{"Budget review", "the q3 budget is late", []string{"work"}},
		{"Meeting notes", "# Agenda\n\nthe budget, again", []string{"work", "24.04 Dark patterns"}},
		{"Reading list", "a book", nil},
		{"Raw html import", "a <div class=\"x\">html</div> here", []string{"ops"}},
	} {
		if _, err := cmd.Add(env, cmd.AddOptions{Title: str(note.title), Content: str(note.body), Tags: note.tags}); err != nil {
			t.Fatal(err)
		}
	}
	scratch := t.TempDir()
	for name, content := range map[string]string{
		"rack.png": "\x89PNG\r\n\x1a\nnot really",
		"plan.svg": "<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>",
	} {
		source := filepath.Join(scratch, name)
		if err := os.WriteFile(source, []byte(content), 0o666); err != nil {
			t.Fatal(err)
		}
		if _, err := cmd.FileAdd(env, []string{source}, ""); err != nil {
			t.Fatal(err)
		}
	}
	if _, err := cmd.Add(env, cmd.AddOptions{Title: str("The rack"), Content: str("see ![the rack](rack.png)")}); err != nil {
		t.Fatal(err)
	}
	if _, err := cmd.Readme(env, false); err != nil {
		t.Fatal(err)
	}
	return env
}

func serving(t *testing.T, allowed ...string) *site {
	t.Helper()
	return servingOn(t, aNotebook(t), allowed...)
}

func servingOn(t *testing.T, env cmd.Env, allowed ...string) *site {
	t.Helper()
	logged := &bytes.Buffer{}
	web := NewServer(Options{Env: env, AllowHosts: allowed, LogFormat: LogText, LogLevel: -4, Stderr: logged})
	server := httptest.NewServer(web.Handler())
	t.Cleanup(func() {
		// A watch never ends by itself, and Close waits for every request.
		web.watching.Stop()
		server.Close()
	})
	client := &http.Client{CheckRedirect: func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }}
	// Transparent decompression would hide the very header under test.
	client.Transport = &http.Transport{DisableCompression: true}
	return &site{t: t, env: env, server: server, web: web, log: logged, client: client}
}

func (s *site) send(method, path string, headers map[string]string, body string) reply {
	s.t.Helper()
	request, err := http.NewRequest(method, s.server.URL+path, strings.NewReader(body))
	if err != nil {
		s.t.Fatal(err)
	}
	for name, value := range headers {
		if strings.EqualFold(name, "Host") {
			request.Host = value
			continue
		}
		request.Header.Set(name, value)
	}
	response, err := s.client.Do(request)
	if err != nil {
		s.t.Fatal(err)
	}
	defer func() { _ = response.Body.Close() }()
	content, err := io.ReadAll(response.Body)
	if err != nil {
		s.t.Fatal(err)
	}
	return reply{status: response.StatusCode, header: response.Header, body: string(content)}
}

func (s *site) get(path string) reply { return s.send("GET", path, nil, "") }

func (s *site) request(path string, headers map[string]string) reply {
	return s.send("GET", path, headers, "")
}

func (s *site) idOf(slug string) string {
	s.t.Helper()
	dir := s.env.Paths.NotebookDir(config.DefaultNotebook)
	entries, err := os.ReadDir(dir)
	if err != nil {
		s.t.Fatal(err)
	}
	for _, entry := range entries {
		if id, ok := strings.CutSuffix(entry.Name(), "-"+slug+".md"); ok {
			return id
		}
	}
	s.t.Fatalf("no note called %s", slug)
	return ""
}

func TestTheFrontPageListsTheNotebooks(t *testing.T) {
	got := serving(t).get("/")
	if got.status != 200 || !got.says("default") || !got.says("/nb/default") {
		t.Fatalf("%d %s", got.status, got.body)
	}
	for header, want := range map[string]string{
		"Content-Type": "text/html; charset=utf-8", "Vary": "X-Noda-Fragment",
		"Referrer-Policy": "same-origin", "Cache-Control": "no-cache",
	} {
		if got.header.Get(header) != want {
			t.Errorf("%s: %q", header, got.header.Get(header))
		}
	}
}

func TestTheListingNamesEveryNote(t *testing.T) {
	got := serving(t).get("/nb/default")
	if got.status != 200 {
		t.Fatalf("%d", got.status)
	}
	for _, title := range []string{"Budget review", "Meeting notes", "Reading list", "Raw html import", "The rack"} {
		if !got.says(title) {
			t.Errorf("%s is missing", title)
		}
	}
}

func TestAQueryNarrowsTheListingAndAnUnfinishedOneSaysWhy(t *testing.T) {
	site := serving(t)
	narrowed := site.get("/nb/default?q=tag%3Awork")
	if narrowed.status != 200 || !narrowed.says("Budget review") {
		t.Fatalf("%d", narrowed.status)
	}
	quoted := site.get("/nb/default?q=tag%3A%2224.04+Dark+patterns%22")
	if quoted.status != 200 || !quoted.says("Meeting notes") {
		t.Fatalf("%d", quoted.status)
	}
	unfinished := site.get("/nb/default?q=OR")
	if unfinished.status != 200 || !unfinished.says("needs a term on both sides") || !unfinished.says("Reading list") {
		t.Fatalf("%d %s", unfinished.status, unfinished.body)
	}
}

func TestASlugAndAPrefixBothLeadToTheID(t *testing.T) {
	site := serving(t)
	id := site.idOf("budget-review")
	for _, key := range []string{"budget-review", id[:4]} {
		got := site.get("/nb/default/n/" + key)
		if got.status != 303 || got.header.Get("Location") != "/nb/default/n/"+id {
			t.Errorf("%s: %d %s", key, got.status, got.header.Get("Location"))
		}
	}
	if got := site.get("/nb/default/n/" + id); got.status != 200 || !got.says("Budget review") {
		t.Fatalf("%d", got.status)
	}
}

func TestAWrongAddressIsToldApartFromABrokenNotebook(t *testing.T) {
	site := serving(t)
	if got := site.get("/nb/ghost"); got.status != 404 || !got.says("No such notebook") {
		t.Fatalf("%d", got.status)
	}
	if got := site.get("/nb/default/n/zzzzzzzz"); got.status != 404 || !got.says("No such note") {
		t.Fatalf("%d", got.status)
	}
}

func TestAnOrderIsAskedForByTheAddressAndAnUnknownOneIsTheDefault(t *testing.T) {
	site := serving(t)
	for _, path := range []string{"/nb/default?sort=title", "/nb/default?sort=created&r=1", "/nb/default?sort=nonsense"} {
		if got := site.get(path); got.status != 200 {
			t.Errorf("%s: %d", path, got.status)
		}
	}
}

func TestTheFilesPageListsWhatIsNotANote(t *testing.T) {
	site := serving(t)
	got := site.get("/nb/default/files")
	for _, want := range []string{"rack.png", "plan.svg", `href="/nb/default/f/rack.png"`, "in 1 note", "nothing links to it", `href="/nb/default/f/README.md"`} {
		if !got.says(want) {
			t.Errorf("%q missing", want)
		}
	}
	if got.says(site.idOf("budget-review") + "-budget-review.md") {
		t.Error("a note is listed as a file")
	}
}

func TestOnlyTheSafeFilesAreShownInPlace(t *testing.T) {
	site := serving(t)
	png := site.get("/nb/default/f/rack.png")
	if png.status != 200 || png.header.Get("Content-Type") != "image/png" ||
		!strings.HasPrefix(png.header.Get("Content-Disposition"), "inline") ||
		png.header.Get("X-Content-Type-Options") != "nosniff" {
		t.Fatalf("%d %v", png.status, png.header)
	}
	svg := site.get("/nb/default/f/plan.svg")
	if !strings.HasPrefix(svg.header.Get("Content-Disposition"), "attachment") ||
		svg.header.Get("Content-Security-Policy") != "default-src 'none'; sandbox" {
		t.Fatalf("%v", svg.header)
	}
}

func TestAFileRequestCannotClimbOutOfTheNotebook(t *testing.T) {
	site := serving(t)
	for _, path := range []string{
		"/nb/default/f/..%2f..%2f..%2fetc%2fpasswd", "/nb/default/f/%2Fetc%2Fpasswd", "/nb/default/f/nothing-here.png",
		"/nb/default/f/..%2f..%2f..%2fetc%2fpasswd/backlinks", "/nb/default/f/%2e%2e%2f%2e%2e%2fsecret",
	} {
		if got := site.get(path); got.status != 404 {
			t.Errorf("%s was answered %d:\n%s", path, got.status, got.body)
		}
	}
}

func TestANoteIsNotServedAsAFileButAMarkdownFileThatIsNotANoteIs(t *testing.T) {
	site := serving(t)
	if got := site.get("/nb/default/f/" + site.idOf("budget-review") + "-budget-review.md"); got.status != 404 {
		t.Fatalf("%d", got.status)
	}
	if got := site.get("/nb/default/f/README.md"); got.status != 200 || !got.says("default") {
		t.Fatalf("%d", got.status)
	}
	if got := site.get("/nb/default/f/README.md/backlinks"); got.status != 200 {
		t.Fatalf("%d", got.status)
	}
}

func TestAnEmbeddedImagePointsAtTheFileRoute(t *testing.T) {
	site := serving(t)
	got := site.get("/nb/default/n/" + site.idOf("the-rack"))
	if !got.says(`src="/nb/default/f/rack.png"`) {
		t.Fatal(got.body)
	}
}

func TestABodyHoldingMarkupArrivesAsCode(t *testing.T) {
	site := serving(t)
	got := site.get("/nb/default/n/" + site.idOf("raw-html-import"))
	if got.says(`<div class="x">`) || !got.says("&lt;div") {
		t.Fatal(got.body)
	}
}

func TestAPageOnAnotherSiteIsTurnedAway(t *testing.T) {
	got := serving(t).request("/", map[string]string{"Origin": "https://elsewhere.example"})
	if got.status != 403 || !got.says("elsewhere.example") {
		t.Fatalf("%d %s", got.status, got.body)
	}
}

func TestAHostnameNobodyAskedForIsTurnedAway(t *testing.T) {
	got := serving(t).request("/", map[string]string{"Host": "evil.example", "Origin": "http://evil.example"})
	if got.status != 403 || !got.says("--allow-host evil.example") {
		t.Fatalf("%d %s", got.status, got.body)
	}
}

func TestAHostnameThatWasAskedForIsAdmitted(t *testing.T) {
	got := serving(t, "noda.tail1234.ts.net").request("/", map[string]string{
		"Host": "noda.tail1234.ts.net", "Origin": "https://noda.tail1234.ts.net",
	})
	if got.status != 200 {
		t.Fatalf("%d", got.status)
	}
}

func TestAnEmptyOriginIsNotAnAbsentOne(t *testing.T) {
	site := serving(t)
	if got := site.request("/", map[string]string{"Origin": ""}); got.status != 403 {
		t.Fatalf("%d", got.status)
	}
}

func TestTheHealthCheckAnswersAnyNameAndNothingElseDoes(t *testing.T) {
	site := serving(t)
	got := site.get("/health")
	if got.status != 200 || got.body != "ok\n" || got.header.Get("Cache-Control") != "no-store" ||
		!strings.HasPrefix(got.header.Get("Content-Type"), "text/plain") {
		t.Fatalf("%d %q %v", got.status, got.body, got.header)
	}
	name := map[string]string{"Host": "kubernetes.default.svc"}
	if site.request("/health", name).status != 200 || site.request("/", name).status != 403 {
		t.Fatal("only /health may skip the guard")
	}
	head := site.send("HEAD", "/health", nil, "")
	if head.status != 200 || head.body != "" {
		t.Fatalf("%d %q", head.status, head.body)
	}
}

func TestTheHealthCheckIsLoggedByRouteTemplate(t *testing.T) {
	site := serving(t)
	site.get("/health")
	if log := site.log.String(); !strings.Contains(log, "route=/health") || !strings.Contains(log, "event=http.request") {
		t.Fatal(log)
	}
}

func TestTheLogNamesTheRouteAndNeverTheNote(t *testing.T) {
	site := serving(t)
	id := site.idOf("budget-review")
	site.get("/nb/default/n/" + id + "?q=secret")
	site.get("/nowhere/at/all")
	log := site.log.String()
	for _, leaked := range []string{id, "secret", "nowhere"} {
		if strings.Contains(log, leaked) {
			t.Errorf("the log says %q:\n%s", leaked, log)
		}
	}
	if !strings.Contains(log, "route=/nb/{book}/n/{key}") || !strings.Contains(log, "route=<unmatched>") {
		t.Fatal(log)
	}
}

func TestAnUnknownPathIsAnEmpty404AndAWrongMethodIsA405WithAllow(t *testing.T) {
	site := serving(t)
	if got := site.get("/nowhere"); got.status != 404 || got.body != "" {
		t.Fatalf("%d %q", got.status, got.body)
	}
	got := site.send("POST", "/nb/default", nil, "")
	if got.status != 405 || got.body != "" || !strings.Contains(got.header.Get("Allow"), "GET") {
		t.Fatalf("%d %q %v", got.status, got.body, got.header)
	}
	// The guard comes first, so a refused name learns nothing about routes.
	if got := site.request("/nowhere", map[string]string{"Host": "evil.example"}); got.status != 403 {
		t.Fatalf("%d", got.status)
	}
}

func TestAnAnswerIsCompressedOnlyWhenTheReaderAsksForIt(t *testing.T) {
	site := serving(t)
	asked := site.request("/nb/default", map[string]string{"Accept-Encoding": "gzip"})
	if asked.status != 200 || asked.header.Get("Content-Encoding") != "gzip" ||
		!strings.Contains(strings.ToLower(strings.Join(asked.header.Values("Vary"), ",")), "accept-encoding") {
		t.Fatalf("%d %v", asked.status, asked.header)
	}
	plain := site.get("/nb/default")
	if plain.header.Get("Content-Encoding") != "" || !plain.says("Budget review") {
		t.Fatal(plain.header)
	}
	refused := site.request("/nb/default", map[string]string{"Accept-Encoding": "gzip;q=0"})
	if refused.header.Get("Content-Encoding") != "" {
		t.Fatal("gzip;q=0 means no")
	}
}

func TestWhatIsAlreadyCompressedIsNotCompressedAgain(t *testing.T) {
	env := aNotebook(t)
	dir := env.Paths.NotebookDir(config.DefaultNotebook)
	filler := strings.Repeat("the quarterly budget is late\n", 40)
	scratch := t.TempDir()
	for _, name := range []string{"notes.txt", "archive.bin"} {
		source := filepath.Join(scratch, name)
		if err := os.WriteFile(source, []byte(filler), 0o666); err != nil {
			t.Fatal(err)
		}
		if _, err := cmd.FileAdd(env, []string{source}, ""); err != nil {
			t.Fatal(err)
		}
	}
	_ = dir
	site := servingOn(t, env)
	gzip := map[string]string{"Accept-Encoding": "gzip"}
	if got := site.request("/nb/default/f/notes.txt", gzip); got.header.Get("Content-Encoding") != "gzip" {
		t.Errorf("text: %v", got.header)
	}
	for path, why := range map[string]string{
		"/nb/default/f/archive.bin": "octet-stream", "/nb/default/f/rack.png": "image", "/health": "shorter than a gzip header",
	} {
		if got := site.request(path, gzip); got.status != 200 || got.header.Get("Content-Encoding") != "" {
			t.Errorf("%s was compressed (%s): %v", path, why, got.header)
		}
	}
}

func TestAStreamIsNeverCompressed(t *testing.T) {
	handler := Gzip(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.Header().Set("Content-Type", "text/event-stream")
		_, _ = w.Write([]byte("data: " + strings.Repeat("x", 100) + "\n\n"))
	}))
	request := httptest.NewRequest("GET", "/", nil)
	request.Header.Set("Accept-Encoding", "gzip")
	recorder := httptest.NewRecorder()
	handler.ServeHTTP(recorder, request)
	if recorder.Header().Get("Content-Encoding") != "" || !strings.HasPrefix(recorder.Body.String(), "data: ") {
		t.Fatalf("%v", recorder.Header())
	}
}

func TestTheStylesheetIsKeptForAYearAndAnUnknownAddressIsNotAnswered(t *testing.T) {
	site := serving(t)
	page := site.get("/nb/default")
	at := strings.Index(page.body, `href="/a/`)
	if at < 0 {
		t.Fatal(page.body)
	}
	href := page.body[at+len(`href="`):]
	href = href[:strings.IndexByte(href, '"')]
	got := site.get(href)
	if got.status != 200 || got.header.Get("Cache-Control") != "public, max-age=31536000, immutable" ||
		got.header.Get("X-Content-Type-Options") != "nosniff" {
		t.Fatalf("%d %v", got.status, got.header)
	}
	if stale := site.get("/a/style.0000000000.css"); stale.status != 404 || stale.body != "no such asset\n" {
		t.Fatalf("%d %q", stale.status, stale.body)
	}
}

func TestTheTagsAndTodoScreensAndBacklinksAnswer(t *testing.T) {
	site := serving(t)
	for _, path := range []string{"/nb/default/tags", "/nb/default/todo", "/nb/default/status"} {
		if got := site.get(path); got.status != 200 {
			t.Errorf("%s: %d", path, got.status)
		}
	}
	if got := site.get("/nb/default/tags"); !got.says("24.04 Dark patterns") || !got.says("work") {
		t.Fatal(got.body)
	}
	id := site.idOf("the-rack")
	if got := site.get("/nb/default/n/" + id + "/backlinks"); got.status != 200 {
		t.Fatalf("%d", got.status)
	}
	if got := site.get("/nb/default/f/rack.png/backlinks"); got.status != 200 || !got.says("The rack") {
		t.Fatalf("%d %s", got.status, got.body)
	}
	if got := site.get("/nb/default/f/..%2fx/backlinks"); got.status != 404 {
		t.Fatalf("%d", got.status)
	}
}

func TestAPushWithNowhereToSendItSaysSoAndNothingIsCalledFetch(t *testing.T) {
	site := serving(t)
	if got := site.send("POST", "/nb/default/status/fetch", nil, ""); got.status != 404 || !got.says("No such errand") {
		t.Fatalf("%d", got.status)
	}
	got := site.send("POST", "/nb/default/status/push", nil, "")
	if got.status != 303 || got.header.Get("Location") != "/nb/default/status" {
		t.Fatalf("%d %v", got.status, got.header)
	}
	site.web.errands.Settle()
	report, ok := site.web.errands.Report("default")
	if !ok || report.Outcome == nil || !report.Outcome.Failed {
		t.Fatalf("%+v", report)
	}
	if status := site.get("/nb/default/status"); !status.says("Push failed") {
		t.Fatal(status.body)
	}
}

func TestAnotherSiteCannotStartAnErrand(t *testing.T) {
	got := serving(t).send("POST", "/nb/default/status/sync", map[string]string{"Origin": "https://elsewhere.example"}, "")
	if got.status != 403 {
		t.Fatalf("%d", got.status)
	}
}

func TestAParameterIsDecodedAsAFormSendsIt(t *testing.T) {
	for query, want := range map[string]string{
		"q=budget": "budget", "a=1&q=budget&b=2": "budget", "a=1": "", "q=": "",
		"q=tag%3A%2224.04+Dark+patterns%22": `tag:"24.04 Dark patterns"`,
		"q=%E7%AD%86%E8%A8%98":              "筆記", "q=100%+of+it": "100% of it", "q=%zz": "%zz",
	} {
		if got := parameter(query, "q"); got != want {
			t.Errorf("%s: %q", query, got)
		}
	}
	if got := parameters("keep=a&keep=b+c&saw=d", "keep"); len(got) != 2 || got[1] != "b c" {
		t.Fatalf("%v", got)
	}
}

func TestTheServerTimesOutSlowHeadersButNeverCutsAStream(t *testing.T) {
	server := newHTTPServer(http.NotFoundHandler(), &silentConns{})
	if server.ReadHeaderTimeout != 15*time.Second {
		t.Fatalf("ReadHeaderTimeout is %v", server.ReadHeaderTimeout)
	}
	if server.WriteTimeout != 0 {
		t.Fatalf("WriteTimeout is %v, which cuts a watch", server.WriteTimeout)
	}
}

func TestTheNoteArrivesWithTheNotesThatLinkToIt(t *testing.T) {
	site := serving(t)
	id := site.idOf("budget-review")
	if _, err := cmd.Add(site.env, cmd.AddOptions{Title: str("Pointing back"), Content: str("see [the budget](" + id + "-budget-review.md)")}); err != nil {
		t.Fatal(err)
	}
	want := `<section class="beside"><h2>Linked from <span>1</span></h2>`
	for name, got := range map[string]reply{
		"the page": site.get("/nb/default/n/" + id),
		"the swap": site.request("/nb/default/n/"+id, map[string]string{fragmentHeader: "read"}),
	} {
		if !got.says(want) || !got.says(">Pointing back</a>") {
			t.Errorf("%s does not say what links here:\n%s", name, got.body)
		}
	}
	if swap := site.request("/nb/default/n/"+id, map[string]string{fragmentHeader: "read"}); swap.says("<!doctype html>") {
		t.Fatal("a swap is not a whole page")
	}
}

// The backlinks screen stays for a file, which has no margin to put them in.
func TestTheBacklinksScreenIsAlwaysAWholePage(t *testing.T) {
	site := serving(t)
	for _, fragment := range []string{"", "rows", "read"} {
		got := site.request("/nb/default/f/rack.png/backlinks", map[string]string{fragmentHeader: fragment})
		if got.status != 200 || !got.says("<!doctype html>") || !got.says(`<h1 class="place">Linked from</h1>`) {
			t.Errorf("fragment %q: %d %s", fragment, got.status, got.body)
		}
	}
}

func inZone(t *testing.T, env cmd.Env, tz string) cmd.Env {
	t.Helper()
	isolated := gittest.Isolate(t)
	isolated["TZ"] = tz
	git, err := gitx.New(gitx.Options{Env: isolated})
	if err != nil {
		t.Fatal(err)
	}
	env.Git = git
	return env
}

// aNoteUpdatedAt writes the file itself, as no command lets the clock be set.
func aNoteUpdatedAt(t *testing.T, env cmd.Env, stamp string) {
	t.Helper()
	text := "---\ntitle: Late night\ncreated: " + stamp + "\nupdated: " + stamp + "\n---\n\nbody\n"
	path := filepath.Join(env.Paths.NotebookDir(config.DefaultNotebook), "04kmn123-late-night.md")
	if err := os.WriteFile(path, []byte(text), 0o666); err != nil {
		t.Fatal(err)
	}
}

// A month heading is the one thing a listing says that a stamp's own day does
// not, so the zone it is cut in is the server's, as `noda todo` judges today.
func TestAMonthHeadingIsCutInTheServersZone(t *testing.T) {
	for _, c := range []struct{ name, tz, heading string }{
		{"UTC keeps it in March", "", "March 2024"},
		{"eight hours east is already April", "CST-8", "April 2024"},
		{"five hours west is still March", "EST5", "March 2024"},
	} {
		t.Run(c.name, func(t *testing.T) {
			env := aNotebook(t)
			aNoteUpdatedAt(t, env, "2024-03-31T23:30:00Z")
			site := servingOn(t, inZone(t, env, c.tz))
			got := site.get("/nb/default?sort=updated")
			if !got.says("<h2>" + c.heading + "</h2>") {
				t.Fatal(got.body)
			}
			if got.says("<h2>Notes</h2>") {
				t.Fatal("a date order has no run for the undated")
			}
		})
	}
}

func TestOnlyTheDateOrdersAreCutByMonth(t *testing.T) {
	env := aNotebook(t)
	aNoteUpdatedAt(t, env, "2024-03-31T23:30:00Z")
	site := servingOn(t, inZone(t, env, ""))
	for _, order := range []string{"", "?sort=title", "?sort=slug"} {
		got := site.get("/nb/default" + order)
		if !got.says(`<section class="run"><h2>Notes</h2>`) || got.says("<h2>March 2024</h2>") {
			t.Errorf("order %q:\n%s", order, got.body)
		}
	}
	if got := site.get("/nb/default?sort=created"); !got.says("<h2>March 2024</h2>") {
		t.Fatal(got.body)
	}
}
