package cmd

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"github.com/henry40408/noda/internal/notebook"
)

// Readme writes and commits the notebook's README.md, which a git host shows
// above the file list. Fixed prose rather than an index of notes, which would
// go stale from the next `noda add`.
func Readme(e Env, force bool) (string, error) {
	active, err := e.openActive()
	if err != nil {
		return "", err
	}
	path := filepath.Join(active.Path, notebook.ReadmeFile)

	existed := fileExists(path)
	if existed && !force {
		return "", fmt.Errorf("%s already exists — pass `--force` to overwrite", notebook.ReadmeFile)
	}
	if err := os.WriteFile(path, []byte(readmeTemplate(active.Name)), 0o666); err != nil {
		return "", err
	}
	verb, past := "add", "wrote"
	if existed {
		verb, past = "update", "rewrote"
	}
	message := fmt.Sprintf("file: %s %s", verb, notebook.ReadmeFile)
	if err := active.Commit([]string{notebook.ReadmeFile}, message); err != nil {
		return "", err
	}
	return fmt.Sprintf("%s %s in `%s`", past, notebook.ReadmeFile, active.Name), nil
}

func readmeTemplate(name string) string {
	return strings.ReplaceAll(readmeText, "{name}", name)
}

const readmeText = "# {name}\n" +
	"\n" +
	"A [noda](https://github.com/henry40408/noda) notebook: plain Markdown notes kept in git.\n" +
	"\n" +
	"## Nothing here needs noda to be read\n" +
	"\n" +
	"Every note is a Markdown file at the root of this repository. Open one in this web view,\n" +
	"in an editor, in anything that renders Markdown. noda makes these notes quicker to write\n" +
	"and to search; it is not what makes them readable.\n" +
	"\n" +
	"## Filenames\n" +
	"\n" +
	"Notes are named `<id>-<slug>.md` — for example `k3f9m2p1-meeting-notes.md`.\n" +
	"\n" +
	"- The **id** (`k3f9m2p1`) is the note's permanent identity, and never changes.\n" +
	"- The **slug** (`meeting-notes`) comes from the title, and changes when the title does.\n" +
	"\n" +
	"A link from one note to another names the whole filename, so links work in this web view,\n" +
	"and a link survives a retitle because the id in it still names the same note.\n" +
	"\n" +
	"## Frontmatter\n" +
	"\n" +
	"Each note opens with a block like this:\n" +
	"\n" +
	"```yaml\n" +
	"---\n" +
	"title: Reading notes on TAOCP\n" +
	"tags: [books, algorithms]\n" +
	"created: 2019-03-14T08:21:00Z\n" +
	"updated: 2024-11-02T16:40:12Z\n" +
	"---\n" +
	"```\n" +
	"\n" +
	"`created` is set once and never moves; `updated` follows every change. `tags` is optional,\n" +
	"and `pinned: true` pins a note to the top of the listing. noda reads those five fields and leaves everything else in the block alone, so any other\n" +
	"field is yours to use.\n" +
	"\n" +
	"## Working on it with noda\n" +
	"\n" +
	"```console\n" +
	"$ noda clone <this repository's URL> {name}\n" +
	"$ noda use {name}\n" +
	"$ noda ls\n" +
	"```\n" +
	"\n" +
	"<!--\n" +
	"  Everything below is yours to write.\n" +
	"\n" +
	"  One thing worth leaving out: a list of the notes. Nothing updates it, so it is wrong\n" +
	"  from the next `noda add` onward — `noda ls` is that list, always current. What belongs\n" +
	"  here is what stays true: what this notebook is for, and links to the few notes a reader\n" +
	"  should start from.\n" +
	"-->\n"
