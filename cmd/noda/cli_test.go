package main

import (
	"bytes"
	"errors"
	"slices"
	"strings"
	"testing"
)

func parse(t *testing.T, args ...string) parsed {
	t.Helper()
	line, err := parseCommandLine(buildSpec(), args, lookupIn(nil))
	if err != nil {
		t.Fatalf("%v: %v", args, err)
	}
	return line
}

func parseError(t *testing.T, args ...string) string {
	t.Helper()
	_, err := parseCommandLine(buildSpec(), args, lookupIn(nil))
	var usage usageError
	if !errors.As(err, &usage) {
		t.Fatalf("%v: want a usage error, got %v", args, err)
	}
	return usage.message
}

func TestFlagsMayComeBeforeOrAfterPositionals(t *testing.T) {
	for _, args := range [][]string{
		{"add", "Title", "--tag", "a", "--tag=b"},
		{"add", "--tag", "a", "Title", "--tag", "b"},
		{"add", "--tag=a", "--tag", "b", "Title"},
	} {
		line := parse(t, args...)
		if line.arg("title") != "Title" || !slices.Equal(line.flags["tag"], []string{"a", "b"}) {
			t.Fatalf("%v: %+v", args, line)
		}
	}
}

func TestShortFlagsClusterAndTakeGluedValues(t *testing.T) {
	line := parse(t, "ls", "-q0")
	if !line.has("quiet") || !line.has("null") {
		t.Fatalf("%+v", line)
	}
	for _, args := range [][]string{{"log", "-n5"}, {"log", "-n", "5"}, {"log", "--max-count=5"}, {"log", "-n=5"}} {
		if got := parse(t, args...).value("max-count"); got != "5" {
			t.Fatalf("%v: %q", args, got)
		}
	}
}

func TestDoubleDashEndsOptions(t *testing.T) {
	line := parse(t, "add", "--", "--not-a-flag")
	if line.arg("title") != "--not-a-flag" {
		t.Fatalf("%+v", line)
	}
}

func TestHyphenValuesAreAllowedWhereTheyWereInClap(t *testing.T) {
	if got := parse(t, "add", "-c", "- [ ] item").value("content"); got != "- [ ] item" {
		t.Fatalf("content %q", got)
	}
	if got := parse(t, "search", "-tag:old", "text").args["query"]; !slices.Equal(got, []string{"-tag:old", "text"}) {
		t.Fatalf("query %q", got)
	}
	line := parse(t, "tag", "n", "+a", "-b")
	if !slices.Equal(line.args["changes"], []string{"+a", "-b"}) {
		t.Fatalf("changes %q", line.args["changes"])
	}
	want := "unexpected argument '-5' found\n\n  tip: to pass '-5' as a value, use '-- -5'"
	if got := parseError(t, "log", "-n", "-5"); got != want {
		t.Fatalf("a hyphen value is refused where it was: %q", got)
	}
	if got := parseError(t, "ls", "--tag", "-x"); got != "unexpected argument '-x' found" {
		t.Fatalf("no tip for a text value: %q", got)
	}
	for _, args := range [][]string{{"log", "-n", "--help"}, {"log", "-n", "-h"}, {"log", "-n", "--"}, {"ls", "--tag", "--json"}, {"ls", "--tag", "-q"}} {
		if got := parseError(t, args...); !strings.HasPrefix(got, "a value is required for '--") {
			t.Errorf("%v: a known option leaves the flag valueless: %q", args, got)
		}
	}
}

func TestOptionsStillCountBeforeAHyphenArgumentHasItsFirstValue(t *testing.T) {
	line := parse(t, "tag", "n", "--no-touch", "+x")
	if !line.has("no-touch") || !slices.Equal(line.args["changes"], []string{"+x"}) {
		t.Fatalf("--no-touch before the tags: %+v", line)
	}
	line = parse(t, "tag", "n", "--", "-x")
	if !slices.Equal(line.args["changes"], []string{"-x"}) {
		t.Fatalf("`--` escapes: %+v", line)
	}
	// Once the first tag is in, everything is a tag, `-h` and `--help` too.
	line = parse(t, "tag", "n", "+a", "-h", "--help", "--no-touch")
	if line.has("no-touch") || len(line.args["changes"]) != 4 {
		t.Fatalf("after the first tag: %+v", line)
	}
	for _, args := range [][]string{{"tag", "n", "--help"}, {"tag", "n", "-h"}, {"search", "--help"}, {"search", "-h"}, {"tag", "n", "--no-touch", "-h"}} {
		_, err := parseCommandLine(buildSpec(), args, lookupIn(nil))
		var request helpRequest
		if !errors.As(err, &request) || request.text == "" {
			t.Errorf("%v: want help, got %v", args, err)
		}
	}
	// Only the exact `-h` asks for help; `-hx` is a tag, as in clap.
	if got := parse(t, "search", "-hx").args["query"]; !slices.Equal(got, []string{"-hx"}) {
		t.Fatalf("query %q", got)
	}
}

func TestAnEmptyValueWhereOnlyAFixedSetIsAllowedIsNoValue(t *testing.T) {
	for _, args := range [][]string{{"ls", "--sort="}, {"ls", "--sort", ""}, {"web", "--log-format="}} {
		got := parseError(t, args...)
		if !strings.HasPrefix(got, "a value is required for '--") || !strings.Contains(got, "\n  [possible values: ") {
			t.Errorf("%v: %q", args, got)
		}
	}
	line := parse(t, "ls", "--tag=")
	if !line.givenEmpty("tag") || !line.has("tag") {
		t.Fatalf("--tag= is given and empty: %+v", line)
	}
	if line = parse(t, "ls"); line.givenEmpty("tag") || line.has("tag") {
		t.Fatalf("an absent --tag is not empty: %+v", line)
	}
	if line = parse(t, "path", ""); !line.givenEmpty("key") {
		t.Fatalf("an empty argument is given: %+v", line)
	}
	if line = parse(t, "path"); line.givenEmpty("key") {
		t.Fatalf("an absent argument is not empty: %+v", line)
	}
}

func TestARepeatedSwitchIsAUsageError(t *testing.T) {
	for want, args := range map[string][]string{
		"the argument '--quiet' cannot be used multiple times":    {"ls", "-qq"},
		"the argument '--reverse' cannot be used multiple times":  {"ls", "-r", "-r"},
		"the argument '--json' cannot be used multiple times":     {"ls", "--json", "--json"},
		"the argument '--long' cannot be used multiple times":     {"ls", "--long", "-l"},
		"the argument '--no-touch' cannot be used multiple times": {"tag", "n", "--no-touch", "--no-touch", "+x"},
	} {
		if got := parseError(t, args...); got != want {
			t.Errorf("%v: got %q want %q", args, got, want)
		}
	}
	if got := parse(t, "add", "x", "--tag", "a", "--tag", "b"); len(got.flags["tag"]) != 2 {
		t.Fatalf("a repeatable option may repeat: %+v", got)
	}
}

func TestRelationsBetweenFlags(t *testing.T) {
	cases := map[string][]string{
		"the argument '--json' cannot be used with '--quiet'":              {"ls", "--json", "-q"},
		"the argument '--tag' cannot be used with '--files-only'":          {"ls", "--tag", "x", "--files-only"},
		"the argument '--edit' cannot be used with '--unset'":              {"config", "--edit", "--unset"},
		"the argument '--edit' cannot be used with '<KEY>'":                {"config", "--edit", "k"},
		"the argument '--unset' cannot be used with '<VALUE>'":             {"config", "k", "v", "--unset"},
		"the argument '--max-count <COUNT>' cannot be used multiple times": {"log", "-n1", "-n2"},
	}
	for want, args := range cases {
		// `--key` is how the message names a positional.
		if got := parseError(t, args...); got != want {
			t.Errorf("%v:\n got %q\nwant %q", args, got, want)
		}
	}
	for _, args := range [][]string{{"ls", "-0"}, {"snapshot", "-m", "text"}, {"config", "--unset"}} {
		if got := parseError(t, args...); !strings.HasPrefix(got, "the following required arguments were not provided") {
			t.Errorf("%v: %q", args, got)
		}
	}
}

func TestUnknownThingsAreUsageErrors(t *testing.T) {
	for want, args := range map[string][]string{
		"unrecognized subcommand 'bogus'":                            {"bogus"},
		"unexpected argument '--nope' found":                         {"ls", "--nope"},
		"unexpected argument '-z' found":                             {"ls", "-z"},
		"unexpected argument 'extra' found":                          {"show", "a", "extra"},
		"'noda file' requires a subcommand but one was not provided": {"file"},
	} {
		if got := parseError(t, args...); got != want {
			t.Errorf("%v: got %q want %q", args, got, want)
		}
	}
	if got := parseError(t, "ls", "--json=yes"); !strings.HasPrefix(got, "unexpected value 'yes' for '--json'") {
		t.Errorf("a switch takes no value: %q", got)
	}
}

func TestSubcommandsNestAndTheFlagEnvFeedsTheDefault(t *testing.T) {
	line := parse(t, "notebook", "rm", "-f", "old")
	if strings.Join(line.path, " ") != "notebook rm" || !line.has("force") || line.arg("name") != "old" {
		t.Fatalf("%+v", line)
	}

	got, err := parseCommandLine(buildSpec(), []string{"web"}, lookupIn(map[string]string{"NODA_LOG_FORMAT": "json"}))
	if err != nil || got.value("log-format") != "json" || got.value("listen") != "127.0.0.1:8080" {
		t.Fatalf("%+v, %v", got, err)
	}
	if got := parse(t, "web", "--log-format", "text"); got.value("log-format") != "text" {
		t.Fatalf("a flag outranks the environment: %+v", got)
	}
	if got := parse(t, "web"); got.value("log-format") != "text" {
		t.Fatalf("the default: %+v", got)
	}
}

func TestHelpTextCarriesTheRustDocComments(t *testing.T) {
	text, err := helpCommand(buildSpec(), []string{"ls"})
	if err != nil {
		t.Fatal(err)
	}
	for _, want := range []string{
		"List what the notebook holds: notes as `id  title  [tags]`, then its files.",
		"Usage: noda ls [OPTIONS]",
		"-q, --quiet",
		"--sort <FIELD>",
		"[possible values: created, updated, title]",
		"-h, --help",
	} {
		if !strings.Contains(text, want) {
			t.Errorf("missing %q in:\n%s", want, text)
		}
	}
	root, _ := helpCommand(buildSpec(), nil)
	if !strings.Contains(root, "A git-native notebook for your terminal") || !strings.Contains(root, "-V, --version") || !strings.Contains(root, "  tui ") {
		t.Fatalf("root help:\n%s", root)
	}
	if _, err := helpCommand(buildSpec(), []string{"nope"}); err == nil {
		t.Fatal("an unknown command has no help")
	}
}

func TestEverySubcommandHasHelp(t *testing.T) {
	var walk func(path []string, spec *commandSpec)
	walk = func(path []string, spec *commandSpec) {
		if text := helpFor(path, spec); !strings.Contains(text, spec.about) {
			t.Errorf("%v: no about in help", path)
		}
		for i := range spec.subs {
			walk(append(append([]string{}, path...), spec.subs[i].name), &spec.subs[i])
		}
	}
	walk([]string{"noda"}, buildSpec())
}

func TestEveryParsedCommandHasAHandler(t *testing.T) {
	known := handlers()
	var walk func(path []string, spec *commandSpec)
	walk = func(path []string, spec *commandSpec) {
		if len(spec.subs) == 0 {
			if _, ok := known[strings.Join(path, " ")]; !ok {
				t.Errorf("`%s` is in the table but has no handler", strings.Join(path, " "))
			}
		}
		for i := range spec.subs {
			walk(append(append([]string{}, path...), spec.subs[i].name), &spec.subs[i])
		}
	}
	for i := range buildSpec().subs {
		sub := buildSpec().subs[i]
		walk([]string{sub.name}, &sub)
	}
}

func TestPrintAddsOneNewlineAndKeepsNULOutputRaw(t *testing.T) {
	cases := []struct {
		text, want string
		color      bool
	}{
		{"", "", false},
		{"a", "a\n", false},
		{"a\n", "a\n", false},
		{"\x1b[1ma\x1b[0m", "a\n", false},
		{"\x1b[1ma\x1b[0m", "\x1b[1ma\x1b[0m\n", true},
		{"a\x00b\x00", "a\x00b\x00", false},
		{"\x1b[1ma\x1b[0m\x00", "\x1b[1ma\x1b[0m\x00", false},
	}
	for _, c := range cases {
		var out bytes.Buffer
		if err := printOutput(&out, c.text, c.color); err != nil || out.String() != c.want {
			t.Errorf("%q: got %q, %v", c.text, out.String(), err)
		}
	}
}
