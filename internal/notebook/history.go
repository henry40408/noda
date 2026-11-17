package notebook

import (
	"fmt"
	"regexp"
	"sort"
	"strings"
	"unicode/utf8"

	"github.com/henry40408/noda/internal/gitx"
	"github.com/henry40408/noda/internal/note"
)

const (
	recordSep = "\x1e"
	fieldSep  = "\x1f"
	// shortLen is how much of an oid is shown; Go cuts it so that git's own
	// `core.abbrev` cannot change it.
	shortLen = 7
)

// Short is the abbreviated oid, or git's `0000000` for an empty one.
func Short(oid string) string {
	if oid == "" {
		return strings.Repeat("0", shortLen)
	}
	if len(oid) <= shortLen {
		return oid
	}
	return oid[:shortLen]
}

// Entry is one commit, as `noda log` reports it.
type Entry struct {
	ID string
	// Seconds comes with the offset, so a commit prints in the zone it was
	// written in.
	Seconds       int64
	OffsetMinutes int
	Summary       string
}

// ShortID is the abbreviated id.
func (e Entry) ShortID() string { return Short(e.ID) }

// Deleted is a note the notebook used to hold, with the name and title from the
// last commit that had it.
type Deleted struct {
	ID    string
	Slug  string
	Title string
	// RemovedIn is the commit that removed it.
	RemovedIn string
	// RestoreFrom is the last commit that still held it, for `restore`.
	RestoreFrom   string
	RemovedAt     int64
	OffsetMinutes int
}

// RestoreFromShort is the abbreviated RestoreFrom.
func (d Deleted) RestoreFromShort() string { return Short(d.RestoreFrom) }

const logFormat = "--format=" + recordSep + "%H" + fieldSep + "%P" + fieldSep + "%cd" + fieldSep + "%s"

// idPathspec matches every file a note has had: the id stays and the slug
// changes, so the log follows a retitle by id rather than by content.
// A glob only; an id holds nothing a glob would read as magic.
var idPattern = regexp.MustCompile(`^[0-9A-Za-z]+$`)

func idPathspec(id string) (string, error) {
	if !idPattern.MatchString(id) {
		return "", fmt.Errorf("invalid note id: %s", id)
	}
	return ":(glob,icase)" + id + "-*.md", nil
}

type logRecord struct {
	entry   Entry
	parents []string
}

func parseLog(output string) ([]logRecord, error) {
	var records []logRecord
	for _, text := range strings.Split(output, recordSep) {
		text = strings.TrimSuffix(text, "\n")
		if text == "" {
			continue
		}
		record, err := parseLogRecord(text)
		if err != nil {
			return nil, err
		}
		records = append(records, record)
	}
	return records, nil
}

func parseLogRecord(text string) (logRecord, error) {
	fields := strings.SplitN(text, fieldSep, 4)
	if len(fields) != 4 {
		return logRecord{}, fmt.Errorf("unexpected git log record %q", text)
	}
	seconds, offset, err := gitx.ParseRawDate(fields[2])
	if err != nil {
		return logRecord{}, err
	}
	return logRecord{
		entry:   Entry{ID: fields[0], Seconds: seconds, OffsetMinutes: offset, Summary: fields[3]},
		parents: strings.Fields(fields[1]),
	}, nil
}

// Log is the commits, newest first; with noteID, only those that changed that
// note. max of zero or less is no limit.
//
// `--date-order` is libgit2's time-then-topological: noda commits several times
// a second, so time alone leaves ties.
func (n *Notebook) Log(noteID string, max int) ([]Entry, error) {
	args := []string{"log", "--date-order", "--date=raw", logFormat}
	if max > 0 {
		args = append(args, fmt.Sprintf("--max-count=%d", max))
	}
	args = append(args, "HEAD", "--")
	if noteID != "" {
		pathspec, err := idPathspec(noteID)
		if err != nil {
			return nil, err
		}
		args = append(args, pathspec)
	}
	output, err := n.read(args...)
	if err != nil {
		return nil, err
	}
	records, err := parseLog(output)
	if err != nil {
		return nil, err
	}
	entries := make([]Entry, len(records))
	for i, record := range records {
		entries[i] = record.entry
	}
	return entries, nil
}

// changedFile is a path a commit touched, against its first parent.
type changedFile struct {
	status string
	name   string
}

type changeSet struct {
	logRecord
	changes []changedFile
}

// walkChanges lists every commit with what it changed against its first parent.
// First parent for a merge as well, so a merge is credited with what it
// carried in. Renames are off: a rename is a deletion and an addition, which is
// what comparing trees by filename gives.
func (n *Notebook) walkChanges(extra ...string) ([]changeSet, error) {
	args := []string{"log", "--date-order", "--date=raw", "--diff-merges=first-parent", "--name-status", "--no-renames", "--no-ext-diff", "-z", logFormat}
	args = append(args, extra...)
	args = append(args, "HEAD", "--")
	result, err := n.run(gitx.Cmd{Args: args, ReadOnly: true})
	if err != nil {
		return nil, err
	}
	var sets []changeSet
	for _, chunk := range strings.Split(string(result.Stdout), recordSep) {
		if chunk == "" {
			continue
		}
		tokens := strings.Split(chunk, "\x00")
		record, err := parseLogRecord(tokens[0])
		if err != nil {
			return nil, err
		}
		set := changeSet{logRecord: record}
		for i := 1; i+1 < len(tokens); i += 2 {
			set.changes = append(set.changes, changedFile{status: strings.TrimLeft(tokens[i], "\n"), name: tokens[i+1]})
		}
		sets = append(sets, set)
	}
	return sets, nil
}

// noteIDOf is the folded id of a root file that is named like a note.
func noteIDOf(name string) (string, bool) {
	if strings.Contains(name, "/") {
		return "", false
	}
	stem, isMarkdown := strings.CutSuffix(name, ".md")
	if !isMarkdown {
		return "", false
	}
	id, _, named := note.SplitStem(stem)
	if !named {
		return "", false
	}
	return note.NormalizeID(id), true
}

// LastChanged is when each note last changed according to git, by folded id, in
// one walk of history rather than a log per note.
func (n *Notebook) LastChanged() (map[string]int64, error) {
	sets, err := n.walkChanges()
	if err != nil {
		return nil, err
	}
	last := map[string]int64{}
	for _, set := range sets {
		for _, change := range set.changes {
			id, ok := noteIDOf(change.name)
			if !ok || change.status == "D" {
				continue
			}
			if _, seen := last[id]; !seen {
				last[id] = set.entry.Seconds
			}
		}
	}
	return last, nil
}

// Deleted is the notes history holds that the notebook no longer does: ids a
// commit's first parent had and the commit lacks, minus the ids on disk now. So
// a rename is not a deletion, a restored note is not reported, and `git rm`
// counts like `noda rm`. A full walk of history, newest first, so an id's first
// disappearance found is its last.
func (n *Notebook) Deleted() ([]Deleted, error) {
	present, err := n.TakenIDs()
	if err != nil {
		return nil, err
	}
	sets, err := n.walkChanges("--diff-filter=D")
	if err != nil {
		return nil, err
	}
	var found []Deleted
	seen := map[string]struct{}{}
	for _, set := range sets {
		if len(set.parents) == 0 || len(set.changes) == 0 {
			continue
		}
		kept, err := n.deletedIn(set, present, seen)
		if err != nil {
			return nil, err
		}
		found = append(found, kept...)
	}
	sort.SliceStable(found, func(i, j int) bool {
		if found[i].RemovedAt != found[j].RemovedAt {
			return found[i].RemovedAt > found[j].RemovedAt
		}
		return found[i].ID < found[j].ID
	})
	return found, nil
}

// deletedIn is what one commit removed that nothing else explains.
func (n *Notebook) deletedIn(set changeSet, present, seen map[string]struct{}) ([]Deleted, error) {
	var still map[string]struct{}
	var found []Deleted
	for _, change := range set.changes {
		id, ok := noteIDOf(change.name)
		if !ok {
			continue
		}
		if _, onDisk := present[id]; onDisk {
			continue
		}
		if _, already := seen[id]; already {
			continue
		}
		if still == nil {
			var err error
			if still, err = n.idsAt(set.entry.ID); err != nil {
				return nil, err
			}
		}
		if _, kept := still[id]; kept {
			continue
		}
		seen[id] = struct{}{}
		parent := set.parents[0]
		slug, text, held, err := n.NoteAt(parent, id)
		if err != nil || !held {
			return nil, err
		}
		title := ""
		if parsed, err := note.Parse(text); err == nil {
			title = parsed.Title
		}
		found = append(found, Deleted{
			ID: id, Slug: slug, Title: title,
			RemovedIn: set.entry.ID, RestoreFrom: parent,
			RemovedAt: set.entry.Seconds, OffsetMinutes: set.entry.OffsetMinutes,
		})
	}
	return found, nil
}

// treeEntry is one root entry of a commit's tree.
type treeEntry struct {
	oid  string
	name string
}

// rootBlobs is the files at the root of a commit, in git's tree order.
func (n *Notebook) rootBlobs(commit string) ([]treeEntry, error) {
	result, err := n.run(gitx.Cmd{Args: []string{"ls-tree", "-z", commit}, ReadOnly: true})
	if err != nil {
		return nil, err
	}
	var entries []treeEntry
	for _, record := range gitx.SplitNUL(result.Stdout) {
		meta, name, found := strings.Cut(record, "\t")
		fields := strings.Fields(meta)
		if !found || len(fields) != 3 || fields[1] != "blob" {
			continue
		}
		entries = append(entries, treeEntry{oid: fields[2], name: name})
	}
	return entries, nil
}

// idsAt is the folded ids a commit's root holds.
func (n *Notebook) idsAt(commit string) (map[string]struct{}, error) {
	entries, err := n.rootBlobs(commit)
	if err != nil {
		return nil, err
	}
	ids := map[string]struct{}{}
	for _, entry := range entries {
		if id, ok := noteIDOf(entry.name); ok {
			ids[id] = struct{}{}
		}
	}
	return ids, nil
}

// noteEntry is the file a note occupied at a commit, found by id.
func (n *Notebook) noteEntry(commit, id string) (treeEntry, bool, error) {
	entries, err := n.rootBlobs(commit)
	if err != nil {
		return treeEntry{}, false, err
	}
	wanted := note.NormalizeID(id)
	for _, entry := range entries {
		if found, ok := noteIDOf(entry.name); ok && found == wanted {
			return entry, true, nil
		}
	}
	return treeEntry{}, false, nil
}

// Revision is any revision git accepts, peeled to a commit's id.
func (n *Notebook) Revision(rev string) (string, error) {
	exists, err := n.run(gitx.Cmd{
		Args:     []string{"rev-parse", "--verify", "--quiet", "--end-of-options", rev},
		ReadOnly: true,
		OKCodes:  []int{0, 1},
	})
	if err != nil {
		return "", err
	}
	if exists.Code != 0 {
		return "", fmt.Errorf("unknown revision: %s — revspec '%s' not found", rev, rev)
	}
	peeled, err := n.run(gitx.Cmd{
		Args:     []string{"rev-parse", "--verify", "--quiet", "--end-of-options", rev + "^{commit}"},
		ReadOnly: true,
		OKCodes:  []int{0, 1},
	})
	if err != nil {
		return "", err
	}
	// The only failure left is a blob or a tree.
	if peeled.Code != 0 {
		return "", fmt.Errorf("`%s` is not a commit", rev)
	}
	return strings.TrimSpace(string(peeled.Stdout)), nil
}

// NoteAt is the slug and text of a note as it stood at commit. The bool is
// false when the commit held no such note.
func (n *Notebook) NoteAt(commit, id string) (slug, text string, held bool, err error) {
	entry, found, err := n.noteEntry(commit, id)
	if err != nil || !found {
		return "", "", false, err
	}
	content, err := n.blob(entry.oid)
	if err != nil {
		return "", "", false, err
	}
	slug = entry.name
	if stem, isMarkdown := strings.CutSuffix(entry.name, ".md"); isMarkdown {
		if _, split, named := note.SplitStem(stem); named {
			slug = split
		}
	}
	return slug, strings.ToValidUTF8(string(content), "�"), true, nil
}

// blob is an object's bytes.
func (n *Notebook) blob(oid string) ([]byte, error) {
	result, err := n.run(gitx.Cmd{Args: []string{"cat-file", "blob", oid}, ReadOnly: true})
	return result.Stdout, err
}

// BlobText is a blob's text, for the web layer's edit lock, which carries the
// base version's blob id. False is ordinary: an uncommitted note has no blob,
// and non-UTF-8 bytes are unusable as a base too.
func (n *Notebook) BlobText(oid string) (string, bool, error) {
	if !hexOID.MatchString(oid) {
		return "", false, nil
	}
	result, err := n.run(gitx.Cmd{Args: []string{"cat-file", "blob", oid}, ReadOnly: true, OKCodes: []int{0, 128}})
	if err != nil {
		return "", false, err
	}
	// A missing object is exit 128; so is anything else git cannot read, and
	// both mean there is no usable base.
	if result.Code != 0 {
		return "", false, nil
	}
	text := string(result.Stdout)
	if !utf8.ValidString(text) {
		return "", false, nil
	}
	return text, true, nil
}

var hexOID = regexp.MustCompile(`^[0-9a-f]{40}([0-9a-f]{24})?$`)

// IDAt is the id of a note at commit named by slug or id prefix, so a deleted
// note can still be named.
func (n *Notebook) IDAt(commit, key string) (string, bool, error) {
	entries, err := n.rootBlobs(commit)
	if err != nil {
		return "", false, err
	}
	wanted := note.NormalizeID(key)
	for _, entry := range entries {
		stem, isMarkdown := strings.CutSuffix(entry.name, ".md")
		if !isMarkdown {
			continue
		}
		id, slug, named := note.SplitStem(stem)
		if named && (slug == key || strings.HasPrefix(note.NormalizeID(id), wanted)) {
			return id, true, nil
		}
	}
	return "", false, nil
}
