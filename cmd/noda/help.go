package main

import (
	"strings"
)

// usageLine is `noda ls [OPTIONS]` style text for a command.
func usageLine(path []string, spec *commandSpec) string {
	parts := []string{strings.Join(path, " ")}
	if len(spec.flags) > 0 {
		parts = append(parts, "[OPTIONS]")
	}
	if len(spec.subs) > 0 {
		parts = append(parts, "<COMMAND>")
	}
	for _, slot := range spec.args {
		parts = append(parts, slotLabel(slot))
	}
	return strings.Join(parts, " ")
}

func slotLabel(slot argSpec) string {
	if slot.shown != "" {
		return slot.shown
	}
	label := strings.ToUpper(slot.name)
	if slot.required {
		return "<" + label + ">"
	}
	return "[" + label + "]"
}

// helpFor is the text of `--help`: about, usage, then what the command takes.
func helpFor(path []string, spec *commandSpec) string {
	var out strings.Builder
	out.WriteString(spec.about + "\n")
	if spec.details != "" {
		out.WriteString("\n" + spec.details + "\n")
	}
	out.WriteString("\nUsage: " + usageLine(path, spec) + "\n")
	writeSection(&out, "Commands", commandRows(spec))
	writeSection(&out, "Arguments", argumentRows(spec))
	writeSection(&out, "Options", optionRows(spec, len(path) == 1))
	return out.String()
}

type helpRow struct{ label, text string }

func writeSection(out *strings.Builder, title string, rows []helpRow) {
	if len(rows) == 0 {
		return
	}
	width := 0
	for _, row := range rows {
		width = max(width, len(row.label))
	}
	out.WriteString("\n" + title + ":\n")
	for _, row := range rows {
		out.WriteString("  " + row.label + strings.Repeat(" ", width-len(row.label)+2) + row.text + "\n")
	}
}

func commandRows(spec *commandSpec) []helpRow {
	if len(spec.subs) == 0 {
		return nil
	}
	rows := make([]helpRow, 0, len(spec.subs)+1)
	for _, sub := range spec.subs {
		rows = append(rows, helpRow{sub.name, firstSentence(sub.about)})
	}
	return append(rows, helpRow{"help", "Print this message or the help of the given subcommand(s)"})
}

func argumentRows(spec *commandSpec) []helpRow {
	var rows []helpRow
	for _, slot := range spec.args {
		rows = append(rows, helpRow{slotLabel(slot), slot.help})
	}
	return rows
}

func optionRows(spec *commandSpec, root bool) []helpRow {
	var rows []helpRow
	for _, flag := range spec.flags {
		rows = append(rows, helpRow{flagLabel(flag), flagHelp(flag)})
	}
	rows = append(rows, helpRow{"-h, --help", "Print help"})
	if root {
		rows = append(rows, helpRow{"-V, --version", "Print version"})
	}
	return rows
}

func flagLabel(flag flagSpec) string {
	label := "    --" + flag.long
	if flag.short != 0 {
		label = "-" + string(flag.short) + ", --" + flag.long
	}
	if flag.value != "" {
		label += " <" + flag.value + ">"
	}
	return label
}

func flagHelp(flag flagSpec) string {
	text := flag.help
	if flag.env != "" {
		text += " [env: " + flag.env + "]"
	}
	if flag.fallback != "" {
		text += " [default: " + flag.fallback + "]"
	}
	if len(flag.allowed) > 0 {
		text += " [possible values: " + strings.Join(flag.allowed, ", ") + "]"
	}
	return text
}

// firstSentence keeps the command list to one line a command, as clap does.
func firstSentence(text string) string {
	if end := strings.Index(text, ". "); end >= 0 {
		return text[:end]
	}
	return strings.TrimSuffix(text, ".")
}

// helpCommand answers `noda help [command...]`.
func helpCommand(root *commandSpec, names []string) (string, error) {
	return helpAt([]string{root.name}, root, names)
}

// helpAt answers `help [command...]` typed under the command at path, which
// every command with subcommands has: `noda file help add`.
func helpAt(path []string, spec *commandSpec, names []string) (string, error) {
	for _, name := range names {
		if name == "help" && len(spec.subs) > 0 {
			return helpOfHelp(path), nil
		}
		next := findSub(spec, name)
		if next == nil {
			return "", usageError{message: "unrecognized subcommand '" + name + "'", usage: usageLine(path, spec)}
		}
		spec, path = next, append(path, name)
	}
	return helpFor(path, spec), nil
}

// helpOfHelp is the help of the `help` subcommand itself.
func helpOfHelp(path []string) string {
	return "Print this message or the help of the given subcommand(s)\n\n" +
		"Usage: " + strings.Join(path, " ") + " help [COMMAND]...\n\n" +
		"Arguments:\n  [COMMAND]...  Print help for the subcommand(s)\n"
}

func findSub(spec *commandSpec, name string) *commandSpec {
	for i := range spec.subs {
		if spec.subs[i].name == name {
			return &spec.subs[i]
		}
	}
	return nil
}
