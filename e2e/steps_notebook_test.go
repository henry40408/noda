package e2e

import "github.com/cucumber/godog"

func registerNotebookSteps(scenario *godog.ScenarioContext, w *world) {
	// A note one scenario needs and the fixture would only put in the way of the
	// rest. The server reads the notebook afresh on every request, and the reset
	// after the scenario takes the note away again.
	scenario.Step(`^the notebook holds a note `+quoted+` saying:$`, func(title string, body *godog.DocString) error {
		_, err := w.server.Run("add", title, "-c", body.Content)
		return err
	})
}
