package notebook

import (
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"

	"github.com/henry40408/noda/internal/gitx"
	"github.com/henry40408/noda/internal/gitx/gittest"
	"github.com/henry40408/noda/internal/paths"
)

// A scenario is a deterministic notebook built with nothing but git. Dates,
// authors and note ids are fixed, so every object id is the same on every
// machine; the expected outputs in the tests number ids as <OID:n> and only
// hold while that stays true.

const fixtureGitConfig = "[user]\n\tname = Fixture Author\n\temail = fixture@example.com\n" +
	"[init]\n\tdefaultBranch = main\n[commit]\n\tgpgsign = false\n[tag]\n\tgpgsign = false\n"

const (
	noteBudget       = "01abc234-budget-review.md"
	noteBudgetRename = "01abc234-q3-budget-review.md"
	noteMeeting      = "02def567-meeting-notes.md"
	noteReading      = "03ghj890-reading-list.md"
	noteFresh        = "04kmn123-fresh-idea.md"
)

const bodyBudget = `the q3 budget is late

- first line
- second line
- third line`

const bodyBudgetEdited = `the q3 budget is late again

- first line
- second line changed
- third line
- fourth line`

const bodyMeeting = `# Agenda

the budget, again

see [the budget](01abc234-budget-review.md)`

const bodyReading = `a book
another book`

// scenarios maps a name to the history it builds.
var scenarios = map[string]func(*builder){
	"linear":            buildLinear,
	"merge":             buildMerge,
	"dirty":             buildDirty,
	"rename-rewrite":    buildRenameRewrite,
	"diverged-clean":    buildDivergedClean,
	"diverged-conflict": buildDivergedConflict,
	"ahead":             buildAhead,
	"behind":            buildBehind,
	"tags":              buildTags,
}

// fixture builds one scenario under a temporary root and opens its notebook.
func fixture(t *testing.T, scenario string) *Notebook {
	t.Helper()
	build, ok := scenarios[scenario]
	if !ok {
		t.Fatalf("unknown scenario %q", scenario)
	}
	root := t.TempDir()
	b := newBuilder(t, root)
	build(b)

	env := gittest.Isolate(t)
	env["TZ"] = "UTC"
	git, err := gitx.New(gitx.Options{Env: env})
	if err != nil {
		t.Fatal(err)
	}
	nb, err := Open(git, paths.Rooted(root), "default")
	if err != nil {
		t.Fatal(err)
	}
	return nb
}

// builder runs git against the scenario's repositories with a clock that moves
// one hour per commit or tag.
type builder struct {
	t     *testing.T
	root  string
	nb    string
	env   []string
	clock int
	stamp string
}

func newBuilder(t *testing.T, root string) *builder {
	t.Helper()
	env := gittest.Isolate(t)
	gittest.WriteGlobalConfig(t, env, fixtureGitConfig, false)
	env["TZ"] = "UTC"
	env["LC_ALL"] = "C"
	env["GIT_TERMINAL_PROMPT"] = "0"
	pairs := make([]string, 0, len(env))
	for name, value := range env {
		pairs = append(pairs, name+"="+value)
	}
	b := &builder{t: t, root: root, nb: filepath.Join(root, "data", "noda", "notebooks", "default"), env: pairs}
	for _, dir := range []string{"config/noda", "state/noda", "cache/noda"} {
		b.mkdir(filepath.Join(root, dir))
	}
	b.write(filepath.Join(root, "config", "noda", "config.toml"), "sign = false\n")
	b.write(filepath.Join(root, "state", "noda", "active"), "default\n")
	return b
}

func (b *builder) mkdir(dir string) {
	b.t.Helper()
	if err := os.MkdirAll(dir, 0o755); err != nil {
		b.t.Fatal(err)
	}
}

func (b *builder) write(path, text string) {
	b.t.Helper()
	if err := os.WriteFile(path, []byte(text), 0o644); err != nil {
		b.t.Fatal(err)
	}
}

// tick moves the clock to an hour after the last commit or tag.
func (b *builder) tick() {
	b.clock++
	b.stamp = fmt.Sprintf("%d +0000", 1700000000+b.clock*3600)
}

func (b *builder) git(dir string, args ...string) {
	b.t.Helper()
	cmd := exec.Command("git", append([]string{"-C", dir}, args...)...)
	cmd.Env = append(append([]string{}, b.env...), "GIT_AUTHOR_DATE="+b.stamp, "GIT_COMMITTER_DATE="+b.stamp)
	if out, err := cmd.CombinedOutput(); err != nil {
		b.t.Fatalf("git %s: %v\n%s", strings.Join(args, " "), err, out)
	}
}

// commit records everything in the working tree.
func (b *builder) commit(dir, message string) {
	b.t.Helper()
	b.tick()
	b.git(dir, "add", "--all")
	b.git(dir, "commit", "--quiet", "--no-verify", "-m", message)
}

func (b *builder) newNotebook(dir string) {
	b.t.Helper()
	if out, err := exec.Command("git", "init", "--quiet", dir).CombinedOutput(); err != nil {
		b.t.Fatalf("git init: %v\n%s", err, out)
	}
	b.tick()
	b.git(dir, "commit", "--quiet", "--allow-empty", "--no-verify", "-m", "chore: initialize notebook")
}

type noteFile struct {
	name    string
	title   string
	tags    string
	created string
	updated string
	body    string
}

func day(n int) string { return fmt.Sprintf("2024-01-%02dT00:00:00Z", n) }

func (b *builder) note(dir string, n noteFile) {
	b.t.Helper()
	var text strings.Builder
	fmt.Fprintf(&text, "---\ntitle: %s\n", n.title)
	if n.tags != "" {
		fmt.Fprintf(&text, "tags: [%s]\n", n.tags)
	}
	fmt.Fprintf(&text, "created: %s\nupdated: %s\n---\n\n%s\n", n.created, n.updated, n.body)
	b.write(filepath.Join(dir, n.name), text.String())
}

func buildLinear(b *builder) {
	nb := b.nb
	b.newNotebook(nb)
	b.note(nb, noteFile{noteBudget, "Budget review", "work", day(1), day(1), bodyBudget})
	b.commit(nb, "add: Budget review")
	b.note(nb, noteFile{noteMeeting, "Meeting notes", "work, q3", day(2), day(2), bodyMeeting})
	b.commit(nb, "add: Meeting notes")
	b.note(nb, noteFile{noteBudget, "Budget review", "work", day(1), day(3), bodyBudgetEdited})
	b.commit(nb, "edit: Budget review")
	b.git(nb, "mv", noteBudget, noteBudgetRename)
	b.note(nb, noteFile{noteBudgetRename, "Q3 budget review", "work", day(1), day(4), bodyBudgetEdited})
	b.commit(nb, "mv: Q3 budget review")
	b.git(nb, "rm", "--quiet", noteMeeting)
	b.commit(nb, "rm: Meeting notes")
	b.note(nb, noteFile{noteMeeting, "Meeting notes", "work, q3", day(2), day(2), bodyMeeting})
	b.commit(nb, "restore: Meeting notes")
	b.note(nb, noteFile{noteReading, "Reading list", "", day(5), day(5), bodyReading})
	b.commit(nb, "add: Reading list")
	b.note(nb, noteFile{noteBudgetRename, "Q3 budget review", "work, urgent", day(1), day(6), bodyBudgetEdited})
	b.commit(nb, "tag: Q3 budget review")
	b.git(nb, "rm", "--quiet", noteReading)
	b.commit(nb, "rm: Reading list")
}

func buildMerge(b *builder) {
	nb := b.nb
	b.newNotebook(nb)
	b.note(nb, noteFile{noteBudget, "Budget review", "work", day(1), day(1), bodyBudget})
	b.note(nb, noteFile{noteMeeting, "Meeting notes", "work", day(2), day(2), bodyMeeting})
	b.commit(nb, "add: two notes")
	b.git(nb, "checkout", "--quiet", "-b", "side")
	b.note(nb, noteFile{noteMeeting, "Meeting notes", "work", day(2), day(3), bodyMeeting + "\n- a line from the side branch"})
	b.commit(nb, "edit: Meeting notes on side")
	b.git(nb, "checkout", "--quiet", "main")
	b.note(nb, noteFile{noteBudget, "Budget review", "work", day(1), day(4), bodyBudgetEdited})
	b.commit(nb, "edit: Budget review on main")
	b.tick()
	b.git(nb, "merge", "--quiet", "--no-ff", "--no-verify", "-m", "merge: side", "side")
	b.note(nb, noteFile{noteBudget, "Budget review", "work", day(1), day(5), bodyBudgetEdited + "\n- after the merge"})
	b.commit(nb, "edit: Budget review after merge")
}

func buildDirty(b *builder) {
	nb := b.nb
	b.newNotebook(nb)
	b.note(nb, noteFile{noteBudget, "Budget review", "work", day(1), day(1), bodyBudget})
	b.commit(nb, "add: Budget review")
	b.note(nb, noteFile{noteMeeting, "Meeting notes", "work", day(2), day(2), bodyMeeting})
	b.commit(nb, "add: Meeting notes")
	b.note(nb, noteFile{noteBudget, "Budget review", "work", day(1), day(9), bodyBudgetEdited})
	b.note(nb, noteFile{noteFresh, "Fresh idea", "", day(9), day(9), "not committed yet"})
	b.write(filepath.Join(nb, "diagram.png"), "not really a png\n")
}

// A rename that rewrites almost everything: where following by id and following
// by content part ways.
func buildRenameRewrite(b *builder) {
	nb := b.nb
	b.newNotebook(nb)
	b.note(nb, noteFile{noteBudget, "Budget review", "work", day(1), day(1),
		"alpha\nbravo\ncharlie\ndelta\necho\nfoxtrot\ngolf\nhotel\nindia\njuliet\nkilo\nlima"})
	b.commit(nb, "add: Budget review")
	b.git(nb, "mv", noteBudget, noteBudgetRename)
	rewritten := "one\ntwo\nthree\nfour\nfive\nsix\ngolf\nseven\neight\nnine\nten\nlima"
	b.note(nb, noteFile{noteBudgetRename, "Q3 budget review", "work", day(1), day(2), rewritten})
	b.commit(nb, "mv: Q3 budget review, rewritten")
	b.note(nb, noteFile{noteBudgetRename, "Q3 budget review", "work", day(1), day(3), rewritten + "\neleven"})
	b.commit(nb, "edit: Q3 budget review")
}

// remote is a bare repository the notebook was pushed to, and other is a second
// clone that writes to it, so the remote can be ahead of what the notebook has
// seen.
func (b *builder) remotePair() (other string) {
	b.t.Helper()
	remote := filepath.Join(b.root, "remote.git")
	other = filepath.Join(b.root, "other")
	if out, err := exec.Command("git", "init", "--quiet", "--bare", remote).CombinedOutput(); err != nil {
		b.t.Fatalf("git init --bare: %v\n%s", err, out)
	}
	url := "file://" + remote
	b.newNotebook(b.nb)
	b.note(b.nb, noteFile{noteBudget, "Budget review", "work", day(1), day(1), bodyBudget})
	b.note(b.nb, noteFile{noteMeeting, "Meeting notes", "work", day(2), day(2), bodyMeeting})
	b.commit(b.nb, "add: two notes")
	b.git(b.nb, "remote", "add", "origin", url)
	b.git(b.nb, "push", "--quiet", "--no-verify", "origin", "main")
	if out, err := exec.Command("git", "clone", "--quiet", url, other).CombinedOutput(); err != nil {
		b.t.Fatalf("git clone: %v\n%s", err, out)
	}
	return other
}

// otherCommit has the second clone push whatever is in its tree.
func (b *builder) otherCommit(other, message string) {
	b.t.Helper()
	b.commit(other, message)
	b.git(other, "push", "--quiet", "--no-verify", "origin", "main")
}

func buildDivergedClean(b *builder) {
	other := b.remotePair()
	b.note(other, noteFile{noteBudget, "Budget review", "work", day(1), day(7), bodyBudgetEdited})
	b.otherCommit(other, "edit: Budget review elsewhere")
	b.note(b.nb, noteFile{noteMeeting, "Meeting notes", "work", day(2), day(8), bodyMeeting + "\n- a local line"})
	b.commit(b.nb, "edit: Meeting notes here")
}

func buildDivergedConflict(b *builder) {
	other := b.remotePair()
	const tail = "\n\n- first line\n- second line\n- third line"
	b.note(other, noteFile{noteBudget, "Budget review", "work", day(1), day(7), "the q3 budget is on time" + tail})
	b.otherCommit(other, "edit: Budget review elsewhere")
	b.note(b.nb, noteFile{noteBudget, "Budget review", "work", day(1), day(8), "the q3 budget is cancelled" + tail})
	b.commit(b.nb, "edit: Budget review here")
}

func buildAhead(b *builder) {
	b.remotePair()
	b.note(b.nb, noteFile{noteReading, "Reading list", "", day(5), day(5), bodyReading})
	b.commit(b.nb, "add: Reading list")
	b.note(b.nb, noteFile{noteReading, "Reading list", "books", day(5), day(6), bodyReading})
	b.commit(b.nb, "tag: Reading list")
	b.tick()
	b.git(b.nb, "tag", "-a", "-m", "before the books", "before-books", "HEAD~1")
}

func buildBehind(b *builder) {
	other := b.remotePair()
	b.note(other, noteFile{noteBudget, "Budget review", "work", day(1), day(7), bodyBudgetEdited})
	b.otherCommit(other, "edit: Budget review elsewhere")
	b.note(other, noteFile{noteBudget, "Budget review", "work, urgent", day(1), day(8), bodyBudgetEdited})
	b.otherCommit(other, "tag: Budget review elsewhere")
}

// A local tag the remote lacks, one it already has, and one it has at another commit.
func buildTags(b *builder) {
	other := b.remotePair()
	b.note(b.nb, noteFile{noteReading, "Reading list", "", day(5), day(5), bodyReading})
	b.commit(b.nb, "add: Reading list")
	b.tick()
	b.git(b.nb, "tag", "-a", "-m", "only here", "local-only", "HEAD")
	b.tick()
	b.git(b.nb, "tag", "-a", "-m", "on both", "shared", "HEAD~1")
	b.git(b.nb, "push", "--quiet", "--no-verify", "origin", "shared")
	b.tick()
	b.git(b.nb, "tag", "-a", "-m", "here it is the new commit", "clash", "HEAD")
	b.tick()
	b.git(other, "fetch", "--quiet", "origin")
	b.git(other, "tag", "-a", "-m", "there it is the old commit", "clash", "HEAD")
	b.git(other, "push", "--quiet", "--no-verify", "origin", "clash")
}
