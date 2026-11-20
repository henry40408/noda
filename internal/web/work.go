package web

import (
	"cmp"
	"slices"
	"sync"
	"time"

	"github.com/henry40408/noda/internal/cmd"
	"github.com/henry40408/noda/internal/notebook"
)

// Job is an errand that talks to a remote, so no request waits on it.
type Job int

const (
	JobSync Job = iota
	JobPull
	JobPush
)

// JobOf is the errand a word in a URL names.
func JobOf(word string) (Job, bool) {
	switch word {
	case "sync":
		return JobSync, true
	case "pull":
		return JobPull, true
	case "push":
		return JobPush, true
	}
	return 0, false
}

// Name is the command's name.
func (e Job) Name() string { return [...]string{"sync", "pull", "push"}[e] }

// Doing is for while it is happening.
func (e Job) Doing() string { return [...]string{"Syncing", "Pulling", "Pushing"}[e] }

// Done is for when it went well.
func (e Job) Done() string { return [...]string{"Synced", "Pulled", "Pushed"}[e] }

// Stuck is for when it did not.
func (e Job) Stuck() string { return [...]string{"Sync failed", "Pull failed", "Push failed"}[e] }

func (e Job) run(opened *notebook.Notebook) (string, error) {
	switch e {
	case JobPull:
		return cmd.PullIn(opened)
	case JobPush:
		return cmd.PushIn(opened)
	}
	return cmd.SyncIn(opened)
}

// Outcome is how an errand ended: what it printed, or what went wrong.
type Outcome struct {
	Said   string
	Failed bool
}

// Work runs an errand on a notebook that may have failed to open.
func Work(errand Job, opened *notebook.Notebook, openErr error) Outcome {
	if openErr != nil {
		return Outcome{Said: openErr.Error(), Failed: true}
	}
	said, err := errand.run(opened)
	if err != nil {
		return Outcome{Said: err.Error(), Failed: true}
	}
	return Outcome{Said: said}
}

type doing struct {
	errand  Job
	outcome *Outcome
	started time.Time
	took    time.Duration
}

// Report is what the status screen says about a notebook's last errand.
type Report struct {
	Job     Job
	Outcome *Outcome // nil while it is still going
	Took    time.Duration
}

// Running is whether it has not ended.
func (r Report) Running() bool { return r.Outcome == nil }

// Left is an errand still going.
type Left struct {
	Book string
	Job  Job
}

// Errands is what is being done to which notebook. It outlives a request,
// because an errand does; what it did stays until the next one begins.
type Errands struct {
	mutex     sync.Mutex
	ended     *sync.Cond
	each      map[string]*doing
	abandoned bool
}

// NewErrands is empty.
func NewErrands() *Errands {
	errands := &Errands{each: map[string]*doing{}}
	errands.ended = sync.NewCond(&errands.mutex)
	return errands
}

// Begin records an errand and says whether to run it: false when one is
// already running on the notebook, which is not an error.
func (e *Errands) Begin(book string, errand Job) bool {
	e.mutex.Lock()
	defer e.mutex.Unlock()
	if current, ok := e.each[book]; ok && current.outcome == nil {
		return false
	}
	e.each[book] = &doing{errand: errand, started: time.Now()}
	return true
}

// Finish records how it ended and wakes Settle.
func (e *Errands) Finish(book string, outcome Outcome) {
	e.mutex.Lock()
	if current, ok := e.each[book]; ok {
		current.took = time.Since(current.started)
		current.outcome = &outcome
	}
	e.mutex.Unlock()
	e.ended.Broadcast()
}

// Running is the errands still going, in notebook order.
func (e *Errands) Running() []Left {
	e.mutex.Lock()
	defer e.mutex.Unlock()
	return e.running()
}

func (e *Errands) running() []Left {
	var left []Left
	for book, current := range e.each {
		if current.outcome == nil {
			left = append(left, Left{Book: book, Job: current.errand})
		}
	}
	slices.SortFunc(left, func(a, b Left) int { return cmp.Compare(a.Book, b.Book) })
	return left
}

// Settle waits until nothing is running or Abandon is called, and returns what
// was left behind. A push to a dead host can take minutes, hence the way out.
func (e *Errands) Settle() []Left {
	e.mutex.Lock()
	defer e.mutex.Unlock()
	for !e.abandoned && len(e.running()) > 0 {
		e.ended.Wait()
	}
	return e.running()
}

// Abandon makes Settle return now.
func (e *Errands) Abandon() {
	e.mutex.Lock()
	e.abandoned = true
	e.mutex.Unlock()
	e.ended.Broadcast()
}

// Report is the notebook's latest errand, if there was one.
func (e *Errands) Report(book string) (Report, bool) {
	e.mutex.Lock()
	defer e.mutex.Unlock()
	current, ok := e.each[book]
	if !ok {
		return Report{}, false
	}
	took := current.took
	if current.outcome == nil {
		took = time.Since(current.started)
	}
	return Report{Job: current.errand, Outcome: current.outcome, Took: took}, true
}
