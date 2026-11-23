package e2e

import (
	"github.com/cucumber/godog"

	"github.com/henry40408/noda/e2e/internal/harness"
)

func registerNotebookSteps(scenario *godog.ScenarioContext, w *world) {
	// A note one scenario needs and the fixture would only put in the way of the
	// rest. The server reads the notebook afresh on every request, and the reset
	// after the scenario takes the note away again.
	scenario.Step(`^the notebook holds a note `+quoted+` saying:$`, func(title string, body *godog.DocString) error {
		_, err := w.server.Run("add", title, "-c", body.Content)
		return err
	})
	// What Chrome does with all cookies blocked: reading the storage object
	// itself throws, as well as using it.
	scenario.Step(`^the browser refuses to store anything$`, func() error {
		return w.browser.BeforeEveryPage(`for (const name of ['localStorage', 'sessionStorage']) {
			Object.defineProperty(window, name, {get() { throw new DOMException('The operation is insecure.', 'SecurityError'); }});
		}`)
	})
	// A screen left open while the notebook was removed: the press then goes to
	// an address the server answers with a refusal, which no fixture produces.
	scenario.Step(`^the errand buttons lead to a notebook that is gone$`, func() error {
		_, err := harness.Eval[int](w.browser, `(function() {
			let moved = 0;
			for (const form of document.querySelectorAll('form[action*="/status/"]')) {
				form.setAttribute('action', form.getAttribute('action').replace('/nb/default/', '/nb/gone/'));
				moved += 1;
			}
			return moved;
		})()`)
		return err
	})
	// What another machine's sync does to a note this browser has open.
	scenario.Step(`^the note `+quoted+` is deleted from another machine$`, func(key string) error {
		_, err := w.server.Run("rm", key)
		return err
	})
}
