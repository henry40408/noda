package cmd

import (
	"errors"
	"fmt"
	"slices"
	"strings"

	"github.com/henry40408/noda/internal/notebook"
)

// Change is what a bulk step does. The changes that make sense over a set of
// notes: a retitle needs one title per note.
type Change struct {
	// Remove deletes the notes; otherwise Tags are applied.
	Remove bool
	Tags   []string
	Touch  Touch
}

// Step is one change and the notes it is aimed at, by id.
type Step struct {
	Keys   []string
	Change Change
}

// Describe is the line for both the commit message and the browser's queue,
// so what you read before sending is what the history says.
func (s Step) Describe() string {
	notes := count(len(s.Keys), "note")
	if s.Change.Remove {
		return "rm: " + notes
	}
	kept := ""
	if s.Change.Touch == TouchKeep {
		kept = ", keeping updated"
	}
	return fmt.Sprintf("tag: %s (%s%s)", strings.Join(s.Change.Tags, " "), notes, kept)
}

// count is `1 note` / `3 notes`.
func count(n int, thing string) string {
	if n == 1 {
		return "1 " + thing
	}
	return fmt.Sprintf("%d %ss", n, thing)
}

// Check is whether a change could be carried out at all, without opening
// anything, so the browser refuses a bad tag when it is queued, not when the
// queue is sent.
func Check(change Change) error {
	if change.Remove {
		return nil
	}
	_, err := parseTags(change.Tags, "")
	return err
}

// planned is a step's tags parsed once rather than once per note.
type planned struct {
	edits  []tagEdit
	remove bool
	touch  Touch
}

// Bulk applies several changes across several notes in one commit, because a
// queue is one intention. It writes through the same code as `tag` and `rm`.
//
// What can be refused is refused before anything is written; what cannot be
// known in advance (a note deleted mid-queue) is reported without stopping the
// rest.
func Bulk(e Env, steps []Step) (string, error) {
	if len(steps) == 0 {
		return "", errors.New("there is nothing to send")
	}
	plan, err := planSteps(steps)
	if err != nil {
		return "", err
	}
	active, err := e.openActive()
	if err != nil {
		return "", err
	}

	var files, problems []string
	touched := map[string]struct{}{}
	for i, step := range steps {
		for _, key := range step.Keys {
			done, err := plan[i].apply(active, key)
			switch {
			case err != nil:
				problems = append(problems, fmt.Sprintf("%s: %v", key, err))
			case done.changed:
				files = append(files, done.files...)
				touched[done.id] = struct{}{}
			}
		}
	}
	slices.Sort(files)
	files = slices.Compact(files)

	report := "nothing to change"
	if len(touched) > 0 {
		if err := active.Commit(files, commitMessage(steps)); err != nil {
			return "", err
		}
		report = fmt.Sprintf("%s over %s, in one commit", count(len(steps), "change"), count(len(touched), "note"))
	}
	for _, problem := range problems {
		report += "\n" + problem
	}
	return report, nil
}

func planSteps(steps []Step) ([]planned, error) {
	plan := make([]planned, 0, len(steps))
	for _, step := range steps {
		if len(step.Keys) == 0 {
			return nil, errors.New("a change has to be aimed at a note")
		}
		if step.Change.Remove {
			plan = append(plan, planned{remove: true})
			continue
		}
		edits, err := parseTags(step.Change.Tags, "")
		if err != nil {
			return nil, err
		}
		plan = append(plan, planned{edits: edits, touch: step.Change.Touch})
	}
	return plan, nil
}

func (p planned) apply(active *notebook.Notebook, key string) (applied, error) {
	if p.remove {
		return applyRemove(active, key)
	}
	return applyTags(active, key, p.edits, p.touch)
}

// commitMessage gives one step its own subject; several get a count and a body
// listing them.
func commitMessage(steps []Step) string {
	if len(steps) == 1 {
		return steps[0].Describe()
	}
	keys := map[string]struct{}{}
	for _, step := range steps {
		for _, key := range step.Keys {
			keys[key] = struct{}{}
		}
	}
	var message strings.Builder
	fmt.Fprintf(&message, "bulk: %s over %s\n", count(len(steps), "change"), count(len(keys), "note"))
	for _, step := range steps {
		message.WriteString("\n" + step.Describe())
	}
	return message.String()
}
