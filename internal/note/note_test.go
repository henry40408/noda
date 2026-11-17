package note

import (
	"reflect"
	"strings"
	"testing"
	"time"
)

func ptr(s string) *string { return &s }

func mustParse(t *testing.T, text string) Note {
	t.Helper()
	n, err := Parse(text)
	if err != nil {
		t.Fatal(err)
	}
	return n
}

func TestSetBodyKeepsTheFrontmatterExactly(t *testing.T) {
	text := "---\nweird: yes\ntitle: Kept\nsomebody-elses: field\n---\n\nold body\n"
	got, ok := SetBody(text, "new body")
	want := "---\nweird: yes\ntitle: Kept\nsomebody-elses: field\n---\n\nnew body\n"
	if !ok || got != want {
		t.Fatalf("got %q", got)
	}
}

func TestSetBodyTakesTheCarriageReturnsOut(t *testing.T) {
	got, _ := SetBody("---\ntitle: T\n---\n\nold\n", "one\r\ntwo\r\n")
	if got != "---\ntitle: T\n---\n\none\ntwo\n" {
		t.Fatalf("got %q", got)
	}
}

func TestSetBodyGivesUpOnAFileWithNoFrontmatter(t *testing.T) {
	if _, ok := SetBody("just a markdown file\n", "x"); ok {
		t.Fatal("expected no result")
	}
}

func TestSetBodyAcceptsNothingAtAll(t *testing.T) {
	got, _ := SetBody("---\ntitle: T\n---\n\nsomething\n", "")
	if got != "---\ntitle: T\n---\n\n" {
		t.Fatalf("got %q", got)
	}
	mustParse(t, got)
}

func TestSlugifyCollapsesPunctuationAndLowercases(t *testing.T) {
	cases := map[string]string{
		"Meeting Notes":       "meeting-notes",
		"  Q3 // planning!! ": "q3-planning",
		"C++ vs Rust":         "c-vs-rust",
		"會議 筆記":               "會議-筆記",
		"!!!":                 "note",
		"":                    "note",
	}
	for title, want := range cases {
		if got := Slugify(title); got != want {
			t.Errorf("%q: got %q, want %q", title, got, want)
		}
	}
}

func TestSlugifyCutsALongTitleAtAWordBoundary(t *testing.T) {
	slug := Slugify("GitHub - coding-horror/basic-computer-games: An updated version of the " +
		"classic Basic Computer Games book, with well-written examples in a variety " +
		"of common MEMORY SAFE, SCRIPTING programming languages.")
	want := "github-coding-horror-basic-computer-games-an-updated-version-of-the-classic-basic-computer-games"
	if slug != want {
		t.Fatalf("got %q", slug)
	}
	if len(FileName("k3f9m2p1", slug)) > MaxFileNameLen {
		t.Fatal("file name too long")
	}
}

func TestSlugifyCutsASingleLongWordAtTheCap(t *testing.T) {
	if got := Slugify(strings.Repeat("a", 300)); len(got) != maxSlugLen {
		t.Fatalf("%d bytes", len(got))
	}
}

func TestSlugifyCutsCJKOnACharacterBoundary(t *testing.T) {
	got := Slugify(strings.Repeat("漢", 200))
	if got != strings.Repeat("漢", maxSlugLen/3) {
		t.Fatalf("%d bytes", len(got))
	}
}

func TestSlugifyLeavesNoTrailingDash(t *testing.T) {
	for extra := range 8 {
		slug := Slugify(strings.Repeat("ab ", 40) + " " + strings.Repeat("c", extra))
		if strings.HasSuffix(slug, "-") || len(slug) > maxSlugLen {
			t.Errorf("%q", slug)
		}
	}
}

func TestSlugifyLowercasesDottedCapitalIAsRustDoes(t *testing.T) {
	if got := Slugify("İ"); got != "i̇" {
		t.Fatalf("got %q", got)
	}
}

func TestLowerFoldsAFinalSigmaSoBothSpellingsMatch(t *testing.T) {
	if Lower("ΟΔΟΣ") != Lower("οδος") || Lower("οδοσ") != Lower("οδος") {
		t.Fatalf("%q %q", Lower("ΟΔΟΣ"), Lower("οδος"))
	}
}

func TestNormalizeIDFoldsConfusableCharacters(t *testing.T) {
	if got := NormalizeID("K3F9ABCD"); got != "k3f9abcd" {
		t.Error(got)
	}
	if got := NormalizeID("IL0O"); got != "1100" {
		t.Error(got)
	}
}

func TestMintedIDsUseTheCrockfordAlphabetAndAvoidCollisions(t *testing.T) {
	id := MintID(map[string]struct{}{})
	if len(id) != IDLen || !IsIDShaped(id) || strings.ToLower(id) != id {
		t.Fatalf("id %q", id)
	}
	taken := map[string]struct{}{NormalizeID(id): {}}
	if MintID(taken) == id {
		t.Fatal("minted a taken id")
	}
}

func TestAWidenedIDStaysRandomPastTheFirstDraw(t *testing.T) {
	long := randomID(24)
	if len(long) != 24 || strings.Trim(long[charsPerDraw:], "0") == "" {
		t.Fatalf("the tail past one draw is all zeros: %s", long)
	}
}

func TestAFilenameSplitsIntoTheIDAndTheSlug(t *testing.T) {
	for stem, want := range map[string][2]string{
		"k3f9m2p1-meeting-notes": {"k3f9m2p1", "meeting-notes"},
		"k3f9m2p1-會議-筆記":         {"k3f9m2p1", "會議-筆記"},
		"abcdefgh-hello":         {"abcdefgh", "hello"},
	} {
		id, slug, ok := SplitStem(stem)
		if !ok || id != want[0] || slug != want[1] {
			t.Errorf("%s: %q %q %v", stem, id, slug, ok)
		}
	}
}

func TestASlugThatMerelyStartsWithAHyphenatedWordIsNotAnID(t *testing.T) {
	for _, stem := range []string{"c-vs-rust", "meeting-notes", "nohyphen", "k3f9m2p1", "k3f9m2p1-", "untitled-thing"} {
		if _, _, ok := SplitStem(stem); ok {
			t.Errorf("%q read as a note", stem)
		}
	}
}

func TestNamesANoteNeedsMdAndAnIDShapedStem(t *testing.T) {
	for name, want := range map[string]bool{
		"k3f9m2p1-hello.md":  true,
		"README.md":          false,
		"k3f9m2p1-hello.txt": false,
		"k3f9m2p1-hello":     false,
	} {
		if NamesANote(name) != want {
			t.Errorf("%s: want %v", name, want)
		}
	}
}

func TestRenderAndParseRoundTrip(t *testing.T) {
	n := Note{
		Title:   "Meeting notes",
		Tags:    []string{"work", "q3"},
		Created: ptr("2019-03-14T08:21:00Z"),
		Updated: ptr("2024-11-02T16:40:12Z"),
		Body:    "Body line one.\nBody line two.\n",
	}
	text := n.Render()
	prefix := "---\ntitle: Meeting notes\ntags: [work, q3]\ncreated: 2019-03-14T08:21:00Z\nupdated: 2024-11-02T16:40:12Z\n---\n\n"
	if !strings.HasPrefix(text, prefix) || strings.Contains(text, "id:") {
		t.Fatalf("got %q", text)
	}
	if got := mustParse(t, text); !reflect.DeepEqual(got, n) {
		t.Fatalf("got %+v", got)
	}
}

func TestAPinRoundTripsAfterTheStamps(t *testing.T) {
	n := Note{
		Title:   "Meeting notes",
		Updated: ptr("2024-11-02T16:40:12Z"),
		Pinned:  ptr(PinnedValue),
		Extra:   []string{"source: elsewhere"},
		Body:    "hi\n",
	}
	text := n.Render()
	prefix := "---\ntitle: Meeting notes\nupdated: 2024-11-02T16:40:12Z\npinned: true\nsource: elsewhere\n---\n\n"
	if !strings.HasPrefix(text, prefix) {
		t.Fatalf("got %q", text)
	}
	if got := mustParse(t, text); !reflect.DeepEqual(got, n) || !n.IsPinned() {
		t.Fatalf("got %+v", got)
	}
}

func TestAnUnpinnedNoteWritesNoLine(t *testing.T) {
	text := "---\ntitle: Alpha\n---\n\nbody\n"
	n := mustParse(t, text)
	if n.IsPinned() || n.Render() != text {
		t.Fatal("an unpinned note must render as found")
	}
	n.Pinned = ptr(PinnedValue)
	if !strings.Contains(n.Render(), "pinned: true") {
		t.Fatal("pin missing")
	}
	n.Pinned = nil
	if n.Render() != text {
		t.Fatal("unpin must restore the file")
	}
}

func TestAPinIsTrueAndNothingElseButSurvivesBeingSomethingElse(t *testing.T) {
	for value, pinned := range map[string]bool{
		"true": true, "True": true, "TRUE": true, "false": false, "yes": false, "1": false, "": false,
	} {
		text := "---\ntitle: Alpha\npinned: " + value + "\n---\n\nbody\n"
		n := mustParse(t, text)
		if n.IsPinned() != pinned || n.Render() != text {
			t.Errorf("pinned: %q", value)
		}
	}
}

func TestParseToleratesMissingTagsAndColonsInTitles(t *testing.T) {
	n := mustParse(t, "---\ntitle: Rust: a tour\n---\n\nhi\n")
	if n.Title != "Rust: a tour" || len(n.Tags) != 0 || n.Body != "hi\n" {
		t.Fatalf("got %+v", n)
	}
}

func TestAStrayIDFieldIsIgnoredRatherThanObeyed(t *testing.T) {
	n := mustParse(t, "---\nid: zzzz\ntitle: Alpha\n---\n\nbody\n")
	if n.Title != "Alpha" || !reflect.DeepEqual(n.Extra, []string{"id: zzzz"}) || !strings.Contains(n.Render(), "id: zzzz") {
		t.Fatalf("got %+v", n)
	}
}

func TestFieldsNodaDoesNotKnowSurviveAWriteBack(t *testing.T) {
	n := mustParse(t, "---\ntitle: Imported\nsource_id: 4821\nstarred: true\ntags: [work]\n---\n\nbody\n")
	if !reflect.DeepEqual(n.Extra, []string{"source_id: 4821", "starred: true"}) {
		t.Fatalf("extra %v", n.Extra)
	}
	n.Tags = append(n.Tags, "q3")
	rewritten := n.Render()
	if !strings.Contains(rewritten, "source_id: 4821") || !strings.Contains(rewritten, "starred: true") {
		t.Fatal(rewritten)
	}
	if got := mustParse(t, rewritten); !reflect.DeepEqual(got, n) {
		t.Fatalf("got %+v", got)
	}
}

func TestUnknownFieldsKeepTheirSequenceButMoveBelowTheKnownOnes(t *testing.T) {
	n := mustParse(t, "---\nzebra: 1\ntitle: Alpha\nalpha: 2\n---\n\nbody\n")
	if !reflect.DeepEqual(n.Extra, []string{"zebra: 1", "alpha: 2"}) {
		t.Fatalf("extra %v", n.Extra)
	}
	if n.Render() != "---\ntitle: Alpha\nzebra: 1\nalpha: 2\n---\n\nbody\n" {
		t.Fatalf("got %q", n.Render())
	}
}

func TestATimeWrittenSomewhereElseIsNotRestated(t *testing.T) {
	n := mustParse(t, "---\ntitle: Imported\ncreated: 2019-03-14T16:21:00+08:00\n---\n\nbody\n")
	if n.Created == nil || *n.Created != "2019-03-14T16:21:00+08:00" {
		t.Fatalf("got %v", n.Created)
	}
	if !strings.Contains(n.Render(), "created: 2019-03-14T16:21:00+08:00") {
		t.Fatal("offset lost")
	}
}

func TestTheTimeNodaWritesIsFixedWidthUTC(t *testing.T) {
	now := Now()
	if len(now) != 20 || !strings.HasSuffix(now, "Z") {
		t.Fatal(now)
	}
	if _, err := time.Parse(time.RFC3339, now); err != nil {
		t.Fatal(err)
	}
}

func TestSettingAFieldMovesNothingElse(t *testing.T) {
	got, _ := SetField("---\nzebra: 1\nupdated: old\ntitle: Alpha\n---\n\nbody\n", "updated", "new")
	if got != "---\nzebra: 1\nupdated: new\ntitle: Alpha\n---\n\nbody\n" {
		t.Fatalf("got %q", got)
	}
}

func TestSettingAFieldThatIsNotThereYetAppendsIt(t *testing.T) {
	got, _ := SetField("---\ntitle: Alpha\n---\n\nbody\n", "updated", "new")
	if got != "---\ntitle: Alpha\nupdated: new\n---\n\nbody\n" {
		t.Fatalf("got %q", got)
	}
	got, _ = SetField("---\n---\n\nbody\n", "updated", "new")
	if got != "---\nupdated: new\n---\n\nbody\n" {
		t.Fatalf("got %q", got)
	}
	if _, ok := SetField("no frontmatter\n", "updated", "new"); ok {
		t.Fatal("expected no result")
	}
}

func TestSettingADuplicatedFieldCollapsesIt(t *testing.T) {
	got, _ := SetField("---\nupdated: a\ntitle: Alpha\nupdated: b\n---\n\nbody\n", "updated", "new")
	if got != "---\nupdated: new\ntitle: Alpha\n---\n\nbody\n" {
		t.Fatalf("got %q", got)
	}
}

func TestAFrontmatterLineWithoutAColonIsCarriedAsWell(t *testing.T) {
	n := mustParse(t, "---\ntitle: Alpha\n# a comment\n---\n\nbody\n")
	if !reflect.DeepEqual(n.Extra, []string{"# a comment"}) {
		t.Fatalf("extra %v", n.Extra)
	}
	if got := mustParse(t, n.Render()); !reflect.DeepEqual(got, n) {
		t.Fatalf("got %+v", got)
	}
}

func TestValuesThatWouldNotSurviveTheRoundTripAreRefused(t *testing.T) {
	if ValidateTitle("Meeting Notes") != nil {
		t.Fatal("plain title refused")
	}
	if err := ValidateTitle("Meeting\ntitle: other"); err == nil || !strings.Contains(err.Error(), "one line") {
		t.Fatalf("got %v", err)
	}
	if ValidateTitle("Meeting\rnotes") == nil {
		t.Fatal("CR accepted")
	}
	if ValidateTag("work") != nil || ValidateTag("會議") != nil {
		t.Fatal("good tag refused")
	}
	for _, bad := range []string{"", "work, secret", "a]", "[a", "two\nlines"} {
		if ValidateTag(bad) == nil {
			t.Errorf("%q should be refused", bad)
		}
	}
}

func TestTagErrorsNameTheOffender(t *testing.T) {
	if err := ValidateTag("a,b"); err == nil || err.Error() != "a tag cannot contain `,`: a,b" {
		t.Fatalf("got %v", err)
	}
	if err := ValidateTag("a\nb"); err == nil || err.Error() != "a tag cannot contain a line break: a\nb" {
		t.Fatalf("got %v", err)
	}
}

func TestParseRejectsFilesWithoutFrontmatter(t *testing.T) {
	for _, text := range []string{"just markdown\n", "# A heading\n\nprose\n"} {
		if _, err := Parse(text); err == nil || err.Error() != "note has no frontmatter block" {
			t.Errorf("%q: %v", text, err)
		}
	}
	mustParse(t, "---\n---\n\nbody\n")
}

func TestLinesSplitLikeRust(t *testing.T) {
	cases := map[string][]string{
		"":         nil,
		"a":        {"a"},
		"a\n":      {"a"},
		"a\r\nb\n": {"a", "b"},
		"a\n\nb":   {"a", "", "b"},
		"a\r":      {"a\r"},
		"a\rb\n":   {"a\rb"},
		"\n":       {""},
	}
	for text, want := range cases {
		if got := lines(text); !reflect.DeepEqual(got, want) {
			t.Errorf("%q: got %q, want %q", text, got, want)
		}
	}
}
