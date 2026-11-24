package e2e

import (
	"fmt"
	"math"
	"regexp"
	"strconv"
	"strings"

	"github.com/cucumber/godog"

	"github.com/henry40408/noda/e2e/internal/harness"
)

// Mean-channel brightness thresholds, rather than exact colours that would
// copy the theme.
const (
	lightEnough = 200.0
	darkEnough  = 60.0
)

var idPattern = regexp.MustCompile(`^[0-9A-Za-z]{8}$`)

func registerLayoutSteps(scenario *godog.ScenarioContext, w *world) {
	scenario.Step(`^no control is smaller than (\d+) by (\d+)$`, func(wide, tall int) error {
		short, err := w.page().ControlsSmallerThan(wide, tall)
		if err != nil {
			return err
		}
		if len(short) > 0 {
			return fmt.Errorf("a thumb cannot reach these: %s", strings.Join(short, "; "))
		}
		return nil
	})
	scenario.Step(`^the tag `+quoted+` is no taller than (\d+) pixels$`, func(name string, most int) error {
		var last float64
		err := harness.Eventually(fmt.Sprintf("the tag %q to be drawn", name), func() (bool, error) {
			height, present, err := w.page().TagChipHeight(name)
			last = height
			return present, err
		})
		if err != nil {
			return err
		}
		if last > float64(most) {
			return fmt.Errorf("the tag %q is %gpx tall: a pointer needs no more than %dpx", name, last, most)
		}
		return nil
	})
	scenario.Step(`^the order line has at least (\d+) pixels to spare above and below$`, func(least int) error {
		var above, below float64
		err := harness.Eventually("the order line to be drawn", func() (bool, error) {
			var present bool
			var err error
			above, below, present, err = w.page().OrderLineRoom()
			return present, err
		})
		if err != nil {
			return err
		}
		// Layout lands on fractions of a pixel; a whole one is a real change.
		if above < float64(least)-0.5 || below < float64(least)-0.5 {
			return fmt.Errorf("the order line has %gpx above, to the search field, and %gpx below, to the list: it needs %dpx of both",
				above, below, least)
		}
		return nil
	})
	scenario.Step(`^the search field's text is at least (\d+) pixels$`, func(least int) error {
		size, err := w.page().SearchFieldFontSize()
		if err != nil {
			return err
		}
		if size < float64(least) {
			return fmt.Errorf("the search field is set at %gpx: below %dpx, iOS Safari zooms the page on focus", size, least)
		}
		return nil
	})
	// Below 16px, iOS Safari zooms the page when a field takes focus.
	scenario.Step(`^no field is smaller than (\d+) pixels$`, func(least int) error {
		small, err := w.page().FieldsUnder(float64(least))
		if err != nil {
			return err
		}
		if len(small) > 0 {
			return fmt.Errorf("a phone will zoom in on these: %s", strings.Join(small, "; "))
		}
		return nil
	})
	scenario.Step(`^the page does not scroll sideways$`, func() error {
		over, err := w.page().ScrollsSideways()
		if err != nil {
			return err
		}
		if over {
			return fmt.Errorf("the page is wider than the phone it is on")
		}
		return nil
	})

	// The row is two lines at every width: the day and the tags start where
	// the title does.
	scenario.Step(`^the row's tags sit under the title$`, func() error {
		title, under, err := w.titleAndTags()
		if err != nil {
			return err
		}
		if math.Abs(under-title) >= 1 {
			return fmt.Errorf("the tags start at %g and the title at %g: they are not stacked", under, title)
		}
		return nil
	})
	// Wrapped, the fourth order sits alone and reads as something different.
	scenario.Step(`^the order chips are on one line$`, func() error {
		lines, err := w.page().LinesOf(".sortbar")
		if err != nil {
			return err
		}
		if lines != 1 {
			return fmt.Errorf("the order is on %d lines at this width", lines)
		}
		return nil
	})
	scenario.Step(`^the row shows the note's id$`, func() error {
		id, present, err := w.page().ShownID()
		if err != nil {
			return err
		}
		if !present {
			return fmt.Errorf("no id is drawn on the row at this width")
		}
		// Ids are minted: any 8 characters of Crockford base32 will do.
		if !idPattern.MatchString(id) {
			return fmt.Errorf("the id column says %q, which is not an id", id)
		}
		return nil
	})

	// The front page has no rail, but the rail's grid column once left 76px of
	// nothing down the left, invisible in the markup. The pane, not the list:
	// the list is a column of its own width in the middle of it.
	scenario.Step(`^the notebooks fill the window$`, func() error {
		box, err := w.page().BoxOf(".pane")
		if err != nil {
			return err
		}
		if box.Left > 1 || math.Abs(box.Container-box.Width) > 1 {
			return fmt.Errorf("the notebooks start %g in and are %g wide in a %g window",
				box.Left, box.Width, box.Container)
		}
		return nil
	})
	// Against the pane, not the window, which also holds a rail and an index.
	scenario.Step(`^the reading column is narrower than its pane$`, func() error {
		box, err := w.page().BoxIn(".pane.read article", ".pane.read")
		if err != nil {
			return err
		}
		if box.Width >= box.Container {
			return fmt.Errorf("the column is %g wide in a %g pane: a line that long is not read, it is scanned",
				box.Width, box.Container)
		}
		return nil
	})
	scenario.Step(`^the reading column is centred in its pane$`, func() error {
		box, err := w.page().BoxIn(".pane.read article", ".pane.read")
		if err != nil {
			return err
		}
		return requireCentred(box, "column")
	})
	// A form page's strip spans the pane; placed inside the form, it got the
	// form's padding twice and stood 16px right of its buttons. Measured on the
	// first bold run, since the paragraph itself is full-bleed.
	scenario.Step(`^the words line up with the buttons under them$`, func() error {
		words, err := w.page().BoxIn(".said b", "main")
		if err != nil {
			return err
		}
		buttons, err := w.page().BoxIn(".buttons > :first-child", "main")
		if err != nil {
			return err
		}
		if math.Abs(words.Left-buttons.Left) > 1 {
			return fmt.Errorf("the words start %g into the pane and the buttons %g", words.Left, buttons.Left)
		}
		return nil
	})

	scenario.Step(`^the page is dark$`, func() error {
		colour, brightness, err := w.background()
		if err != nil {
			return err
		}
		if brightness >= darkEnough {
			return fmt.Errorf("the background is %s: that is not the dark palette", colour)
		}
		return nil
	})
	scenario.Step(`^the page is light$`, func() error {
		colour, brightness, err := w.background()
		if err != nil {
			return err
		}
		if brightness <= lightEnough {
			return fmt.Errorf("the background is %s: that is not the light palette", colour)
		}
		return nil
	})
}

func (w *world) titleAndTags() (title, under float64, err error) {
	titleBox, err := w.page().BoxOf(".row .title")
	if err != nil {
		return 0, 0, err
	}
	underBox, err := w.page().BoxOf(".row .under")
	if err != nil {
		return 0, 0, err
	}
	return titleBox.Left, underBox.Left, nil
}

func requireCentred(box harness.Box, what string) error {
	right := box.Container - box.Left - box.Width
	if math.Abs(box.Left-right) > 2 {
		return fmt.Errorf("there is %g to the left of the %s and %g to the right of it", box.Left, what, right)
	}
	return nil
}

// background is the body's colour and its mean-channel brightness;
// getComputedStyle always answers in rgb(...) or rgba(...).
func (w *world) background() (string, float64, error) {
	colour, err := w.page().Background()
	if err != nil {
		return "", 0, err
	}
	channels := strings.FieldsFunc(strings.Trim(colour[strings.Index(colour, "(")+1:], ") "),
		func(r rune) bool { return r == ',' || r == ' ' || r == '/' })
	if len(channels) < 3 {
		return colour, 0, fmt.Errorf("could not read a colour out of %q", colour)
	}
	sum := 0.0
	for _, channel := range channels[:3] {
		value, err := strconv.ParseFloat(strings.TrimSpace(channel), 64)
		if err != nil {
			return colour, 0, fmt.Errorf("could not read a colour out of %q", colour)
		}
		sum += value
	}
	return colour, sum / 3, nil
}
