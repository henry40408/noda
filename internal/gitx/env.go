package gitx

import (
	"slices"
	"strings"
)

// dropped names the variables that would aim git at another repository or
// inject configuration. noda can run inside a git hook or an alias, and those
// set several of them.
var dropped = map[string]struct{}{
	"GIT_DIR": {}, "GIT_WORK_TREE": {}, "GIT_INDEX_FILE": {}, "GIT_OBJECT_DIRECTORY": {},
	"GIT_ALTERNATE_OBJECT_DIRECTORIES": {}, "GIT_COMMON_DIR": {}, "GIT_NAMESPACE": {},
	"GIT_PREFIX": {}, "GIT_CONFIG_PARAMETERS": {}, "GIT_CONFIG_COUNT": {},
	// LANGUAGE outranks LC_ALL for message catalogs, and noda matches on git's
	// English wording.
	"LANGUAGE": {},
	// Each of these changes what noda parses or where it writes.
	"GIT_DIFF_OPTS": {}, "GIT_CONFIG": {}, "GIT_EXEC_PATH": {}, "GIT_DEFAULT_HASH": {},
}

func isDropped(name string) bool {
	if _, found := dropped[name]; found {
		return true
	}
	for _, prefix := range []string{"GIT_CONFIG_KEY_", "GIT_CONFIG_VALUE_", "GIT_TRACE"} {
		if strings.HasPrefix(name, prefix) {
			return true
		}
	}
	return false
}

// sanitizedEnv is the environment of one git run: base without what redirects
// git, with what makes its output predictable, then extra on top.
func sanitizedEnv(base map[string]string, readOnly bool, extra map[string]string) []string {
	env := map[string]string{}
	for name, value := range base {
		if !isDropped(name) {
			env[name] = value
		}
	}
	// English, so error text maps to what noda explains.
	env["LC_ALL"] = "C"
	// Fail, as libgit2 did, instead of asking for a password nobody can type.
	env["GIT_TERMINAL_PROMPT"] = "0"
	env["GIT_PAGER"] = "cat"
	env["GIT_EDITOR"] = ":"
	env["GIT_MERGE_AUTOEDIT"] = "no"
	if readOnly {
		// A listing must not take the index lock a concurrent commit needs.
		env["GIT_OPTIONAL_LOCKS"] = "0"
	}
	for name, value := range extra {
		env[name] = value
	}

	names := make([]string, 0, len(env))
	for name := range env {
		names = append(names, name)
	}
	slices.Sort(names)
	list := make([]string, 0, len(names))
	for _, name := range names {
		list = append(list, name+"="+env[name])
	}
	return list
}

// fixedConfig is passed to every run so a user's own settings cannot change the
// output noda parses. Each line names the setting it guards against.
// diff.external cannot be neutralised here: an empty value is run as a command.
// Callers that parse a diff pass --no-ext-diff.
var fixedConfig = []string{
	"core.quotePath=false",          // octal-escaped non-ASCII paths
	"color.ui=never",                // colour codes in parsed output
	"color.diff=never",              // outranks color.ui, even for `log -p`
	"diff.suppressBlankEmpty=false", // a blank context line without its leading space
	"diff.orderFile=/dev/null",      // files in a user's order; an empty value is an error
	"merge.conflictStyle=merge",     // diff3 sections in `merge-file` output
	"core.fsmonitor=false",          // a watcher daemon started by a status call
	"log.showSignature=false",       // a gpg report spliced into `git log` output
	"log.decorate=false",            // `(HEAD -> main)` after an id
	"log.follow=false",              // a pathspec log that follows renames by content
	"diff.noprefix=false",           // paths without `a/` and `b/`
	"diff.mnemonicPrefix=false",     // `i/` and `w/` instead of `a/` and `b/`
	"diff.relative=false",           // paths relative to the current directory
	"blame.ignoreRevsFile=",         // lines attributed past a user's ignore list
	"blame.markUnblamableLines=false",
	"blame.markIgnoredLines=false",
	"advice.defaultBranchName=false", // a hint block on stderr of `init`
	"advice.detachedHead=false",
}
