package harness

import (
	"fmt"
	"image/color"
	"strings"
	"time"

	"github.com/chromedp/chromedp"
	"github.com/chromedp/chromedp/kb"
)

// Page is the pages as the steps talk about them. Everything is found by the
// classes the stylesheet already uses; nothing is added to the markup for
// tests, since a test-only hook breaks unnoticed.
type Page struct {
	Browser *Browser
	BaseURL string
}

// Go opens a path on the server.
func (p Page) Go(path string) error { return p.Browser.Goto(p.BaseURL + path) }

// Path is the address bar's path, without the query string.
func (p Page) Path() (string, error) {
	current, err := p.Browser.URL()
	if err != nil {
		return "", err
	}
	return current.Path, nil
}

// Searched is the address's q parameter, if any.
func (p Page) Searched() (string, bool, error) {
	current, err := p.Browser.URL()
	if err != nil {
		return "", false, err
	}
	query := current.Query()
	return query.Get("q"), query.Has("q"), nil
}

// Rows is every visible row's text, in one round trip so a navigation between
// finding and reading cannot go stale. The visibility filter is required: the
// listing carries every note and hides the excluded ones, and innerText falls
// back to textContent on an unrendered element.
func (p Page) Rows() ([]string, error) {
	return Exec[[]string](p.Browser, `return Array.from(document.querySelectorAll('.row'))
		.filter(e => e.offsetParent !== null).map(e => e.innerText);`)
}

// RunHeadings are the headings the listing's rows are cut under, as drawn.
func (p Page) RunHeadings() ([]string, error) {
	return Exec[[]string](p.Browser, `return Array.from(document.querySelectorAll('.index .run h2'))
		.filter(e => e.offsetParent !== null).map(e => e.textContent.trim());`)
}

// HiddenRows are rows the query excluded: on the page, not on the screen.
func (p Page) HiddenRows() ([]string, error) {
	return Exec[[]string](p.Browser, `return Array.from(document.querySelectorAll('.row'))
		.filter(e => e.offsetParent === null).map(e => e.textContent);`)
}

// Press presses any link naming what, rows included.
func (p Page) Press(what string) error {
	return p.click("a link naming "+quote(what), `//a[contains(., `+xpathString(what)+`)]`)
}

// PressOrder presses the order chip of that name, "title" for one that sorts
// by title: a row's title is also a link, and Press would find it first.
func (p Page) PressOrder(name string) error {
	return p.click("the order chip "+quote(name),
		`//nav[contains(@class, 'sortbar')]//a[normalize-space(.)=`+xpathString(name)+`]`)
}

// NothingIsWaiting is true when no pane of the screen is marked as waiting for
// an answer: the mark the page puts on a pane while a press is on its way.
func (p Page) NothingIsWaiting() (bool, error) {
	return Exec[bool](p.Browser, `return document.querySelector('[aria-busy]') === null;`)
}

// PressInMargin is Press within the margin note, whose titles usually also
// appear in the listing, where Press would find them first.
func (p Page) PressInMargin(what string) error {
	return p.click("a link naming "+quote(what)+" in the margin note",
		`//section[contains(@class, 'beside')]//a[contains(., `+xpathString(what)+`)]`)
}

// Submit presses the button containing what.
func (p Page) Submit(what string) error {
	return p.click("a button saying "+quote(what), `//button[contains(., `+xpathString(what)+`)]`)
}

// TapBack presses the page's own way back.
func (p Page) TapBack() error { return p.clickCSS("the way back", ".back") }

// TapWrite presses the new-note button, the one round button in the rail.
func (p Page) TapWrite() error { return p.clickCSS("the button to write", ".rail .fab") }

// Untick unticks the box for a tag.
func (p Page) Untick(tag string) error {
	return p.clickCSS("the box for "+quote(tag), fmt.Sprintf("input[name='keep'][value=%s]", cssString(tag)))
}

// TagBoxFocused is whether the focus is on the box for a tag.
func (p Page) TagBoxFocused(tag string) (bool, error) {
	return Exec[bool](p.Browser, `return document.activeElement.matches(arguments[0]);`,
		fmt.Sprintf("input[name='keep'][value=%s]", cssString(tag)))
}

// tagChipBox is the box of the displayed tag chip with this name, in the
// viewport after the chip was scrolled to its middle: left, top, width, height.
func (p Page) tagChipBox(name string) (*[4]float64, error) {
	return Exec[*[4]float64](p.Browser, `
		const chip = Array.from(document.querySelectorAll('a.tag'))
			.find((el) => el.offsetParent !== null && el.textContent.trim() === arguments[0]);
		if (!chip) return null;
		chip.scrollIntoView({block: 'center', inline: 'center'});
		const box = chip.getBoundingClientRect();
		return [box.left, box.top, box.width, box.height];`, name)
}

// TagChipHeight is the height of the displayed tag chip with this name.
func (p Page) TagChipHeight(name string) (height float64, present bool, err error) {
	box, err := p.tagChipBox(name)
	if err != nil || box == nil {
		return 0, false, err
	}
	return box[3], true, nil
}

// OrderLineRoom is how much clear space the order line's drawn pill has above
// it, to the search field, and below it, to the list. The pill is the chip's
// box less --grow top and bottom, so a bigger hit area does not count as room.
func (p Page) OrderLineRoom() (above, below float64, present bool, err error) {
	room, err := Exec[*[2]float64](p.Browser, `
		const chip = document.querySelector('.index .sortbar a');
		const field = document.querySelector('.index .searchbar input[type=search]');
		const rows = document.querySelector('.index .rows');
		if (!chip || !field || !rows) return null;
		const grow = parseFloat(getComputedStyle(chip).getPropertyValue('--grow')) || 0;
		const box = chip.getBoundingClientRect();
		return [
			box.top + grow - field.getBoundingClientRect().bottom,
			rows.getBoundingClientRect().top - (box.bottom - grow),
		];`)
	if err != nil || room == nil {
		return 0, 0, false, err
	}
	return room[0], room[1], true, nil
}

// RestOnTag puts the pointer on the middle of the note's tag chip.
func (p Page) RestOnTag(name string) error {
	return Eventually("resting the pointer on the tag "+quote(name), func() (bool, error) {
		box, err := p.tagChipBox(name)
		if err != nil || box == nil {
			return false, err
		}
		return true, p.Browser.PointerTo(box[0]+box[2]/2, box[1]+box[3]/2)
	})
}

// TagIsReadable reports whether the name of a tag the pointer rests on is drawn
// over the chip's highlight. A hit test cannot say: a pseudo-element answers as
// its link whichever way round they are stacked, so the answer is in the
// pixels. Beside the chip's own fill, the name's ink must show.
func (p Page) TagIsReadable(name string) (bool, error) {
	box, err := p.tagChipBox(name)
	if err != nil || box == nil {
		return false, err
	}
	lit, err := Exec[bool](p.Browser, `return Array.from(document.querySelectorAll('a.tag:hover'))
		.some((el) => el.textContent.trim() === arguments[0]);`, name)
	if err != nil {
		return false, err
	}
	if !lit {
		return false, fmt.Errorf("the pointer is not on the tag %s, so the highlight is not drawn", quote(name))
	}
	pixels, err := p.Browser.Pixels()
	if err != nil {
		return false, err
	}
	middle := int(box[1] + box[3]/2)
	fill := pixels.At(int(box[0])+3, middle)
	inked := 0
	for y := middle - 6; y <= middle+6; y++ {
		for x := int(box[0]) + 4; x < int(box[0]+box[2])-4; x++ {
			if differsFrom(fill, pixels.At(x, y)) {
				inked++
			}
		}
	}
	return inked >= minInkedPixels, nil
}

// minInkedPixels is far under the few dozen a short word covers and far over
// the stray ones a rounded edge could add.
const minInkedPixels = 12

// differsFrom is whether two pixels are plainly two colours: a channel apart
// by a fifth of its range, which a glyph's edge is and a fill's noise is not.
func differsFrom(a, b color.Color) bool {
	ar, ag, ab, _ := a.RGBA()
	br, bg, bb, _ := b.RGBA()
	return channelApart(ar, br) || channelApart(ag, bg) || channelApart(ab, bb)
}

func channelApart(a, b uint32) bool {
	const fifth = 0xffff / 5
	if a > b {
		return a-b > fifth
	}
	return b-a > fifth
}

// xpathString is a string literal for XPath 1.0, which has no escape
// character: a value with a quote is assembled with concat.
func xpathString(value string) string {
	if !strings.ContainsRune(value, '\'') {
		return "'" + value + "'"
	}
	pieces := strings.Split(value, "'")
	out := "concat("
	for i, piece := range pieces {
		if i > 0 {
			out += `, "'", `
		}
		out += "'" + piece + "'"
	}
	return out + ")"
}

const findByXPath = `const found = document.evaluate(arguments[0], document, null,
	XPathResult.ORDERED_NODE_SNAPSHOT_TYPE, null);
	const all = []; for (let i = 0; i < found.snapshotLength; i++) all.push(found.snapshotItem(i));
	return all;`

func (p Page) click(what, xpath string) error {
	return p.clickMatching(what, "xpath", xpath)
}

func (p Page) clickCSS(what, selector string) error {
	return p.clickMatching(what, "css", selector)
}

// clickMatching clicks the first displayed match, retrying until WaitTimeout.
// A match that is gone by the next look counts as "not yet": the network
// screen reloads itself during an errand. Hidden matches are skipped because
// panes can hold two of a control. The click is a real one at the element's
// centre, so reachability is tested and the script-less pass is not bypassed.
func (p Page) clickMatching(what, kind, locator string) error {
	return Eventually("pressing "+what, func() (bool, error) {
		point, err := Exec[*[2]float64](p.Browser, `
			const all = arguments[0] === 'xpath' ? (function() {`+findByXPath+`}).call(null, arguments[1])
				: Array.from(document.querySelectorAll(arguments[1]));
			for (const el of all) {
				const box = el.getBoundingClientRect();
				if (box.width === 0 || box.height === 0 || getComputedStyle(el).visibility === 'hidden') continue;
				el.scrollIntoView({block: 'center', inline: 'center'});
				const at = el.getBoundingClientRect();
				// A page wider than the phone (a long path in the text) is shrunk to
				// fit, and the mouse is placed in the shrunk viewport's own pixels.
				const view = window.visualViewport;
				return [(at.left + at.width / 2 - view.offsetLeft) * view.scale,
					(at.top + at.height / 2 - view.offsetTop) * view.scale];
			}
			return null;`, kind, locator)
		if err != nil || point == nil {
			return false, err
		}
		if err := p.Browser.WaitForLoad(); err != nil {
			return false, err
		}
		return p.clickAt(point)
	})
}

// clickAt sends one real click and, when scripts run, confirms the page got
// it. A page that reloads itself (the network screen polls while an errand
// runs) can be replaced between the lookup and the click, and the click is
// lost; an unconfirmed click is reported as "not yet", so the lookup is
// repeated. The recorder writes to sessionStorage, which survives the
// navigation the click itself causes.
func (p Page) clickAt(point *[2]float64) (bool, error) {
	if !p.Browser.scripts {
		return true, p.Browser.Do(chromedp.MouseClickXY(point[0], point[1]))
	}
	if _, err := Eval[bool](p.Browser, `sessionStorage.removeItem('noda_clicked');
		document.addEventListener('click', () => sessionStorage.setItem('noda_clicked', '1'), {capture: true, once: true});
		true`); err != nil {
		return false, err
	}
	if err := p.Browser.Do(chromedp.MouseClickXY(point[0], point[1])); err != nil {
		return false, err
	}
	deadline := time.Now().Add(clickSettle)
	for time.Now().Before(deadline) {
		got, err := Eval[bool](p.Browser, `sessionStorage.getItem('noda_clicked') === '1'`)
		if err == nil && got {
			return true, nil
		}
		time.Sleep(WaitInterval)
	}
	return false, nil
}

// clickSettle is how long a click gets to reach the page.
const clickSettle = 3 * time.Second

// Fill types into the field with this name: what the form sends, unlike a
// label, which is prose and gets rewritten. The typing is real keystrokes.
func (p Page) Fill(name, value string) error {
	return p.typeInto("[name="+cssString(name)+"]", value)
}

// FillWithBytes puts this many bytes into the field with this name in one go.
// Typing is how a reader fills a field, but not two megabytes of it.
func (p Page) FillWithBytes(name string, size int) error {
	return Eventually("filling "+name, func() (bool, error) {
		return Exec[bool](p.Browser, `const field = document.querySelector('[name=' + CSS.escape(arguments[0]) + ']');
			if (!field) return false;
			field.value = 'x'.repeat(arguments[1]);
			return true;`, name, size)
	})
}

const searchField = "input[name='q']"

// Search types a query and presses Enter, as a phone keyboard would.
func (p Page) Search(query string) error { return p.typeInto(searchField, query+kb.Enter) }

// typeInto empties the field matching selector, focuses it and types keys.
// The field is awaited: the script-less pass clicks a link or button and gets
// no signal when the next page has loaded, so the field may not exist yet.
func (p Page) typeInto(selector, keys string) error {
	err := Eventually("focusing "+selector, func() (bool, error) {
		focused, err := Exec[bool](p.Browser, `const el = document.querySelector(arguments[0]);
			if (!el) return false;
			el.focus();
			el.value = '';
			return true;`, selector)
		if err != nil {
			return false, err
		}
		if !focused {
			return false, fmt.Errorf("this page has no field matching %s", selector)
		}
		return true, nil
	})
	if err != nil {
		return err
	}
	return p.Browser.Do(chromedp.KeyEvent(keys))
}

// TypeSearch types a query without submitting, for filter-as-you-type. It uses
// real backspaces rather than clearing the value, which fires no input event
// and so let "delete what you typed" pass against a script that never ran.
func (p Page) TypeSearch(query string) error {
	held, err := Exec[int](p.Browser, `const el = document.querySelector(arguments[0]);
		if (!el) return -1;
		el.focus();
		return el.value.length;`, searchField)
	if err != nil {
		return err
	}
	if held < 0 {
		return fmt.Errorf("this page has no search field")
	}
	actions := []chromedp.Action[chromedp.Void]{}
	for range held {
		actions = append(actions, chromedp.KeyEvent(kb.Backspace))
	}
	if query != "" {
		actions = append(actions, chromedp.KeyEvent(query))
	}
	return p.Browser.Do(actions...)
}

// HitKeys sends real keystrokes to whatever has the focus, as a keyboard does.
// Nothing here dispatches an event of its own, so a shortcut is tested through
// the same listener a reader's key reaches.
func (p Page) HitKeys(keys string) error {
	for _, key := range keys {
		if err := p.Browser.KeyPress(key); err != nil {
			return err
		}
	}
	return nil
}

// HitChord sends Ctrl, or Command on a Mac, held with the key: the page tells
// the two apart no more than the keyboard does, so one is enough.
func (p Page) HitChord(key rune) error {
	return p.Browser.KeyPressWith(key, kb.ModifierCtrl)
}

// HitKeyUnderInputMethod is a letter's keydown that an input method was
// handling, which a shortcut must leave alone.
func (p Page) HitKeyUnderInputMethod(letter string) error {
	return p.Browser.KeyDownDuringComposition(letter, "Key"+strings.ToUpper(letter))
}

// WaitingForSecondKey reports the strip that follows `g`, which lists the
// places a second key goes to.
func (p Page) WaitingForSecondKey() (bool, error) {
	return Eval[bool](p.Browser, "!!document.querySelector('.keys-next')")
}

// ShortcutCardOpen reports whether the card that lists the keys is showing.
func (p Page) ShortcutCardOpen() (bool, error) {
	return Eval[bool](p.Browser, "!!document.querySelector('dialog.keys[open]')")
}

// SearchFieldFocused reports whether the search field has the focus.
func (p Page) SearchFieldFocused() (bool, error) {
	return Exec[bool](p.Browser, `return document.activeElement === document.querySelector(arguments[0]);`, searchField)
}

// FieldHolds is what the field with this name holds right now, and whether
// there is one.
func (p Page) FieldHolds(name string) (string, bool, error) {
	return p.optionalText(`const el = document.querySelector('[name="' + arguments[0] + '"]');
		return el ? el.value : null;`, name)
}

// GoBack is the browser's own back button, which pops the history entry a
// press pushed, unlike the page's chevron (a plain link).
func (p Page) GoBack() error { return p.Browser.Back() }

// Remember marks the window, to tell a pane swap from a navigation: the mark
// survives exactly as long as the document.
func (p Page) Remember() error {
	_, err := Eval[int](p.Browser, "window.__noda_here = 1")
	return err
}

// Remembered reports whether the document marked by Remember is still loaded.
func (p Page) Remembered() (bool, error) { return Eval[bool](p.Browser, "!!window.__noda_here") }

// RememberNote marks the note's body element, to tell a note replaced by a
// fetch from one that was only scrolled: Remember cannot, for the document is
// the same either way.
func (p Page) RememberNote() error {
	_, err := Eval[int](p.Browser, "window.__noda_note = document.querySelector('.body'), 1")
	return err
}

// NoteRemembered reports whether the body element marked by RememberNote is
// still the one on screen.
func (p Page) NoteRemembered() (bool, error) {
	return Eval[bool](p.Browser, "!!window.__noda_note && window.__noda_note === document.querySelector('.body')")
}

// PressFootnote presses the number a footnote leaves in the text.
func (p Page) PressFootnote(number string) error {
	return p.clickCSS("the footnote "+quote(number), `a[href="#fn:`+number+`"]`)
}

// PressFootnoteBack presses the arrow that leads from a footnote to its
// citation.
func (p Page) PressFootnoteBack(number string) error {
	return p.clickCSS("the way back from footnote "+quote(number), `a[href="#fnref:`+number+`"]`)
}

// FootnoteLit reports whether the footnote's text, or with cited the number it
// leaves in the text, is what the address names.
func (p Page) FootnoteLit(number string, cited bool) (bool, error) {
	id := "fn:" + number
	if cited {
		id = "fnref:" + number
	}
	return Exec[bool](p.Browser, `const el = document.getElementById(arguments[0]);
		return !!el && el.matches(':target');`, id)
}

// MarkedPlace is the rail's place marked aria-current, the attribute the colour
// also hangs off. Empty and false when none, as on the network screen. The
// textContent is read because the label is drawn only from 640px up.
func (p Page) MarkedPlace() (string, bool, error) {
	text, err := Exec[*string](p.Browser, `const at = document.querySelector('.rail .actionbar a[aria-current]');
		return at ? at.textContent.trim() : null;`)
	if err != nil || text == nil {
		return "", false, err
	}
	return *text, true, nil
}

// IsOverdue reports whether this text is drawn as overdue: by the run it sits
// in, not by colour, which is web/theme's business.
func (p Page) IsOverdue(text string) (bool, error) {
	return Exec[bool](p.Browser, `return Array.from(document.querySelectorAll('.run.late .due'))
		.some(e => e.innerText.includes(arguments[0]));`, text)
}

// optionalText runs a script that returns a string, or null for "not there".
func (p Page) optionalText(body string, arguments ...any) (string, bool, error) {
	text, err := Exec[*string](p.Browser, body, arguments...)
	if err != nil || text == nil {
		return "", false, err
	}
	return *text, true, nil
}

// Hint is the search bar's hint; absent and hidden are both "not there".
func (p Page) Hint() (string, bool, error) {
	return p.optionalText(`const hint = document.querySelector('.searchbar .hint');
		return hint && hint.offsetParent !== null ? hint.innerText : null;`)
}

// ShownID is the first row's id if drawn, which asks offsetParent and not the
// markup: a row that was only written is not one that can be read.
func (p Page) ShownID() (string, bool, error) {
	return p.optionalText(`const id = document.querySelector('.rows .row .id');
		return id && id.offsetParent !== null ? id.innerText : null;`)
}

// Grouping is the search field's parsed grouping, flattened as
// `(tag:work or tag:q3) and (budget)`: the brackets stand for the on-screen
// pills, which a feature file cannot quote.
func (p Page) Grouping() (string, bool, error) {
	return p.optionalText(`const parse = document.querySelector('.searchbar .parse');
		if (!parse || parse.offsetParent === null) return null;
		return [...parse.children].map((piece) => piece.className === 'g'
			? '(' + [...piece.children].map((t) => t.textContent).join(' ') + ')'
			: piece.textContent).join(' ');`)
}

// ReloadsItself reports whether a <meta refresh> is present: the script-less
// network screen reloads by it, and the script removes it to poll instead.
func (p Page) ReloadsItself() (bool, error) {
	return Eval[bool](p.Browser, `!!document.querySelector('meta[http-equiv="refresh"]')`)
}

// reads is the rendered text (innerText) of selector, or "" when absent: not
// an error, because after a submit the page may still be the previous one and
// the retry loop must see "not yet".
func (p Page) reads(selector string) (string, error) {
	return Exec[string](p.Browser, `const el = document.querySelector(arguments[0]);
		return el ? el.innerText : '';`, selector)
}

// Heading is the h1 of what is being read. Beside a note the listing's own h1
// comes first in the document, so the index pane is passed over.
func (p Page) Heading() (string, error) {
	return Exec[string](p.Browser, `const el = document.querySelector('.pane:not(.index) h1') || document.querySelector('h1');
		return el ? el.innerText : '';`)
}

// Tab is the tab's title. A pane swap delivers it as a <title> in the
// fragment, so this checks the server's string reached the tab.
func (p Page) Tab() (string, error) { return Eval[string](p.Browser, "document.title") }

// Filename is the note's filename line.
func (p Page) Filename() (string, error) { return p.reads(".filename") }

// Body is the note's rendered body.
func (p Page) Body() (string, error) { return p.reads(".body") }

// About is what a note says about itself apart from its text: its tags and its
// two stamps, term and answer.
func (p Page) About() (string, error) { return p.reads(".about") }

// Stamps is only the times in About, as the script may have restated them.
func (p Page) Stamps() (string, error) {
	return Exec[string](p.Browser, `return Array.from(document.querySelectorAll('.about time'))
		.map(e => e.innerText).join(' | ');`)
}

// textInBody finds the first text node of the note's body holding text, in the
// script that follows.
const textInBody = `const walker = document.createTreeWalker(document.querySelector('.body'), NodeFilter.SHOW_TEXT);
	let node = null;
	while ((node = walker.nextNode())) if (node.data.includes(arguments[0])) break;
	if (!node) return null;`

// IsStruck reports whether the body's text is drawn with a line through it.
//
// A computed style only holds the decoration an element sets itself, but a
// decoration is drawn through everything inside the element too, except what
// is atomic inline-level, floated or positioned out of flow: those start over.
// So this walks up from the text to the first element that stops a decoration,
// which is the rule a browser draws by. The answer is nil when the body does
// not hold the text.
func (p Page) IsStruck(text string) (*bool, error) {
	return Exec[*bool](p.Browser, textInBody+`
		for (let el = node.parentElement; el; el = el.parentElement) {
			const style = getComputedStyle(el);
			if (style.textDecorationLine.includes('line-through')) return true;
			const startsOver = style.display.startsWith('inline-') || style.cssFloat !== 'none'
				|| style.position === 'absolute' || style.position === 'fixed';
			if (startsOver || el.classList.contains('body')) return false;
		}
		return false;`, text)
}

// ColourOf is the colour the body's text is drawn in, as the browser spells it.
func (p Page) ColourOf(text string) (string, bool, error) {
	return p.optionalText(textInBody+`
		return getComputedStyle(node.parentElement).color;`, text)
}

// Text is the whole page's rendered text.
func (p Page) Text() (string, error) { return Eval[string](p.Browser, "document.body.innerText") }

// HasProblem reports whether the page shows an error message.
func (p Page) HasProblem() (string, bool, error) {
	return p.optionalText(`const p = document.querySelector('.problem'); return p ? p.innerText : null;`)
}

// ClearOfTheBar is whether the first element matching selector starts below
// the sticky bar: a screen scrolled to its top edge puts what is first on it
// under the bar, where its first line cannot be read.
func (p Page) ClearOfTheBar(selector string) (bool, error) {
	below, err := Exec[*bool](p.Browser, `
		const el = document.querySelector(arguments[0]);
		const bar = document.querySelector('.topbar');
		if (!el || !bar) return null;
		return el.getBoundingClientRect().top >= bar.getBoundingClientRect().bottom - 0.5;`, selector)
	if err != nil {
		return false, err
	}
	if below == nil {
		return false, fmt.Errorf("no %s, or no bar, on the page", selector)
	}
	return *below, nil
}

// ControlsSmallerThan lists every control smaller than a thumb, with its
// measurements: only a laid-out page knows whether the minimum height held.
// It measures the hit area, not the ink (a 24px chevron in a 48px target).
func (p Page) ControlsSmallerThan(wide, tall int) ([]string, error) {
	return Exec[[]string](p.Browser, `
		const wide = arguments[0], tall = arguments[1];
		const short = [];
		for (const el of document.querySelectorAll('a, input, button')) {
			// A note's rendered checkboxes are disabled (ticking one would be a
			// commit), so they are text, not controls.
			if (el.disabled) continue;
			// A checkbox's target is its label.
			const target = el.closest('label') || el;
			const r = target.getBoundingClientRect();
			// Laid out to nothing: no target at all.
			if (r.width === 0 && r.height === 0) continue;
			if (r.width < wide || r.height < tall) {
				const what = (target.textContent || el.getAttribute('aria-label')
					|| el.name || el.tagName).trim().slice(0, 40);
				short.push(what + ' — ' + Math.round(r.width) + 'x' + Math.round(r.height));
			}
		}
		return short;`, wide, tall)
}

// FieldsUnder lists every field below least px, which iOS Safari zooms into
// on focus.
func (p Page) FieldsUnder(least float64) ([]string, error) {
	return Exec[[]string](p.Browser, `
		const small = [];
		for (const el of document.querySelectorAll('input, textarea, select')) {
			if (el.type === 'hidden' || el.type === 'checkbox') continue;
			const size = parseFloat(getComputedStyle(el).fontSize);
			if (size < arguments[0]) small.push((el.name || el.type) + ' — ' + size + 'px');
		}
		return small;`, least)
}

// SearchFieldFontSize is the search field's computed font size in CSS pixels.
func (p Page) SearchFieldFontSize() (float64, error) {
	size, err := Exec[*float64](p.Browser, `const field = document.querySelector("input[name='q']");
		return field ? parseFloat(getComputedStyle(field).fontSize) : null;`)
	if err != nil {
		return 0, err
	}
	if size == nil {
		return 0, fmt.Errorf("no search field on this page")
	}
	return *size, nil
}

// Background is the body's computed background colour.
func (p Page) Background() (string, error) {
	return Eval[string](p.Browser, "getComputedStyle(document.body).backgroundColor")
}

// Box is a left edge and a width, with the width of what holds it.
type Box struct{ Left, Width, Container float64 }

// BoxOf is the box of selector against the viewport: layout questions are
// about relationships, which a width alone cannot answer.
func (p Page) BoxOf(selector string) (Box, error) {
	return p.measure(selector, "", `
		const el = document.querySelector(arguments[0]);
		if (!el) return null;
		const r = el.getBoundingClientRect();
		return [r.left, r.width, window.innerWidth];`)
}

// BoxIn is the box of child relative to parent: the reading column is
// centred in its pane, not the window.
func (p Page) BoxIn(child, parent string) (Box, error) {
	return p.measure(child, parent, `
		const p = document.querySelector(arguments[1]);
		const c = document.querySelector(arguments[0]);
		if (!p || !c) return null;
		const pr = p.getBoundingClientRect();
		const cr = c.getBoundingClientRect();
		return [cr.left - pr.left, cr.width, pr.width];`)
}

func (p Page) measure(selector, parent, script string) (Box, error) {
	numbers, err := Exec[*[3]float64](p.Browser, script, selector, parent)
	if err != nil {
		return Box{}, err
	}
	if numbers == nil {
		return Box{}, fmt.Errorf("nothing matches %s %s", selector, parent)
	}
	return Box{Left: numbers[0], Width: numbers[1], Container: numbers[2]}, nil
}

// LinesOf is how many lines a bar's children wrap onto, which only the boxes
// reveal. It counts children starting below the previous ones' bottom, not
// distinct tops: chips sharing a baseline have several tops but are one line.
func (p Page) LinesOf(selector string) (int, error) {
	lines, err := Exec[*int](p.Browser, `
		const el = document.querySelector(arguments[0]);
		if (!el) return null;
		const kids = [...el.children].filter(k => k.getClientRects().length)
			.map(k => k.getBoundingClientRect()).sort((a, b) => a.top - b.top);
		let lines = 0;
		let floor = -Infinity;
		for (const r of kids) {
			if (r.top >= floor - 0.5) { lines += 1; floor = r.bottom; }
			else { floor = Math.min(floor, r.bottom); }
		}
		return lines;`, selector)
	if err != nil {
		return 0, err
	}
	if lines == nil {
		return 0, fmt.Errorf("nothing matches %s", selector)
	}
	return *lines, nil
}

// ListingOnScreen reports whether the listing pane has width and holds rows;
// either alone can be true by accident.
func (p Page) ListingOnScreen() (bool, error) {
	return Eval[bool](p.Browser, `(function() {
		const pane = document.querySelector('.pane.index');
		if (!pane) return false;
		return pane.getBoundingClientRect().width > 0 && !!pane.querySelector('main.rows a.row');
	})()`)
}

// BarOffsets is how far the note's bar sits below the listing's bar, measured
// at the rule under each and at the middle of what each holds. Side by side
// they read as one strip only when both are zero. It is not present until both
// bars are drawn: a hidden pane keeps its markup but has no box.
func (p Page) BarOffsets() (rule, middle float64, present bool, err error) {
	offsets, err := Exec[*[2]float64](p.Browser, `
		const list = document.querySelector('.pane.index .topbar');
		const count = document.querySelector('.pane.index .topbar .count');
		const note = document.querySelector('.pane.read .actionbar');
		const action = document.querySelector('.pane.read .actionbar a');
		if (!list || !count || !note || !action) return null;
		if (![list, count, note, action].every((el) => el.getClientRects().length)) return null;
		const middleOf = (el) => { const r = el.getBoundingClientRect(); return r.top + r.height / 2; };
		return [note.getBoundingClientRect().bottom - list.getBoundingClientRect().bottom,
			middleOf(action) - middleOf(count)];`)
	if err != nil || offsets == nil {
		return 0, 0, false, err
	}
	return offsets[0], offsets[1], true, nil
}

// GapUnderTitleBar is how far the note's action bar starts below the foot of
// the title bar above it. It is not present until both are drawn.
func (p Page) GapUnderTitleBar() (gap float64, present bool, err error) {
	measured, err := Exec[*float64](p.Browser, `
		const title = document.querySelector('.pane.read .topbar');
		const actions = document.querySelector('.pane.read .actionbar');
		if (!title || !actions || !title.getClientRects().length || !actions.getClientRects().length) return null;
		return actions.getBoundingClientRect().top - title.getBoundingClientRect().bottom;`)
	if err != nil || measured == nil {
		return 0, false, err
	}
	return *measured, true, nil
}

// MarkedRow is the title of the listing row marked as being read.
func (p Page) MarkedRow() (string, bool, error) {
	return p.optionalText(`const row = document.querySelector('.pane.index main.rows a.row.here');
		return row ? row.querySelector('.title').textContent.trim() : null;`)
}

// MarginNote is what the note's margin says links to it: one line per link, or
// its single "no other note links here" line. It is nil when no note is open,
// asked via offsetParent since the box is in the markup of a note at any width.
func (p Page) MarginNote() ([]string, bool, error) {
	lines, err := Exec[*[]string](p.Browser, `
		const aside = document.querySelector('.pane.read .beside');
		if (!aside || aside.offsetParent === null) return null;
		const minis = [...aside.querySelectorAll('.mini')];
		if (minis.length) return minis.map((m) => m.textContent.trim());
		const one = aside.querySelector('.none');
		return one ? [one.innerText.trim()] : [];`)
	if err != nil || lines == nil {
		return nil, false, err
	}
	return *lines, true, nil
}

// ScrollsSideways reports a layout that did not fit.
func (p Page) ScrollsSideways() (bool, error) {
	return Eval[bool](p.Browser, "document.documentElement.scrollWidth > window.innerWidth")
}

func quote(text string) string { return fmt.Sprintf("%q", text) }

// cssString is a CSS string literal for text that may hold quotes.
func cssString(text string) string {
	return `"` + strings.ReplaceAll(strings.ReplaceAll(text, `\`, `\\`), `"`, `\"`) + `"`
}
