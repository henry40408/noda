package e2e

import (
	"fmt"

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
	// A slow server: the first answer a row press or an order press asks for
	// waits until the scenario lets it through, so the next press can overtake it.
	scenario.Step(`^the browser can hold back an answer$`, func() error {
		return w.browser.BeforeEveryPage(`const fetched = window.fetch;
			window.heldBack = [];
			window.holdNext = false;
			window.fetch = (url, init) => {
				const kind = init && init.headers && init.headers['x-noda-fragment'];
				if (!window.holdNext || (kind !== 'read' && kind !== 'index')) return fetched(url, init);
				window.holdNext = false;
				return new Promise((resolve, reject) => {
					window.heldBack.push(() => fetched(url, init).then(resolve, reject));
				});
			};`)
	})
	scenario.Step(`^the next answer to a press is held back$`, func() error {
		_, err := harness.Eval[bool](w.browser, `window.holdNext = true`)
		return err
	})
	scenario.Step(`^the held answer arrives$`, func() error {
		released, err := harness.Eval[int](w.browser, `(function() {
			const sends = window.heldBack.splice(0);
			sends.forEach((send) => send());
			return sends.length;
		})()`)
		if err == nil && released == 0 {
			return fmt.Errorf("no answer was held back, so nothing was overtaken")
		}
		return err
	})
	scenario.Step(`^no pane is waiting for an answer$`, func() error {
		return harness.Eventually("no pane to wait for an answer", w.page().NothingIsWaiting)
	})
	// What each write was answered with, kept in sessionStorage because a save
	// ends in a navigation that takes the page's own variables with it.
	scenario.Step(`^the browser notes how each write is answered$`, func() error {
		return w.browser.BeforeEveryPage(`const fetched = window.fetch;
			window.fetch = async (url, init) => {
				const answer = await fetched(url, init);
				if (init && init.method === 'POST') {
					const seen = JSON.parse(sessionStorage.getItem('answered') || '[]');
					seen.push({status: answer.status, redirected: answer.redirected});
					sessionStorage.setItem('answered', JSON.stringify(seen));
				}
				return answer;
			};`)
	})
	scenario.Step(`^no write was answered with the page it led to$`, func() error {
		seen, err := harness.Eval[string](w.browser, `sessionStorage.getItem('answered') || '[]'`)
		if err != nil {
			return err
		}
		if seen != `[{"status":204,"redirected":false}]` {
			return fmt.Errorf("the save was answered with %s, not with where it went", seen)
		}
		return nil
	})
	// A proxy that drops a response header it does not know: the server's 204 then
	// arrives with nothing in it to say where the write went.
	scenario.Step(`^a proxy strips the address from every answer to a write$`, func() error {
		return w.browser.BeforeEveryPage(`const fetched = window.fetch;
			window.fetch = async (url, init) => {
				const answer = await fetched(url, init);
				if (!init || init.method !== 'POST') return answer;
				const headers = new Headers(answer.headers);
				headers.delete('x-noda-location');
				// A 204 may not be given a body, even an empty one.
				const body = answer.status === 204 ? null : answer.body;
				return new Response(body, {status: answer.status, headers});
			};`)
	})
	scenario.Step(`^another machine changes the line `+quoted+` of `+quoted+` to `+quoted+`$`, func(from, key, to string) error {
		return w.server.ChangeLine(key, from, to)
	})
	scenario.Step(`^the notice is not under the bar$`, func() error {
		below, err := w.page().ClearOfTheBar(".said")
		if err == nil && !below {
			err = fmt.Errorf("the notice starts under the bar, so its first line cannot be read")
		}
		return err
	})
	scenario.Step(`^the notebook is also kept under the name `+quoted+`$`, func(name string) error {
		return w.server.CopyNotebook(name)
	})
	// What another machine's sync does to a note this browser has open.
	scenario.Step(`^the note `+quoted+` is deleted from another machine$`, func(key string) error {
		_, err := w.server.Run("rm", key)
		return err
	})
}
