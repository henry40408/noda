package e2e

import (
	"fmt"

	"github.com/chromedp/chromedp/kb"
	"github.com/cucumber/godog"

	"github.com/henry40408/noda/e2e/internal/harness"
)

// The keys are real keystrokes, sent to whatever has the focus: a step that
// dispatched its own event would pass against a listener no reader's key reaches.

func registerKeyboardSteps(scenario *godog.ScenarioContext, w *world) {
	scenario.Step(`^I hit the keys `+quoted+`$`, func(keys string) error { return w.page().HitKeys(keys) })
	scenario.Step(`^I hit the Escape key$`, func() error { return w.page().HitKeys(kb.Escape) })
	// What Safari reports for the key that ends a composition.
	scenario.Step(`^I hit the key `+quoted+` while an input method is composing$`, func(letter string) error {
		return w.page().HitKeyUnderInputMethod(letter)
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
