package web

import (
	"fmt"
	"regexp"
	"strings"
	"testing"
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
		{AssetBeside, `^/a/beside\.[0-9a-f]{12}\.js$`, `<script src="%s" defer></script>`, "text/javascript; charset=utf-8"},
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

func TestAnAddressThisBuildDidNotWriteIsNotAnswered(t *testing.T) {
	for _, file := range []string{"style.000000000000.css", "style.css", "../../etc/passwd"} {
		if _, ok := FindAsset(file); ok {
			t.Errorf("%s answered", file)
		}
	}
}
