package main

// The command table: help texts for the commands and their flags.

const noteHelp = "Note id (`k3f9m2p1`, or any prefix naming exactly one) or slug (`meeting-notes`)."

const noTouchHelp = "Leave `updated` as it stands instead of setting it to now."

func noteArg() argSpec { return argSpec{name: "note", required: true, help: noteHelp} }

func noTouch() flagSpec { return flagSpec{long: "no-touch", help: noTouchHelp} }

var sortFields = []string{"created", "updated", "title"}

func buildSpec() *commandSpec {
	return &commandSpec{
		name:  "noda",
		about: "A git-native notebook for your terminal",
		subs: []commandSpec{
			{name: "init", about: "Create the XDG directories and a default notebook."},
			{name: "readme", about: "Write the notebook's README.md, which a git host shows as its front page. Auto-commits.",
				flags: []flagSpec{{long: "force", help: "Replace a README.md the notebook already holds."}}},
			{name: "add", about: "Create a note. Opens $EDITOR when no content is given. Auto-commits.",
				args: []argSpec{{name: "title", shown: "[TITLE]"}},
				flags: []flagSpec{
					{long: "content", short: 'c', value: "CONTENT", hyphenValue: true, help: "Note body, instead of opening $EDITOR."},
					{long: "tag", value: "TAG", repeat: true, help: "Tag to attach; repeat for several."},
				}},
			{name: "ls", about: "List what the notebook holds: notes as `id  title  [tags]`, then its files.",
				flags: []flagSpec{
					{long: "tag", value: "TAG", conflicts: []string{"files-only"}, help: "Only notes carrying this tag. Anything more selective is `noda search`, which is where the query language lives."},
					{long: "notebook", value: "NOTEBOOK", help: "List another notebook instead of the active one."},
					{long: "json", conflicts: []string{"quiet"}, help: "Print one JSON object instead of a table."},
					{long: "quiet", short: 'q', help: "Print one identifier per record and nothing else: a note's id, a file's name."},
					{long: "null", short: '0', requires: []string{"quiet"}, help: "Separate what `--quiet` prints with NUL, for `xargs -0`. A file's name may contain a space."},
					{long: "notes-only", conflicts: []string{"files-only"}, help: "Leave out the notebook's files."},
					{long: "files-only", help: "Leave out the notes."},
					{long: "long", short: 'l', help: "Show the whole row: the slug and both timestamps as well as the title. `--json` carries every field either way."},
					{long: "sort", value: "FIELD", allowed: sortFields, help: "Order the notes. `created` and `updated` put the newest first; `title` is alphabetical."},
					{long: "reverse", short: 'r', help: "Run the listing the other way. Reverses whatever `--sort` asked for, or the default order when it was not passed."},
				}},
			{name: "show", about: "Print a note to stdout, addressed by id or slug.", args: []argSpec{noteArg()}},
			{name: "edit", about: "Open a note in $EDITOR; auto-commits on save.", args: []argSpec{noteArg()}, flags: []flagSpec{noTouch()}},
			{name: "mv", about: "Retitle a note. The slug follows the title; the id is preserved.",
				args: []argSpec{noteArg(), {name: "new_title", required: true, shown: "<NEW_TITLE>"}},
				flags: []flagSpec{
					{long: "update-links", help: "Rewrite the links that point at this note, instead of reporting them. Matched on its id, so a link written before an earlier retitle is caught too."},
					noTouch(),
				}},
			{name: "pin", about: "Float a note to the top of every listing. Auto-commits.", args: []argSpec{noteArg()}, flags: []flagSpec{noTouch()}},
			{name: "unpin", about: "Let a pinned note back down among the rest. Auto-commits.", args: []argSpec{noteArg()}, flags: []flagSpec{noTouch()}},
			{name: "rm", about: "Delete a note. The removal is a commit, so `git revert` undoes it.", args: []argSpec{noteArg()}},
			{name: "status", about: "Where the active notebook stands: notes, changes, drift from the remote."},
			{name: "doctor", about: "Diagnose the notebook, and adopt the notes that only lack an id. Names the `- [ ]` items raw HTML hides from `todo` and any git hooks noda will never run, neither of which needs a flag.",
				flags: []flagSpec{
					{long: "dry-run", help: "Report what would change without writing or committing anything."},
					{long: "links", help: "Also follow every link in every note: report the files no note links to, the links whose note was retitled, and the links that name nothing. Reads the whole notebook, so it is asked for rather than assumed."},
					{long: "times", help: "Also check the timestamps: values that cannot be read, notes changed before they were created, and notes git has a newer commit for than their own `updated` claims. Walks all of history, so it is asked for rather than assumed."},
				}},
			{name: "path", about: "Print where something lives, for the tools noda does not wrap: `pandoc \"$(noda path meeting-notes)\"`.",
				args: []argSpec{{name: "key", shown: "[KEY]"}}},
			{name: "search", about: "Search the active notebook: `noda search tag:work OR tag:q3 budget`.",
				args: []argSpec{{name: "query", required: true, multiple: true, hyphen: true, shown: "<QUERY>..."}}},
			{name: "tui", about: "Browse the notebook: the listing, the same query language `noda search` takes, and Enter to open what the cursor is on as a screen of its own."},
			{name: "web", about: "Serve the notebooks in a browser, for reading them from a phone.",
				details: "There is no password on this. It is meant to be reached over a tailnet or from behind something that already does authentication, and the defaults are set for that: it listens on this machine only until told otherwise, and it answers to a hostname only when told to.",
				flags: []flagSpec{
					{long: "listen", short: 'l', value: "ADDR", fallback: "127.0.0.1:8080", help: "Address to listen on. Give `0.0.0.0:8080` to reach it from elsewhere."},
					{long: "allow-host", value: "NAME", repeat: true, help: "A hostname to answer to, repeatable."},
					{long: "log-format", value: "FORMAT", env: "NODA_LOG_FORMAT", fallback: "text", allowed: []string{"text", "json"}, help: "How the log is rendered: for a person, or for something that collects it."},
				}},
			{name: "log", about: "Show commit history for the notebook, or for one note.",
				args:  []argSpec{{name: "note", shown: "[NOTE]"}},
				flags: []flagSpec{{long: "max-count", short: 'n', value: "COUNT", number: true, help: "Show at most this many commits."}}},
			{name: "backlinks", about: "List the notes that link to a note or a file. Reads every note's body, so it costs what `doctor --links` costs.",
				args: []argSpec{{name: "key", required: true}},
				flags: []flagSpec{
					{long: "json", conflicts: []string{"quiet"}, help: "Print one JSON object instead of a table."},
					{long: "quiet", short: 'q', help: "Print one note id per line and nothing else."},
				}},
			{name: "todo", about: "List every unticked `- [ ]` in the notebook, soonest due first. Reads every note's body, so it costs what `search` costs.",
				flags: []flagSpec{{long: "json", help: "Print one JSON object instead of a table."}}},
			{name: "blame", about: "Show which commit put each line of a note where it is.", args: []argSpec{noteArg()}},
			{name: "diff", about: "Show uncommitted changes, or what the last commit changed.",
				args:  []argSpec{{name: "note", shown: "[NOTE]"}},
				flags: []flagSpec{{long: "remote", help: "Show what a push would carry instead: everything committed here that the remote has not got."}}},
			{name: "deleted", about: "List notes the notebook no longer holds, with the commit to restore each from. Walks all of history.",
				flags: []flagSpec{
					{long: "notebook", value: "NOTEBOOK", help: "Look at another notebook instead of the active one."},
					{long: "json", help: "Print one JSON object instead of a table."},
				}},
			{name: "snapshot", about: "Mark the notebook as it stands, so `restore` can name that moment later. Omit the name to list what has been marked.",
				args:  []argSpec{{name: "name", shown: "[NAME]"}},
				flags: []flagSpec{{long: "message", short: 'm', value: "TEXT", requires: []string{"name"}, help: "What it marks. Defaults to the name."}}},
			{name: "restore", about: "Restore a note to an earlier version, as a new commit.",
				args:  []argSpec{noteArg(), {name: "commit", required: true}},
				flags: []flagSpec{{long: "no-touch", help: "Restore `updated` along with the rest of the note, instead of setting it to now."}}},
			{name: "config", about: "Show or change settings: editor, author, notebook, sign.",
				args: []argSpec{{name: "key", shown: "[KEY]"}, {name: "value", shown: "[VALUE]"}},
				flags: []flagSpec{
					{long: "unset", requires: []string{"key"}, conflicts: []string{"value"}, help: "Remove the setting, going back to the default."},
					{long: "edit", conflicts: []string{"key", "unset"}, help: "Open config.toml in the editor."},
				}},
			{name: "import", about: "Bring a notebook in from somewhere else.", subs: []commandSpec{
				{name: "tiddlywiki", about: "Import a TiddlyWiki 5 export: the JSON `export all` writes, or a saved single-file wiki.",
					args:  []argSpec{{name: "files", required: true, multiple: true, shown: "<FILE>..."}},
					flags: []flagSpec{{long: "no-convert", help: "Bring the WikiText in as it stands instead of converting it to Markdown."}}},
			}},
			{name: "file", about: "Put files in the notebook, and take them out again.", subs: []commandSpec{
				{name: "add", about: "Copy files into the active notebook. Auto-commits.",
					args:  []argSpec{{name: "paths", required: true, multiple: true, shown: "<PATH>..."}},
					flags: []flagSpec{{long: "as", value: "NAME", help: "Store it under this name instead of its own. One file at a time."}}},
				{name: "mv", about: "Rename one of the notebook's files. Auto-commits.",
					args:  []argSpec{{name: "old", required: true}, {name: "new", required: true}},
					flags: []flagSpec{{long: "update-links", help: "Rewrite the links that named it, instead of reporting them."}}},
				{name: "rm", about: "Remove one of the notebook's files (as a revertible commit).", args: []argSpec{{name: "name", required: true}}},
			}},
			{name: "notebook", about: "Manage notebooks.", subs: []commandSpec{
				{name: "add", about: "Create a notebook (a new git repo).",
					args:  []argSpec{{name: "name", required: true}},
					flags: []flagSpec{{long: "remote", value: "REMOTE", help: "Remote to sync with, e.g. `git@github.com:me/notes.git`."}}},
				{name: "ls", about: "List notebooks; marks the active one."},
				{name: "rm", about: "Remove a notebook's local repository. This is not a commit and cannot be undone.",
					args:  []argSpec{{name: "name", required: true}},
					flags: []flagSpec{{long: "force", short: 'f', help: "Delete without asking."}}},
				{name: "rename", about: "Rename a notebook.", args: []argSpec{{name: "old", required: true}, {name: "new", required: true}}},
				{name: "current", about: "Print the active notebook."},
			}},
			{name: "clone", about: "Clone an existing remote notebook.",
				args: []argSpec{{name: "url", required: true}, {name: "name", shown: "[NAME]"}}},
			{name: "remote", about: "Show or set the active notebook's remote.", subs: []commandSpec{
				{name: "set", about: "Set the active notebook's remote.", args: []argSpec{{name: "url", required: true}}},
				{name: "show", about: "Print the configured remote."},
			}},
			{name: "sync", about: "Pull, then push. Commits pending changes first."},
			{name: "push", about: "Send the active notebook's commits to its remote."},
			{name: "pull", about: "Bring in the remote's commits."},
			{name: "use", about: "Set the active notebook.", args: []argSpec{{name: "name", required: true}}},
			{name: "tag", about: "Add and remove tags: `noda tag meeting-notes +work -q3`.",
				args:  []argSpec{noteArg(), {name: "changes", required: true, multiple: true, hyphen: true, shown: "<+TAG|-TAG>..."}},
				flags: []flagSpec{noTouch()}},
		},
	}
}
