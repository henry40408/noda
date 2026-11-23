package e2e

import (
	"fmt"
	"strings"
	"time"

	"github.com/chromedp/chromedp/kb"
	"github.com/cucumber/godog"

	"github.com/henry40408/noda/e2e/internal/harness"
)

// The keys are real keystrokes, sent to whatever has the focus: a step that
// dispatched its own event would pass against a listener no reader's key reaches.

func registerKeyboardSteps(scenario *godog.ScenarioContext, w *world) {
	scenario.Step(`^I hit the keys `+quoted+`$`, func(keys string) error { return w.page().HitKeys(keys) })
	scenario.Step(`^I hit the Escape key$`, func() error { return w.page().HitKeys(kb.Escape) })
	// Ctrl on a PC and Command on a Mac are one chord to the page.
	scenario.Step(`^I hit Ctrl and Enter$`, func() error { return w.page().HitChord('\r') })
	// What Safari reports for the key that ends a composition.
	scenario.Step(`^I hit the key `+quoted+` while an input method is composing$`, func(letter string) error {
		return w.page().HitKeyUnderInputMethod(letter)
	})

	// Firefox's search field has no Escape of its own, where Chrome's and
	// Safari's empty themselves: this is a page in which the default is gone.
	scenario.Step(`^the browser does not empty a search field on Escape$`, func() error {
		return w.browser.BeforeEveryPage(`addEventListener('keydown', (event) => {
			if (event.key === 'Escape' && event.target.type === 'search') event.preventDefault();
		}, true);`)
	})
	// A write sent in error is on its way the moment the key is hit, and a
	// second is far longer than it takes; only the failing case pays the wait.
	scenario.Step(`^the notebook still holds `+quoted+` after a moment$`, func(title string) error {
		time.Sleep(time.Second)
		listing, err := w.server.Run("ls")
		if err != nil {
			return err
		}
		if !strings.Contains(listing, title) {
			return fmt.Errorf("the notebook no longer holds %q:\n%s", title, listing)
		}
		return nil
	})

	scenario.Step(`^the page is waiting for the second key$`, func() error {
		return harness.Eventually("the strip that follows g", w.page().WaitingForSecondKey)
	})
	// A key that did nothing leaves nothing to wait for, so the second step looks
	// once, after the first has had its chance to act.
	scenario.Step(`^the page is not waiting for a second key$`, func() error {
		waiting, err := w.page().WaitingForSecondKey()
		if err != nil {
			return err
		}
		if waiting {
			return fmt.Errorf("the page took a key for a prefix that no one meant as one")
		}
		return nil
	})
	scenario.Step(`^the shortcut card is open$`, func() error {
		return harness.Eventually("the shortcut card", w.page().ShortcutCardOpen)
	})
	scenario.Step(`^the shortcut card is not open$`, func() error {
		open, err := w.page().ShortcutCardOpen()
		if err != nil {
			return err
		}
		if open {
			return fmt.Errorf("a key typed into a field opened the shortcut card")
		}
		return nil
	})
	scenario.Step(`^the box for `+quoted+` has the focus$`, func(tag string) error {
		return harness.Eventually("the box for "+tag+" to have the focus", func() (bool, error) {
			return w.page().TagBoxFocused(tag)
		})
	})
	scenario.Step(`^the search field has the focus$`, func() error {
		return harness.Eventually("the search field to have the focus", w.page().SearchFieldFocused)
	})
	scenario.Step(`^the field named `+quoted+` holds `+quoted+`$`, func(name, held string) error {
		var last string
		err := harness.Eventually(fmt.Sprintf("the %s field to hold %q", name, held), func() (bool, error) {
			value, present, err := w.page().FieldHolds(name)
			last = value
			return present && value == held, err
		})
		if err != nil {
			return fmt.Errorf("%w (it holds %q)", err, last)
		}
		return nil
	})
}
