// Package web builds noda's pages: a note body as HTML, and every page the
// browser shows as a string. Nothing here touches a repository or the network.
//
// A body is parsed by goldmark and written by hand, not by goldmark's renderer,
// for four decisions:
//
// Where a destination leads: relative paths become /nb/<book>/n/<id> and
// /nb/<book>/f/<name>, resolved by link.Target, the same function doctor and
// file mv use, so "is this inside the notebook" has one answer.
//
// What raw HTML turns into: a code block, shown and not run. Not dropped,
// because `noda import tiddlywiki` leaves unconverted HTML in the body as the
// only copy of what it said. A destination with a scheme noda does not allow
// keeps its text and loses its link.
//
// Bare URLs: CommonMark has none, but notes written elsewhere rely on them.
// GFM's rules, narrowed to http:// and https://.
//
// What a link that leaves carries: a note's address holds its id, which
// Referer would hand over. Such links get target="_blank" and
// rel="noopener noreferrer".
package web

import (
	"fmt"
	"strconv"
	"strings"

	"github.com/yuin/goldmark/ast"
	extast "github.com/yuin/goldmark/extension/ast"
	"github.com/yuin/goldmark/text"

	"github.com/henry40408/noda/internal/link"
	"github.com/henry40408/noda/internal/markdown"
)

// Named is a note's id and slug: the pair a filename is made of.
type Named struct{ ID, Slug string }

// Around is which notebook the page belongs to, and which filenames are notes.
type Around struct {
	Book string
	// Notes maps a filename to an id: the spelling a body uses, and the
	// address the web has.
	Notes map[string]string
}

// AroundOf builds an Around from the notebook's named files, which opens
// nothing: rendering one note is not a reason to parse every other one.
func AroundOf(book string, named []Named) Around {
	notes := make(map[string]string, len(named))
	for _, note := range named {
		notes[note.ID+"-"+note.Slug+".md"] = note.ID
	}
	return Around{Book: book, Notes: notes}
}

type routeKind int

const (
	routeNote    routeKind = iota // a note: given a class
	routeTo                       // this origin or not a page: no new tab
	routeAway                     // http(s): opened by hand
	routeNowhere                  // the link is dropped; its text stays
)

type route struct {
	kind routeKind
	url  string
	id   string // the note's, for routeNote only
}

// noteLink opens a link to a note in the colour the note wears everywhere else,
// so a reader who has learned a note by its colour finds it again in a sentence.
func noteLink(target route) string {
	return "<a class=\"note " + hueClass(target.id) + "\" href=\"" + escape(target.url) + "\">"
}

// Body renders markdown as HTML.
func Body(source string, around Around) string {
	document := markdown.Rich(source)
	writer := &bodyWriter{around: around, source: document.Source}
	writer.blocks(document.Root)
	return writer.out.String()
}

type bodyWriter struct {
	out    strings.Builder
	prose  strings.Builder
	around Around
	source []byte
	// Depths, not flags: an image's alt text may contain a link, and prose
	// inside either is never scanned for bare addresses.
	links  int
	images int
}

func (w *bodyWriter) blocks(parent ast.Node) {
	for child := parent.FirstChild(); child != nil; child = child.NextSibling() {
		w.block(child)
	}
}

func (w *bodyWriter) startBlock() {
	if w.out.Len() > 0 && !strings.HasSuffix(w.out.String(), "\n") {
		w.out.WriteByte('\n')
	}
}

func (w *bodyWriter) block(node ast.Node) {
	switch n := node.(type) {
	case *ast.Paragraph:
		w.startBlock()
		w.out.WriteString("<p>")
		w.inlines(n)
		w.out.WriteString("</p>\n")
	case *ast.TextBlock:
		w.inlines(n)
	case *ast.Heading:
		w.startBlock()
		tag := "h" + strconv.Itoa(n.Level)
		w.out.WriteString("<" + tag + ">")
		w.inlines(n)
		w.out.WriteString("</" + tag + ">\n")
	case *ast.Blockquote:
		w.startBlock()
		w.out.WriteString("<blockquote>\n")
		w.blocks(n)
		w.out.WriteString("</blockquote>\n")
	case *ast.List:
		w.list(n)
	case *ast.ListItem:
		w.listItem(n)
	case *ast.FencedCodeBlock:
		w.codeBlock(languageOf(n, w.source), n.Lines())
	case *ast.CodeBlock:
		w.codeBlock("", n.Lines())
	case *ast.HTMLBlock:
		w.htmlBlock(n)
	case *ast.ThematicBreak:
		w.startBlock()
		w.out.WriteString("<hr />\n")
	case *extast.Table:
		w.table(n)
	case *extast.FootnoteList:
		w.footnoteList(n)
	case *extast.Footnote:
		w.footnote(n)
	case *extast.FootnoteBacklink:
		// Directly under the footnote when its last block is not a paragraph.
		w.footnoteBacklink(n)
	default:
		// A block kind noda does not enable: keep what it holds.
		w.blocks(n)
	}
}

func languageOf(block *ast.FencedCodeBlock, source []byte) string {
	if block.Info == nil {
		return ""
	}
	info := markdown.Unescape(block.Language(source))
	return info
}

func (w *bodyWriter) list(list *ast.List) {
	w.startBlock()
	tag := "ul"
	opening := "<ul>\n"
	if list.IsOrdered() {
		tag = "ol"
		opening = "<ol>\n"
		if list.Start != 1 {
			opening = fmt.Sprintf("<ol start=\"%d\">\n", list.Start)
		}
	}
	w.out.WriteString(opening)
	w.blocks(list)
	w.out.WriteString("</" + tag + ">\n")
}

func (w *bodyWriter) listItem(item *ast.ListItem) {
	w.out.WriteString("<li>")
	w.blocks(item)
	w.out.WriteString("</li>\n")
}

func (w *bodyWriter) codeBlock(language string, lines *text.Segments) {
	w.startBlock()
	if language == "" {
		w.out.WriteString("<pre><code>")
	} else {
		w.out.WriteString("<pre><code class=\"language-" + escapeAttribute(language) + "\">")
	}
	for i := 0; i < lines.Len(); i++ {
		segment := lines.At(i)
		w.out.WriteString(escapeText(string(segment.Value(w.source))))
	}
	w.out.WriteString("</code></pre>\n")
}

// htmlBlock turns raw HTML into a code block: shown, not run.
func (w *bodyWriter) htmlBlock(block *ast.HTMLBlock) {
	w.startBlock()
	w.out.WriteString("<pre><code class=\"language-html\">")
	lines := block.Lines()
	for i := 0; i < lines.Len(); i++ {
		segment := lines.At(i)
		w.out.WriteString(escapeText(string(segment.Value(w.source))))
	}
	if block.HasClosure() {
		closure := block.ClosureLine
		w.out.WriteString(escapeText(string(closure.Value(w.source))))
	}
	w.out.WriteString("</code></pre>\n")
}

func (w *bodyWriter) table(table *extast.Table) {
	w.startBlock()
	w.out.WriteString("<table>")
	bodyStarted := false
	for child := table.FirstChild(); child != nil; child = child.NextSibling() {
		switch row := child.(type) {
		case *extast.TableHeader:
			w.out.WriteString("<thead><tr>")
			w.cells(row, "th")
			w.out.WriteString("</tr></thead>")
		case *extast.TableRow:
			if !bodyStarted {
				w.out.WriteString("<tbody>\n")
				bodyStarted = true
			}
			w.out.WriteString("<tr>")
			w.cells(row, "td")
			w.out.WriteString("</tr>\n")
		}
	}
	if bodyStarted {
		w.out.WriteString("</tbody>")
	}
	w.out.WriteString("</table>\n")
}

// footnoteList is the notes gathered at the end of the body, in the order they
// were first cited; the parser drops a definition nothing cites.
func (w *bodyWriter) footnoteList(list *extast.FootnoteList) {
	w.startBlock()
	w.out.WriteString("<div class=\"footnotes\" role=\"doc-endnotes\">\n<hr />\n<ol>\n")
	w.blocks(list)
	w.out.WriteString("</ol>\n</div>\n")
}

func (w *bodyWriter) footnote(note *extast.Footnote) {
	w.out.WriteString("<li id=\"fn:" + strconv.Itoa(note.Index) + "\">\n")
	w.blocks(note)
	w.out.WriteString("</li>\n")
}

// footnoteLink is the number in the text. Inside a link it stays a bare number:
// an anchor in an anchor would end the outer one early. The id stays either way,
// for the arrow under the footnote to return to.
func (w *bodyWriter) footnoteLink(reference *extast.FootnoteLink) {
	number := strconv.Itoa(reference.Index)
	opening := "<sup id=\"" + referenceID(reference.Index, reference.RefIndex) + "\">"
	if w.links > 0 {
		w.out.WriteString(opening + number + "</sup>")
		return
	}
	w.out.WriteString(opening + "<a href=\"#fn:" + number + "\" class=\"footnote-ref\" role=\"doc-noteref\">" + number + "</a></sup>")
}

// footnoteBacklink returns to the reference it answers; a note cited twice gets
// two, told apart by the citation's number.
func (w *bodyWriter) footnoteBacklink(back *extast.FootnoteBacklink) {
	w.out.WriteString("&#160;<a href=\"#" + referenceID(back.Index, back.RefIndex) +
		"\" class=\"footnote-backref\" role=\"doc-backlink\">&#x21a9;&#xfe0e;")
	if back.RefIndex > 0 {
		w.out.WriteString("<sup>" + strconv.Itoa(back.RefIndex+1) + "</sup>")
	}
	w.out.WriteString("</a>")
}

// referenceID names where a note is cited: the first citation is fnref:1, a
// second citation of the same note fnref1:1.
func referenceID(note, citation int) string {
	if citation == 0 {
		return "fnref:" + strconv.Itoa(note)
	}
	return "fnref" + strconv.Itoa(citation) + ":" + strconv.Itoa(note)
}

func (w *bodyWriter) cells(row ast.Node, tag string) {
	for cell := row.FirstChild(); cell != nil; cell = cell.NextSibling() {
		alignment := ""
		if c, ok := cell.(*extast.TableCell); ok {
			alignment = alignmentStyle(c.Alignment)
		}
		w.out.WriteString("<" + tag + alignment + ">")
		w.inlines(cell)
		w.out.WriteString("</" + tag + ">")
	}
}

func alignmentStyle(alignment extast.Alignment) string {
	switch alignment {
	case extast.AlignLeft:
		return ` style="text-align: left"`
	case extast.AlignCenter:
		return ` style="text-align: center"`
	case extast.AlignRight:
		return ` style="text-align: right"`
	}
	return ""
}

func (w *bodyWriter) inlines(parent ast.Node) {
	for child := parent.FirstChild(); child != nil; child = child.NextSibling() {
		w.inline(child)
	}
	w.spill()
}

func (w *bodyWriter) inline(node ast.Node) {
	if text, ok := node.(*ast.Text); ok {
		w.text(text)
		return
	}
	w.spill()
	switch n := node.(type) {
	case *ast.Emphasis:
		tag := "em"
		if n.Level == 2 {
			tag = "strong"
		}
		w.wrapped(tag, n)
	case *extast.Strikethrough:
		w.wrapped("del", n)
	case *ast.CodeSpan:
		w.out.WriteString("<code>" + escapeText(codeSpanText(n, w.source)) + "</code>")
	case *ast.RawHTML:
		w.out.WriteString("<code>" + escapeText(rawHTMLText(n, w.source)) + "</code>")
	case *extast.TaskCheckBox:
		w.checkbox(n)
	case *extast.FootnoteLink:
		w.footnoteLink(n)
	case *extast.FootnoteBacklink:
		w.footnoteBacklink(n)
	case *ast.Link:
		w.link(n)
	case *ast.AutoLink:
		w.autoLink(n)
	case *ast.Image:
		w.image(n)
	default:
		w.inlinesOf(n)
	}
}

func (w *bodyWriter) inlinesOf(parent ast.Node) {
	for child := parent.FirstChild(); child != nil; child = child.NextSibling() {
		w.inline(child)
	}
	w.spill()
}

func (w *bodyWriter) wrapped(tag string, node ast.Node) {
	w.out.WriteString("<" + tag + ">")
	w.inlinesOf(node)
	w.out.WriteString("</" + tag + ">")
}

func (w *bodyWriter) checkbox(box *extast.TaskCheckBox) {
	if box.IsChecked {
		w.out.WriteString("<input disabled=\"\" type=\"checkbox\" checked=\"\"/>\n")
		return
	}
	w.out.WriteString("<input disabled=\"\" type=\"checkbox\"/>\n")
}

// text gathers prose before it is scanned: the parser splits text at every `_`
// or `*` it considered, which would cut an address at its first underscore.
func (w *bodyWriter) text(text *ast.Text) {
	w.prose.WriteString(markdown.Unescape(text.Segment.Value(w.source)))
	switch {
	case text.HardLineBreak():
		w.spill()
		w.out.WriteString("<br />\n")
	case text.SoftLineBreak():
		w.spill()
		w.out.WriteByte('\n')
	}
}

// spill writes a gathered run of prose out, opening any bare address in it.
func (w *bodyWriter) spill() {
	if w.prose.Len() == 0 {
		return
	}
	prose := w.prose.String()
	w.prose.Reset()
	if w.links > 0 || w.images > 0 {
		w.out.WriteString(escapeText(prose))
		return
	}
	at := 0
	for _, span := range bareURLs(prose) {
		w.out.WriteString(escapeText(prose[at:span[0]]))
		address := prose[span[0]:span[1]]
		w.out.WriteString(leaving(address) + escapeText(address) + "</a>")
		at = span[1]
	}
	w.out.WriteString(escapeText(prose[at:]))
}

func (w *bodyWriter) link(link *ast.Link) {
	destination := markdown.Unescape(link.Destination)
	w.links++
	defer func() { w.links-- }()
	switch target := w.route(destination, false); target.kind {
	case routeNote:
		w.out.WriteString(noteLink(target))
		w.inlinesOf(link)
		w.out.WriteString("</a>")
	case routeTo:
		w.out.WriteString("<a href=\"" + escapeHref(target.url) + "\"")
		if title := markdown.Unescape(link.Title); title != "" {
			w.out.WriteString(" title=\"" + escapeAttribute(title) + "\"")
		}
		w.out.WriteString(">")
		w.inlinesOf(link)
		w.out.WriteString("</a>")
	case routeAway:
		w.out.WriteString(leaving(target.url))
		w.inlinesOf(link)
		w.out.WriteString("</a>")
	default:
		w.inlinesOf(link)
	}
}

func (w *bodyWriter) autoLink(auto *ast.AutoLink) {
	address := string(auto.URL(w.source))
	label := string(auto.Label(w.source))
	if auto.AutoLinkType == ast.AutoLinkEmail {
		// The address has no scheme; the renderer adds mailto:, so route
		// would take it for a relative filename.
		w.out.WriteString("<a href=\"mailto:" + escapeHref(address) + "\">" + escapeText(label) + "</a>")
		return
	}
	switch target := w.route(address, false); target.kind {
	case routeNote:
		w.out.WriteString(noteLink(target) + escapeText(label) + "</a>")
	case routeTo:
		w.out.WriteString("<a href=\"" + escapeHref(target.url) + "\">" + escapeText(label) + "</a>")
	case routeAway:
		w.out.WriteString(leaving(target.url) + escapeText(label) + "</a>")
	default:
		w.out.WriteString(escapeText(label))
	}
}

func (w *bodyWriter) image(image *ast.Image) {
	destination := markdown.Unescape(image.Destination)
	alt := escapeAttribute(w.altText(image))
	target := w.route(destination, true)
	// Only the alt text is kept for a note or a refused address:
	// `![x](a-note.md)` would display a page as a picture.
	if target.kind != routeTo && target.kind != routeAway {
		w.out.WriteString(alt)
		return
	}
	w.out.WriteString("<img src=\"" + escapeHref(target.url) + "\" alt=\"" + alt + "\"")
	if title := markdown.Unescape(image.Title); title != "" {
		w.out.WriteString(" title=\"" + escapeAttribute(title) + "\"")
	}
	w.out.WriteString(" />")
}

// altText is the plain text under an image, markup stripped.
func (w *bodyWriter) altText(image *ast.Image) string {
	var alt strings.Builder
	var collect func(node ast.Node)
	collect = func(node ast.Node) {
		for child := node.FirstChild(); child != nil; child = child.NextSibling() {
			switch c := child.(type) {
			case *ast.Text:
				alt.WriteString(markdown.Unescape(c.Segment.Value(w.source)))
				if c.SoftLineBreak() || c.HardLineBreak() {
					alt.WriteByte(' ')
				}
			case *ast.CodeSpan:
				alt.WriteString(codeSpanText(c, w.source))
			case *ast.RawHTML:
				alt.WriteString(rawHTMLText(c, w.source))
			default:
				collect(child)
			}
		}
	}
	collect(image)
	return alt.String()
}

func codeSpanText(span *ast.CodeSpan, source []byte) string {
	var text strings.Builder
	for child := span.FirstChild(); child != nil; child = child.NextSibling() {
		if segment, ok := child.(*ast.Text); ok {
			text.Write(segment.Segment.Value(source))
		}
	}
	return strings.ReplaceAll(text.String(), "\n", " ")
}

func rawHTMLText(raw *ast.RawHTML, source []byte) string {
	var text strings.Builder
	for i := 0; i < raw.Segments.Len(); i++ {
		segment := raw.Segments.At(i)
		text.Write(segment.Value(source))
	}
	return text.String()
}

// route decides where a destination is allowed to lead. embed (an image,
// fetched unasked) narrows what is allowed.
func (w *bodyWriter) route(destination string, embed bool) route {
	if strings.HasPrefix(destination, "#") {
		return route{kind: routeTo, url: destination}
	}
	if path, ok := link.Target(destination); ok {
		if id, isNote := w.around.Notes[path]; isNote {
			return route{kind: routeNote, url: "/nb/" + urlPath(w.around.Book) + "/n/" + urlPath(id), id: id}
		}
		return route{kind: routeTo, url: "/nb/" + urlPath(w.around.Book) + "/f/" + urlPath(path)}
	}
	scheme, ok := link.Scheme(destination)
	if !ok || !serveable(scheme, embed) {
		// No scheme means a path link.Target refused, e.g. one climbing out.
		return route{kind: routeNowhere}
	}
	if strings.EqualFold(scheme, "http") || strings.EqualFold(scheme, "https") {
		return route{kind: routeAway, url: destination}
	}
	return route{kind: routeTo, url: destination}
}

// serveable is an allow-list, so `data:` and any scheme invented later are
// refused too.
func serveable(scheme string, embed bool) bool {
	allowed := []string{"http", "https", "mailto", "tel"}
	if embed {
		allowed = allowed[:2]
	}
	for _, name := range allowed {
		if strings.EqualFold(scheme, name) {
			return true
		}
	}
	return false
}

// leaving is the anchor for a destination that leaves the notebook, shared by
// written and bare links so both behave the same.
func leaving(url string) string {
	return "<a href=\"" + escape(url) + "\" target=\"_blank\" rel=\"noopener noreferrer\">"
}

// urlPath percent-encodes a path; a space, `#` or `?` would end it. `/` is
// kept so a nested path means the same as on disk.
func urlPath(text string) string {
	var out strings.Builder
	for i := 0; i < len(text); i++ {
		b := text[i]
		if isPathSafe(b) {
			out.WriteByte(b)
			continue
		}
		fmt.Fprintf(&out, "%%%02X", b)
	}
	return out.String()
}

func isPathSafe(b byte) bool {
	switch {
	case b >= 'a' && b <= 'z', b >= 'A' && b <= 'Z', b >= '0' && b <= '9':
		return true
	}
	return strings.IndexByte("-_.~/", b) >= 0
}

// bareURLs is the [start, end) span of each bare http:// and https:// address
// in a run of prose.
//
// GFM's autolink literal rules, narrowed to two schemes:
//   - Start after nothing, whitespace or one of `*_~(`, so
//     `xhttps://example.com` does not match.
//   - End at the first whitespace or `<`, then back over `?!.,:*_~`; a
//     trailing `)` is dropped only if unbalanced, so `https://en.example.org/A_(b)`
//     keeps it. Quotes and `;` stay, as in GFM.
//
// Running after the parser, an address the parser actually turned into
// emphasis is not found. That is rare, fails towards prose, and fixing it
// would mean owning an inline parser.
func bareURLs(text string) [][2]int {
	var found [][2]int
	from := 0
	for from < len(text) {
		offset := strings.Index(text[from:], "http")
		if offset < 0 {
			break
		}
		start := from + offset
		from = start + len("http")
		if !startsAddress(text, start) {
			continue
		}
		host := start + schemeLength(text[start:])
		if host == start {
			continue
		}
		stop := strings.IndexFunc(text[host:], func(r rune) bool { return isSpace(r) || r == '<' })
		if stop < 0 {
			stop = len(text) - host
		}
		end := sentenceOff(text, host, host+stop)
		if end > host {
			found = append(found, [2]int{start, end})
			from = end
		}
	}
	return found
}

func startsAddress(text string, start int) bool {
	if start == 0 {
		return true
	}
	before := []rune(text[:start])
	last := before[len(before)-1]
	return isSpace(last) || strings.ContainsRune("*_~(", last)
}

func schemeLength(text string) int {
	switch {
	case strings.HasPrefix(text, "https://"):
		return len("https://")
	case strings.HasPrefix(text, "http://"):
		return len("http://")
	}
	return 0
}

// sentenceOff walks end back over the sentence's punctuation. host is where
// the address's own text begins, so the walk cannot eat the scheme.
func sentenceOff(text string, host, end int) int {
	for end > host {
		last := text[end-1]
		switch {
		case strings.IndexByte("?!.,:*_~", last) >= 0:
			end--
		case last == ')':
			inside := text[host:end]
			if strings.Count(inside, ")") <= strings.Count(inside, "(") {
				return end
			}
			end--
		default:
			return end
		}
	}
	return end
}

// isSpace is Unicode White_Space, as Rust's char::is_whitespace.
func isSpace(r rune) bool {
	switch r {
	case '\t', '\n', '\v', '\f', '\r', ' ', 0x85, 0xA0, 0x1680, 0x2028, 0x2029, 0x202F, 0x205F, 0x3000:
		return true
	}
	return r >= 0x2000 && r <= 0x200A
}
