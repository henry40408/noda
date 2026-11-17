package notebook

import (
	"context"
	"fmt"
	"sort"
	"strings"

	"github.com/henry40408/noda/internal/gitx"
)

// Snapshot is one git tag, as `noda snapshot` lists it.
type Snapshot struct {
	Name string
	// Target is the commit, not the tag object.
	Target        string
	Seconds       int64
	OffsetMinutes int
	Message       string
}

// ShortTarget is the abbreviated target.
func (s Snapshot) ShortTarget() string { return Short(s.Target) }

// Snapshot makes an annotated tag on HEAD, so it carries an author, a time and
// a message. It never moves an existing one: a snapshot must mean one thing to
// be cited. It returns the commit tagged.
func (n *Notebook) Snapshot(name, message string) (string, error) {
	if err := n.checkSnapshotName(name); err != nil {
		return "", err
	}
	values, err := gitx.LoadConfig(context.Background(), n.git, n.Path)
	if err != nil {
		return "", err
	}
	head, err := n.read("rev-parse", "--verify", "HEAD^{commit}")
	if err != nil {
		return "", err
	}
	// The tag object is written by hand, as the commit is: `git tag` would
	// refuse a name starting with `-` that libgit2 took, tidy the message, and
	// sign by the user's `tag.gpgSign`. noda never signs a tag.
	object := "object " + head + "\ntype commit\ntag " + name + "\ntagger " + n.signature(values).String() + "\n\n" + message
	stored, err := n.run(gitx.Cmd{Args: []string{"mktag"}, Stdin: []byte(object)})
	if err != nil {
		return "", err
	}
	tag := strings.TrimSpace(string(stored.Stdout))
	// The old value is zero: the ref must not exist, or a snapshot made
	// meanwhile would be moved.
	_, err = n.run(gitx.Cmd{Args: []string{"update-ref", "refs/tags/" + name, tag, strings.Repeat("0", len(tag))}})
	return head, err
}

func (n *Notebook) checkSnapshotName(name string) error {
	refname := "refs/tags/" + name
	valid, err := n.run(gitx.Cmd{Args: []string{"check-ref-format", refname}, ReadOnly: true, OKCodes: []int{0, 1}})
	if err != nil {
		return err
	}
	if valid.Code != 0 || strings.ContainsAny(name, "\n\r") {
		return fmt.Errorf("invalid snapshot name: %s — no spaces, no `..`, no `~^:?*[\\`", name)
	}
	exists, err := n.hasRef(refname)
	if err != nil {
		return err
	}
	if exists {
		return fmt.Errorf("snapshot already exists: %s — pick another name, or remove it with `git tag -d %s` in %s", name, name, n.Path)
	}
	return nil
}

const snapshotFormat = "--format=" + recordSep +
	"%(refname)" + fieldSep + "%(objecttype)" + fieldSep + "%(*objecttype)" + fieldSep +
	"%(objectname)" + fieldSep + "%(*objectname)" + fieldSep +
	"%(committerdate:raw)" + fieldSep + "%(*committerdate:raw)" + fieldSep +
	"%(contents:subject)" + fieldSep + "%(contents)"

// Snapshots is newest first by the tagged commit's time, as `log` orders.
// Lightweight tags made outside noda are listed too; a tag that names no
// commit, such as one on a tree or a blob, is skipped.
func (n *Notebook) Snapshots() ([]Snapshot, error) {
	output, err := n.read("for-each-ref", snapshotFormat, "refs/tags")
	if err != nil {
		return nil, err
	}
	var found []Snapshot
	for _, text := range strings.Split(output, recordSep) {
		if text == "" {
			continue
		}
		snapshot, ok, err := parseSnapshot(text)
		if err != nil {
			return nil, err
		}
		if ok {
			found = append(found, snapshot)
		}
	}
	sort.SliceStable(found, func(i, j int) bool {
		if found[i].Seconds != found[j].Seconds {
			return found[i].Seconds > found[j].Seconds
		}
		return found[i].Name < found[j].Name
	})
	return found, nil
}

func parseSnapshot(text string) (Snapshot, bool, error) {
	fields := strings.SplitN(text, fieldSep, 9)
	if len(fields) != 9 {
		return Snapshot{}, false, fmt.Errorf("unexpected git for-each-ref record %q", text)
	}
	refname, kind, peeledKind := fields[0], fields[1], fields[2]
	target, peeledTarget, date, peeledDate := fields[3], fields[4], fields[5], fields[6]
	annotated := kind == "tag"
	if annotated {
		kind, target, date = peeledKind, peeledTarget, peeledDate
	}
	if kind != "commit" {
		return Snapshot{}, false, nil
	}
	seconds, offset, err := gitx.ParseRawDate(date)
	if err != nil {
		return Snapshot{}, false, err
	}
	// A lightweight tag has no message; its commit's summary stands in.
	message := strings.TrimSpace(fields[8])
	if !annotated {
		message = strings.TrimSuffix(fields[7], "\n")
	}
	return Snapshot{
		Name:          strings.TrimPrefix(refname, "refs/tags/"),
		Target:        target,
		Seconds:       seconds,
		OffsetMinutes: offset,
		Message:       message,
	}, true, nil
}
