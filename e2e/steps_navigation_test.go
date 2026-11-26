package e2e

import (
	"fmt"
	"strings"

	"github.com/cucumber/godog"

	"github.com/henry40408/noda/e2e/internal/harness"
)

// Steps speak of pressing and reading, never of selectors, which live in the
// page object so a stylesheet change does not reach the features.

const quoted = `"([^"]*)"`

func registerNavigationSteps(scenario *godog.ScenarioContext, w *world) {
	notebook := "/nb/" + harness.Notebook
	openAt := func(viewport *harness.Viewport, path string) error {
		if viewport != nil {
			if err := w.browser.SetViewport(*viewport); err != nil {
				return err
			}
		}
		return w.page().Go(path)
	}
	opening := func(pattern, path string, viewport *harness.Viewport) {
		scenario.Step(pattern, func() error { return openAt(viewport, path) })
	}
	opening(`^I open the front page$`, "/", nil)
	opening(`^I open the front page on a tablet$`, "/", &harness.Tablet)
	opening(`^I open the notebook$`, notebook, nil)
	opening(`^I open the notebook on a phone$`, notebook, &harness.Phone)
	opening(`^I open the notebook on a tablet$`, notebook, &harness.Tablet)
	opening(`^I open the notebook on a desktop$`, notebook, &harness.Desktop)
	opening(`^I open the notebook on a monitor$`, notebook, &harness.Monitor)
	scenario.Step(`^I open the notebook on a (\d+) pixel phone$`, func(width int) error {
		viewport := harness.Phone
		if int64(width) <= harness.Narrow.Width {
			viewport = harness.Narrow
		}
		return openAt(&viewport, notebook)
	})
	scenario.Step(`^I open `+quoted+`$`, func(path string) error { return w.page().Go(path) })
	scenario.Step(`^I open `+quoted+` on a desktop$`, func(path string) error { return openAt(&harness.Desktop, path) })
	scenario.Step(`^my screen is a touch screen$`, func() error { return w.browser.EmulateTouch() })
	scenario.Step(`^I open the notebook on a touch screen (\d+) pixels wide$`, func(width int) error {
		viewport := harness.Desktop
		viewport.Width = int64(width)
		return openAt(&viewport, notebook)
	})

	scenario.Step(`^my phone prefers a dark theme$`, func() error { return w.browser.PreferScheme("dark") })
	scenario.Step(`^my phone prefers a light theme$`, func() error { return w.browser.PreferScheme("light") })

	scenario.Step(`^I press `+quoted+`$`, func(what string) error { return w.page().Press(what) })
	scenario.Step(`^I press `+quoted+` in the margin note$`, func(what string) error {
		return w.page().PressInMargin(what)
	})
	scenario.Step(`^I press the order `+quoted+`$`, func(name string) error { return w.page().PressOrder(name) })
	scenario.Step(`^I press back$`, func() error { return w.page().TapBack() })
	scenario.Step(`^I go back$`, func() error { return w.page().GoBack() })
	scenario.Step(`^I remember this page$`, func() error { return w.page().Remember() })
	scenario.Step(`^it is still the same page$`, func() error {
		return harness.Eventually("the page to still be the one that was marked", w.page().Remembered)
	})
	scenario.Step(`^I remember the note on screen$`, func() error { return w.page().RememberNote() })
	scenario.Step(`^it is still the note I remembered$`, func() error {
		return harness.Eventually("the note on screen to still be the one that was marked", w.page().NoteRemembered)
	})
	scenario.Step(`^I press the footnote `+quoted+`$`, func(number string) error { return w.page().PressFootnote(number) })
	scenario.Step(`^I press the way back from the footnote `+quoted+`$`, func(number string) error {
		return w.page().PressFootnoteBack(number)
	})
	scenario.Step(`^the footnote `+quoted+` is the one lit$`, func(number string) error {
		return harness.Eventually("the footnote "+number+" to be the target", func() (bool, error) {
			return w.page().FootnoteLit(number, false)
		})
	})
	scenario.Step(`^the citation of the footnote `+quoted+` is the one lit$`, func(number string) error {
		return harness.Eventually("the citation of footnote "+number+" to be the target", func() (bool, error) {
			return w.page().FootnoteLit(number, true)
		})
	})
	scenario.Step(`^I press the summary of the section$`, func() error { return w.page().PressSummary() })
	scenario.Step(`^the section is unfolded$`, func() error {
		return harness.Eventually("the section to be open", w.page().SectionUnfolded)
	})
	scenario.Step(`^I search for `+quoted+`$`, func(query string) error { return w.page().Search(query) })
	scenario.Step(`^I type `+quoted+` into the search field$`, func(query string) error {
		return w.page().TypeSearch(query)
	})

	scenario.Step(`^I am at `+quoted+`$`, func(path string) error {
		return harness.Eventually(fmt.Sprintf("the address to become %q", path), func() (bool, error) {
			current, err := w.page().Path()
			return current == path, err
		})
	})
	scenario.Step(`^I am not at `+quoted+`$`, func(path string) error {
		return harness.Eventually(fmt.Sprintf("the address to leave %q", path), func() (bool, error) {
			current, err := w.page().Path()
			return current != path, err
		})
	})
	scenario.Step(`^the address carries the search `+quoted+`$`, func(query string) error {
		return harness.Eventually(fmt.Sprintf("the address to carry %q", query), func() (bool, error) {
			sent, present, err := w.page().Searched()
			return present && sent == query, err
		})
	})
	scenario.Step(`^the address carries no search$`, func() error {
		sent, present, err := w.page().Searched()
		if err != nil {
			return err
		}
		if present {
			return fmt.Errorf("the query was sent after all: %q", sent)
		}
		return nil
	})

	scenario.Step(`^I write `+quoted+` as the title$`, func(text string) error {
		return w.page().Fill("title", text)
	})
	// `\n` in the feature is a newline: Gherkin cannot write one inside a string.
	scenario.Step(`^I write `+quoted+` as the body$`, func(text string) error {
		return w.page().Fill("body", strings.ReplaceAll(text, `\n`, "\n"))
	})
	// Over the 2 MiB the server reads of a form.
	scenario.Step(`^I paste a body longer than the server takes$`, func() error {
		return w.page().FillWithBytes("body", 2<<20+1024)
	})
	scenario.Step(`^I submit `+quoted+`$`, func(what string) error { return w.page().Submit(what) })
	scenario.Step(`^I untick `+quoted+`$`, func(tag string) error { return w.page().Untick(tag) })
	scenario.Step(`^I press the button to write$`, func() error { return w.page().TapWrite() })
}
