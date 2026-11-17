package main

import (
	"fmt"
	"strings"
)

// flagSpec describes one option. A flag with an empty value is a switch.
type flagSpec struct {
	long  string
	short byte
	// value is the placeholder shown in help; empty makes the flag a switch.
	value  string
	repeat bool
	// hyphenValue lets the value start with `-`, as a note body may.
	hyphenValue bool
	// number marks a numeric value: a refused `-5` then comes with a tip.
	number   bool
	env      string
	fallback string
	allowed  []string
	help     string
	// requires and conflicts name other flags (or, for requires, a positional).
	requires  []string
	conflicts []string
}

// argSpec describes one positional argument.
type argSpec struct {
	name     string
	required bool
	multiple bool
	// hyphen lets this argument, and everything after it, start with `-`.
	hyphen bool
	// shown overrides name in the usage line.
	shown string
	help  string
}

// commandSpec describes a command and, recursively, its subcommands.
type commandSpec struct {
	name  string
	about string
	// details is the longer text help adds under about.
	details string
	flags   []flagSpec
	args    []argSpec
	subs    []commandSpec
}

// parsed is what a command line selected.
type parsed struct {
	// path is the command names from the root down, without the root.
	path  []string
	spec  *commandSpec
	flags map[string][]string
	args  map[string][]string
}

func (p parsed) has(flag string) bool { _, ok := p.flags[flag]; return ok }

func (p parsed) value(flag string) string {
	values := p.flags[flag]
	if len(values) == 0 {
		return ""
	}
	return values[len(values)-1]
}

func (p parsed) arg(name string) string {
	values := p.args[name]
	if len(values) == 0 {
		return ""
	}
	return values[0]
}

// helpRequest is returned in place of a parse when the user asked for help or
// the version, which are answered before anything else is checked.
type helpRequest struct {
	text    string
	version bool
}

func (h helpRequest) Error() string { return "help requested" }

// usageError is a command line that does not parse: exit 2.
type usageError struct {
	message string
	usage   string
}

func (u usageError) Error() string {
	return fmt.Sprintf("error: %s\n\nUsage: %s\n\nFor more information, try '--help'.", u.message, u.usage)
}

// parseCommandLine walks args down the spec tree. Flags may come before or
// after the positionals; `--` ends them.
func parseCommandLine(root *commandSpec, args []string, lookupEnv func(string) (string, bool)) (parsed, error) {
	state := &parseState{root: root, spec: root, path: []string{root.name}, lookupEnv: lookupEnv}
	state.out = parsed{spec: root, flags: map[string][]string{}, args: map[string][]string{}}
	return state.run(args)
}

type parseState struct {
	root       *commandSpec
	spec       *commandSpec
	path       []string
	out        parsed
	positional []string
	// rest is set by `--`: everything after it is positional.
	rest      bool
	lookupEnv func(string) (string, bool)
}

func (s *parseState) fail(format string, a ...any) error {
	return usageError{message: fmt.Sprintf(format, a...), usage: usageLine(s.path, s.spec)}
}

func (s *parseState) run(args []string) (parsed, error) {
	for i := 0; i < len(args); i++ {
		next, err := s.take(args, i)
		if err != nil {
			return parsed{}, err
		}
		i = next
	}
	return s.finish()
}

// take handles args[i] and returns the index of the last argument it used.
func (s *parseState) take(args []string, i int) (int, error) {
	arg := args[i]
	switch {
	case s.rest || (s.hyphenNext() && !s.isOptionBeforeFirstValue(arg)):
		return i, s.addPositional(arg)
	case arg == "--":
		s.rest = true
		return i, nil
	case strings.HasPrefix(arg, "--"):
		return s.takeLong(args, i)
	case len(arg) > 1 && arg[0] == '-':
		return s.takeShort(args, i)
	}
	return i, s.addPositionalOrSub(arg)
}

func (s *parseState) addPositionalOrSub(arg string) error {
	if len(s.positional) == 0 && len(s.spec.subs) > 0 {
		return s.descend(arg)
	}
	return s.addPositional(arg)
}

func (s *parseState) descend(name string) error {
	for i := range s.spec.subs {
		if s.spec.subs[i].name != name {
			continue
		}
		s.spec = &s.spec.subs[i]
		s.path = append(s.path, name)
		s.out.spec = s.spec
		s.out.path = append(s.out.path, name)
		return nil
	}
	return s.fail("unrecognized subcommand '%s'", name)
}

func (s *parseState) addPositional(arg string) error {
	slot := s.slotFor(len(s.positional))
	if slot == nil {
		return s.fail("unexpected argument '%s' found", arg)
	}
	s.positional = append(s.positional, arg)
	s.out.args[slot.name] = append(s.out.args[slot.name], arg)
	return nil
}

// hyphenNext says the next positional takes anything, `-q3` included.
func (s *parseState) hyphenNext() bool {
	if len(s.spec.subs) > 0 {
		return false
	}
	slot := s.slotFor(len(s.positional))
	return slot != nil && slot.hyphen
}

// isOptionBeforeFirstValue is clap's rule for a hyphen-tolerant argument: until
// it has its first value, `--`, `-h`, `--help` and the command's own long
// flags are still options. Afterwards every argument is a value.
func (s *parseState) isOptionBeforeFirstValue(arg string) bool {
	slot := s.slotFor(len(s.positional))
	if len(s.out.args[slot.name]) > 0 {
		return false
	}
	if arg == "--" || arg == "-h" {
		return true
	}
	if !strings.HasPrefix(arg, "--") {
		return false
	}
	name, _, _ := strings.Cut(arg[2:], "=")
	return name == "help" || s.findLong(name) != nil
}

// slotFor is the argument the n-th positional fills.
func (s *parseState) slotFor(n int) *argSpec {
	for i := range s.spec.args {
		slot := &s.spec.args[i]
		if slot.multiple {
			return slot
		}
		if n == 0 {
			return slot
		}
		n--
	}
	return nil
}

func (s *parseState) takeLong(args []string, i int) (int, error) {
	name, inline, hasInline := strings.Cut(args[i][2:], "=")
	if name == "help" {
		return i, helpRequest{text: helpFor(s.path, s.spec)}
	}
	if name == "version" && s.spec == s.root {
		return i, helpRequest{version: true}
	}
	spec := s.findLong(name)
	if spec == nil {
		return i, s.fail("unexpected argument '--%s' found", name)
	}
	if spec.value == "" {
		if hasInline {
			return i, s.fail("unexpected value '%s' for '--%s' found; no more were expected", inline, name)
		}
		return i, s.set(spec, "")
	}
	if hasInline {
		return i, s.set(spec, inline)
	}
	return s.takeValue(args, i, spec)
}

func (s *parseState) takeValue(args []string, i int, spec *flagSpec) (int, error) {
	if i+1 >= len(args) {
		return i, s.missingValue(spec)
	}
	next := args[i+1]
	if spec.hyphenValue || !strings.HasPrefix(next, "-") || next == "-" {
		return i + 1, s.set(spec, next)
	}
	if s.isKnownOption(next) {
		return i, s.missingValue(spec)
	}
	return i, s.unexpectedOption(spec, next)
}

// isKnownOption says arg names something this command takes: clap then calls
// the flag before it valueless, where an unknown one is an unexpected argument.
func (s *parseState) isKnownOption(arg string) bool {
	if arg == "--" || arg == "-h" {
		return true
	}
	if strings.HasPrefix(arg, "--") {
		name, _, _ := strings.Cut(arg[2:], "=")
		return name == "help" || s.findLong(name) != nil
	}
	return s.findShort(arg[1]) != nil
}

// unexpectedOption is a value that looks like an option. clap suggests `--`
// only for a number, where `-5` is most likely meant as one.
func (s *parseState) unexpectedOption(spec *flagSpec, arg string) error {
	message := fmt.Sprintf("unexpected argument '%s' found", arg)
	if spec.number {
		message += fmt.Sprintf("\n\n  tip: to pass '%s' as a value, use '-- %s'", arg, arg)
	}
	return s.fail("%s", message)
}

// missingValue is also what clap says for an empty value where only a fixed
// set is allowed: the empty string is no value at all there.
func (s *parseState) missingValue(spec *flagSpec) error {
	message := fmt.Sprintf("a value is required for '--%s <%s>' but none was supplied", spec.long, spec.value)
	if len(spec.allowed) > 0 {
		message += "\n  [possible values: " + strings.Join(spec.allowed, ", ") + "]"
	}
	return s.fail("%s", message)
}

func (s *parseState) takeShort(args []string, i int) (int, error) {
	cluster := args[i][1:]
	for pos := 0; pos < len(cluster); pos++ {
		letter := cluster[pos]
		if letter == 'h' {
			return i, helpRequest{text: helpFor(s.path, s.spec)}
		}
		if letter == 'V' && s.spec == s.root {
			return i, helpRequest{version: true}
		}
		spec := s.findShort(letter)
		if spec == nil {
			return i, s.fail("unexpected argument '-%c' found", letter)
		}
		if spec.value == "" {
			if err := s.set(spec, ""); err != nil {
				return i, err
			}
			continue
		}
		return s.takeShortValue(args, i, spec, cluster[pos+1:])
	}
	return i, nil
}

// takeShortValue reads `-n5`, `-n=5` or `-n 5`.
func (s *parseState) takeShortValue(args []string, i int, spec *flagSpec, glued string) (int, error) {
	if glued != "" {
		return i, s.set(spec, strings.TrimPrefix(glued, "="))
	}
	return s.takeValue(args, i, spec)
}

func (s *parseState) findLong(name string) *flagSpec {
	for i := range s.spec.flags {
		if s.spec.flags[i].long == name {
			return &s.spec.flags[i]
		}
	}
	return nil
}

func (s *parseState) findShort(letter byte) *flagSpec {
	for i := range s.spec.flags {
		if s.spec.flags[i].short == letter {
			return &s.spec.flags[i]
		}
	}
	return nil
}

func (s *parseState) set(spec *flagSpec, value string) error {
	if len(spec.allowed) > 0 && value == "" {
		return s.missingValue(spec)
	}
	if len(spec.allowed) > 0 && !contains(spec.allowed, value) {
		return s.fail("invalid value '%s' for '--%s <%s>'\n  [possible values: %s]",
			value, spec.long, spec.value, strings.Join(spec.allowed, ", "))
	}
	if _, seen := s.out.flags[spec.long]; seen && !spec.repeat {
		return s.fail("the argument '%s' cannot be used multiple times", flagName(spec))
	}
	s.out.flags[spec.long] = append(s.out.flags[spec.long], value)
	return nil
}

// flagName names a flag the way clap does: a switch by its long name, an
// option with its value placeholder.
func flagName(spec *flagSpec) string {
	if spec.value == "" {
		return "--" + spec.long
	}
	return "--" + spec.long + " <" + spec.value + ">"
}

// finish applies what only the whole line can tell: a missing subcommand or
// argument, environment fallbacks, and the flag relations.
func (s *parseState) finish() (parsed, error) {
	if len(s.spec.subs) > 0 {
		return parsed{}, s.fail("'%s' requires a subcommand but one was not provided", strings.Join(s.path, " "))
	}
	if err := s.requireArguments(); err != nil {
		return parsed{}, err
	}
	s.applyFallbacks()
	if err := s.checkRelations(); err != nil {
		return parsed{}, err
	}
	return s.out, nil
}

func (s *parseState) requireArguments() error {
	var missing []string
	for _, slot := range s.spec.args {
		if slot.required && len(s.out.args[slot.name]) == 0 {
			missing = append(missing, "<"+strings.ToUpper(slot.name)+">")
		}
	}
	if len(missing) == 0 {
		return nil
	}
	return s.fail("the following required arguments were not provided:\n  %s", strings.Join(missing, "\n  "))
}

// applyFallbacks fills a flag from its environment variable, then its default.
// A variable counts only when non-empty, as clap's `env` does.
func (s *parseState) applyFallbacks() {
	for _, spec := range s.spec.flags {
		if s.out.has(spec.long) {
			continue
		}
		if value, _ := s.lookupEnv(spec.env); value != "" && spec.env != "" {
			s.out.flags[spec.long] = []string{value}
		} else if spec.fallback != "" {
			s.out.flags[spec.long] = []string{spec.fallback}
		}
	}
}

// checkRelations reports conflicts before missing requirements, as clap does.
func (s *parseState) checkRelations() error {
	for _, spec := range s.spec.flags {
		if !s.out.has(spec.long) {
			continue
		}
		for _, other := range spec.conflicts {
			if s.present(other) {
				return s.fail("the argument '--%s' cannot be used with %s", spec.long, s.label(other))
			}
		}
	}
	for _, spec := range s.spec.flags {
		if !s.out.has(spec.long) {
			continue
		}
		for _, needed := range spec.requires {
			if !s.present(needed) {
				return s.fail("the following required arguments were not provided:\n  %s", s.label(needed))
			}
		}
	}
	return nil
}

// label names a flag as `'--x'` and a positional as `'<X>'`.
func (s *parseState) label(name string) string {
	if s.findLong(name) != nil {
		return "'--" + name + "'"
	}
	return "'<" + strings.ToUpper(name) + ">'"
}

// present says whether a flag or a positional was given.
func (s *parseState) present(name string) bool {
	return s.out.has(name) || len(s.out.args[name]) > 0
}

func contains(values []string, want string) bool {
	for _, value := range values {
		if value == want {
			return true
		}
	}
	return false
}
