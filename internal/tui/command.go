package tui

import "strings"

// Spec is one command `:` accepts: noda's own subcommand names, since only a
// handful of them are worth a key. A table rather than a switch so Ctrl-a can
// list it; what each command does is in app.go.
type Spec struct {
	Name string
	// Aliases are shortest first.
	Aliases []string
	// Takes is in the CLI help's notation: <required>, [optional], ...
	Takes string
	What  string
}

// Usage is the name followed by what it takes.
func (s Spec) Usage() string {
	if s.Takes == "" {
		return s.Name
	}
	return s.Name + " " + s.Takes
}

// Commands is getting about, then changing a note, then the notebook, then
// leaving.
var Commands = []Spec{
	{Name: "open", Aliases: []string{"o", "show"}, Takes: "<note>", What: "open a note by id or by slug"},
	{Name: "notes", Aliases: []string{"ls"}, Takes: "[query...]", What: "back to the listing, filtered if a query is given"},
	{Name: "todo", Aliases: []string{"t"}, Takes: "", What: "every unticked box, soonest due first"},
	{Name: "tags", Takes: "", What: "every tag; enter filters the listing by one"},
	{Name: "backlinks", Aliases: []string{"b"}, Takes: "[note]", What: "what links to a note — the one shown, or one named"},
	{Name: "log", Aliases: []string{"l"}, Takes: "[note]", What: "commits, newest first: the notebook's or a note's"},
	{Name: "blame", Takes: "[note]", What: "which commit put each line of a note where it is"},
	{Name: "diff", Takes: "", What: "what is uncommitted, or what the last commit did"},
	{Name: "deleted", Takes: "", What: "notes history holds that the notebook no longer does"},
	{Name: "files", Takes: "", What: "what the notebook holds that is not a note"},
	{Name: "notebooks", Aliases: []string{"nb"}, Takes: "", What: "every notebook; enter moves this session to one"},
	{Name: "edit", Aliases: []string{"e"}, Takes: "[note]", What: "open in $EDITOR: the one named, or the one shown"},
	{Name: "add", Aliases: []string{"new"}, Takes: "[title...]", What: "make a note; no title takes it from the body"},
	{Name: "mv", Aliases: []string{"retitle"}, Takes: "<title...>", What: "retitle the note on screen; the slug follows"},
	{Name: "tag", Takes: "[note] +tag -tag...", What: "add and remove tags: +work -q3 -\"two words\""},
	{Name: "pin", Takes: "[note]", What: "float a note to the top of every listing"},
	{Name: "unpin", Takes: "[note]", What: "let a pinned note back down among the rest"},
	{Name: "rm", Takes: "", What: "delete the note on screen, after a y"},
	{Name: "restore", Takes: "<note> <rev>", What: "put a note back as it was at a revision"},
	{Name: "use", Takes: "<notebook>", What: "move this session to another notebook"},
	{Name: "status", Takes: "", What: "notes, changes, and drift from the remote"},
	{Name: "doctor", Takes: "[--links] [--times]", What: "diagnose. Reports only; it changes nothing"},
	{Name: "snapshot", Takes: "[name]", What: "mark the notebook, or list what is marked"},
	{Name: "readme", Takes: "", What: "write the notebook's front page"},
	{Name: "sync", Takes: "", What: "pull, then push. Commits pending changes"},
	{Name: "push", Takes: "", What: "send this notebook's commits to the remote"},
	{Name: "pull", Takes: "", What: "bring in the remote's commits"},
	{Name: "reload", Aliases: []string{"r"}, Takes: "", What: "read the notebook again"},
	{Name: "keys", Aliases: []string{"help"}, Takes: "", What: "the keys this screen answers to"},
	{Name: "quit", Aliases: []string{"q"}, Takes: "", What: "leave, asking first about an unsent queue"},
}

// FindCommand is exact only: a prefix like "pu" is ambiguous, and guessing wrong
// is a network call nobody asked for.
func FindCommand(name string) (Spec, bool) {
	for _, spec := range Commands {
		if spec.Name == name {
			return spec, true
		}
		for _, alias := range spec.Aliases {
			if alias == name {
				return spec, true
			}
		}
	}
	return Spec{}, false
}

// MatchingCommands searches descriptions too, so "remote" finds push and pull.
func MatchingCommands(filter string) []Spec {
	filter = strings.ToLower(strings.TrimSpace(filter))
	var found []Spec
	for _, spec := range Commands {
		if filter == "" || strings.Contains(spec.Name, filter) || anyContains(spec.Aliases, filter) ||
			strings.Contains(strings.ToLower(spec.What), filter) {
			found = append(found, spec)
		}
	}
	return found
}

func anyContains(words []string, part string) bool {
	for _, word := range words {
		if strings.Contains(word, part) {
			return true
		}
	}
	return false
}
