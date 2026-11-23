package e2e

import (
	"fmt"
	"regexp"
	"slices"

	"github.com/cucumber/godog"

	"github.com/henry40408/noda/e2e/internal/harness"
)

// A month is named by what the server's zone says, which the feature file
// cannot know, so a heading is recognised by its shape.
var monthHeading = regexp.MustCompile(`^[A-Z][a-z]+ [0-9]{4}$`)

func registerRunSteps(scenario *godog.ScenarioContext, w *world) {
	scenario.Step(`^the note `+quoted+` is pinned$`, func(key string) error {
		_, err := w.server.Run("pin", key)
		return err
	})

	scenario.Step(`^the listing has the heading `+quoted+`$`, func(heading string) error {
		return harness.Eventually(fmt.Sprintf("a run headed %q", heading), func() (bool, error) {
			headings, err := w.page().RunHeadings()
			return slices.Contains(headings, heading), err
		})
	})
	scenario.Step(`^the listing does not have the heading `+quoted+`$`, func(heading string) error {
		// Polled: without scripts the search is a navigation, and the page it
		// leaves still has the heading.
		return harness.Eventually(fmt.Sprintf("the run headed %q to go", heading), func() (bool, error) {
			headings, err := w.page().RunHeadings()
			return !slices.Contains(headings, heading), err
		})
	})
	scenario.Step(`^the listing is headed by a month and a year$`, func() error {
		return harness.Eventually("a run headed by a month", func() (bool, error) {
			headings, err := w.page().RunHeadings()
			return slices.ContainsFunc(headings, monthHeading.MatchString), err
		})
	})
	scenario.Step(`^the listing is not cut by month$`, func() error {
		headings, err := w.page().RunHeadings()
		if err != nil {
			return err
		}
		if slices.ContainsFunc(headings, monthHeading.MatchString) {
			return fmt.Errorf("the rows are cut by month under an order that is not a date: %q", headings)
		}
		return nil
	})
}
