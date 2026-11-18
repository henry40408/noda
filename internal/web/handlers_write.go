package web

import (
	"context"
	"io"
	"net/http"
	"strings"
	"unicode/utf8"

	"github.com/henry40408/noda/internal/cmd"
	"github.com/henry40408/noda/internal/gitx"
	"github.com/henry40408/noda/internal/note"
	"github.com/henry40408/noda/internal/query"
)

// formLimit is axum's default body limit, which the Rust server answered with.
const formLimit = 2 << 20

func (s *Server) addWriteRoutes(mux *http.ServeMux) {
	// GET shows the form and POST does it, at one address: a GET that changed
	// something would be a link a prefetcher could press.
	s.route(mux, "GET /nb/{book}/new", s.newForm)
	s.route(mux, "POST /nb/{book}/new", s.newNote)
	s.route(mux, "GET /nb/{book}/n/{key}/edit", s.editForm)
	s.route(mux, "POST /nb/{book}/n/{key}/edit", s.editNote)
	// Not through route: the answer is a stream, not a page.
	mux.Handle("GET /nb/{book}/n/{key}/watch", s.guarded(http.HandlerFunc(s.watch)))
	s.route(mux, "GET /nb/{book}/n/{key}/rename", s.renameForm)
	s.route(mux, "POST /nb/{book}/n/{key}/rename", s.renameNote)
	s.route(mux, "GET /nb/{book}/n/{key}/tags", s.tagsForm)
	s.route(mux, "POST /nb/{book}/n/{key}/tags", s.tagNote)
	// Two routes, not a toggle: a retried POST must be a no-op.
	s.route(mux, "POST /nb/{book}/n/{key}/pin", s.pinNote(true))
	s.route(mux, "POST /nb/{book}/n/{key}/unpin", s.pinNote(false))
	s.route(mux, "GET /nb/{book}/n/{key}/delete", s.deleteForm)
	s.route(mux, "POST /nb/{book}/n/{key}/delete", s.deleteNote)
}

// readForm is the body as text, or the refusal axum's String extractor gave.
func readForm(r *http.Request) (string, answer) {
	content, err := io.ReadAll(io.LimitReader(r.Body, formLimit+1))
	if err != nil {
		return "", plainRefusal(http.StatusBadRequest, "Failed to buffer the request body\n")
	}
	if len(content) > formLimit {
		return "", plainRefusal(http.StatusRequestEntityTooLarge, "length limit exceeded\n")
	}
	if !utf8.Valid(content) {
		return "", plainRefusal(http.StatusBadRequest, "Request body didn't contain valid UTF-8\n")
	}
	return string(content), nil
}

func plainRefusal(status int, text string) answer {
	return func(w http.ResponseWriter) { writePlain(w, status, text) }
}

func backToNote(book, id string) answer { return elsewhere("/nb/" + book + "/n/" + id) }

func (s *Server) newForm(r *http.Request) (answer, error) {
	book := r.PathValue("book")
	_, found, err := s.openNotebook(r.Context(), book)
	if err != nil || !found {
		return orMissing(err, missingNotebook(book))
	}
	return pageOf(Composing(book, Draft{}, nil)), nil
}

func (s *Server) newNote(r *http.Request) (answer, error) {
	form, refused := readForm(r)
	if refused != nil {
		return refused, nil
	}
	book := r.PathValue("book")
	opened, found, err := s.openNotebook(workContext(r), book)
	if err != nil || !found {
		return orMissing(err, missingNotebook(book))
	}
	draft := Draft{Title: parameter(form, "title"), Tags: parameter(form, "tags"), Body: parameter(form, "body")}
	var title *string
	if trimmed := strings.TrimSpace(draft.Title); trimmed != "" {
		title = &trimmed
	}

	lock := s.writing.of(book)
	lock.Lock()
	defer lock.Unlock()
	// Diffed rather than parsed out of add's prose.
	before, err := opened.TakenIDs()
	if err != nil {
		return nil, err
	}
	options := cmd.AddOptions{Title: title, Content: &draft.Body, Tags: query.Split(draft.Tags)}
	if _, err := cmd.AddIn(opened, options); err != nil {
		problem := err.Error()
		return pageOf(Composing(book, draft, &problem)), nil
	}
	after, err := opened.TakenIDs()
	if err != nil {
		return nil, err
	}
	for id := range after {
		if _, old := before[id]; !old {
			return backToNote(book, id), nil
		}
	}
	// Removed again between the two reads.
	return elsewhere("/nb/" + book), nil
}

func (s *Server) editForm(r *http.Request) (answer, error) {
	at, err := s.aim(r, "/edit")
	if err != nil || at.other != nil {
		return orMissing(err, at.other)
	}
	parsed, _, err := readNote(at.opened, at.id, at.slug)
	if err != nil {
		return nil, err
	}
	was, err := blobID(at.opened.NotePath(at.id, at.slug))
	if err != nil {
		return nil, err
	}
	return pageOf(Editing(Edit{
		Book: r.PathValue("book"), About: About{ID: at.id, Slug: at.slug, Title: parsed.Title}, Body: parsed.Body, Was: was,
	})), nil
}

func (s *Server) editNote(r *http.Request) (answer, error) {
	form, refused := readForm(r)
	if refused != nil {
		return refused, nil
	}
	book := r.PathValue("book")
	at, err := s.aim(r, "/edit")
	if err != nil || at.other != nil {
		return orMissing(err, at.other)
	}
	body, was := parameter(form, "body"), parameter(form, "fingerprint")

	lock := s.writing.of(book)
	lock.Lock()
	defer lock.Unlock()
	path := at.opened.NotePath(at.id, at.slug)
	now, err := blobID(path)
	if err != nil {
		return nil, err
	}
	if now != was {
		settled, stop, err := s.settleEdit(r.Context(), at, book, editClash{mine: body, was: was, now: now})
		if err != nil || stop != nil {
			return stop, err
		}
		body = settled
	}
	if _, err := cmd.RewriteIn(at.opened, at.id, body, cmd.TouchStamp); err != nil {
		problem := err.Error()
		return pageOf(Editing(Edit{
			Book: book, About: About{ID: at.id, Slug: at.slug}, Body: body, Was: now, Problem: &problem,
		})), nil
	}
	return backToNote(book, at.id), nil
}

type editClash struct{ mine, was, now string }

// settleEdit is for a note that changed since the form was drawn. It returns
// the body to save when the two edits merge cleanly, or the page to show
// instead. The fingerprint is a blob id, so it also finds the merge base.
//
// Bodies only: merging frontmatter would make somebody else's tag change a
// conflict over a line the reader never saw, and RewriteIn keeps whatever
// frontmatter is on disk anyway.
func (s *Server) settleEdit(ctx context.Context, at located, book string, clash editClash) (string, answer, error) {
	theirs, _, err := readNote(at.opened, at.id, at.slug)
	if err != nil {
		return "", nil, err
	}
	about := About{ID: at.id, Slug: at.slug, Title: theirs.Title}
	baseText, found, err := at.opened.BlobText(clash.was)
	if err != nil {
		return "", nil, err
	}
	base, parsed := baseBody(baseText, found)
	if !parsed {
		// No base (the note was never committed): write nothing, and show both
		// versions so neither is lost.
		return "", pageOf(Clashed(Clash{Book: book, About: about, Theirs: theirs.Body, Mine: clash.mine, Now: clash.now})), nil
	}
	merged, err := gitx.MergeFile(ctx, s.env.Git, gitx.MergeInput{
		Base: base, Ours: clash.mine, Theirs: theirs.Body, OursLabel: "what you wrote", TheirsLabel: "saved since",
	})
	if err != nil {
		return "", nil, err
	}
	if !merged.Clean {
		return "", pageOf(Conflicted(book, about, merged.Text, clash.now)), nil
	}
	// A clean merge is saved without asking; git keeps both sides.
	return merged.Text, nil, nil
}

func (s *Server) renameForm(r *http.Request) (answer, error) {
	at, err := s.aim(r, "/rename")
	if err != nil || at.other != nil {
		return orMissing(err, at.other)
	}
	parsed, _, err := readNote(at.opened, at.id, at.slug)
	if err != nil {
		return nil, err
	}
	return pageOf(Renaming(r.PathValue("book"), About{ID: at.id, Slug: at.slug, Title: parsed.Title}, parsed.Title, nil)), nil
}

func (s *Server) renameNote(r *http.Request) (answer, error) {
	form, refused := readForm(r)
	if refused != nil {
		return refused, nil
	}
	book := r.PathValue("book")
	at, err := s.aim(r, "/rename")
	if err != nil || at.other != nil {
		return orMissing(err, at.other)
	}
	title := parameter(form, "title")
	lock := s.writing.of(book)
	lock.Lock()
	defer lock.Unlock()
	// As the TUI's m calls it: --update-links must be asked for, and this form
	// has no way to.
	_, err = cmd.MvIn(at.opened, cmd.Rename{Key: at.id, NewTitle: title, UpdateLinks: false, Touch: cmd.TouchStamp})
	if err != nil {
		problem := err.Error()
		return pageOf(Renaming(book, About{ID: at.id, Slug: at.slug, Title: title}, title, &problem)), nil
	}
	return backToNote(book, at.id), nil
}

func (s *Server) tagsForm(r *http.Request) (answer, error) {
	at, err := s.aim(r, "/tags")
	if err != nil || at.other != nil {
		return orMissing(err, at.other)
	}
	parsed, _, err := readNote(at.opened, at.id, at.slug)
	if err != nil {
		return nil, err
	}
	return pageOf(Tagging(r.PathValue("book"), About{ID: at.id, Slug: at.slug, Title: parsed.Title}, parsed.Tags, nil)), nil
}

func (s *Server) tagNote(r *http.Request) (answer, error) {
	form, refused := readForm(r)
	if refused != nil {
		return refused, nil
	}
	book := r.PathValue("book")
	at, err := s.aim(r, "/tags")
	if err != nil || at.other != nil {
		return orMissing(err, at.other)
	}
	parsed, _, err := readNote(at.opened, at.id, at.slug)
	if err != nil {
		return nil, err
	}
	changes := tagChanges(form)
	if len(changes) == 0 {
		return backToNote(book, at.id), nil
	}
	lock := s.writing.of(book)
	lock.Lock()
	defer lock.Unlock()
	if _, err := cmd.TagIn(at.opened, at.id, changes, cmd.TouchStamp); err != nil {
		problem := err.Error()
		return pageOf(Tagging(book, About{ID: at.id, Slug: at.slug, Title: parsed.Title}, parsed.Tags, &problem)), nil
	}
	return backToNote(book, at.id), nil
}

// tagChanges: removed = offered (saw) minus still ticked (keep). Against what
// the page offered, not the file, so a tag added elsewhere since the page was
// drawn is not removed as if unticked.
func tagChanges(form string) []string {
	kept := map[string]bool{}
	for _, tag := range parameters(form, "keep") {
		kept[tag] = true
	}
	var changes []string
	for _, tag := range parameters(form, "saw") {
		if !kept[tag] {
			changes = append(changes, "-"+tag)
		}
	}
	for _, added := range query.Split(parameter(form, "add")) {
		changes = append(changes, "+"+added)
	}
	return changes
}

// pinNote has no form in between: there is nothing to fill in.
func (s *Server) pinNote(pinned bool) func(*http.Request) (answer, error) {
	tail := "/unpin"
	if pinned {
		tail = "/pin"
	}
	return func(r *http.Request) (answer, error) {
		book := r.PathValue("book")
		at, err := s.aim(r, tail)
		if err != nil || at.other != nil {
			return orMissing(err, at.other)
		}
		lock := s.writing.of(book)
		lock.Lock()
		defer lock.Unlock()
		if _, err := cmd.PinIn(at.opened, at.id, pinned, cmd.TouchStamp); err != nil {
			return nil, err
		}
		return backToNote(book, at.id), nil
	}
}

func (s *Server) deleteForm(r *http.Request) (answer, error) {
	at, err := s.aim(r, "/delete")
	if err != nil || at.other != nil {
		return orMissing(err, at.other)
	}
	parsed, _, err := readNote(at.opened, at.id, at.slug)
	if err != nil {
		return nil, err
	}
	return pageOf(Deleting(r.PathValue("book"), About{ID: at.id, Slug: at.slug, Title: parsed.Title})), nil
}

func (s *Server) deleteNote(r *http.Request) (answer, error) {
	book := r.PathValue("book")
	at, err := s.aim(r, "/delete")
	if err != nil || at.other != nil {
		return orMissing(err, at.other)
	}
	lock := s.writing.of(book)
	lock.Lock()
	defer lock.Unlock()
	if _, err := cmd.RmIn(at.opened, at.id); err != nil {
		return nil, err
	}
	return elsewhere("/nb/" + book), nil
}

// baseBody is the body of the version the edit began from, when git has it and
// it still parses.
func baseBody(text string, found bool) (string, bool) {
	if !found {
		return "", false
	}
	parsed, err := note.Parse(text)
	if err != nil {
		return "", false
	}
	return parsed.Body, true
}
