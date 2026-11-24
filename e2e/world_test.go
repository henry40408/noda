package e2e

import (
	"context"
	"fmt"

	"github.com/cucumber/godog"

	"github.com/henry40408/noda/e2e/internal/harness"
)

// world is the state of one scenario: a browser session over the shared server.
type world struct {
	server  *harness.Server
	scripts bool
	browser *harness.Browser
}

func initializeScenario(scenario *godog.ScenarioContext, server *harness.Server, scripts bool) {
	w := &world{server: server, scripts: scripts}
	scenario.Before(w.start)
	scenario.After(w.stop)
	registerNotebookSteps(scenario, w)
	registerNavigationSteps(scenario, w)
	registerKeyboardSteps(scenario, w)
	registerReadingSteps(scenario, w)
	registerRunSteps(scenario, w)
	registerLayoutSteps(scenario, w)
}

func (w *world) start(ctx context.Context, _ *godog.Scenario) (context.Context, error) {
	browser, err := harness.Open(w.scripts)
	if err != nil {
		return ctx, fmt.Errorf("could not open a browser session: %w", err)
	}
	w.browser = browser
	return ctx, nil
}

// stop runs on every outcome, so a failed scenario leaves no Chrome behind.
// Some scenarios write; restoring the fixture lets later ones, and the second
// pass, see the notes they expect.
func (w *world) stop(ctx context.Context, _ *godog.Scenario, _ error) (context.Context, error) {
	if w.browser != nil {
		w.browser.Close()
		w.browser = nil
	}
	if err := w.server.Reset(); err != nil {
		return ctx, fmt.Errorf("could not put the notebook back: %w", err)
	}
	return ctx, nil
}

func (w *world) page() harness.Page {
	return harness.Page{Browser: w.browser, BaseURL: w.server.BaseURL}
}
