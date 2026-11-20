package cmd

import (
	"cmp"
	"fmt"
	"os"
	"path/filepath"
	"slices"
	"strings"

	"github.com/henry40408/noda/internal/note"
	"github.com/henry40408/noda/internal/notebook"
	"github.com/henry40408/noda/internal/style"
)

// UnconvertedField is the frontmatter key an importer leaves on a note it could
// not finish.
const UnconvertedField = "unconverted"

// DoctorOptions are the flags of `doctor`. Links and Times are flags because
// they cost a read of every body and a walk of history.
type DoctorOptions struct {
	DryRun bool
	Links  bool
	Times  bool
}

// Doctor diagnoses what noda cannot act on, and adopts notes that only lack an
// id: the one repair that loses nothing. A shared id or a note-named file
// without frontmatter is reported and left to its author. Unlike `status`, it
// names every file.
func Doctor(e Env, options DoctorOptions) (string, error) {
	active, err := e.openActive()
	if err != nil {
		return "", err
	}
	scan, err := active.Scan()
	if err != nil {
		return "", err
	}
	problems := scan.Problems()
	extras, err := extraFindings(active, options)
	if err != nil {
		return "", err
	}

	if len(problems) == 0 {
		if extras == "" {
			return "the notebook is in order", nil
		}
		return extras, nil
	}

	var out strings.Builder
	for _, group := range problems {
		out.WriteString(group.Kind.Describe(len(group.Subjects)) + "\n")
		for _, subject := range group.Subjects {
			out.WriteString("  " + subject + "\n")
		}
	}
	for _, line := range advice(scan) {
		out.WriteString(line + "\n")
	}
	if extras != "" {
		out.WriteString(extras + "\n")
	}
	if len(scan.Unnamed) == 0 {
		return trimEnd(out.String()), nil
	}

	count := len(scan.Unnamed)
	noun := plural(count, "note", "notes")
	if options.DryRun {
		out.WriteString(style.Muted.Paint(fmt.Sprintf("would adopt %d %s — nothing was changed", count, noun)))
		return trimEnd(out.String()), nil
	}
	if err := adoptUnnamed(active, scan.Unnamed, fmt.Sprintf("doctor: adopt %d %s", count, noun)); err != nil {
		return "", err
	}
	fmt.Fprintf(&out, "adopted %d %s", count, noun)
	return trimEnd(out.String()), nil
}

// extraFindings is what `doctor` has to say beyond the scan's problems, one
// blank line between the sections.
func extraFindings(active *notebook.Notebook, options DoctorOptions) (string, error) {
	var sections []string
	if options.Links {
		audit, err := active.AuditLinks()
		if err != nil {
			return "", err
		}
		sections = append(sections, describeAudit(audit))
	}
	notes, err := active.Notes()
	if err != nil {
		return "", err
	}
	if options.Times {
		last, err := active.LastChanged()
		if err != nil {
			return "", err
		}
		sections = append(sections, describeTimes(notes, last))
	}
	// Always on: it parses no links and walks no history.
	sections = append(sections, describeUnconverted(notes))
	hooks, err := active.Hooks()
	if err != nil {
		return "", err
	}
	sections = append(sections, describeHooks(hooks))

	var present []string
	for _, section := range sections {
		if section != "" {
			present = append(present, section)
		}
	}
	return strings.Join(present, "\n"), nil
}

// adoptUnnamed gives each unnamed file an id, in one commit, so an unwanted
// repair is revertible.
func adoptUnnamed(active *notebook.Notebook, unnamed []string, message string) error {
	taken, err := active.TakenIDs()
	if err != nil {
		return err
	}
	var changed []string
	for _, file := range unnamed {
		id := note.MintID(taken)
		taken[note.NormalizeID(id)] = struct{}{}
		adopted := note.FileName(id, note.Slugify(strings.TrimSuffix(file, ".md")))
		if err := os.Rename(filepath.Join(active.Path, file), filepath.Join(active.Path, adopted)); err != nil {
			return err
		}
		changed = append(changed, file, adopted)
	}
	return active.Commit(changed, message)
}

// describeAudit reports orphaned files, stale links and broken links. Nothing
// is repaired: only the author knows whether an orphan or a broken link is
// intended, and fixing a stale link edits notes this command was not pointed
// at.
func describeAudit(audit notebook.Audit) string {
	var out strings.Builder
	if len(audit.Orphans) > 0 {
		// `links` either way: the subject is `no note`, always singular.
		fmt.Fprintf(&out, "%d %s no note links to\n", len(audit.Orphans), plural(len(audit.Orphans), "file", "files"))
		for _, file := range audit.Orphans {
			out.WriteString("  " + file + "\n")
		}
	}
	if len(audit.Stale) > 0 {
		fmt.Fprintf(&out, "%d stale %s\n", len(audit.Stale), plural(len(audit.Stale), "link", "links"))
		for _, stale := range audit.Stale {
			fmt.Fprintf(&out, "  %s %s %s\n    %s\n", stale.Note, style.Muted.Paint("->"), stale.Destination,
				style.Muted.Paint("now "+stale.Current))
		}
	}
	if len(audit.Broken) > 0 {
		fmt.Fprintf(&out, "%d broken %s\n", len(audit.Broken), plural(len(audit.Broken), "link", "links"))
		for _, broken := range audit.Broken {
			fmt.Fprintf(&out, "  %s %s %s\n", broken.Note, style.Muted.Paint("->"), broken.Destination)
		}
	}
	return trimEnd(out.String())
}

// commitLagSeconds is how long a commit may trail the `updated` noda wrote.
// noda writes and commits together, so this only covers a slow commit; an
// outside edit shows up far later.
const commitLagSeconds = 60

// describeTimes checks timestamps against themselves and against git. Nothing
// is repaired: the only fix would overwrite somebody's record with a guess.
func describeTimes(notes []notebook.NoteFile, last map[string]int64) string {
	var unreadable, reversed, stale []string
	for _, file := range notes {
		name := note.FileName(file.ID, file.Slug)
		for _, field := range []struct {
			name  string
			value *string
		}{{"created", file.Note.Created}, {"updated", file.Note.Updated}} {
			if _, ok := instant(field.value); field.value != nil && !ok {
				unreadable = append(unreadable, fmt.Sprintf("%s %s: %s", name, field.name, *field.value))
			}
		}

		created, createdOK := instant(file.Note.Created)
		updated, updatedOK := instant(file.Note.Updated)
		if createdOK && updatedOK && updated.Before(created) {
			reversed = append(reversed, name)
		}
		// git can only say the file changed later, which is all that is claimed.
		committed, held := last[note.NormalizeID(file.ID)]
		if updatedOK && held && committed-updated.Unix() > commitLagSeconds {
			stale = append(stale, name)
		}
	}
	return formatTimeFindings(unreadable, reversed, stale)
}

func formatTimeFindings(unreadable, reversed, stale []string) string {
	var out strings.Builder
	list := func(heading string, names []string) {
		if len(names) == 0 {
			return
		}
		out.WriteString(heading + "\n")
		for _, name := range names {
			out.WriteString("  " + name + "\n")
		}
	}
	list(fmt.Sprintf("%d %s cannot be read", len(unreadable), plural(len(unreadable), "time", "times")), unreadable)
	list(fmt.Sprintf("%d %s changed before being created", len(reversed), plural(len(reversed), "note", "notes")), reversed)
	verb := "were"
	if len(stale) == 1 {
		verb = "was"
	}
	list(fmt.Sprintf("%d %s %s changed outside noda", len(stale), plural(len(stale), "note", "notes"), verb), stale)
	if len(stale) > 0 {
		out.WriteString(style.Muted.Paint("git has a commit newer than the note's own `updated`") + "\n")
	}
	return trimEnd(out.String())
}

// unconverted is a note and what an importer left unfinished in it.
type unconverted struct{ file, what string }

// describeUnconverted names the notes whose `unconverted:` field says an
// importer could not finish them: the one place that field is surfaced, since
// `search` does not read it. Nothing is repaired: only the author knows what
// the WikiText should say.
func describeUnconverted(notes []notebook.NoteFile) string {
	prefix := UnconvertedField + ": "
	var found []unconverted
	for _, file := range notes {
		for _, line := range file.Note.Extra {
			if what, ok := strings.CutPrefix(line, prefix); ok {
				found = append(found, unconverted{note.FileName(file.ID, file.Slug), strings.TrimSpace(what)})
				break
			}
		}
	}
	if len(found) == 0 {
		return ""
	}
	// Ties are equal in both fields, so the unstable sort shows nothing.
	slices.SortFunc(found, func(a, b unconverted) int {
		return cmp.Or(cmp.Compare(a.file, b.file), cmp.Compare(a.what, b.what))
	})

	var out strings.Builder
	verb := "carry"
	if len(found) == 1 {
		verb = "carries"
	}
	fmt.Fprintf(&out, "%d %s %s text an importer did not convert\n", len(found), plural(len(found), "note", "notes"), verb)

	// Counted by kind rather than listed by note: after an import that can be
	// most of the notebook.
	for _, kind := range countKinds(found) {
		fmt.Fprintf(&out, "  %d %s %s\n", kind.count, plural(kind.count, "note", "notes"), kind.name)
	}

	// The cap is announced, or the list reads as complete.
	const named = 5
	out.WriteString("  " + style.Muted.Paint("for example:") + "\n")
	for _, item := range found[:min(named, len(found))] {
		out.WriteString("    " + item.file + "\n")
	}
	if len(found) > named {
		out.WriteString("    " + style.Muted.Paint(fmt.Sprintf("and %d more, each with its own `unconverted:` field", len(found)-named)) + "\n")
	}
	return out.String()
}

type kindCount struct {
	name  string
	count int
}

// countKinds is most common first, then by name.
func countKinds(found []unconverted) []kindCount {
	counts := map[string]int{}
	for _, item := range found {
		for _, kind := range strings.Split(item.what, ",") {
			counts[strings.TrimSpace(kind)]++
		}
	}
	kinds := make([]kindCount, 0, len(counts))
	for name, count := range counts {
		kinds = append(kinds, kindCount{name, count})
	}
	slices.SortFunc(kinds, func(a, b kindCount) int {
		return cmp.Or(cmp.Compare(b.count, a.count), cmp.Compare(a.name, b.name))
	})
	return kinds
}

// describeHooks names the hooks that will never fire. It is not behind a flag,
// being one directory read; and not a Problem (so not in `status`), since
// `.git` is not the notebook.
func describeHooks(hooks []string) string {
	if len(hooks) == 0 {
		return ""
	}
	var out strings.Builder
	fmt.Fprintf(&out, "%d git %s will never run\n", len(hooks), plural(len(hooks), "hook", "hooks"))
	for _, hook := range hooks {
		out.WriteString("  " + hook + "\n")
	}
	out.WriteString(style.Muted.Paint("noda writes commits itself and never calls git commit, which is what would run them"))
	return out.String()
}

// advice is how to settle the two problems noda leaves to the user.
func advice(scan notebook.Scan) []string {
	var out []string
	seen := map[string]struct{}{}
	shared := false
	for _, found := range scan.Notes {
		id := note.NormalizeID(found.ID)
		if _, dup := seen[id]; dup {
			shared = true
		}
		seen[id] = struct{}{}
	}
	if shared {
		out = append(out, "  to settle a shared id, rename one of the files so it starts with a different id")
	}
	if len(scan.Suspicious) > 0 {
		out = append(out, "  a file named like a note but holding no frontmatter is either a note that lost it — add a `---` block back — or a file that was never one, which you can rename so it no longer starts with an id")
	}
	return out
}
