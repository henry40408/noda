package importer

import (
	"bytes"
	"fmt"
	"slices"
	"strings"
	"unicode"
	"unicode/utf8"
)

// The names written into the `unconverted:` field.
const (
	leftTransclusion    = "transclusion"
	leftMacro           = "macro"
	leftWidget          = "widget"
	leftHTML            = "html"
	leftTable           = "table"
	leftDefinitionList  = "definition list"
	leftStyle           = "style"
	leftUnderline       = "underline"
	leftSuperscript     = "superscript"
	leftSubscript       = "subscript"
	leftImageAttributes = "image attributes"
	leftLink            = "unresolved link"
)

// Converted is the result of converting one tiddler's text.
type Converted struct {
	Text string
	// Left is for the note's `unconverted:` field, so sorted and deduplicated.
	Left []string
}

// Resolve turns a tiddler title into the filename noda gave it, or reports
// that the export holds no such tiddler.
type Resolve func(title string) (string, bool)

// Convert turns one tiddler's WikiText into Markdown.
//
// Nothing is guessed: what cannot be translated faithfully is copied through
// as written and named in Converted.Left, because leftover WikiText is
// findable and Markdown that says something else is not. Nothing is dropped:
// every character reaches the output as markup or text.
func Convert(text string, resolve Resolve) Converted {
	b := &blocks{left: map[string]struct{}{}, resolve: resolve}
	b.out.Grow(len(text) + len(text)/8)
	b.run(text)
	left := make([]string, 0, len(b.left))
	for name := range b.left {
		left = append(left, name)
	}
	slices.Sort(left)
	return Converted{Text: b.out.String(), Left: left}
}

// blocks is one walk down the lines; everything that is not a block construct
// goes through inline.
type blocks struct {
	out     bytes.Buffer
	left    map[string]struct{}
	resolve Resolve
}

func (b *blocks) note(name string) { b.left[name] = struct{}{} }

func (b *blocks) line(text string) {
	b.out.WriteString(text)
	b.out.WriteByte('\n')
}

func (b *blocks) atLineStart() bool {
	written := b.out.Bytes()
	return len(written) == 0 || written[len(written)-1] == '\n'
}

func (b *blocks) run(text string) {
	lines := splitLines(text)
	for i := 0; i < len(lines); {
		i = b.block(lines, i)
	}
}

// block converts the construct starting at lines[i] and returns the index of
// the line after it.
func (b *blocks) block(lines []string, i int) int {
	line := lines[i]

	// First, and byte for byte: `''` in code is not bold.
	if strings.HasPrefix(trimStart(line), "```") {
		return b.fence(lines, i)
	}
	// A pragma makes the tiddler a program; nothing after it is prose.
	if strings.HasPrefix(line, "\\") && isDefinition(line) {
		b.note(leftMacro)
		for _, rest := range lines[i:] {
			b.line(rest)
		}
		return len(lines)
	}
	if strings.HasPrefix(line, "|") {
		return b.table(lines, i)
	}
	if strings.HasPrefix(line, "<<<") {
		return b.quote(lines, i)
	}
	if hashes, body, styled, ok := heading(line); ok {
		b.heading(hashes, body, styled)
		return i + 1
	}
	if prefix, body, styled, ok := list(line); ok {
		if styled {
			b.note(leftStyle)
		}
		b.listItem(prefix, body)
		return i + 1
	}
	b.rest(line)
	return i + 1
}

// rest is a line that is no block construct Markdown shares, or plain prose.
func (b *blocks) rest(line string) {
	switch {
	// Markdown has no definition list.
	case strings.HasPrefix(line, "; ") || strings.HasPrefix(line, ": "):
		b.note(leftDefinitionList)
		b.line(line)
	case strings.TrimRightFunc(line, unicode.IsSpace) == "@@" || isStyleOpen(line):
		b.note(leftStyle)
		b.line(line)
	default:
		b.inline(line)
		b.out.WriteByte('\n')
	}
}

func (b *blocks) heading(hashes, body string, styled bool) {
	if styled {
		b.note(leftStyle)
	}
	b.out.WriteString(hashes + " ")
	b.inline(body)
	b.out.WriteByte('\n')
}

// fence copies a code fence untouched, closing fence included; an
// unterminated fence takes the rest, as in Markdown.
func (b *blocks) fence(lines []string, start int) int {
	b.line(lines[start])
	for offset, line := range lines[start+1:] {
		b.line(line)
		if strings.HasPrefix(trimStart(line), "```") {
			return start + offset + 2
		}
	}
	return len(lines)
}

// table is decided as a whole: GFM has only a header row, so a table with a
// footer, caption, class, merged cell or vertical alignment is copied through.
func (b *blocks) table(lines []string, start int) int {
	end := len(lines)
	for n, line := range lines[start:] {
		if !strings.HasPrefix(line, "|") {
			end = start + n
			break
		}
	}
	rows := lines[start:end]

	if !allPlain(rows) {
		b.note(leftTable)
		for _, row := range rows {
			b.line(row)
		}
		return end
	}

	for n, row := range rows {
		b.tableRow(row, n == 0)
	}
	return end
}

func (b *blocks) tableRow(row string, first bool) {
	cells := splitRow(row)
	b.out.WriteByte('|')
	for _, cell := range cells {
		b.out.WriteByte(' ')
		b.inline(strings.TrimLeft(strings.TrimSpace(cell), "!"))
		b.out.WriteString(" |")
	}
	b.out.WriteByte('\n')
	// WikiText marks header cells with `!`; there may be no header.
	if !first || !allHeaderCells(cells) {
		return
	}
	b.out.WriteByte('|')
	for range cells {
		b.out.WriteString(" --- |")
	}
	b.out.WriteByte('\n')
}

func allHeaderCells(cells []string) bool {
	for _, cell := range cells {
		if !strings.HasPrefix(trimStart(cell), "!") {
			return false
		}
	}
	return true
}

// quote is `<<<` ... `<<<`, with the citation WikiText allows on the closing
// line.
func (b *blocks) quote(lines []string, start int) int {
	for offset, line := range lines[start+1:] {
		rest, closes := strings.CutPrefix(line, "<<<")
		if !closes {
			b.out.WriteString("> ")
			b.inline(line)
			b.out.WriteByte('\n')
			continue
		}
		if cite := strings.TrimSpace(rest); cite != "" {
			b.out.WriteString("> — ")
			b.inline(cite)
			b.out.WriteByte('\n')
		}
		return start + offset + 2
	}
	return len(lines)
}

// listItem: `*#` is a numbered item inside a bullet, the prefix's length is
// the depth and its last character the marker.
func (b *blocks) listItem(prefix, body string) {
	b.out.WriteString(strings.Repeat("  ", utf8.RuneCountInString(prefix)-1))
	if strings.HasSuffix(prefix, "#") {
		b.out.WriteString("1. ")
	} else {
		b.out.WriteString("- ")
	}
	b.inline(body)
	b.out.WriteByte('\n')
}

func (b *blocks) inline(line string) {
	rest := line
	for rest != "" {
		if length, ok := b.inlineAt(rest); ok {
			rest = rest[length:]
			continue
		}
		ch, size := utf8.DecodeRuneInString(rest)
		b.escaped(ch, b.atLineStart())
		rest = rest[size:]
	}
}

// inlineAt writes the inline construct at rest, if there is one, and returns
// its length; otherwise the caller takes one character.
func (b *blocks) inlineAt(rest string) (int, bool) {
	// First: a URL is full of `//` and `__`.
	if length, ok := url(rest); ok {
		b.out.WriteString(rest[:length])
		return length, true
	}
	for _, try := range []func(string) (int, bool){
		b.codeSpan, b.image, b.link, b.camelCase, b.passedThrough, b.emphasis,
	} {
		if length, ok := try(rest); ok {
			return length, true
		}
	}
	return 0, false
}

func (b *blocks) codeSpan(rest string) (int, bool) {
	after, ok := strings.CutPrefix(rest, "`")
	if !ok {
		return 0, false
	}
	end := strings.IndexByte(after, '`')
	if end < 0 {
		return 0, false
	}
	b.out.WriteString(rest[:end+2])
	return end + 2, true
}

// passedThrough copies what Markdown has no equivalent for to its closing
// syntax.
func (b *blocks) passedThrough(rest string) (int, bool) {
	for _, kind := range []struct{ open, close, name string }{
		{"{{", "}}", leftTransclusion},
		{"<<", ">>", leftMacro},
		{"<$", ">", leftWidget},
	} {
		if !strings.HasPrefix(rest, kind.open) {
			continue
		}
		return b.copyThrough(rest, kind.close, kind.name), true
	}
	if !isHTMLTag(rest) {
		return 0, false
	}
	return b.copyThrough(rest, ">", leftHTML), true
}

// copyThrough writes rest up to and including the first close, or all of it,
// and returns the length written.
func (b *blocks) copyThrough(rest, close, name string) int {
	length := len(rest)
	if at := strings.Index(rest, close); at >= 0 {
		length = at + len(close)
	}
	b.note(name)
	b.out.WriteString(rest[:length])
	return length
}

// delimiters are the paired marks; an empty open means Markdown has no form
// for it, so the inside is kept and the mark named.
var delimiters = []struct{ mark, open, close, name string }{
	{"''", "**", "**", ""},
	{"//", "*", "*", ""},
	{"~~", "~~", "~~", ""},
	{"__", "", "", leftUnderline},
	{"^^", "", "", leftSuperscript},
	{",,", "", "", leftSubscript},
	{"@@", "", "", leftStyle},
}

// emphasis recurses on the inside, which handles italic around bold.
func (b *blocks) emphasis(rest string) (int, bool) {
	for _, d := range delimiters {
		if !strings.HasPrefix(rest, d.mark) {
			continue
		}
		// Unclosed: punctuation, not markup.
		end := strings.Index(rest[len(d.mark):], d.mark)
		if end < 0 {
			continue
		}
		if d.name != "" {
			b.note(d.name)
		}
		b.out.WriteString(d.open)
		b.inline(rest[len(d.mark) : len(d.mark)+end])
		b.out.WriteString(d.close)
		return len(d.mark)*2 + end, true
	}
	return 0, false
}

// camelCase is always a link in WikiText; `~CamelCase` turns that off. Only
// those that resolve become links: in TiddlyWiki's own documentation fewer than
// a third name a tiddler (the rest are the likes of `JavaScript`). The `~` is
// dropped either way.
func (b *blocks) camelCase(rest string) (int, bool) {
	word, suppressed := strings.CutPrefix(rest, "~")
	length, ok := camelLen(word)
	if !ok {
		return 0, false
	}
	name := word[:length]
	destination, resolved := b.resolved(name)
	if resolved && !suppressed {
		fmt.Fprintf(&b.out, "[%s](%s)", name, destination)
	} else {
		b.out.WriteString(name)
	}
	if suppressed {
		length++
	}
	return length, true
}

// image is `[img[pic.jpg]]`, with the tooltip and attribute forms.
func (b *blocks) image(rest string) (int, bool) {
	after, ok := strings.CutPrefix(rest, "[img")
	if !ok {
		return 0, false
	}
	open := strings.IndexByte(after, '[')
	if open < 0 {
		return 0, false
	}
	// Width, class and style: Markdown has nowhere to put them.
	if strings.TrimSpace(after[:open]) != "" {
		b.note(leftImageAttributes)
	}
	end := strings.Index(after[open:], "]]")
	if end < 0 {
		return 0, false
	}
	caption, target, found := strings.Cut(after[open+1:open+end], "|")
	if !found {
		caption, target = "", after[open+1:open+end]
	}
	// Unlike a link, an unresolved target is kept: it names a file the
	// notebook is expected to gain, which `doctor --links` checks.
	target = strings.TrimSpace(target)
	destination, resolved := b.resolved(target)
	if !resolved {
		destination = target
	}
	fmt.Fprintf(&b.out, "![%s](%s)", caption, destination)
	return len("[img") + open + end + 2, true
}

// link is `[[caption|target]]`: caption first, as in Markdown (it is MediaWiki
// that puts it second).
func (b *blocks) link(rest string) (int, bool) {
	// `[ext[..]]` targets are taken as written.
	after, external := strings.CutPrefix(rest, "[ext[")
	skip := len("[ext[")
	if !external {
		var ok bool
		after, ok = strings.CutPrefix(rest, "[[")
		if !ok {
			return 0, false
		}
		skip = len("[[")
	}
	end := strings.Index(after, "]]")
	if end < 0 {
		return 0, false
	}
	body := after[:end]
	caption, target, found := strings.Cut(body, "|")
	if !found {
		caption, target = body, body
	}
	length := skip + end + 2

	destination, resolved := strings.TrimSpace(target), true
	if !external {
		destination, resolved = b.resolved(destination)
	}
	// Left as WikiText rather than linking to a file that will not exist.
	if !resolved {
		b.note(leftLink)
		b.out.WriteString(rest[:length])
		return length, true
	}
	b.out.WriteByte('[')
	b.inline(caption)
	fmt.Fprintf(&b.out, "](%s)", destination)
	return length, true
}

// resolved is a URL or anchor as itself, a tiddler title as its filename, or
// false.
func (b *blocks) resolved(target string) (string, bool) {
	if length, ok := url(target); (ok && length == len(target)) || strings.HasPrefix(target, "#") {
		return target, true
	}
	return b.resolve(target)
}

// escaped writes ch escaped only where Markdown would read it as syntax, so
// `snake_case` stays clean.
func (b *blocks) escaped(ch rune, atLineStart bool) {
	needs := false
	switch ch {
	case '\\', '`', '*', '[', '<':
		needs = true
	case '#', '>', '-', '+', '=', '_':
		needs = atLineStart
	}
	if needs {
		b.out.WriteByte('\\')
	}
	b.out.WriteRune(ch)
}

// heading is `!` ... `!!!!!!`, and the text after it.
func heading(line string) (hashes, body string, styled, ok bool) {
	level := len(line) - len(strings.TrimLeft(line, "!"))
	if level == 0 || level > 6 {
		return "", "", false, false
	}
	// The space after the marks is optional: `!Heading` is a heading.
	rest, styled := declassed(line[level:])
	return strings.Repeat("#", level), trimStart(rest), styled, true
}

// list is a list line, as its `*`/`#` prefix and the text after it.
func list(line string) (prefix, body string, styled, ok bool) {
	depth := len(line) - len(strings.TrimLeft(line, "*#"))
	if depth == 0 {
		return "", "", false, false
	}
	rest, styled := declassed(line[depth:])
	if !strings.HasPrefix(rest, " ") {
		return "", "", false, false
	}
	return line[:depth], trimStart(rest), styled, true
}

// declassed strips the `.myClass.another` a heading or list item may carry
// after its marker. noda has no stylesheet for them, so they go, but the
// caller reports it.
func declassed(rest string) (text string, styled bool) {
	classes, ok := strings.CutPrefix(rest, ".")
	if !ok {
		return rest, false
	}
	_, text, _ = strings.Cut(classes, " ")
	return text, true
}

// isDefinition is `\define`, `\procedure` and the other pragmas that make a
// tiddler a program.
func isDefinition(line string) bool {
	for _, pragma := range []string{
		"\\define", "\\procedure", "\\function", "\\widget",
		"\\import", "\\parameters", "\\rules", "\\whitespace",
	} {
		if strings.HasPrefix(line, pragma) {
			return true
		}
	}
	return false
}

// isStyleOpen is `@@color:red;` and `@@.myClass` at the head of a block.
func isStyleOpen(line string) bool {
	return strings.HasPrefix(line, "@@") && !strings.Contains(line[2:], "@@")
}

func allPlain(rows []string) bool {
	for _, row := range rows {
		if !plainRow(row) {
			return false
		}
	}
	return true
}

func plainRow(row string) bool {
	trimmed := strings.TrimRightFunc(row, unicode.IsSpace)
	// `|...|h`, `|...|f`, `|...|c`, `|...|k`: header, footer, caption, class.
	if n := len(trimmed); n >= 2 && strings.IndexByte("hfck", trimmed[n-1]) >= 0 && trimmed[n-2] == '|' {
		return false
	}
	for _, cell := range splitRow(row) {
		switch strings.TrimSpace(cell) {
		case "~", "<", ">":
			return false
		}
		if strings.HasPrefix(cell, "^") || strings.HasPrefix(cell, ",") {
			return false
		}
	}
	return true
}

func splitRow(row string) []string {
	inner := strings.TrimRight(strings.TrimLeft(strings.TrimRightFunc(row, unicode.IsSpace), "|"), "|")
	return strings.Split(inner, "|")
}

// url is the length of the URL at the head of text.
func url(text string) (int, bool) {
	for _, scheme := range []string{"https://", "http://", "mailto:", "ftp://", "file://"} {
		if !strings.HasPrefix(text, scheme) {
			continue
		}
		end := len(text)
		if n := strings.IndexFunc(text[len(scheme):], func(c rune) bool {
			return unicode.IsSpace(c) || c == '|' || c == ']'
		}); n >= 0 {
			end = len(scheme) + n
		}
		return len(strings.TrimRight(text[:end], ",.;:")), true
	}
	return 0, false
}

// camelLen is the length of the CamelCase word at the head of text: two
// capitals with lower case between and after them.
func camelLen(text string) (int, bool) {
	first, size := utf8.DecodeRuneInString(text)
	if text == "" || !isUpper(first) {
		return 0, false
	}
	lowerSeen, secondCapital := false, false
	end := size
	for at, ch := range text[size:] {
		at += size
		switch {
		case isUpper(ch):
			if !lowerSeen {
				return 0, false
			}
			secondCapital = true
		case isLower(ch) || unicode.IsNumber(ch):
			lowerSeen = true
		default:
			return finishCamel(text, at, secondCapital)
		}
		end = at + utf8.RuneLen(ch)
	}
	return finishCamel(text, end, secondCapital)
}

// finishCamel wants lower case after the second capital too, so `IDs` is not
// a link.
func finishCamel(text string, end int, secondCapital bool) (int, bool) {
	last, _ := utf8.DecodeLastRuneInString(text[:end])
	return end, secondCapital && isLower(last)
}

// isUpper and isLower are Rust's `char::is_uppercase` and `is_lowercase`,
// which also take the Other_* properties.
func isUpper(r rune) bool { return unicode.IsUpper(r) || unicode.Is(unicode.Other_Uppercase, r) }
func isLower(r rune) bool { return unicode.IsLower(r) || unicode.Is(unicode.Other_Lowercase, r) }

// isHTMLTag is an HTML tag opening here, as opposed to a `<` in prose.
func isHTMLTag(text string) bool {
	after := strings.TrimPrefix(text, "<")
	if after == text {
		after = ""
	}
	after = strings.TrimPrefix(after, "/")
	if after == "" {
		return false
	}
	letter := after[0]
	return (letter >= 'a' && letter <= 'z' || letter >= 'A' && letter <= 'Z') && strings.Contains(text, ">")
}

func trimStart(text string) string { return strings.TrimLeftFunc(text, unicode.IsSpace) }

// splitLines is Rust's `str::lines`: `\n` or `\r\n` ends a line, and the empty
// piece after a final newline is not a line.
func splitLines(text string) []string {
	if text == "" {
		return nil
	}
	parts := strings.Split(text, "\n")
	if parts[len(parts)-1] == "" {
		parts = parts[:len(parts)-1]
	}
	for i, part := range parts {
		// The last piece has no line ending when the text lacks a final newline.
		if i < len(parts)-1 || strings.HasSuffix(text, "\n") {
			parts[i] = strings.TrimSuffix(part, "\r")
		}
	}
	return parts
}
