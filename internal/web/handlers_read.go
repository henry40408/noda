package web

import (
	"fmt"
	"net/http"
	"os"
	"path/filepath"
	"slices"

	"github.com/henry40408/noda/internal/clock"
	"github.com/henry40408/noda/internal/cmd"
	"github.com/henry40408/noda/internal/link"
	"github.com/henry40408/noda/internal/note"
	"github.com/henry40408/noda/internal/notebook"
	"github.com/henry40408/noda/internal/query"
	"github.com/henry40408/noda/internal/todo"
)

// rowOf is a listing's row; by decides only the stamp. A page that is not a
// listing passes the default order and gets the updated stamp.
func rowOf(file notebook.NoteFile, by cmd.Sort) Row {
	stamp := file.Note.Updated
	if by == cmd.SortCreated {
		stamp = file.Note.Created
	}
	return Row{ID: file.ID, Title: file.Note.Title, Tags: file.Note.Tags, Stamp: stamp, Shown: true, Pinned: file.Note.IsPinned()}
}

func rowsOf(files []notebook.NoteFile) []Row {
	rows := make([]Row, len(files))
	for i, file := range files {
		rows[i] = rowOf(file, cmd.SortSlug)
	}
	return rows
}

// aroundOf is the notebook's named files, which opens nothing: rendering one
// note is not a reason to parse every other one.
func aroundOf(book string, opened *notebook.Notebook) (Around, error) {
	named, err := opened.NamedFiles()
	if err != nil {
		return Around{}, err
	}
	files := make([]Named, len(named))
	for i, each := range named {
		files[i] = Named{ID: each.ID, Slug: each.Slug}
	}
	return AroundOf(book, files), nil
}

// readNote parses a note's file, naming the file when it cannot be.
func readNote(opened *notebook.Notebook, id, slug string) (note.Note, string, error) {
	text, err := os.ReadFile(opened.NotePath(id, slug))
	if err != nil {
		return note.Note{}, "", err
	}
	parsed, err := note.Parse(string(text))
	if err != nil {
		return note.Note{}, "", fmt.Errorf("%s-%s.md: %w", id, slug, err)
	}
	return parsed, string(text), nil
}

// standing is the chip on a page: how far the notebook is from its remote,
// from two refs compared rather than status's two walks of the working tree.
func standing(opened *notebook.Notebook) (string, error) {
	branch, err := opened.Branch()
	if err != nil {
		return "", err
	}
	drift, err := opened.Drift(branch)
	if err != nil {
		return "", err
	}
	_, hasRemote := opened.RemoteURL()
	return cmd.Standing(hasRemote, drift), nil
}

func (s *Server) front(r *http.Request) (answer, error) {
	// Not an error: this page is useful before any notebook is chosen.
	active, _ := s.env.Paths.ActiveNotebook()
	names, err := notebook.List(s.env.Paths)
	if err != nil {
		return nil, err
	}
	books := make([]Book, 0, len(names))
	for _, name := range names {
		opened, err := notebook.Open(s.env.Git, s.env.Paths, name)
		if err != nil {
			return nil, err
		}
		opened = opened.WithContext(r.Context())
		status, err := opened.Status()
		if err != nil {
			return nil, err
		}
		seconds, offset, err := opened.LastCommit()
		if err != nil {
			return nil, err
		}
		book := Book{
			Active: active == name, Name: name, Notes: status.Notes, Files: status.Files,
			Uncommitted: status.Uncommitted, Last: clock.FormatTime(seconds, offset)[:clock.DateWidth],
		}
		// Nil: no remote, and the chip is not a link.
		if status.HasRemote {
			drift := cmd.Drifted(status.Drift)
			book.Drift = &drift
		}
		books = append(books, book)
	}
	return pageOf(Notebooks(books)), nil
}

// frontPage is the notebook's README.md rendered for the pane beside the
// listing; nil gets the invitation. Sent on every listing but drawn only above
// 1024px: a couple of kilobytes against hundreds of rows.
func frontPage(opened *notebook.Notebook, book string) (*string, error) {
	text, err := os.ReadFile(filepath.Join(opened.Path, notebook.ReadmeFile))
	if err != nil {
		return nil, nil
	}
	around, err := aroundOf(book, opened)
	if err != nil {
		return nil, err
	}
	rendered := Body(string(text), around)
	return &rendered, nil
}

func (s *Server) listing(r *http.Request) (answer, error) {
	book := r.PathValue("book")
	typed := parameter(r.URL.RawQuery, "q")
	// An unknown sort is the default rather than an error: links write it, so
	// anything else is a hand-edited address. r is a checkbox: present means
	// yes.
	sorted, _ := cmd.SortNamed(parameter(r.URL.RawQuery, "sort"))
	order := Order{Sort: sorted, Reversed: parameter(r.URL.RawQuery, "r") != ""}
	// A search swaps just the column; going back needs the front page too.
	column := fragmentIndex.wanted(r)
	screen := fragmentScreen.wanted(r)

	opened, found, err := s.openNotebook(r.Context(), book)
	if err != nil || !found {
		return orMissing(err, missingNotebook(book))
	}
	notes, err := opened.Notes()
	if err != nil {
		return nil, err
	}
	// ls's own sort and reversal, so the orders cannot drift apart.
	cmd.SortNotes(notes, order.Sort)
	if order.Reversed {
		for i, j := 0, len(notes)-1; i < j; i, j = i+1, j-1 {
			notes[i], notes[j] = notes[j], notes[i]
		}
	}
	drift, err := standing(opened)
	if err != nil {
		return nil, err
	}
	rows := make([]Row, len(notes))
	for i, file := range notes {
		rows[i] = rowOf(file, order.Sort)
	}
	var front *string
	if !column {
		if front, err = frontPage(opened, book); err != nil {
			return nil, err
		}
	}
	// The server's own zone, as `noda todo` judges "today" in: a month heading
	// is the one thing a listing says that a stamp's own day does not.
	tz, set := s.env.Git.Getenv("TZ")
	zone := clock.Location(tz, set)
	draw := func(asked Asked) string {
		list := Listing{Book: book, Rows: rows, Asked: asked, Order: order, Drift: drift, Zone: zone}
		switch {
		case column:
			return ListingPane(list)
		case screen:
			return ListingScreen(list, front)
		}
		return ListingPage(list, front)
	}

	// A query that does not parse says why and leaves every row shown. An empty
	// one is handled first, as Parse refuses it. Excluded rows are still sent,
	// hidden, so the script can widen a query too.
	tokens := query.Split(typed)
	if len(tokens) == 0 {
		return pageOf(draw(Asked{Typed: typed})), nil
	}
	parsed, err := query.Parse(tokens)
	if err != nil {
		problem := err.Error()
		return pageOf(draw(Asked{Typed: typed, Problem: &problem})), nil
	}
	for i, file := range notes {
		rows[i].Shown = parsed.Matches(file.ID, file.Note)
	}
	return pageOf(draw(Asked{Typed: typed, Grouping: parsed.Grouping(), Terms: parsed.ExcerptTerms()})), nil
}

// orMissing is the answer for an open that either failed or found nothing.
func orMissing(err error, none answer) (answer, error) {
	if err != nil {
		return nil, err
	}
	return none, nil
}

// located is the notebook open and the note found, or the answer to give
// instead: no notebook, no note, or a redirect to the id's address.
type located struct {
	opened *notebook.Notebook
	id     string
	slug   string
	other  answer
}

// about is the note a form is about, titled by what the file says now: the form's
// way back is named after the note, which a refused form's own fields (the
// title being typed, say) do not say.
func (at located) about() (About, error) {
	parsed, _, err := readNote(at.opened, at.id, at.slug)
	if err != nil {
		return About{}, err
	}
	return About{ID: at.id, Slug: at.slug, Title: parsed.Title}, nil
}

// aim finds the note a key names. tail is what follows the id in the address a
// slug or prefix redirects to.
func (s *Server) aim(r *http.Request, tail string) (located, error) {
	book, key := r.PathValue("book"), r.PathValue("key")
	opened, found, err := s.openNotebook(workContext(r), book)
	if err != nil {
		return located{}, err
	}
	if !found {
		return located{other: missingNotebook(book)}, nil
	}
	resolved, err := opened.Resolve(key)
	if err != nil {
		return located{other: missingNote(book, key)}, nil
	}
	if key != resolved.ID {
		return located{other: elsewhere("/nb/" + book + "/n/" + resolved.ID + tail)}, nil
	}
	return located{opened: opened, id: resolved.ID, slug: resolved.Slug}, nil
}

func (s *Server) reading(r *http.Request) (answer, error) {
	book := r.PathValue("book")
	part := fragmentRead.wanted(r)
	at, err := s.aim(r, "")
	if err != nil || at.other != nil {
		return orMissing(err, at.other)
	}
	parsed, _, err := readNote(at.opened, at.id, at.slug)
	if err != nil {
		return nil, err
	}
	around, err := aroundOf(book, at.opened)
	if err != nil {
		return nil, err
	}
	// Sent with the note, on the swap too: the margin is part of what the note is.
	linking, err := at.opened.BacklinksToNote(at.id)
	if err != nil {
		return nil, err
	}
	shown := Reading{
		ID: at.id, Slug: at.slug, Title: parsed.Title, Tags: parsed.Tags, Created: parsed.Created,
		Updated: parsed.Updated, Rendered: Body(parsed.Body, around), Pinned: parsed.IsPinned(),
		LinkedFrom: rowsOf(linking),
	}
	if part {
		return pageOf(NotePane(book, shown)), nil
	}
	// For the pill on the index pane, which is sent empty, and the rail.
	drift, err := standing(at.opened)
	if err != nil {
		return nil, err
	}
	return pageOf(NotePage(book, shown, drift)), nil
}

// files is everything the notebook holds that is not a note, with how many
// notes link to each: counted by link.Targets, as doctor --links counts
// orphans.
func (s *Server) files(r *http.Request) (answer, error) {
	book := r.PathValue("book")
	opened, found, err := s.openNotebook(r.Context(), book)
	if err != nil || !found {
		return orMissing(err, missingNotebook(book))
	}
	notes, names, err := opened.Inventory()
	if err != nil {
		return nil, err
	}
	used := map[string]int{}
	for _, file := range notes {
		for _, target := range link.Targets(file.Note.Body) {
			used[target]++
		}
	}
	rows := make([]Held, 0, len(names))
	for _, name := range names {
		var size uint64
		if info, err := os.Stat(filepath.Join(opened.Path, name)); err == nil {
			size = uint64(info.Size())
		}
		kind, _ := holding(name)
		rows = append(rows, Held{Name: name, Size: size, Kind: withoutCharset(kind), Used: used[name]})
	}
	drift, err := standing(opened)
	if err != nil {
		return nil, err
	}
	return pageOf(Files(book, rows, drift)), nil
}

func withoutCharset(kind string) string {
	for i := 0; i < len(kind); i++ {
		if kind[i] == ';' {
			return kind[:i]
		}
	}
	return kind
}

// tags is the tag tally, the TUI's tag list too.
func (s *Server) tags(r *http.Request) (answer, error) {
	book := r.PathValue("book")
	opened, found, err := s.openNotebook(r.Context(), book)
	if err != nil || !found {
		return orMissing(err, missingNotebook(book))
	}
	notes, err := opened.Notes()
	if err != nil {
		return nil, err
	}
	var tallies []Tally
	for _, count := range notebook.TagTally(notes) {
		tallies = append(tallies, Tally{Tag: count.Tag, Notes: count.Notes})
	}
	drift, err := standing(opened)
	if err != nil {
		return nil, err
	}
	return pageOf(Tags(book, tallies, drift)), nil
}

// todo is noda todo's list, with each note's title. Overdue is judged against
// the local date, as the CLI does.
func (s *Server) todo(r *http.Request) (answer, error) {
	book := r.PathValue("book")
	opened, found, err := s.openNotebook(r.Context(), book)
	if err != nil || !found {
		return orMissing(err, missingNotebook(book))
	}
	today := cmd.Today(s.env)
	notes, err := opened.Notes()
	if err != nil {
		return nil, err
	}
	type entry struct {
		id, slug, title string
		item            todo.Item
	}
	var entries []entry
	for _, file := range notes {
		for _, item := range todo.Items(file.Note.Body) {
			entries = append(entries, entry{file.ID, file.Slug, file.Note.Title, item})
		}
	}
	slices.SortStableFunc(entries, func(a, b entry) int {
		return todo.Compare(a.slug, a.item, b.slug, b.item)
	})
	tasks := make([]Task, len(entries))
	for i, each := range entries {
		tasks[i] = Task{ID: each.id, Title: each.title, Text: each.item.Text, Due: each.item.Due, Overdue: each.item.Overdue(today)}
	}
	drift, err := standing(opened)
	if err != nil {
		return nil, err
	}
	return pageOf(Todo(book, tasks, drift)), nil
}

// status touches no network, as noda status does not: drift is against the last
// fetch, and only the three errand buttons go out.
func (s *Server) status(r *http.Request) (answer, error) {
	book := r.PathValue("book")
	part := fragmentNews.wanted(r)
	opened, found, err := s.openNotebook(r.Context(), book)
	if err != nil || !found {
		return orMissing(err, missingNotebook(book))
	}
	status, err := opened.Status()
	if err != nil {
		return nil, err
	}
	var errand *Errand
	if report, ok := s.errands.Report(book); ok {
		errand = errandOf(report)
	}
	shown := Standing{
		Branch: status.Branch, Notes: status.Notes, Files: status.Files, Uncommitted: status.Uncommitted,
		Drift: cmd.Standing(status.HasRemote, status.Drift),
	}
	if status.HasRemote {
		remote := status.Remote
		shown.Remote = &remote
	}
	for _, group := range status.Problems {
		shown.Problems = append(shown.Problems, group.Kind.Describe(len(group.Subjects)))
	}
	if part {
		return pageOf(StandingMain(book, shown, errand)), nil
	}
	return pageOf(StandingPage(book, shown, errand)), nil
}

func errandOf(report Report) *Errand {
	failed := report.Outcome != nil && report.Outcome.Failed
	shown := &Errand{Doing: report.Job.Doing(), Done: report.Job.Done(), Failed: failed, Seconds: uint64(report.Took.Seconds())}
	if failed {
		shown.Done = report.Job.Stuck()
	}
	if report.Outcome != nil {
		said := report.Outcome.Said
		shown.Said = &said
	}
	return shown
}

// errand starts an errand and redirects to the status screen at once, so a
// reload is a GET and cannot start a second push. Pressing while one runs is
// not an error: the status screen answers it.
func (s *Server) errand(r *http.Request) (answer, error) {
	book, which := r.PathValue("book"), r.PathValue("errand")
	job, ok := JobOf(which)
	if !ok {
		return missing("No such errand", "There is nothing called "+which+" to do to a notebook."), nil
	}
	if !notebook.Exists(s.env.Paths, book) {
		return missingNotebook(book), nil
	}
	if s.errands.Begin(book, job) {
		// Its own goroutine, as no request waits on it. It opens its own
		// notebook and takes the write lock against a mid-commit fetch.
		go s.runErrand(book, job)
	}
	return elsewhere("/nb/" + encoded(book) + "/status"), nil
}

func (s *Server) runErrand(book string, job Job) {
	lock := s.writing.of(book)
	lock.Lock()
	defer lock.Unlock()
	opened, err := notebook.Open(s.env.Git, s.env.Paths, book)
	s.errands.Finish(book, Work(job, opened, err))
}

// noteBacklinks is the notes that link to a note, as noda backlinks: matched on
// the id, so it survives a retitle.
func (s *Server) noteBacklinks(r *http.Request) (answer, error) {
	book := r.PathValue("book")
	at, err := s.aim(r, "/backlinks")
	if err != nil || at.other != nil {
		return orMissing(err, at.other)
	}
	parsed, _, err := readNote(at.opened, at.id, at.slug)
	if err != nil {
		return nil, err
	}
	linking, err := at.opened.BacklinksToNote(at.id)
	if err != nil {
		return nil, err
	}
	subject := Subject{What: parsed.Title, At: "/nb/" + book + "/n/" + at.id}
	return backlinksAnswer(book, at.opened, subject, rowsOf(linking))
}

func backlinksAnswer(book string, opened *notebook.Notebook, subject Subject, rows []Row) (answer, error) {
	drift, err := standing(opened)
	if err != nil {
		return nil, err
	}
	return pageOf(Backlinks(book, subject, rows, drift)), nil
}

// heldName is the file a URL names, or nothing. It is the only place noda opens
// a path a reader named, so it goes through link.Target first: that is what
// makes ../../.ssh/id_rsa name nothing. A note is not a file.
func heldName(opened *notebook.Notebook, name string) (string, bool) {
	path, ok := link.Target(name)
	if !ok {
		return "", false
	}
	// Not by suffix: README.md is served, and NOTES.MD is an attachment.
	if note.NamesANote(path) {
		return "", false
	}
	info, err := os.Stat(filepath.Join(opened.Path, path))
	if err != nil || !info.Mode().IsRegular() {
		return "", false
	}
	return path, true
}

// fileBacklinks is the notes linking to a file, which has no id: what a file mv
// without --update-links would break.
func (s *Server) fileBacklinks(r *http.Request) (answer, error) {
	book, name := r.PathValue("book"), r.PathValue("name")
	opened, found, err := s.openNotebook(r.Context(), book)
	if err != nil || !found {
		return orMissing(err, missingNotebook(book))
	}
	path, ok := heldName(opened, name)
	if !ok {
		return missing("No such file", book+" holds no file called "+name+"."), nil
	}
	linking, err := opened.BacklinksToFile(path)
	if err != nil {
		return nil, err
	}
	subject := Subject{What: path, At: "/nb/" + encoded(book) + "/files", Mono: true}
	return backlinksAnswer(book, opened, subject, rowsOf(linking))
}

// heldFile serves a file the notebook holds. Never a note.
func (s *Server) heldFile(r *http.Request) (answer, error) {
	book, name := r.PathValue("book"), r.PathValue("name")
	opened, found, err := s.openNotebook(r.Context(), book)
	if err != nil || !found {
		return orMissing(err, missingNotebook(book))
	}
	path, ok := heldName(opened, name)
	if !ok {
		return missing("No such file", book+" holds no file called "+name+"."), nil
	}
	bytes, err := os.ReadFile(filepath.Join(opened.Path, path))
	if err != nil {
		return nil, err
	}
	kind, inline := holding(path)
	return attachment{bytes: bytes, kind: kind, inline: inline, name: path}.answer(), nil
}
