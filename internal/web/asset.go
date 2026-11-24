package web

import (
	"crypto/sha1"
	_ "embed"
	"encoding/hex"
	"strconv"
	"sync"
)

// The stylesheet, icon and scripts are linked and cached rather than inlined.
//
// The name is the content: /a/style.<hash>.css cannot go stale, so it is
// served immutable. The ways to get it wrong are serving a hash nobody wrote
// (so a miss is a 404) and caching the page that links it (no-cache).
//
// There is no bundle: a page links only the scripts it uses.

//go:embed assets/page.css
var pageStylesheet string

//go:embed assets/listing.js
var listingScript string

//go:embed assets/standing.js
var standingScript string

//go:embed assets/panes.js
var panesScript string

//go:embed assets/keys.js
var keysScript string

//go:embed assets/sending.js
var sendingScript string

//go:embed assets/stamps.js
var stampsScript string

//go:embed assets/watching.js
var watchingScript string

const iconSVG = "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 32 32'>" +
	"<rect width='32' height='32' rx='7' fill='#1f2430'/>" +
	"<path d='M9.5 23V11M9.5 15.5Q9.5 11 15.5 11T21.5 15.5V23' fill='none' stroke='#fff' " +
	"stroke-width='3.4' stroke-linecap='round' stroke-linejoin='round'/>" +
	"<circle cx='24' cy='8' r='3' fill='#f5c542'/></svg>"

// Asset is one thing a page links.
type Asset int

const (
	AssetIcon     Asset = iota // the tab's icon: an "n" with a commit dot
	AssetStyle                 // the whole of the layout, both themes included
	AssetListing               // the listing's filter
	AssetStanding              // the network screen's poll
	AssetPanes                 // the two panes
	AssetKeys                  // the keyboard
	AssetSending               // a write, answered where it was made
	AssetStamps                // every stamp, in the reader's own zone
	AssetWatching              // the editor's ear on the note under it
)

var allAssets = []Asset{
	AssetIcon, AssetStyle, AssetListing, AssetStanding,
	AssetPanes, AssetKeys, AssetSending, AssetStamps, AssetWatching,
}

var assetNames = map[Asset]string{
	AssetIcon: "icon", AssetStyle: "style", AssetListing: "listing", AssetStanding: "standing",
	AssetPanes: "panes", AssetKeys: "keys", AssetSending: "sending", AssetStamps: "stamps", AssetWatching: "watching",
}

func (a Asset) extension() string {
	switch a {
	case AssetIcon:
		return "svg"
	case AssetStyle:
		return "css"
	}
	return "js"
}

// Kind is sent with nosniff, so it is all the browser will treat it as.
func (a Asset) Kind() string {
	switch a {
	case AssetIcon:
		return "image/svg+xml"
	case AssetStyle:
		return "text/css; charset=utf-8"
	}
	return "text/javascript; charset=utf-8"
}

func (a Asset) body() string {
	switch a {
	case AssetIcon:
		return iconSVG
	case AssetStyle:
		return themeStylesheet() + pageStylesheet
	case AssetListing:
		return listingScript
	case AssetStanding:
		return standingScript
	case AssetPanes:
		return panesScript
	case AssetKeys:
		return keysScript
	case AssetSending:
		return sendingScript
	case AssetStamps:
		return stampsScript
	}
	return watchingScript
}

// Href is where a page finds it.
func (a Asset) Href() string { return held()[a].at }

// Tag is the markup that links it. Scripts are deferred: they read the rows,
// so they run after parsing but download during it.
func (a Asset) Tag() string {
	switch a {
	case AssetIcon:
		return "<link rel=\"icon\" type=\"image/svg+xml\" href=\"" + a.Href() + "\">"
	case AssetStyle:
		return "<link rel=\"stylesheet\" href=\"" + a.Href() + "\">"
	}
	return "<script src=\"" + a.Href() + "\" defer></script>"
}

// HeldAsset is an asset with its address worked out.
type HeldAsset struct {
	at   string
	file string
	Body string
	Kind string
}

var (
	heldOnce   sync.Once
	heldAssets []HeldAsset
)

// held is hashed at first use, so commands that serve no page never pay for it.
func held() []HeldAsset {
	heldOnce.Do(func() {
		for _, asset := range allAssets {
			body := asset.body()
			file := assetNames[asset] + "." + fingerprint(body) + "." + asset.extension()
			heldAssets = append(heldAssets, HeldAsset{at: "/a/" + file, file: file, Body: body, Kind: asset.Kind()})
		}
	})
	return heldAssets
}

// fingerprint is twelve hex digits of the git blob id: a cache key over a
// handful of strings, not an identity.
func fingerprint(body string) string {
	sum := sha1.Sum([]byte("blob " + strconv.Itoa(len(body)) + "\x00" + body))
	return hex.EncodeToString(sum[:])[:12]
}

// FindAsset matches exactly: answering a stale hash with current bytes would
// break the promise the address makes.
func FindAsset(file string) (HeldAsset, bool) {
	for _, asset := range held() {
		if asset.file == file {
			return asset, true
		}
	}
	return HeldAsset{}, false
}
