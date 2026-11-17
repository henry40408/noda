package harness

import (
	"fmt"
	"time"
)

// WaitTimeout is how long a query waits for a condition before giving up.
// Only a genuine failure pays it in full.
const WaitTimeout = 30 * time.Second

// WaitInterval is how often a query re-checks while waiting.
const WaitInterval = 100 * time.Millisecond

// Eventually polls probe until it reports true. An error from probe means
// "not yet", never a failure: during a navigation the page evaluates nothing,
// and the next poll succeeds. The last error is named when the wait times out.
func Eventually(what string, probe func() (bool, error)) error {
	deadline := time.Now().Add(WaitTimeout)
	for {
		done, err := probe()
		if done && err == nil {
			return nil
		}
		if time.Now().After(deadline) {
			if err != nil {
				return fmt.Errorf("%s: still not true after %v, last error: %w", what, WaitTimeout, err)
			}
			return fmt.Errorf("%s: still not true after %v", what, WaitTimeout)
		}
		time.Sleep(WaitInterval)
	}
}
