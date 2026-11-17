package gitx

import (
	"context"
	"os"
	"strconv"
	"strings"
)

// ConfigValues is git's configuration as one read leaves it: every entry, in
// the order git applies them, so the last value of a key is the one that wins.
type ConfigValues struct {
	entries []configEntry
}

type configEntry struct {
	key   string
	value string
	// bare is a key written with no `=`, which git reads as true.
	bare bool
}

// LoadConfig reads everything git would apply in dir, `[include]` and
// `[includeIf]` files too. An empty dir reads the system and global files only,
// as there is no repository to ask: the one case is deciding how to make one.
//
// One read serves every lookup: a `git config` run per key would cost a process
// each.
func LoadConfig(ctx context.Context, g *Git, dir string) (ConfigValues, error) {
	cmd := Cmd{Dir: dir, Args: []string{"config", "-z", "--list", "--includes"}, ReadOnly: true}
	if dir == "" {
		// A directory that is no repository: git would otherwise find the one
		// the caller happens to stand in.
		cmd.Dir = os.TempDir()
		cmd.ExtraEnv = map[string]string{"GIT_DIR": os.DevNull}
	}
	result, err := g.Run(ctx, cmd)
	if err != nil {
		return ConfigValues{}, err
	}
	return parseConfig(result.Stdout), nil
}

// parseConfig reads `git config -z --list`: `key\nvalue\0`, or `key\0` when
// the key has no value at all.
func parseConfig(output []byte) ConfigValues {
	var values ConfigValues
	for _, record := range SplitNUL(output) {
		key, value, hasValue := strings.Cut(record, "\n")
		values.entries = append(values.entries, configEntry{key: key, value: value, bare: !hasValue})
	}
	return values
}

// Get is the last value of key, which git spells with the section and name in
// lower case and the subsection as written. False when the key is not set.
func (c ConfigValues) Get(key string) (string, bool) {
	key = canonicalKey(key)
	for i := len(c.entries) - 1; i >= 0; i-- {
		if c.entries[i].key == key {
			return c.entries[i].value, true
		}
	}
	return "", false
}

// Bool is the last value of key read as git reads a boolean. False for both
// results when the key is not set or is no boolean.
func (c ConfigValues) Bool(key string) (value, ok bool) {
	key = canonicalKey(key)
	for i := len(c.entries) - 1; i >= 0; i-- {
		entry := c.entries[i]
		if entry.key != key {
			continue
		}
		if entry.bare {
			return true, true
		}
		return ParseBool(entry.value)
	}
	return false, false
}

// canonicalKey lower-cases the section and the variable name, the parts git
// does not distinguish by case.
func canonicalKey(key string) string {
	first := strings.Index(key, ".")
	last := strings.LastIndex(key, ".")
	if first < 0 {
		return strings.ToLower(key)
	}
	return strings.ToLower(key[:first]) + key[first:last] + "." + strings.ToLower(key[last+1:])
}

// ParseBool follows git: true, yes, on and a non-zero integer are true; false,
// no, off, zero and the empty string are false. Anything else is no boolean.
func ParseBool(text string) (value, ok bool) {
	switch strings.ToLower(text) {
	case "true", "yes", "on":
		return true, true
	case "false", "no", "off", "":
		return false, true
	}
	number, err := strconv.ParseInt(text, 10, 64)
	if err != nil {
		return false, false
	}
	return number != 0, true
}
