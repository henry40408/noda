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

	scenario.Step(`^my phone prefers a dark theme$`, func() error { return w.browser.PreferScheme("dark") })
	scenario.Step(`^my phone prefers a light theme$`, func() error { return w.browser.PreferScheme("light") })

	scenario.Step(`^I press `+quoted+`$`, func(what string) error { return w.page().Press(what) })
	scenario.Step(`^I press `+quoted+` in the margin note$`, func(what string) error {
		return w.page().PressInMargin(what)
	})
	scenario.Step(`^I press back$`, func() error { return w.page().TapBack() })
	scenario.Step(`^I go back$`, func() error { return w.page().GoBack() })
	scenario.Step(`^I remember this page$`, func() error { return w.page().Remember() })
	scenario.Step(`^it is still the same page$`, func() error {
		return harness.Eventually("the page to still be the one that was marked", w.page().Remembered)
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
	scenario.Step(`^I submit `+quoted+`$`, func(what string) error { return w.page().Submit(what) })
	scenario.Step(`^I untick `+quoted+`$`, func(tag string) error { return w.page().Untick(tag) })
	scenario.Step(`^I press the button to write$`, func() error { return w.page().TapWrite() })
}
