package web

import (
	"encoding/json"
	"os"
	"strings"
	"testing"
)

func TestAssetsMatchGolden(t *testing.T) {
	var expected []struct {
		Bytes int
		File  string
		Href  string
		Kind  string
		Tag   string
	}
	raw, err := os.ReadFile("../../testdata/golden/asset/expected.json")
	if err != nil {
		t.Fatal(err)
	}
	if err := json.Unmarshal(raw, &expected); err != nil {
		t.Fatal(err)
	}
	if len(expected) != len(allAssets) {
		t.Fatalf("%d assets, golden has %d", len(allAssets), len(expected))
	}
	for i, asset := range allAssets {
		want := expected[i]
		if asset.Href() != want.Href || asset.Tag() != want.Tag || asset.Kind() != want.Kind {
			t.Errorf("%s: got %s %s %s", want.File, asset.Href(), asset.Tag(), asset.Kind())
		}
		if len(asset.body()) != want.Bytes {
			t.Errorf("%s: %d bytes, want %d", want.File, len(asset.body()), want.Bytes)
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
