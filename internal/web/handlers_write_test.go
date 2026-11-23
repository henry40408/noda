package web

import (
	"bufio"
	"context"
	"net/http"
	"net/http/httptest"
	"net/url"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/henry40408/noda/internal/cmd"
	"github.com/henry40408/noda/internal/config"
)

func (s *site) post(path string, pairs ...string) reply {
	form := url.Values{}
	for i := 0; i+1 < len(pairs); i += 2 {
		form.Add(pairs[i], pairs[i+1])
	}
	return s.send("POST", path, map[string]string{"Content-Type": "application/x-www-form-urlencoded"}, form.Encode())
}

func (s *site) fingerprintOn(path string) string {
	s.t.Helper()
	body := s.get(path).body
	const marker = `name="fingerprint" value="`
	at := strings.Index(body, marker)
	if at < 0 {
		s.t.Fatalf("no fingerprint on %s:\n%s", path, body)
	}
	rest := body[at+len(marker):]
	return rest[:strings.IndexByte(rest, '"')]
}

func (s *site) notePath(slug string) string {
	return filepath.Join(s.env.Paths.NotebookDir(config.DefaultNotebook), s.idOf(slug)+"-"+slug+".md")
}

func TestANoteCanBeWrittenFromTheBrowser(t *testing.T) {
	site := serving(t)
	made := site.post("/nb/default/new", "title", "From the phone", "tags", "web ops", "body", "first line\r\nsecond line")
	at := made.header.Get("Location")
	if made.status != 303 || !strings.HasPrefix(at, "/nb/default/n/") {
		t.Fatalf("%d %q", made.status, at)
	}
	if note := site.get(at); !note.says("From the phone") || !note.says("second line") {
		t.Fatal(note.body)
	}
	written, err := os.ReadFile(site.notePath("from-the-phone"))
	if err != nil || strings.Contains(string(written), "\r") {
		t.Fatalf("%v %q", err, written)
	}
}

func TestARefusedNewNoteComesBackWithWhatWasTyped(t *testing.T) {
	site := serving(t)
	got := site.post("/nb/default/new", "title", "", "tags", "keep", "body", "")
	if got.status != 200 || !got.says(`value="keep"`) || !got.says("<form") {
		t.Fatalf("%d %s", got.status, got.body)
	}
}

func TestAnEditThatOverlapsOneSavedSinceComesBackToBeSettled(t *testing.T) {
	site := serving(t)
	id := site.idOf("budget-review")
	edit := "/nb/default/n/" + id + "/edit"
	stale := site.fingerprintOn(edit)
	if landed := site.post(edit, "fingerprint", stale, "body", "what the terminal wrote"); landed.status != 303 {
		t.Fatalf("%d", landed.status)
	}
	refused := site.post(edit, "fingerprint", stale, "body", "what the phone wrote")
	if refused.status != 200 || !refused.says("Someone else saved while you were writing") ||
		!refused.says("&lt;&lt;&lt;&lt;&lt;&lt;&lt; what you wrote") ||
		!refused.says("what the terminal wrote") || !refused.says("what the phone wrote") {
		t.Fatalf("%d %s", refused.status, refused.body)
	}
	onDisk := site.get("/nb/default/n/" + id)
	if !onDisk.says("what the terminal wrote") || onDisk.says("what the phone wrote") {
		t.Fatal(onDisk.body)
	}
}

func TestTwoEditsInDifferentPartsOfANoteAreMergedAndBothSurvive(t *testing.T) {
	site := serving(t)
	if _, err := cmd.Add(site.env, cmd.AddOptions{
		Title: str("Release checklist"), Content: str("one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nnine\nten"),
	}); err != nil {
		t.Fatal(err)
	}
	id := site.idOf("release-checklist")
	edit := "/nb/default/n/" + id + "/edit"
	base := site.fingerprintOn(edit)
	if landed := site.post(edit, "fingerprint", base, "body", "TERMINAL\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nnine\nten"); landed.status != 303 {
		t.Fatalf("%d", landed.status)
	}
	saved := site.post(edit, "fingerprint", base, "body", "one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nnine\nPHONE")
	if saved.status != 303 {
		t.Fatalf("a merge that holds is a save, not a page:\n%s", saved.body)
	}
	onDisk := site.get("/nb/default/n/" + id)
	if !onDisk.says("TERMINAL") || !onDisk.says("PHONE") {
		t.Fatal(onDisk.body)
	}
}

func TestAClashWithNoCommittedVersionToMergeFromHandsBackBoth(t *testing.T) {
	site := serving(t)
	id := site.idOf("reading-list")
	path := site.notePath("reading-list")
	held, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(path, []byte(strings.Replace(string(held), "a book", "a book, uncommitted", 1)), 0o666); err != nil {
		t.Fatal(err)
	}
	edit := "/nb/default/n/" + id + "/edit"
	stale := site.fingerprintOn(edit)
	if landed := site.post(edit, "fingerprint", stale, "body", "what the terminal wrote"); landed.status != 303 {
		t.Fatalf("%d", landed.status)
	}
	refused := site.post(edit, "fingerprint", stale, "body", "what the phone wrote")
	if refused.status != 200 || !refused.says("This note changed while you were writing") ||
		!refused.says("what the terminal wrote") || !refused.says("what the phone wrote") {
		t.Fatalf("%d %s", refused.status, refused.body)
	}
}

func TestAnEditWithNoFingerprintIsNeverWrittenBlind(t *testing.T) {
	site := serving(t)
	id := site.idOf("budget-review")
	got := site.post("/nb/default/n/"+id+"/edit", "body", "blind")
	if got.status != 200 || !got.says("This note changed while you were writing") {
		t.Fatalf("%d %s", got.status, got.body)
	}
	if site.get("/nb/default/n/" + id).says("blind") {
		t.Fatal("written blind")
	}
}

func TestOnlyTheFormsThatCarryAFingerprintListen(t *testing.T) {
	site := serving(t)
	id := site.idOf("budget-review")
	hook := site.watchHook()
	if !site.get("/nb/default/n/" + id + "/edit").says(hook) {
		t.Fatal("the editor does not listen")
	}
	for _, tail := range []string{"rename", "tags", "delete"} {
		if site.get("/nb/default/n/" + id + "/" + tail).says(hook) {
			t.Errorf("/%s listens and need not", tail)
		}
	}
}

func (s *site) watchHook() string {
	s.t.Helper()
	body := s.get("/nb/default/n/" + s.idOf("budget-review") + "/edit").body
	at := strings.Index(body, `src="/a/watching.`)
	if at < 0 {
		s.t.Fatal(body)
	}
	rest := body[at+len(`src="`):]
	return rest[:strings.IndexByte(rest, '"')]
}

func TestSavingTagsFromAStalePageDoesNotRemoveATagAddedSince(t *testing.T) {
	site := serving(t)
	id := site.idOf("budget-review")
	form := site.get("/nb/default/n/" + id + "/tags")
	if !form.says(`name="saw" value="work"`) {
		t.Fatal(form.body)
	}
	if _, err := cmd.Tag(site.env, id, []string{"+q3"}, cmd.TouchStamp); err != nil {
		t.Fatal(err)
	}
	if saved := site.post("/nb/default/n/"+id+"/tags", "saw", "work", "keep", "work", "add", "ops"); saved.status != 303 {
		t.Fatalf("%d", saved.status)
	}
	if note := site.get("/nb/default/n/" + id); !note.says(`<dd class="chips"><a class="tag" href="/nb/default?q=tag%3Awork">work</a><a class="tag" href="/nb/default?q=tag%3Aq3">q3</a><a class="tag" href="/nb/default?q=tag%3Aops">ops</a></dd>`) {
		t.Fatal(note.body)
	}
}

func TestUntickingATagOnThePageThatOfferedItRemovesIt(t *testing.T) {
	site := serving(t)
	id := site.idOf("budget-review")
	if saved := site.post("/nb/default/n/"+id+"/tags", "saw", "work"); saved.status != 303 {
		t.Fatalf("%d", saved.status)
	}
	if note := site.get("/nb/default/n/" + id); !note.says(`<span class="none">none</span>`) {
		t.Fatal(note.body)
	}
}

func TestTheAddFieldTakesMoreThanOneTagAndNothingToChangeIsASilentRedirect(t *testing.T) {
	site := serving(t)
	id := site.idOf("reading-list")
	saved := site.post("/nb/default/n/"+id+"/tags", "add", "alpha beta")
	if saved.status != 303 {
		t.Fatalf("%d", saved.status)
	}
	note := site.get("/nb/default/n/" + id)
	if !note.says("alpha") || !note.says("beta") {
		t.Fatal(note.body)
	}
	if again := site.post("/nb/default/n/" + id + "/tags"); again.status != 303 || again.header.Get("Location") != "/nb/default/n/"+id {
		t.Fatalf("%d", again.status)
	}
}

func TestANoteCanBeRenamedAndKeepsItsAddress(t *testing.T) {
	site := serving(t)
	id := site.idOf("budget-review")
	saved := site.post("/nb/default/n/"+id+"/rename", "title", "Budget review 2026")
	if saved.status != 303 || saved.header.Get("Location") != "/nb/default/n/"+id {
		t.Fatalf("%d %v", saved.status, saved.header)
	}
	note := site.get("/nb/default/n/" + id)
	if !note.says("Budget review 2026") || !note.says("</span>-budget-review-2026.md</div>") {
		t.Fatal(note.body)
	}
}

func TestARefusedChangeIsHandedBackWithTheReason(t *testing.T) {
	site := serving(t)
	id := site.idOf("budget-review")
	refused := site.post("/nb/default/n/"+id+"/rename", "title", "  ")
	if refused.status != 200 || !refused.says("a note needs a title") || !refused.says("<form") {
		t.Fatalf("%d %s", refused.status, refused.body)
	}
	// The way back is labelled with the note's own title, which the refused
	// form does not carry.
	if !refused.says(`aria-label="Back to Budget review"`) {
		t.Fatal(refused.body)
	}
}

// A note whose frontmatter was broken by hand since the form was drawn can be
// found by its address but not read. The refusal is still the answer, with the
// id where the title would be: a 500 would hide why the write was refused.
func TestARefusalForANoteThatCannotBeReadIsStillAForm(t *testing.T) {
	for _, change := range []struct{ route, field, value string }{
		{"/edit", "body", "new text"},
		{"/rename", "title", "A new title"},
	} {
		t.Run(change.route, func(t *testing.T) {
			site := serving(t)
			id := site.idOf("budget-review")
			path := site.notePath("budget-review")
			if err := os.WriteFile(path, []byte("no frontmatter any more\n"), 0o644); err != nil {
				t.Fatal(err)
			}
			// The form's own fingerprint, so an edit is not turned away as stale
			// before it is refused.
			fingerprint, err := blobID(path)
			if err != nil {
				t.Fatal(err)
			}
			refused := site.post("/nb/default/n/"+id+change.route, change.field, change.value, "fingerprint", fingerprint)
			if refused.status != 200 || !refused.says("<form") || !refused.says(`aria-label="Back to `+id+`"`) {
				t.Fatalf("%d %s", refused.status, refused.body)
			}
		})
	}
}

func TestANoteCanBeDeletedButAskingToDeleteOnlyAsks(t *testing.T) {
	site := serving(t)
	id := site.idOf("reading-list")
	if asked := site.get("/nb/default/n/" + id + "/delete"); asked.status != 200 || !asked.says("Delete") {
		t.Fatalf("%d", asked.status)
	}
	if site.get("/nb/default/n/"+id).status != 200 {
		t.Fatal("a GET deleted the note")
	}
	gone := site.post("/nb/default/n/" + id + "/delete")
	if gone.status != 303 || gone.header.Get("Location") != "/nb/default" || site.get("/nb/default/n/"+id).status != 404 {
		t.Fatalf("%d %v", gone.status, gone.header)
	}
	if site.get("/nb/default").says("Reading list") {
		t.Fatal("still listed")
	}
}

func TestTheBarPinsANoteAndThenOffersToLetItDown(t *testing.T) {
	site := serving(t)
	id := site.idOf("budget-review")
	at := "/nb/default/n/" + id
	if before := site.get(at); !before.says(">Pin</span>") || before.says(">Unpin</span>") {
		t.Fatal(before.body)
	}
	if site.post(at+"/pin").status != 303 || !site.get(at).says(">Unpin</span>") {
		t.Fatal("not pinned")
	}
	if !site.get("/nb/default").says(`<span class="pin">pinned</span>`) {
		t.Fatal("the listing does not show the pin")
	}
	// Idempotent: the route names the state it wants.
	if site.post(at+"/pin").status != 303 || !site.get(at).says(">Unpin</span>") {
		t.Fatal("a second pin changed it")
	}
	if site.post(at+"/unpin").status != 303 || site.get(at).says(">Unpin</span>") {
		t.Fatal("not unpinned")
	}
}

func TestAnotherSiteCannotWrite(t *testing.T) {
	site := serving(t)
	id := site.idOf("budget-review")
	foreign := map[string]string{"Origin": "https://elsewhere.example"}
	for _, path := range []string{"/nb/default/n/" + id + "/pin", "/nb/default/new", "/nb/default/n/" + id + "/delete", "/nb/default/n/" + id + "/edit"} {
		if got := site.send("POST", path, foreign, ""); got.status != 403 {
			t.Errorf("%s: %d", path, got.status)
		}
	}
	if site.get("/nb/default/n/"+id).says(">Unpin</span>") || site.get("/nb/default/n/"+id).status != 200 {
		t.Fatal("a refused request still wrote")
	}
}

func TestAFormBodyThatIsTooBigOrNotTextIsRefusedBeforeAnythingIsOpened(t *testing.T) {
	site := serving(t)
	big := site.send("POST", "/nb/ghost/new", nil, strings.Repeat("a", formLimit+1))
	if big.status != http.StatusRequestEntityTooLarge {
		t.Fatalf("%d", big.status)
	}
	bad := site.send("POST", "/nb/ghost/new", nil, "title=\xff\xfe")
	if bad.status != http.StatusBadRequest {
		t.Fatalf("%d", bad.status)
	}
	exact := site.send("POST", "/nb/ghost/new", nil, strings.Repeat("a", formLimit))
	if exact.status != http.StatusNotFound {
		t.Fatalf("%d", exact.status)
	}
}

func TestAWriteToAMissingNoteOrNotebookIsA404(t *testing.T) {
	site := serving(t)
	for _, path := range []string{"/nb/ghost/new", "/nb/default/n/zzzzzzzz/edit", "/nb/default/n/zzzzzzzz/delete", "/nb/default/n/zzzzzzzz/pin"} {
		if got := site.post(path, "title", "x"); got.status != 404 {
			t.Errorf("%s: %d", path, got.status)
		}
	}
}

func TestASlugInAWriteAddressRedirectsToTheIDWithTheSameTail(t *testing.T) {
	site := serving(t)
	id := site.idOf("budget-review")
	got := site.get("/nb/default/n/budget-review/edit")
	if got.status != 303 || got.header.Get("Location") != "/nb/default/n/"+id+"/edit" {
		t.Fatalf("%d %v", got.status, got.header)
	}
}

// hears reads the stream until text shows, so the test needs no parser for a
// body with no length.
func hears(t *testing.T, reader *bufio.Reader, text string) string {
	t.Helper()
	done := make(chan string, 1)
	go func() {
		var heard strings.Builder
		for {
			line, err := reader.ReadString('\n')
			heard.WriteString(line)
			if strings.Contains(heard.String(), text) || err != nil {
				done <- heard.String()
				return
			}
		}
	}()
	select {
	case heard := <-done:
		if !strings.Contains(heard, text) {
			t.Fatalf("the stream ended without %q:\n%s", text, heard)
		}
		return heard
	case <-time.After(15 * time.Second):
		t.Fatalf("nothing said %q", text)
		return ""
	}
}

func TestAnOpenEditorIsToldTheNoteMovedUnderItAndIsNotCompressed(t *testing.T) {
	site := serving(t)
	id := site.idOf("budget-review")
	was := site.fingerprintOn("/nb/default/n/" + id + "/edit")

	request, _ := http.NewRequest("GET", site.server.URL+"/nb/default/n/"+id+"/watch", nil)
	request.Header.Set("Accept-Encoding", "gzip")
	response, err := site.client.Do(request)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = response.Body.Close() }()
	if response.StatusCode != 200 || response.Header.Get("Content-Type") != "text/event-stream" ||
		response.Header.Get("Content-Encoding") != "" {
		t.Fatalf("%d %v", response.StatusCode, response.Header)
	}
	if saved := site.post("/nb/default/n/"+id+"/edit", "fingerprint", was, "body", "written somewhere else"); saved.status != 303 {
		t.Fatalf("%d", saved.status)
	}
	heard := hears(t, bufio.NewReader(response.Body), "data: ")
	if strings.Contains(heard, "data: "+was) {
		t.Fatalf("it said the note is at the fingerprint the form already holds:\n%s", heard)
	}
}

func TestAStopDoesNotWaitForAWatchThatNeverEnds(t *testing.T) {
	site := serving(t)
	id := site.idOf("budget-review")
	response, err := site.client.Get(site.server.URL + "/nb/default/n/" + id + "/watch")
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = response.Body.Close() }()
	site.web.watching.Stop()
	ended := make(chan struct{})
	go func() {
		_, _ = bufio.NewReader(response.Body).ReadString(0)
		close(ended)
	}()
	select {
	case <-ended:
	case <-time.After(10 * time.Second):
		t.Fatal("the stream outlived Stop")
	}
}

func TestAWatchEndsWhenItsReaderHangsUpAndStopThenFinishes(t *testing.T) {
	site := serving(t)
	id := site.idOf("budget-review")
	ctx, hangUp := context.WithCancel(context.Background())
	request, _ := http.NewRequestWithContext(ctx, "GET", site.server.URL+"/nb/default/n/"+id+"/watch", nil)
	response, err := site.client.Do(request)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = response.Body.Close() }()
	hangUp()
	time.Sleep(watchEvery + time.Second)
	site.web.watching.Stop()
	closed := make(chan struct{})
	go func() {
		site.server.CloseClientConnections()
		_ = site.server.Config.Shutdown(context.Background())
		close(closed)
	}()
	select {
	case <-closed:
	case <-time.After(10 * time.Second):
		t.Fatal("shutdown waited on a watch whose reader had gone")
	}
}

func TestASubscriptionWhoseReaderHasGoneIsClosedByTheWatcher(t *testing.T) {
	watch := NewWatch()
	defer watch.Stop()
	gone := make(chan struct{})
	changes := watch.Subscribe("default", "1", filepath.Join(t.TempDir(), "n.md"), "x", gone)
	close(gone)
	select {
	case _, open := <-changes:
		if open {
			t.Fatal("a value arrived")
		}
	case <-time.After(watchEvery + 5*time.Second):
		t.Fatal("the channel of a departed reader stayed open")
	}
}

func TestAWriteStartedByAReaderWhoHungUpStillCommits(t *testing.T) {
	site := serving(t)
	ctx, hangUp := context.WithCancel(context.Background())
	hangUp()
	form := url.Values{"title": {"Written anyway"}, "body": {"text"}}
	request := httptest.NewRequest("POST", "/nb/default/new", strings.NewReader(form.Encode())).WithContext(ctx)
	request.Header.Set("Content-Type", "application/x-www-form-urlencoded")
	request.Host = strings.TrimPrefix(site.server.URL, "http://")
	request.Header.Set("Origin", site.server.URL)
	recorded := httptest.NewRecorder()
	site.web.Handler().ServeHTTP(recorded, request)
	if recorded.Code != 303 {
		t.Fatalf("%d: %s", recorded.Code, recorded.Body)
	}
	if out, err := exec.Command("git", "-C", site.env.Paths.NotebookDir(config.DefaultNotebook), "status", "--porcelain").Output(); err != nil || len(out) != 0 {
		t.Fatalf("the note was written but not committed: %q %v", out, err)
	}
	if found := site.get("/nb/default"); !found.says("Written anyway") {
		t.Fatal("the note is not there")
	}
}
