package e2e

import (
	"fmt"
	"slices"
	"strings"

	"github.com/cucumber/godog"

	"github.com/henry40408/noda/e2e/internal/harness"
)

// textHolds waits for a page text, read by read, to satisfy holds.
func textHolds(what string, read func() (string, error), holds func(string) bool) error {
	return harness.Eventually(what, func() (bool, error) {
		text, err := read()
		return holds(text), err
	})
}

func containing(want string) func(string) bool {
	return func(text string) bool { return strings.Contains(text, want) }
}

func registerReadingSteps(scenario *godog.ScenarioContext, w *world) {
	anyRowContains := func(want string) func() (bool, error) {
		return func() (bool, error) {
			rows, err := w.page().Rows()
			return slices.ContainsFunc(rows, containing(want)), err
		}
	}
	scenario.Step(`^I see a row for `+quoted+`$`, func(what string) error {
		return harness.Eventually(fmt.Sprintf("a row for %q", what), anyRowContains(what))
	})
	scenario.Step(`^I do not see a row for `+quoted+`$`, func(what string) error {
		return harness.Eventually(fmt.Sprintf("no row for %q", what), func() (bool, error) {
			found, err := anyRowContains(what)()
			return !found, err
		})
	})
	// On the page but not on the screen: "I do not see a row for" alone would
	// also pass if the listing had left the note out.
	scenario.Step(`^the page holds a hidden row for `+quoted+`$`, func(what string) error {
		hidden, err := w.page().HiddenRows()
		if err != nil {
			return err
		}
		if !slices.ContainsFunc(hidden, containing(what)) {
			return fmt.Errorf("no hidden row for %q; the page holds %q", what, hidden)
		}
		return nil
	})
	scenario.Step(`^the first row is `+quoted+`$`, func(what string) error {
		return harness.Eventually(fmt.Sprintf("%q to be the first row", what), func() (bool, error) {
			rows, err := w.page().Rows()
			return len(rows) > 0 && strings.Contains(rows[0], what), err
		})
	})

	remarkSays := func(words string) error {
		return harness.Eventually("the remark under the field", func() (bool, error) {
			hint, present, err := w.page().Hint()
			return present && strings.Contains(hint, words), err
		})
	}
	scenario.Step(`^the listing says it filtered by title and tag$`, func() error { return remarkSays("title and tag") })
	scenario.Step(`^the listing says it has not filtered yet$`, func() error { return remarkSays("Not filtered yet") })
	scenario.Step(`^the listing says nothing about whose answer it is$`, func() error {
		hint, present, err := w.page().Hint()
		if err != nil {
			return err
		}
		if present {
			return fmt.Errorf("the listing called a whole answer partial: %q", hint)
		}
		return nil
	})
	scenario.Step(`^the page is not reloading itself$`, func() error {
		reloads, err := w.page().ReloadsItself()
		if err != nil {
			return err
		}
		if reloads {
			return fmt.Errorf("the script left the meta refresh in place, so the page is doing both")
		}
		return nil
	})
	scenario.Step(`^the field groups it as `+quoted+`$`, func(expected string) error {
		return harness.Eventually(fmt.Sprintf("the field to group it as %q", expected), func() (bool, error) {
			grouping, present, err := w.page().Grouping()
			return present && grouping == expected, err
		})
	})
	scenario.Step(`^the field groups nothing$`, func() error {
		return harness.Eventually("the field to group nothing", func() (bool, error) {
			_, present, err := w.page().Grouping()
			return !present, err
		})
	})

	scenario.Step(`^the note is headed `+quoted+`$`, func(title string) error {
		return textHolds(fmt.Sprintf("a heading of %q", title), w.page().Heading,
			func(text string) bool { return text == title })
	})
	scenario.Step(`^the tab is named `+quoted+`$`, func(name string) error {
		return textHolds(fmt.Sprintf("a tab named %q", name), w.page().Tab,
			func(text string) bool { return text == name })
	})
	scenario.Step(`^the filename ends with `+quoted+`$`, func(ending string) error {
		return textHolds(fmt.Sprintf("a filename ending %q", ending), w.page().Filename,
			func(text string) bool { return strings.HasSuffix(text, ending) })
	})
	scenario.Step(`^the body says `+quoted+`$`, func(text string) error {
		return textHolds(fmt.Sprintf("a body saying %q", text), w.page().Body, containing(text))
	})
	scenario.Step(`^the page says `+quoted+`$`, func(text string) error {
		return textHolds(fmt.Sprintf("the page to say %q", text), w.page().Text, containing(text))
	})
	scenario.Step(`^the page does not say `+quoted+`$`, func(text string) error {
		return textHolds(fmt.Sprintf("the page to stop saying %q", text), w.page().Text,
			func(page string) bool { return !strings.Contains(page, text) })
	})
	scenario.Step(`^the page complains$`, func() error {
		return harness.Eventually("a complaint", func() (bool, error) {
			_, present, err := w.page().HasProblem()
			return present, err
		})
	})
	scenario.Step(`^the page says nothing is wrong$`, func() error {
		problem, present, err := w.page().HasProblem()
		if err != nil {
			return err
		}
		if present {
			return fmt.Errorf("the page complained without being asked anything: %q", problem)
		}
		return nil
	})
	scenario.Step(`^the note says when it was made and when it changed$`, func() error {
		said, err := w.page().About()
		if err != nil {
			return err
		}
		for _, stamp := range []string{"Created", "Updated"} {
			if !strings.Contains(said, stamp) {
				return fmt.Errorf("the note does not say %s: %s", stamp, said)
			}
		}
		return nil
	})
	// The server cannot know the reader's zone, so it sends the file's
	// `2026-08-15T09:54:23Z` and the script restates it locally. Asserted by
	// shape, since the answer depends on the machine's zone: a converted stamp
	// has a comma and no `Z`.
	scenario.Step(`^the stamps are said in the reader's own words$`, func() error {
		said, err := w.page().Stamps()
		if err != nil {
			return err
		}
		if strings.Contains(said, "Z") {
			return fmt.Errorf("the stamps are still the file's own: %s", said)
		}
		if !strings.Contains(said, ", ") {
			return fmt.Errorf("the stamps were not said again in words: %s", said)
		}
		return nil
	})

	scenario.Step(`^the bar marks `+quoted+`$`, func(place string) error {
		return harness.Eventually(fmt.Sprintf("the bar to mark %q", place), func() (bool, error) {
			marked, present, err := w.page().MarkedPlace()
			return present && marked == place, err
		})
	})
	scenario.Step(`^the bar does not mark `+quoted+`$`, func(place string) error {
		return harness.Eventually(fmt.Sprintf("the bar to leave %q unmarked", place), func() (bool, error) {
			marked, present, err := w.page().MarkedPlace()
			return !present || marked != place, err
		})
	})
	// The network screen is reached by the corner chip, not the bar.
	scenario.Step(`^the bar marks nothing$`, func() error {
		return harness.Eventually("the bar to mark nothing", func() (bool, error) {
			_, present, err := w.page().MarkedPlace()
			return !present, err
		})
	})
	scenario.Step(`^`+quoted+` is marked overdue$`, func(text string) error {
		return harness.Eventually(fmt.Sprintf("%q to be marked overdue", text), func() (bool, error) {
			return w.page().IsOverdue(text)
		})
	})

	// A done item is drawn struck and dimmed; whether a nested item inherits it is
	// the point of the checkbox scenarios, so each reads the text itself.
	scenario.Step(`^`+quoted+` is struck through$`, func(text string) error {
		return harness.Eventually(fmt.Sprintf("%q to be struck through", text), func() (bool, error) {
			struck, err := w.page().IsStruck(text)
			return struck != nil && *struck, err
		})
	})
	scenario.Step(`^`+quoted+` is not struck through$`, func(text string) error {
		return harness.Eventually(fmt.Sprintf("%q to be left unstruck", text), func() (bool, error) {
			struck, err := w.page().IsStruck(text)
			return struck != nil && !*struck, err
		})
	})
	scenario.Step(`^`+quoted+` is drawn in the colour of `+quoted+`$`, func(text, other string) error {
		return sameColour(w, text, other, true)
	})
	scenario.Step(`^`+quoted+` is not drawn in the colour of `+quoted+`$`, func(text, other string) error {
		return sameColour(w, text, other, false)
	})

	scenario.Step(`^the listing is still on screen$`, func() error {
		return harness.Eventually("the listing to stay on screen", w.page().ListingOnScreen)
	})
	scenario.Step(`^the listing is not on screen$`, func() error {
		on, err := w.page().ListingOnScreen()
		if err != nil {
			return err
		}
		if on {
			return fmt.Errorf("the listing is on screen beside the note")
		}
		return nil
	})
	scenario.Step(`^the listing marks `+quoted+`$`, func(what string) error {
		return harness.Eventually(fmt.Sprintf("the listing to mark %q", what), func() (bool, error) {
			marked, present, err := w.page().MarkedRow()
			return present && marked == what, err
		})
	})
	scenario.Step(`^the margin note lists `+quoted+`$`, func(what string) error {
		return harness.Eventually(fmt.Sprintf("the margin note to list %q", what), func() (bool, error) {
			lines, present, err := w.page().MarginNote()
			return present && slices.Contains(lines, what), err
		})
	})
	scenario.Step(`^the margin note does not list `+quoted+`$`, func(what string) error {
		// Polled for the line that must be there, so a margin still on its way
		// is not mistaken for one that leaves the title out.
		return harness.Eventually(fmt.Sprintf("the margin note to leave out %q", what), func() (bool, error) {
			lines, present, err := w.page().MarginNote()
			return present && len(lines) > 0 && !slices.Contains(lines, what), err
		})
	})
	// "No other note links here" is said, not shown as a closed column.
	scenario.Step(`^the margin note says `+quoted+`$`, func(what string) error {
		return harness.Eventually(fmt.Sprintf("the margin note to say %q", what), func() (bool, error) {
			lines, present, err := w.page().MarginNote()
			return present && slices.Equal(lines, []string{what}), err
		})
	})
	scenario.Step(`^I rest the pointer on the tag `+quoted+`$`, func(name string) error {
		return w.page().RestOnTag(name)
	})
	scenario.Step(`^the tag `+quoted+` can still be read$`, func(name string) error {
		return harness.Eventually(fmt.Sprintf("the tag %q to be drawn over its highlight", name), func() (bool, error) {
			return w.page().TagIsReadable(name)
		})
	})
	// Not drawn at all, not merely empty.
	scenario.Step(`^the margin note is not on screen$`, func() error {
		_, present, err := w.page().MarginNote()
		if err != nil {
			return err
		}
		if present {
			return fmt.Errorf("the margin note is on screen beside the note")
		}
		return nil
	})
}

func sameColour(w *world, text, other string, want bool) error {
	colour, found, err := w.page().ColourOf(text)
	if err != nil {
		return err
	}
	against, foundOther, err := w.page().ColourOf(other)
	if err != nil {
		return err
	}
	if !found || !foundOther {
		return fmt.Errorf("the body does not hold both %q and %q", text, other)
	}
	if (colour == against) != want {
		return fmt.Errorf("%q is %s and %q is %s", text, colour, other, against)
	}
	return nil
}
