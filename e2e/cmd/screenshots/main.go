// Command screenshots regenerates the four screenshots README.md embeds, into
// ../screenshots/:
//
//	cd e2e && go run ./cmd/screenshots
//
// The notebook is its own, written by the binary like the features' fixture,
// so what the pictures show is what `add` really writes. Font rendering and
// the Chrome version differ by machine: an unrelated image changing in the
// diff means the browser changed, not the page.
package main

import (
	"fmt"
	"os"
	"path/filepath"
	"regexp"
	"strings"
	"time"

	"github.com/henry40408/noda/e2e/internal/harness"
)

// note is one note with its tags and body. A body names a note it links to by
// title as {Title}, resolved to a filename once that note exists.
type note struct {
	title string
	tags  []string
	body  string
}

// notes are ordered so every note a body links to is written before it. The
// open one, "Notes on TAOCP", is linked from three others, so its margin has
// company.
var notes = []note{
	{"Notes on TAOCP", []string{"books", "algorithms"}, "## Volume 1: Fundamental Algorithms\n\n" +
		"Knuth builds everything from a small machine, **MIX**, so that the cost of " +
		"an algorithm is a number of instructions and not a guess.\n\n" +
		"- Algorithm E, Euclid's: `gcd(m, n)` ends when the remainder is zero\n" +
		"- Linked allocation beats a contiguous array when the size is unknown\n" +
		"- Every analysis starts by counting what the inner loop does\n\n" +
		"```\n" +
		"E1. [Find remainder.] Divide m by n; let r be the remainder.\n" +
		"E2. [Is it zero?] If r = 0, the algorithm terminates; n is the answer.\n" +
		"E3. [Reduce.] Set m <- n, n <- r, and go back to E1.\n" +
		"```\n\n" +
		"> Premature optimization is the root of all evil.\n"},
	{"Reading list", []string{"books"}, "What is next, and what has been read.\n\n" +
		"- [Notes on TAOCP]({Notes on TAOCP}), volume 1, partway\n" +
		"- Designing Data-Intensive Applications\n" +
		"- The Pragmatic Programmer\n"},
	{"Algorithms cheat sheet", []string{"algorithms"}, "Sorting, searching and the bounds to remember. The long form is in " +
		"[Notes on TAOCP]({Notes on TAOCP}).\n\n" +
		"| Operation | Cost |\n|---|---|\n| binary search | O(log n) |\n| heap push | O(log n) |\n"},
	{"Weekly planning", []string{"work", "planning"}, "The week of the Q3 review.\n\n" +
		"- [ ] send the budget draft to Ana due:2999-06-14\n" +
		"- [ ] book the offsite venue due:2999-06-20\n" +
		"- [ ] chase the marketing line due:2000-01-01\n" +
		"- [x] pull the ledger export\n\n" +
		"Reading for the train: [Notes on TAOCP]({Notes on TAOCP}).\n"},
	{"Trip to Kyoto", []string{"travel"}, "Late autumn, five days.\n\n" +
		"- [ ] reserve the ryokan due:2999-09-01\n" +
		"- [ ] buy a rail pass\n" +
		"- [x] renew the passport\n\n" +
		"Fushimi Inari early, before the crowds.\n"},
	{"Home server runbook", []string{"ops", "home"}, "How the box in the closet is kept alive.\n\n" +
		"- Backups run nightly to the second disk\n" +
		"- `noda sync` pushes this notebook to the private remote\n"},
}

// pinned is the note the seed pins.
const pinned = "weekly-planning"

// screen is a note beside its listing and backlinks on a monitor, or the
// listing alone on a phone, which is what `noda web` is for.
type screen struct {
	name     string
	path     string
	viewport harness.Viewport
}

var screens = []screen{
	{"web-note", "/n/notes-on-taocp", harness.Monitor},
	{"web-phone", "", harness.Phone},
}

func main() {
	if err := run(); err != nil {
		fmt.Fprintln(os.Stderr, "screenshots:", err)
		os.Exit(1)
	}
}

func run() error {
	output, err := screenshotDirectory()
	if err != nil {
		return err
	}
	binary, err := harness.EnsureBinary()
	if err != nil {
		return err
	}
	server, err := harness.StartServer(binary, writeDemo)
	if err != nil {
		return err
	}
	defer server.Stop()

	for _, scheme := range []string{"light", "dark"} {
		if err := take(server, scheme, output); err != nil {
			return err
		}
	}
	fmt.Printf("screenshots: wrote four images to %s\n", output)
	return nil
}

// screenshotDirectory is where README.md looks for the images: the
// repository root, one above the e2e module the command runs in.
func screenshotDirectory() (string, error) {
	here, err := os.Getwd()
	if err != nil {
		return "", err
	}
	output := filepath.Join(here, "..", "screenshots")
	return output, os.MkdirAll(output, 0o755)
}

func writeDemo(w harness.Workspace) error {
	if _, err := w.Run("init"); err != nil {
		return err
	}
	files := map[string]string{}
	for _, entry := range notes {
		body := entry.body
		for title, file := range files {
			body = strings.ReplaceAll(body, "{"+title+"}", file)
		}
		args := []string{"add", entry.title, "-c", body}
		for _, tag := range entry.tags {
			args = append(args, "--tag", tag)
		}
		if _, err := w.Run(args...); err != nil {
			return err
		}
		file, err := w.NoteFile(slug(entry.title))
		if err != nil {
			return err
		}
		files[entry.title] = file
	}
	_, err := w.Run("pin", pinned)
	return err
}

var nonAlphanumeric = regexp.MustCompile(`[^a-z0-9]+`)

// slug is what `noda path` takes: the title as the filename spells it.
func slug(title string) string {
	return strings.Trim(nonAlphanumeric.ReplaceAllString(strings.ToLower(title), "-"), "-")
}

// take shoots both screens in one colour scheme.
func take(server *harness.Server, scheme, output string) error {
	browser, err := harness.Open(true)
	if err != nil {
		return err
	}
	defer browser.Close()
	if err := browser.PreferScheme(scheme); err != nil {
		return err
	}
	suffix := ""
	if scheme == "dark" {
		suffix = "-dark"
	}
	page := harness.Page{Browser: browser, BaseURL: server.BaseURL}
	for _, shot := range screens {
		if err := browser.SetViewport(shot.viewport); err != nil {
			return err
		}
		if err := page.Go("/nb/" + harness.Notebook + shot.path); err != nil {
			return err
		}
		// The panes and stamps are scripts that run after the page has loaded.
		time.Sleep(400 * time.Millisecond)
		if err := browser.Screenshot(filepath.Join(output, shot.name+suffix+".png")); err != nil {
			return err
		}
	}
	return nil
}
