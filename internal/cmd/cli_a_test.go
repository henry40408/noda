package cmd

import (
	"context"
	"os"
	"path/filepath"
	"reflect"
	"strconv"
	"strings"
	"testing"

	"github.com/henry40408/noda/internal/config"
	"github.com/henry40408/noda/internal/gitx"
	"github.com/henry40408/noda/internal/gitx/gittest"
	"github.com/henry40408/noda/internal/note"
	"github.com/henry40408/noda/internal/notebook"
	"github.com/henry40408/noda/internal/paths"
)

// aInit is an isolated root with signing off, then `init` run on it. Signing is
// turned off before anything commits: noda's setting outranks git's.
func aInit(t *testing.T) (Env, string) {
	t.Helper()
	e := Env{Paths: paths.Rooted(t.TempDir()), Git: gittest.Require(t)}
	if err := os.MkdirAll(e.Paths.ConfigDir(), 0o777); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(e.Paths.ConfigDir(), "config.toml"), []byte("sign = false\n"), 0o666); err != nil {
		t.Fatal(err)
	}
	if _, err := Init(e); err != nil {
		t.Fatal(err)
	}
	return e, e.Paths.NotebookDir(config.DefaultNotebook)
}

func aRunGit(t *testing.T, e Env, dir string, args ...string) string {
	t.Helper()
	result, err := e.Git.Run(context.Background(), gitx.Cmd{Dir: dir, Args: args, ReadOnly: true})
	if err != nil {
		t.Fatal(err)
	}
	return string(result.Stdout)
}

func aCommitCount(t *testing.T, e Env, dir string) int {
	t.Helper()
	count, err := strconv.Atoi(strings.TrimSpace(aRunGit(t, e, dir, "rev-list", "--count", "HEAD")))
	if err != nil {
		t.Fatal(err)
	}
	return count
}

// aAssertClean fails when anything, untracked files included, is uncommitted.
func aAssertClean(t *testing.T, e Env, dir, why string) {
	t.Helper()
	if out := aRunGit(t, e, dir, "status", "--porcelain"); out != "" {
		t.Fatalf("%s: %s", why, out)
	}
}

func aStr(text string) *string { return &text }

// aAdd is `add` with a title and a body.
func aAdd(t *testing.T, e Env, title, content string, tags ...string) string {
	t.Helper()
	out, err := Add(e, AddOptions{Title: aStr(title), Content: aStr(content), Tags: tags})
	if err != nil {
		t.Fatal(err)
	}
	return out
}

// aOK is the text of a command that must have succeeded.
func aOK(t *testing.T, out string, err error) string {
	t.Helper()
	if err != nil {
		t.Fatal(err)
	}
	return out
}

// aFail is the message of a command that must have failed.
func aFail(t *testing.T, out string, err error) string {
	t.Helper()
	if err == nil {
		t.Fatalf("expected an error, got %q", out)
	}
	return err.Error()
}

func aContains(t *testing.T, text, want string) {
	t.Helper()
	if !strings.Contains(text, want) {
		t.Fatalf("missing %q in: %s", want, text)
	}
}

func aLacks(t *testing.T, text, unwanted string) {
	t.Helper()
	if strings.Contains(text, unwanted) {
		t.Fatalf("unexpected %q in: %s", unwanted, text)
	}
}

func aEqual(t *testing.T, got, want any, why string) {
	t.Helper()
	if !reflect.DeepEqual(got, want) {
		t.Fatalf("%s: got %v, want %v", why, got, want)
	}
}

// aParts is the id and slug from the `id  slug  [tags]` line mutating commands
// print.
func aParts(summary string) (id, slug string) {
	id, rest, _ := strings.Cut(summary, "  ")
	slug, _, _ = strings.Cut(rest, "  ")
	return id, slug
}

func aNoteFile(summary string) string {
	id, slug := aParts(summary)
	return id + "-" + slug + ".md"
}

func aShow(t *testing.T, e Env, key string) string {
	t.Helper()
	out, err := Show(e, key)
	return aOK(t, out, err)
}

func aLs(t *testing.T, e Env, options List) string {
	t.Helper()
	out, err := Ls(e, options)
	return aOK(t, out, err)
}

func aTag(t *testing.T, e Env, key string, touch Touch, changes ...string) string {
	t.Helper()
	out, err := Tag(e, key, changes, touch)
	return aOK(t, out, err)
}

func aPin(t *testing.T, e Env, key string, pinned bool) string {
	t.Helper()
	out, err := Pin(e, key, pinned, TouchStamp)
	return aOK(t, out, err)
}

func aMv(e Env, key, title string, updateLinks bool, touch Touch) (string, error) {
	return Mv(e, Rename{Key: key, NewTitle: title, UpdateLinks: updateLinks, Touch: touch})
}

// aNoteText is the note as it sits on disk; `show` dims the frontmatter.
func aNoteText(t *testing.T, e Env, key string) string {
	t.Helper()
	out, err := Path(e, key)
	text, err := os.ReadFile(strings.TrimSpace(aOK(t, out, err)))
	if err != nil {
		t.Fatal(err)
	}
	return string(text)
}

func aNoteID(t *testing.T, e Env, key string) string {
	t.Helper()
	out, err := Path(e, key)
	stem := strings.TrimSuffix(filepath.Base(strings.TrimSpace(aOK(t, out, err))), ".md")
	id, _, ok := note.SplitStem(stem)
	if !ok {
		t.Fatalf("%s has no id", stem)
	}
	return id
}

type aStamps struct{ created, updated *string }

func aTimes(t *testing.T, e Env, key string) aStamps {
	t.Helper()
	parsed, err := note.Parse(aNoteText(t, e, key))
	if err != nil {
		t.Fatal(err)
	}
	return aStamps{created: parsed.Created, updated: parsed.Updated}
}

func aStampText(stamp *string) string {
	if stamp == nil {
		return "<none>"
	}
	return *stamp
}

// aBackdate sets `updated` far back so a change to it shows however fast the
// test runs.
func aBackdate(t *testing.T, dir, summary string) {
	t.Helper()
	path := filepath.Join(dir, aNoteFile(summary))
	text, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	backdated, ok := note.SetField(string(text), "updated", "2000-01-01T00:00:00Z")
	if !ok {
		t.Fatalf("no updated field to set in %s", text)
	}
	if err := os.WriteFile(path, []byte(backdated), 0o666); err != nil {
		t.Fatal(err)
	}
}

// aEditorScript is an executable stand-in for `$EDITOR`; the note path arrives
// as `$1`. A script rather than `sh -c` because the editor string is split on
// whitespace.
func aEditorScript(t *testing.T, name, script string) string {
	t.Helper()
	path := filepath.Join(t.TempDir(), name+".sh")
	if err := os.WriteFile(path, []byte("#!/bin/sh\n"+script+"\n"), 0o755); err != nil {
		t.Fatal(err)
	}
	return path
}

// aCommitWorkingTree commits the working tree the way an edit made outside noda
// would arrive.
func aCommitWorkingTree(t *testing.T, e Env, message string) {
	t.Helper()
	opened, err := e.open(config.DefaultNotebook)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := opened.CommitAll(message); err != nil {
		t.Fatal(err)
	}
}

func aStatusRow(status, key string) (string, bool) {
	for _, line := range strings.Split(status, "\n") {
		if strings.HasPrefix(line, key) {
			return strings.TrimSpace(line[len(key):]), true
		}
	}
	return "", false
}

// aLinkedPair is two notes, the second linking to the first; it returns the id
// and slug of each.
func aLinkedPair(t *testing.T, e Env) (targetID, targetSlug, sourceID, sourceSlug string) {
	t.Helper()
	target := aAdd(t, e, "Meeting notes", "agenda\n")
	targetID, targetSlug = aParts(target)
	source := aAdd(t, e, "Q3 budget", "see [the meeting]("+targetID+"-"+targetSlug+".md)\n")
	sourceID, sourceSlug = aParts(source)
	return targetID, targetSlug, sourceID, sourceSlug
}

func aWrite(t *testing.T, path, text string) {
	t.Helper()
	if err := os.WriteFile(path, []byte(text), 0o666); err != nil {
		t.Fatal(err)
	}
}

func TestInitCreatesTheXdgLayoutAndIsIdempotent(t *testing.T) {
	e, dir := aInit(t)

	if info, err := os.Stat(filepath.Join(dir, ".git")); err != nil || !info.IsDir() {
		t.Fatal("notebook is a git repo")
	}
	if _, err := os.Stat(filepath.Join(dir, ".noda")); err == nil {
		t.Fatal("noda commits no bookkeeping of its own")
	}
	if info, err := os.Stat(e.Paths.ConfigDir()); err != nil || !info.IsDir() {
		t.Fatal("config dir created")
	}
	if active, err := e.Paths.ActiveNotebook(); err != nil || active != config.DefaultNotebook {
		t.Fatalf("active notebook: %q, %v", active, err)
	}
	aEqual(t, aCommitCount(t, e, dir), 1, "init makes one commit")

	if _, err := Init(e); err != nil {
		t.Fatal(err)
	}
	aEqual(t, aCommitCount(t, e, dir), 1, "re-running init commits nothing")
}

func TestAddWritesTheIdIntoTheFilenameAndCommits(t *testing.T) {
	e, dir := aInit(t)

	out := aAdd(t, e, "Meeting Notes", "agenda\n")
	id, slug := aParts(out)
	aEqual(t, slug, "meeting-notes", "slug")

	raw, err := os.ReadFile(filepath.Join(dir, id+"-meeting-notes.md"))
	if err != nil {
		t.Fatal(err)
	}
	text := string(raw)
	// The identity is the filename; nothing in the file can disagree with it.
	aLacks(t, text, "id:")
	aContains(t, text, "title: Meeting Notes")
	if !strings.HasSuffix(text, "agenda\n") {
		t.Fatalf("body: %s", text)
	}

	aEqual(t, aCommitCount(t, e, dir), 2, "the note is one commit")
	aAssertClean(t, e, dir, "nothing is left uncommitted")
}

func TestAddDerivesTheTitleFromTheBodyWhenOmitted(t *testing.T) {
	e, _ := aInit(t)

	out, err := Add(e, AddOptions{Content: aStr("# Reading Log\n\nsome book\n")})
	if !strings.HasSuffix(aOK(t, out, err), "  reading-log") {
		t.Fatalf("got %q", out)
	}

	aContains(t, aShow(t, e, "reading-log"), "title: Reading Log")
}

func TestAddRejectsAnEmptyNote(t *testing.T) {
	e, _ := aInit(t)
	out, err := Add(e, AddOptions{Content: aStr("   \n\n")})
	aContains(t, aFail(t, out, err), "empty")
}

func TestAddRefusesATitleOrATagTheFrontmatterCannotCarry(t *testing.T) {
	e, dir := aInit(t)
	before := aCommitCount(t, e, dir)

	// A second line would become a field of its own, so `Render` and `Parse`
	// would stop being inverses.
	out, err := Add(e, AddOptions{Title: aStr("Meeting\ntitle: other"), Content: aStr("body\n")})
	aContains(t, aFail(t, out, err), "one line")

	// `,` separates tags and `]` closes the list, so neither can sit inside one.
	for _, tag := range []string{"work, secret", "a]", ""} {
		if out, err := Add(e, AddOptions{Title: aStr("Alpha"), Content: aStr("body\n"), Tags: []string{tag}}); err == nil {
			t.Fatalf("tag %q should be refused, got %q", tag, out)
		}
	}

	if listed := aLs(t, e, List{}); listed != "" {
		t.Fatalf("nothing written: %q", listed)
	}
	aEqual(t, aCommitCount(t, e, dir), before, "and nothing committed")
}

func TestATagIsStoredTheWayItReadsBack(t *testing.T) {
	e, _ := aInit(t)
	aAdd(t, e, "Alpha", "a\n", "  work  ")

	// Trimmed on the way in because it is trimmed on the way out; otherwise the
	// tag shown is not the tag `ls --tag` matches.
	aContains(t, aShow(t, e, "alpha"), "tags: [work]")
	aContains(t, aLs(t, e, List{Tag: "work"}), "Alpha")

	out, err := Tag(e, "alpha", []string{"+q3, urgent"}, TouchStamp)
	aContains(t, aFail(t, out, err), "`")
	// Removal stays permissive so a tag that predates the check can still go.
	if _, err := Tag(e, "alpha", []string{"-q3, urgent"}, TouchStamp); err != nil {
		t.Fatal(err)
	}
}

// `<id>-<slug>.md` spends 12 bytes before the slug, and a long title used to
// push the name past the 255-byte component limit (`File name too long`).
func TestATitleTooLongForAFilenameStillBecomesANote(t *testing.T) {
	e, dir := aInit(t)
	title := "GitHub - coding-horror/basic-computer-games: An updated version of the " +
		"classic \"Basic Computer Games\" book, with well-written examples in a variety of " +
		"common MEMORY SAFE, SCRIPTING programming languages. See " +
		"https://coding-horror.github.io/basic-computer-games/"

	out := aAdd(t, e, title, "body\n")

	file := aNoteFile(out)
	if len(file) > 255 {
		t.Fatalf("the filename has to fit a path component: %d bytes, %s", len(file), file)
	}
	if info, err := os.Stat(filepath.Join(dir, file)); err != nil || !info.Mode().IsRegular() {
		t.Fatal(file)
	}

	// The slug is cut; the title is kept whole.
	_, slug := aParts(out)
	aContains(t, aNoteText(t, e, slug), "title: "+title)
}

// The id keeps the filenames apart. The old `-2` suffix was only local: two
// machines adding "Notes" both wrote `notes.md` and the sync conflicted.
func TestTwoNotesMayShareASlugBecauseTheIdSeparatesThem(t *testing.T) {
	e, dir := aInit(t)

	first := aAdd(t, e, "Notes", "one\n")
	second := aAdd(t, e, "Notes", "two\n")

	firstID, firstSlug := aParts(first)
	secondID, secondSlug := aParts(second)
	aEqual(t, firstSlug, "notes", "first slug")
	aEqual(t, secondSlug, "notes", "no `-2` invented")
	if firstID == secondID {
		t.Fatal("the ids must differ")
	}

	for _, summary := range []string{first, second} {
		if _, err := os.Stat(filepath.Join(dir, aNoteFile(summary))); err != nil {
			t.Fatal(err)
		}
	}
	aEqual(t, len(strings.Split(strings.TrimRight(aLs(t, e, List{}), "\n"), "\n")), 2, "two lines")

	// The slug is ambiguous, so noda asks rather than guesses.
	out, err := Show(e, "notes")
	message := aFail(t, out, err)
	aContains(t, message, "matches 2 notes")
	aContains(t, message, firstID)
	aContains(t, aShow(t, e, firstID), "one")
	aContains(t, aShow(t, e, secondID), "two")
}

// As git resolves an abbreviated object id.
func TestANoteResolvesFromAPrefixOfItsId(t *testing.T) {
	e, _ := aInit(t)
	id, _ := aParts(aAdd(t, e, "Meeting Notes", "agenda\n"))

	whole := aShow(t, e, id)
	aEqual(t, aShow(t, e, id[:4]), whole, "four characters")
	aEqual(t, aShow(t, e, id[:1]), whole, "even one")
}

func TestShowResolvesBySlugAndByIdIncludingConfusableCharacters(t *testing.T) {
	e, _ := aInit(t)
	id, _ := aParts(aAdd(t, e, "Meeting Notes", "agenda\n"))

	bySlug := aShow(t, e, "meeting-notes")
	aEqual(t, aShow(t, e, id), bySlug, "by id")

	// Crockford folds case and the I/L/O confusables.
	var mistyped strings.Builder
	for _, c := range id {
		switch c {
		case '1':
			mistyped.WriteRune('I')
		case '0':
			mistyped.WriteRune('O')
		default:
			mistyped.WriteString(strings.ToUpper(string(c)))
		}
	}
	aEqual(t, aShow(t, e, mistyped.String()), bySlug, "mistyped id")
}

func TestShowReportsUnknownAndUnsafeReferences(t *testing.T) {
	e, _ := aInit(t)
	aAdd(t, e, "Meeting Notes", "agenda\n")

	// A slug is matched whole; only ids take a prefix.
	out, err := Show(e, "meeting")
	aContains(t, aFail(t, out, err), "not found")
	if out, err := Show(e, "../../etc/passwd"); err == nil {
		t.Fatalf("unsafe reference resolved: %q", out)
	}
}

func TestLsListsNotesAndFiltersByTag(t *testing.T) {
	e, _ := aInit(t)
	aAdd(t, e, "Alpha", "a\n", "work")
	aAdd(t, e, "Beta", "b\n")

	all := aLs(t, e, List{})
	lines := strings.Split(strings.TrimRight(all, "\n"), "\n")
	aEqual(t, len(lines), 2, "two lines")
	aContains(t, lines[0], "Alpha")
	// The brackets and the tag are coloured separately, so the raw output does
	// not contain `[work]` as one run.
	aContains(t, plain(Ls(e, List{})), "[work]")

	tagged := aLs(t, e, List{Tag: "work"})
	aEqual(t, len(strings.Split(strings.TrimRight(tagged, "\n"), "\n")), 1, "one tagged line")
	aContains(t, tagged, "Alpha")

	if none := aLs(t, e, List{Tag: "nope"}); none != "" {
		t.Fatalf("got %q", none)
	}
}

func TestLsCanTargetAnotherNotebook(t *testing.T) {
	e, _ := aInit(t)
	aAdd(t, e, "Alpha", "a\n")
	if _, err := notebook.Create(e.Git, e.Paths, "work"); err != nil {
		t.Fatal(err)
	}

	if listed := aLs(t, e, List{Notebook: "work"}); listed != "" {
		t.Fatalf("got %q", listed)
	}
	if out, err := Ls(e, List{Notebook: "missing"}); err == nil {
		t.Fatalf("a missing notebook listed: %q", out)
	}
}

func TestTagAddsAndRemovesAndCommitsOnce(t *testing.T) {
	e, dir := aInit(t)
	aAdd(t, e, "Alpha", "a\n", "work")
	before := aCommitCount(t, e, dir)

	out := aTag(t, e, "alpha", TouchStamp, "+q3", "-work")
	if !strings.HasSuffix(out, "  [q3]") {
		t.Fatalf("got %q", out)
	}
	aEqual(t, aCommitCount(t, e, dir), before+1, "one commit")

	text := aShow(t, e, "alpha")
	aContains(t, text, "tags: [q3]")
	if !strings.HasSuffix(text, "a\n") {
		t.Fatalf("the body survives a tag change: %s", text)
	}
}

func TestPinAndUnpinAreOneCommitEachAndTheSecondPressIsNeither(t *testing.T) {
	e, dir := aInit(t)
	aAdd(t, e, "Alpha", "a\n")
	before := aCommitCount(t, e, dir)

	out := aPin(t, e, "alpha", true)
	if !strings.HasSuffix(out, "  pinned") {
		t.Fatalf("got %q", out)
	}
	aEqual(t, aCommitCount(t, e, dir), before+1, "pin commits")
	aContains(t, aNoteText(t, e, "alpha"), "pinned: true")

	// Already pinned: nothing written, nothing committed.
	again := aPin(t, e, "alpha", true)
	if !strings.HasSuffix(again, "(no change)") {
		t.Fatalf("got %q", again)
	}
	aEqual(t, aCommitCount(t, e, dir), before+1, "no second commit")

	off := aPin(t, e, "alpha", false)
	if !strings.HasSuffix(off, "  unpinned") {
		t.Fatalf("got %q", off)
	}
	aEqual(t, aCommitCount(t, e, dir), before+2, "unpin commits")
	// The line is removed rather than set to false, so the file is as it was.
	aLacks(t, aNoteText(t, e, "alpha"), "pinned")
}

func TestAPinIsAboveEveryListingAndBelowEveryReversedOne(t *testing.T) {
	e, _ := aInit(t)
	aAdd(t, e, "Alpha", "a\n")
	aAdd(t, e, "Beta", "b\n")
	aAdd(t, e, "Gamma", "g\n")
	aPin(t, e, "beta", true)

	listed := func(reverse bool) []string {
		out := aLs(t, e, List{Format: FormatQuiet, Reverse: reverse})
		return strings.Split(strings.TrimRight(out, "\n"), "\n")
	}
	beta := aNoteID(t, e, "beta")

	aEqual(t, listed(false)[0], beta, "a pin comes first")
	// `-r` reverses the whole listing, pins included.
	reversed := listed(true)
	aEqual(t, reversed[len(reversed)-1], beta, "-r left the pin on top")

	table := plain(Ls(e, List{}))
	tableLines := strings.Split(strings.TrimRight(table, "\n"), "\n")
	if !strings.HasSuffix(tableLines[0], "  pinned") {
		t.Fatalf("%s", table)
	}
	pinnedLines := 0
	for _, line := range tableLines {
		if strings.Contains(line, "pinned") {
			pinnedLines++
		}
	}
	aEqual(t, pinnedLines, 1, "one pinned line: "+table)

	json := aLs(t, e, List{Format: FormatJSON})
	aContains(t, json, `"pinned":true`)
	aContains(t, json, `"pinned":false`)
}

func TestSearchNarrowsToThePinnedNotesAndAwayFromThem(t *testing.T) {
	e, _ := aInit(t)
	aAdd(t, e, "Alpha", "a\n")
	aAdd(t, e, "Beta", "b\n")
	aPin(t, e, "beta", true)

	found := func(query string) string {
		out, err := Search(e, []string{query})
		return aOK(t, out, err)
	}
	aContains(t, found("pinned:true"), "Beta")
	aLacks(t, found("pinned:true"), "Alpha")
	aContains(t, found("pinned:false"), "Alpha")
	aLacks(t, found("pinned:false"), "Beta")
	// A typo is refused rather than matching everything.
	if out, err := Search(e, []string{"pinned:ture"}); err == nil {
		t.Fatalf("a typo matched: %q", out)
	}
}

func TestANewNoteIsCreatedAndUpdatedAtTheSameMoment(t *testing.T) {
	e, _ := aInit(t)
	aAdd(t, e, "Alpha", "a\n")

	stamps := aTimes(t, e, "alpha")
	if stamps.created == nil {
		t.Fatal("a new note records when it was made")
	}
	aEqual(t, stamps.updated, stamps.created, "a note nobody has changed was last changed when it was made")
	if !strings.HasSuffix(*stamps.created, "Z") {
		t.Fatalf("got %s", *stamps.created)
	}
}

func TestChangingANoteMovesUpdatedAndLeavesCreatedAlone(t *testing.T) {
	e, dir := aInit(t)
	added := aAdd(t, e, "Alpha", "a\n")
	created := aTimes(t, e, "alpha").created

	// Backdated so the change is visible however fast the test runs.
	aBackdate(t, dir, added)

	aTag(t, e, "alpha", TouchStamp, "+work")
	after := aTimes(t, e, "alpha")
	aEqual(t, after.created, created, "created does not move")
	if aStampText(after.updated) == "2000-01-01T00:00:00Z" {
		t.Fatal("a tag change is a change")
	}

	if _, err := aMv(e, "alpha", "Beta", false, TouchStamp); err != nil {
		t.Fatal(err)
	}
	aEqual(t, aTimes(t, e, "beta").created, created, "a retitle is not a rebirth")
}

// The commit still records the change; only `updated` is left alone.
func TestNoTouchLeavesUpdatedWhereItStands(t *testing.T) {
	e, dir := aInit(t)
	added := aAdd(t, e, "Alpha", "a\n")
	aBackdate(t, dir, added)
	before := aCommitCount(t, e, dir)

	aTag(t, e, "alpha", TouchKeep, "+work")
	aEqual(t, aStampText(aTimes(t, e, "alpha").updated), "2000-01-01T00:00:00Z", "the tag went on without redating the note")

	if _, err := aMv(e, "alpha", "Beta", false, TouchKeep); err != nil {
		t.Fatal(err)
	}
	stamps := aTimes(t, e, "beta")
	aEqual(t, aStampText(stamps.updated), "2000-01-01T00:00:00Z", "and so did the new title")
	if stamps.created == nil {
		t.Fatal("created was never the field in question")
	}

	aEqual(t, aCommitCount(t, e, dir), before+2, "both changes are still commits: the file did change")
	aContains(t, aShow(t, e, "beta"), "work")
}

// `tag` takes hyphen values, so a flag after the tags would arrive as a tag:
// `--no-touch` would become removing `-no-touch`, reporting success and doing
// nothing.
func TestTagSaysWhereAFlagGoesRatherThanSwallowingIt(t *testing.T) {
	e, dir := aInit(t)
	aAdd(t, e, "Alpha", "a\n")
	before := aCommitCount(t, e, dir)

	out, err := Tag(e, "alpha", []string{"+work", "--no-touch"}, TouchStamp)
	message := aFail(t, out, err)
	aContains(t, message, "--no-touch")
	aContains(t, message, "before")
	aEqual(t, aCommitCount(t, e, dir), before, "and nothing was committed on the way to finding out")
}

// The case the flag exists for: an imported note's dates must survive an edit.
func TestNoTouchKeepsAnImportedNotesOwnDatesThroughAnEdit(t *testing.T) {
	e, dir := aInit(t)
	aWrite(t, filepath.Join(dir, "k3f9m2p1-imported.md"),
		"---\ntitle: Imported\ncreated: 2019-03-14T08:21:00Z\nupdated: 2019-03-14T16:21:00+08:00\n---\n\nbody\n")
	aCommitWorkingTree(t, e, "add: imported")

	editor := aEditorScript(t, "append", `printf -- 'and one more line\n' >> "$1"`)
	if _, err := EditWith(e, "imported", editor, TouchKeep); err != nil {
		t.Fatal(err)
	}

	text := aNoteText(t, e, "imported")
	if !strings.Contains(text, "updated: 2019-03-14T16:21:00+08:00") {
		t.Fatalf("the offset it was written with is still the offset it carries: %s", text)
	}
	aContains(t, text, "and one more line")
}

// The only honest value would come from git; one taken from the filesystem
// would be invented after a clone.
func TestANoteWithoutTimesDoesNotGetThemInvented(t *testing.T) {
	e, dir := aInit(t)
	aWrite(t, filepath.Join(dir, "k3f9m2p1-imported.md"), "---\ntitle: Imported\n---\n\nbody\n")

	aEqual(t, aTimes(t, e, "imported"), aStamps{}, "no times to begin with")

	// A change noda makes is dated, but the missing `created` is not backfilled.
	aTag(t, e, "imported", TouchStamp, "+work")
	stamps := aTimes(t, e, "imported")
	if stamps.created != nil {
		t.Fatal("noda does not know when this was written")
	}
	if stamps.updated == nil {
		t.Fatal("it does know when it just touched it")
	}
}

// `tag` rewrites through `Render`, which is where unknown fields would be lost,
// and for an imported note the file is the only copy.
func TestAWriteBackKeepsTheFieldsNodaDoesNotUnderstand(t *testing.T) {
	e, dir := aInit(t)
	aWrite(t, filepath.Join(dir, "k3f9m2p1-imported.md"),
		"---\ntitle: Imported\nsource_id: 4821\nstarred: true\n---\n\nbody\n")

	aTag(t, e, "imported", TouchStamp, "+work")

	text := aShow(t, e, "imported")
	aContains(t, text, "source_id: 4821")
	aContains(t, text, "starred: true")
	aContains(t, text, "tags: [work]")
	if !strings.HasSuffix(text, "body\n") {
		t.Fatalf("%s", text)
	}
}

func TestTagDropsTheTagsLineWhenTheLastTagGoes(t *testing.T) {
	e, _ := aInit(t)
	aAdd(t, e, "Alpha", "a\n", "work")

	aTag(t, e, "alpha", TouchStamp, "-work")
	aLacks(t, aShow(t, e, "alpha"), "tags:")
	if listed := aLs(t, e, List{Tag: "work"}); listed != "" {
		t.Fatalf("got %q", listed)
	}
}

func TestTagRequiresASignAndCommitsNothingWhenThereIsNoChange(t *testing.T) {
	e, dir := aInit(t)
	aAdd(t, e, "Alpha", "a\n", "work")
	before := aCommitCount(t, e, dir)

	out, err := Tag(e, "alpha", []string{"work"}, TouchStamp)
	aContains(t, aFail(t, out, err), "+work")

	out = aTag(t, e, "alpha", TouchStamp, "+work", "-q3")
	aContains(t, out, "no change")
	aEqual(t, aCommitCount(t, e, dir), before, "nothing to commit")
}

func TestTagResolvesByIdToo(t *testing.T) {
	e, _ := aInit(t)
	id, _ := aParts(aAdd(t, e, "Alpha", "a\n"))

	aTag(t, e, id, TouchStamp, "+work")
	aContains(t, aShow(t, e, "alpha"), "tags: [work]")
}

func TestMvRenamesTheSlugAndKeepsTheId(t *testing.T) {
	e, dir := aInit(t)
	id, _ := aParts(aAdd(t, e, "Alpha", "a\n", "work"))

	out, err := aMv(e, "alpha", "Beta Notes", false, TouchStamp)
	aEqual(t, aOK(t, out, err), id+"  beta-notes  [work]", "summary")

	if _, err := os.Stat(filepath.Join(dir, id+"-alpha.md")); err == nil {
		t.Fatal("old file is gone")
	}
	if info, err := os.Stat(filepath.Join(dir, id+"-beta-notes.md")); err != nil || !info.Mode().IsRegular() {
		t.Fatal("new file exists")
	}

	// Only the slug moved, so the id still resolves and the old slug does not.
	aContains(t, aShow(t, e, id), "title: Beta Notes")
	aShow(t, e, "beta-notes")
	if out, err := Show(e, "alpha"); err == nil {
		t.Fatalf("the old slug resolved: %q", out)
	}

	aAssertClean(t, e, dir, "the rename is fully committed, with no leftover")
}

func TestMvRetitlesWithoutMovingWhenTheSlugIsUnchanged(t *testing.T) {
	e, _ := aInit(t)
	aAdd(t, e, "Alpha", "a\n")

	if _, err := aMv(e, "alpha", "  ALPHA  ", false, TouchStamp); err != nil {
		t.Fatal(err)
	}
	aContains(t, aShow(t, e, "alpha"), "title: ALPHA")
}

// The links are stale rather than dead (their id still resolves), but any
// Markdown reader outside noda sees only the path that is gone.
func TestMvSaysWhichNotesLinkedToTheNameItLeft(t *testing.T) {
	e, _ := aInit(t)
	targetID, _, _, sourceSlug := aLinkedPair(t, e)

	out := plain(aMv(e, targetID, "Weekly sync", false, TouchStamp))
	aContains(t, out, "1 note links to "+targetID+" by an older name")
	aContains(t, out, sourceSlug+".md")
	aContains(t, aNoteText(t, e, sourceSlug), targetID+"-meeting-notes.md")
}

// The rename and the rewrites are one commit, so no state has half the links
// moved.
func TestMvUpdateLinksRewritesTheNotesThatPointedAtTheOldName(t *testing.T) {
	e, dir := aInit(t)
	targetID, _, _, sourceSlug := aLinkedPair(t, e)
	commits := aCommitCount(t, e, dir)
	before := aTimes(t, e, sourceSlug)

	out := plain(aMv(e, targetID, "Weekly sync", true, TouchStamp))
	aContains(t, out, "updated  1 note")
	aLacks(t, out, "links to")

	text := aNoteText(t, e, sourceSlug)
	aContains(t, text, targetID+"-weekly-sync.md")
	aLacks(t, text, "meeting-notes.md")
	aEqual(t, aTimes(t, e, sourceSlug), before, "a mechanical fixup is not somebody editing their note")

	audit := plain(Doctor(e, DoctorOptions{Links: true}))
	aContains(t, audit, "in order")
	aEqual(t, aCommitCount(t, e, dir), commits+1, "one commit, not two")
	aAssertClean(t, e, dir, "and nothing left in the worktree")
}

// Why the match is on the id: after two retitles a link is two names behind,
// and an exact-name match would miss it.
func TestMvUpdateLinksCatchesALinkTwoRenamesBehind(t *testing.T) {
	e, _ := aInit(t)
	targetID, _, _, sourceSlug := aLinkedPair(t, e)

	if _, err := aMv(e, targetID, "Weekly sync", false, TouchStamp); err != nil {
		t.Fatal(err)
	}
	out := plain(aMv(e, targetID, "Team sync", true, TouchStamp))

	aContains(t, out, "updated  1 note")
	aContains(t, aNoteText(t, e, sourceSlug), targetID+"-team-sync.md")
	aContains(t, plain(Doctor(e, DoctorOptions{Links: true})), "in order")
}

// The flag means "make links say this note's current name", so it also repairs
// what an earlier rename left behind.
func TestMvUpdateLinksRepairsWithoutHavingToRetitleAgain(t *testing.T) {
	e, _ := aInit(t)
	targetID, _, _, sourceSlug := aLinkedPair(t, e)
	if _, err := aMv(e, targetID, "Weekly sync", false, TouchStamp); err != nil {
		t.Fatal(err)
	}

	out := plain(aMv(e, targetID, "Weekly sync", true, TouchStamp))
	aContains(t, out, "updated  1 note")
	aContains(t, aNoteText(t, e, sourceSlug), targetID+"-weekly-sync.md")
}

// Read back from the file the rename just wrote, not the copy from before.
func TestMvUpdateLinksReachesANoteThatLinksToItself(t *testing.T) {
	e, dir := aInit(t)
	id, _ := aParts(aAdd(t, e, "Alpha", "a\n"))

	path := filepath.Join(dir, id+"-alpha.md")
	raw, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	aWrite(t, path, string(raw)+"\nand [me]("+id+"-alpha.md)\n")

	if _, err := aMv(e, id, "Beta", true, TouchStamp); err != nil {
		t.Fatal(err)
	}
	aContains(t, aNoteText(t, e, "beta"), "[me]("+id+"-beta.md)")
}

// And skips the link walk, which is `doctor --links`' cost.
func TestARetitleThatKeepsTheSlugSaysNothingAboutLinks(t *testing.T) {
	e, _ := aInit(t)
	targetID, _, _, _ := aLinkedPair(t, e)

	out := plain(aMv(e, targetID, "  MEETING NOTES  ", false, TouchStamp))
	aEqual(t, out, targetID+"  meeting-notes", "summary only")
}

// The ids differ, so the filenames do.
func TestMvMayLandOnASlugAnotherNoteAlreadyUses(t *testing.T) {
	e, _ := aInit(t)
	alphaID, _ := aParts(aAdd(t, e, "Alpha", "a\n"))
	betaID, _ := aParts(aAdd(t, e, "Beta", "b\n"))

	if out, err := aMv(e, "alpha", "   ", false, TouchStamp); err == nil {
		t.Fatalf("a blank title was accepted: %q", out)
	}

	out, err := aMv(e, alphaID, "Beta", false, TouchStamp)
	if !strings.HasSuffix(aOK(t, out, err), "  beta") {
		t.Fatalf("no `-2` invented: %q", out)
	}
	if !strings.HasSuffix(aShow(t, e, betaID), "b\n") {
		t.Fatal("the other beta is untouched")
	}
	aEqual(t, len(strings.Split(strings.TrimRight(aLs(t, e, List{}), "\n"), "\n")), 2, "two lines")
}

func TestMvRefusesATitleTheFrontmatterCannotCarry(t *testing.T) {
	e, _ := aInit(t)
	aAdd(t, e, "Alpha", "a\n")

	out, err := aMv(e, "alpha", "Renamed\ntitle: hijacked", false, TouchStamp)
	aContains(t, aFail(t, out, err), "one line")
	aContains(t, aShow(t, e, "alpha"), "title: Alpha")
}

func TestEditCommitsWhatWasSaved(t *testing.T) {
	e, dir := aInit(t)
	id, _ := aParts(aAdd(t, e, "Alpha", "a\n"))
	before := aCommitCount(t, e, dir)

	editor := aEditorScript(t, "append", `printf 'appended\n' >> "$1"`)
	out, err := EditWith(e, "alpha", editor, TouchStamp)
	aEqual(t, aOK(t, out, err), id+"  alpha", "summary")
	aEqual(t, aCommitCount(t, e, dir), before+1, "one commit")
	aContains(t, aShow(t, e, "alpha"), "appended")
	aAssertClean(t, e, dir, "the edit is committed")
}

// `updated` is stamped in place: noda does not rearrange a block somebody just
// arranged in their editor.
func TestEditRecordsTheChangeWithoutRearrangingTheBlock(t *testing.T) {
	e, _ := aInit(t)
	aAdd(t, e, "Alpha", "a\n")

	editor := aEditorScript(t, "rewrite",
		`printf -- '---\nzebra: 1\ntitle: Alpha\nupdated: 2000-01-01T00:00:00Z\n---\n\nsaved\n' > "$1"`)
	if _, err := EditWith(e, "alpha", editor, TouchStamp); err != nil {
		t.Fatal(err)
	}

	text := aNoteText(t, e, "alpha")
	block, _, ok := strings.Cut(text, "\n---\n")
	if !ok {
		t.Fatalf("no frontmatter: %s", text)
	}
	var keys []string
	for _, line := range strings.Split(strings.TrimPrefix(block, "---\n"), "\n") {
		if key, _, found := strings.Cut(line, ":"); found {
			keys = append(keys, key)
		}
	}
	aEqual(t, keys, []string{"zebra", "title", "updated"}, "every line stayed where it was put: "+text)
	aLacks(t, text, "2000-01-01")
	if !strings.HasSuffix(text, "saved\n") {
		t.Fatalf("%s", text)
	}
}

func TestEditCommitsNothingWhenTheFileIsUntouched(t *testing.T) {
	e, dir := aInit(t)
	aAdd(t, e, "Alpha", "a\n")
	before := aCommitCount(t, e, dir)

	editor := aEditorScript(t, "noop", "true")
	out, err := EditWith(e, "alpha", editor, TouchStamp)
	aContains(t, aOK(t, out, err), "unchanged")
	aEqual(t, aCommitCount(t, e, dir), before, "no commit")
}

func TestEditRefusesToCommitABrokenNote(t *testing.T) {
	e, dir := aInit(t)
	file := aNoteFile(aAdd(t, e, "Alpha", "a\n"))
	before := aCommitCount(t, e, dir)

	wiped := aEditorScript(t, "wipe", `printf 'no frontmatter\n' > "$1"`)
	out, err := EditWith(e, "alpha", wiped, TouchStamp)
	aContains(t, aFail(t, out, err), "not committed")
	aEqual(t, aCommitCount(t, e, dir), before, "no commit")
	raw, readErr := os.ReadFile(filepath.Join(dir, file))
	if readErr != nil {
		t.Fatal(readErr)
	}
	aContains(t, string(raw), "no frontmatter") // the edit is left on disk to be fixed or discarded, never dropped
}

// The id is in the filename, which an editor cannot change.
func TestAnEditCannotChangeANotesIdentity(t *testing.T) {
	e, _ := aInit(t)
	id, _ := aParts(aAdd(t, e, "Alpha", "a\n"))

	// An `id:` line in the frontmatter is just another field.
	reid := aEditorScript(t, "reid", `printf -- '---\ntitle: Alpha\nid: zzzz\n---\n\nbody\n' > "$1"`)
	out, err := EditWith(e, "alpha", reid, TouchStamp)
	if !strings.HasPrefix(aOK(t, out, err), id) {
		t.Fatalf("the id is unmoved: %q", out)
	}
	aContains(t, aShow(t, e, id), "body")
}

func TestEditReportsAnAbortedEditor(t *testing.T) {
	e, _ := aInit(t)
	aAdd(t, e, "Alpha", "a\n")

	editor := aEditorScript(t, "abort", "exit 1")
	out, err := EditWith(e, "alpha", editor, TouchStamp)
	aContains(t, aFail(t, out, err), "exited with")
}

func TestRmDeletesTheNoteAndLeavesARevertibleCommit(t *testing.T) {
	e, dir := aInit(t)
	added := aAdd(t, e, "Alpha", "a\n", "work")
	id, _ := aParts(added)
	file := aNoteFile(added)
	aAdd(t, e, "Beta", "b\n")
	before := aCommitCount(t, e, dir)

	out, err := Rm(e, "alpha")
	aOK(t, out, err)
	aContains(t, out, id)
	aContains(t, out, "alpha")

	if _, err := os.Stat(filepath.Join(dir, file)); err == nil {
		t.Fatal("the file is still there")
	}
	for _, key := range []string{"alpha", id} {
		if out, err := Show(e, key); err == nil {
			t.Fatalf("%s still resolves: %q", key, out)
		}
	}
	aShow(t, e, "beta") // other notes are untouched

	aEqual(t, aCommitCount(t, e, dir), before+1, "one commit")
	aAssertClean(t, e, dir, "the removal is committed")

	// Still in history: the commit before HEAD carries the file.
	aRunGit(t, e, dir, "cat-file", "-e", "HEAD~1:"+file)
}

// Commands that only need to know which note must not refuse a broken one;
// they are the tools for clearing it up.
func TestTheCommandsThatDoNotReadANoteWorkOnOneThatCannotBeRead(t *testing.T) {
	e, dir := aInit(t)
	added := aAdd(t, e, "Alpha", "the original body\n")
	id, _ := aParts(added)
	file := aNoteFile(added)
	aWrite(t, filepath.Join(dir, file), "frontmatter is gone\n")

	// `Resolve` reads only the filename, and history is how you find out why it
	// will not parse.
	logged, err := Log(e, id, 0)
	aContains(t, aOK(t, logged, err), "add:")
	diffed, err := Diff(e, id, false)
	aContains(t, aOK(t, diffed, err), file)

	// It writes over the file, so it never needs to read it.
	restored, err := Restore(e, id, "HEAD", TouchStamp)
	aOK(t, restored, err)
	aContains(t, aShow(t, e, id), "the original body")
	if row, found := aStatusRow(plain(Status(e)), "problems"); found {
		t.Fatalf("the notebook is whole again, but problems says %q", row)
	}
}

func TestRmRemovesANoteWhoseFrontmatterIsGone(t *testing.T) {
	e, dir := aInit(t)
	added := aAdd(t, e, "Alpha", "a\n")
	id, _ := aParts(added)
	file := aNoteFile(added)

	aWrite(t, filepath.Join(dir, file), "broken\n")
	out, err := Rm(e, id)
	aContains(t, aOK(t, out, err), id) // the filename still said what it was
	if _, err := os.Stat(filepath.Join(dir, file)); err == nil {
		t.Fatal("the file is still there")
	}
}

func TestTheCommandsThatReadANoteStillRefuseOneThatCannotBeRead(t *testing.T) {
	e, dir := aInit(t)
	added := aAdd(t, e, "Alpha", "a\n")
	id, _ := aParts(added)
	aWrite(t, filepath.Join(dir, aNoteFile(added)), "frontmatter is gone\n")

	// These rewrite the frontmatter, so they must read it first.
	renamed, renameErr := aMv(e, id, "Renamed", false, TouchStamp)
	tagged, tagErr := Tag(e, id, []string{"+work"}, TouchStamp)
	aContains(t, aFail(t, renamed, renameErr), "frontmatter")
	aContains(t, aFail(t, tagged, tagErr), "frontmatter")
}

// Neither an id in its name nor frontmatter: a file, not a note.
func TestAFileThatIsNotANoteDoesNotResolve(t *testing.T) {
	e, dir := aInit(t)
	aAdd(t, e, "Alpha", "a\n")

	aWrite(t, filepath.Join(dir, "orphan.md"), "junk\n")
	out, err := Log(e, "orphan", 0)
	aContains(t, aFail(t, out, err), "not found")
}

func TestRmResolvesByIdAndReportsAnUnknownNote(t *testing.T) {
	e, _ := aInit(t)
	id, _ := aParts(aAdd(t, e, "Alpha", "a\n"))

	if out, err := Rm(e, "nope"); err == nil {
		t.Fatalf("an unknown note was removed: %q", out)
	}
	if out, err := Rm(e, id); err != nil {
		t.Fatalf("%q, %v", out, err)
	}
	if listed := aLs(t, e, List{}); listed != "" {
		t.Fatalf("got %q", listed)
	}
}
