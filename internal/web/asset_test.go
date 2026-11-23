package web

import (
	"fmt"
	"regexp"
	"strconv"
	"strings"
	"testing"

	"github.com/henry40408/noda/internal/style"
)

func TestAnAssetIsLinkedByNameKindAndContentHash(t *testing.T) {
	tests := []struct {
		asset Asset
		href  string
		tag   string
		kind  string
	}{
		{AssetIcon, `^/a/icon\.[0-9a-f]{12}\.svg$`, `<link rel="icon" type="image/svg+xml" href="%s">`, "image/svg+xml"},
		{AssetStyle, `^/a/style\.[0-9a-f]{12}\.css$`, `<link rel="stylesheet" href="%s">`, "text/css; charset=utf-8"},
		{AssetListing, `^/a/listing\.[0-9a-f]{12}\.js$`, `<script src="%s" defer></script>`, "text/javascript; charset=utf-8"},
		{AssetStanding, `^/a/standing\.[0-9a-f]{12}\.js$`, `<script src="%s" defer></script>`, "text/javascript; charset=utf-8"},
		{AssetPanes, `^/a/panes\.[0-9a-f]{12}\.js$`, `<script src="%s" defer></script>`, "text/javascript; charset=utf-8"},
		{AssetKeys, `^/a/keys\.[0-9a-f]{12}\.js$`, `<script src="%s" defer></script>`, "text/javascript; charset=utf-8"},
		{AssetSending, `^/a/sending\.[0-9a-f]{12}\.js$`, `<script src="%s" defer></script>`, "text/javascript; charset=utf-8"},
		{AssetStamps, `^/a/stamps\.[0-9a-f]{12}\.js$`, `<script src="%s" defer></script>`, "text/javascript; charset=utf-8"},
		{AssetWatching, `^/a/watching\.[0-9a-f]{12}\.js$`, `<script src="%s" defer></script>`, "text/javascript; charset=utf-8"},
	}
	if len(tests) != len(allAssets) {
		t.Fatalf("%d cases for %d assets", len(tests), len(allAssets))
	}
	for _, tt := range tests {
		if !regexp.MustCompile(tt.href).MatchString(tt.asset.Href()) {
			t.Errorf("href %q does not match %s", tt.asset.Href(), tt.href)
		}
		if want := fmt.Sprintf(tt.tag, tt.asset.Href()); tt.asset.Tag() != want {
			t.Errorf("tag %q, want %q", tt.asset.Tag(), want)
		}
		if tt.asset.Kind() != tt.kind {
			t.Errorf("%s: kind %q, want %q", tt.asset.Href(), tt.asset.Kind(), tt.kind)
		}
		if tt.asset.body() == "" {
			t.Errorf("%s has no body", tt.asset.Href())
		}
	}
}

// The expected values are what `git hash-object` prints, so the address of an
// asset stays a git blob id and a change of body changes it.
func TestFingerprintIsTheGitBlobIDPrefix(t *testing.T) {
	tests := map[string]string{
		"":        "e69de29bb2d1",
		"hello\n": "ce013625030b",
	}
	for body, want := range tests {
		if got := fingerprint(body); got != want {
			t.Errorf("fingerprint(%q) = %s, want %s", body, got, want)
		}
	}
}

func TestWhatAPageLinksIsWhatTheRouteAnswers(t *testing.T) {
	for _, asset := range allAssets {
		found, ok := FindAsset(strings.TrimPrefix(asset.Href(), "/a/"))
		if !ok || found.Body != asset.body() || found.Kind != asset.Kind() {
			t.Errorf("%s is not answered", asset.Href())
		}
	}
}

func TestTheStylesheetDefinesEveryHueAndNoOther(t *testing.T) {
	sheet := AssetStyle.body()
	for index := range style.Hues {
		if rule := ".h" + strconv.Itoa(index) + "{--s:oklch(var(--sl) "; !strings.Contains(sheet, rule) {
			t.Errorf("no rule %q", rule)
		}
	}
	if past := ".h" + strconv.Itoa(style.Hues) + "{"; strings.Contains(sheet, past) {
		t.Errorf("a rule for %q, which nothing wears", past)
	}
}

// Whatever a name wears must be styled, or its row would draw with no accent.
func TestEveryNameWearsAClassTheStylesheetDefines(t *testing.T) {
	sheet := AssetStyle.body()
	for _, name := range []string{"01abc234", "default", "", "日本語", strings.Repeat("z", 200)} {
		if rule := "." + hueClass(name) + "{--s:"; !strings.Contains(sheet, rule) {
			t.Errorf("%q wears %s, which is not defined", name, hueClass(name))
		}
	}
}

// Dark is only what the reader's device asks for, so the dark surfaces come
// after the media query opens and the light ones before it.
func TestTheDarkThemeLivesInsideTheMediaQuery(t *testing.T) {
	sheet := AssetStyle.body()
	query := strings.Index(sheet, "@media (prefers-color-scheme:dark){")
	light, dark := strings.Index(sheet, "--sheet:"+lightTheme.sheet), strings.Index(sheet, "--sheet:"+darkTheme.sheet)
	if query < 0 || light < 0 || dark < 0 || light >= query || query >= dark {
		t.Fatalf("light at %d, media query at %d, dark at %d", light, query, dark)
	}
}

// The three inks are asked of style, so a page and a terminal agree on what is
// muted and what is red.
func TestTheInksFollowTheStyleTheyAreAskedFor(t *testing.T) {
	for name, theme := range map[string]terminal{"light": lightTheme, "dark": darkTheme} {
		properties := theme.properties()
		for _, ink := range []struct{ property, colour string }{
			{"--ink", theme.text}, {"--soft", theme.dim}, {"--go", theme.text}, {"--bad", theme.red},
		} {
			if want := ink.property + ":" + ink.colour + ";"; !strings.Contains(properties, want) {
				t.Errorf("%s theme: no %s in %s", name, want, properties)
			}
		}
	}
}

func TestAnAddressThisBuildDidNotWriteIsNotAnswered(t *testing.T) {
	for _, file := range []string{"style.000000000000.css", "style.css", "../../etc/passwd"} {
		if _, ok := FindAsset(file); ok {
			t.Errorf("%s answered", file)
		}
	}
}
