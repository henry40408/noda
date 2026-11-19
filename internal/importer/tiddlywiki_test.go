package importer

import (
	"slices"
	"strings"
	"testing"
)

func stampText(value string) string {
	if got := stamp(value); got != nil {
		return *got
	}
	return "<none>"
}

func TestATimestampBecomesRFC3339WithItsMilliseconds(t *testing.T) {
	for value, want := range map[string]string{
		"20240515103310243": "2024-05-15T10:33:10.243Z",
		"20240515103310":    "2024-05-15T10:33:10Z",
		"":                  "<none>",
		"last tuesday":      "<none>",
	} {
		if got := stampText(value); got != want {
			t.Fatalf("%q: got %q, want %q", value, got, want)
		}
	}
}

func TestATitleListKeepsTheSpacesInsideDoubleBrackets(t *testing.T) {
	for field, want := range map[string][]string{
		"one two":                        {"one", "two"},
		"[[26.04 Occam's razor]] simple": {"26.04 Occam's razor", "simple"},
		"":                               nil,
		"same same":                      {"same"},
		"[[unclosed tag":                 {"unclosed tag"},
	} {
		if got := splitTags(field); !slices.Equal(got, want) {
			t.Fatalf("%q: got %q, want %q", field, got, want)
		}
	}
}

func mustRead(t *testing.T, text string) Export {
	t.Helper()
	export, err := ReadTiddlyWiki(text)
	if err != nil {
		t.Fatal(err)
	}
	return export
}

func TestTheFieldsNodaDoesNotClaimAreCarriedThrough(t *testing.T) {
	export := mustRead(t, `[{"title":"A","text":"body","creator":"henry","modifier":"henry",
	                         "revision":"0","bag":"default","type":"text/vnd.tiddlywiki"}]`)
	if got := export.Notes[0].Extra; !slices.Equal(got, []string{"creator: henry", "modifier: henry"}) {
		t.Fatalf("the wiki's bookkeeping is not the note's: %q", got)
	}
}

func TestAMultiLineFieldIsCarriedAsOneQuotedLine(t *testing.T) {
	export := mustRead(t, `[{"title":"A","text":"body","caption":"one\ntitle: two\r\n\"three\""}]`)
	want := []string{`caption: "one\ntitle: two\r\n\"three\""`}
	if got := export.Notes[0].Extra; !slices.Equal(got, want) {
		t.Fatalf("got %q, want %q", got, want)
	}
}

func TestQuotedEscapesOnlyWhatJSONRequires(t *testing.T) {
	if got, want := quoted("<a&b> \x01\t\b\f\x7f"), "\"<a&b> \\u0001\\t\\b\\f\x7f\""; got != want {
		t.Fatalf("got %q, want %q", got, want)
	}
}

func TestWhatIsNotANoteIsReportedRatherThanDropped(t *testing.T) {
	export := mustRead(t, `[{"title":"$:/config/x","text":"a"},
	                        {"title":"pic","text":"aGk=","type":"image/png"},
	                        {"title":"blank","text":"  "},
	                        {"title":"real","text":"body"}]`)
	if len(export.Notes) != 1 {
		t.Fatalf("%d notes", len(export.Notes))
	}
	var reasons []string
	for _, skipped := range export.Skipped {
		reasons = append(reasons, skipped.Reason)
	}
	if want := []string{"system tiddler", "not text (image/png)", "empty"}; !slices.Equal(reasons, want) {
		t.Fatalf("got %q, want %q", reasons, want)
	}
}

func TestASavedWikiIsReadOutOfItsStore(t *testing.T) {
	const open = `<script class="tiddlywiki-tiddler-store" type="application/json">`
	wiki := "<html>" + open + `[{"title":"Small","text":"x"}]` + "</script>\n" +
		open + `[{"title":"Big","text":"a body that makes this the larger store"}]` + "</script></html>"
	export := mustRead(t, wiki)
	if len(export.Notes) != 1 || export.Notes[0].Title != "Big" {
		t.Fatalf("%+v", export.Notes)
	}
}

func TestAFileThatIsNotAnExportSaysSo(t *testing.T) {
	_, err := ReadTiddlyWiki("# just some markdown\n")
	if err == nil || !strings.Contains(err.Error(), "no tiddler store") {
		t.Fatalf("a markdown file is not an export: %v", err)
	}
}

func TestAnExportThatIsNotAnArrayOrJSONIsRefused(t *testing.T) {
	_, err := ReadTiddlyWiki("[nope")
	if err == nil || !strings.HasPrefix(err.Error(), "this does not look like a TiddlyWiki export: ") {
		t.Fatalf("%v", err)
	}
	const open = `<script class="tiddlywiki-tiddler-store" type="application/json">`
	_, err = ReadTiddlyWiki(open + `{"title":"x"}</script>`)
	if err == nil || err.Error() != "a TiddlyWiki export is a JSON array of tiddlers" {
		t.Fatalf("%v", err)
	}
}

func TestAnEmptyStoreDoesNotHideTheNext(t *testing.T) {
	const open = `<script class="tiddlywiki-tiddler-store" type="application/json">`
	export := mustRead(t, open+"</script>"+open+`[{"title":"Real","text":"x"}]`+"</script>")
	if len(export.Notes) != 1 || export.Notes[0].Title != "Real" {
		t.Fatalf("%+v", export.Notes)
	}
}

func TestOfTwoStoresOfTheSameSizeTheFirstIsTheWiki(t *testing.T) {
	const open = `<script class="tiddlywiki-tiddler-store" type="application/json">`
	wiki := open + `[{"title":"AAA","text":"x"}]</script>` + open + `[{"title":"BBB","text":"x"}]</script>`
	export := mustRead(t, wiki)
	if len(export.Notes) != 1 || export.Notes[0].Title != "AAA" {
		t.Fatalf("%+v", export.Notes)
	}
}
