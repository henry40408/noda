package cmd

import (
	"fmt"
	"strings"

	"github.com/henry40408/noda/internal/notebook"
	"github.com/henry40408/noda/internal/style"
)

// statusRow is one `key  value` line; a value may run over several lines.
type statusRow struct{ key, value string }

// Status is offline: drift is measured against the last fetch.
func Status(e Env) (string, error) {
	active, err := e.openActive()
	if err != nil {
		return "", err
	}
	status, err := active.Status()
	if err != nil {
		return "", err
	}

	rows := []statusRow{
		{"notebook", active.Name + "  " + style.Muted.Paint("("+status.Branch+")")},
		{"notes", fmt.Sprint(status.Notes)},
	}
	// Rows that would always say "0" are left out, so the others get read.
	if status.Files > 0 {
		rows = append(rows, statusRow{"files", fmt.Sprint(status.Files)})
	}
	rows = append(rows, statusRow{"changes", describeChanges(status.Uncommitted)})
	if len(status.Problems) > 0 {
		rows = append(rows, statusRow{"problems", describeProblems(status.Problems)})
	}
	if !status.HasRemote {
		rows = append(rows, statusRow{"remote", style.Muted.Paint("none — set one with `noda remote set <url>`")})
	} else {
		rows = append(rows, statusRow{"remote", status.Remote}, statusRow{"sync", describeDrift(status.Drift)})
	}
	return alignRows(rows), nil
}

func describeChanges(uncommitted int) string {
	switch uncommitted {
	case 0:
		return "clean"
	case 1:
		return "1 file uncommitted"
	}
	return fmt.Sprintf("%d files uncommitted", uncommitted)
}

// alignRows pads the keys to one column. Continuation lines align under the
// value column.
func alignRows(rows []statusRow) string {
	width := 0
	for _, row := range rows {
		width = max(width, DisplayWidth(row.key))
	}
	var out strings.Builder
	for _, row := range rows {
		lines := strings.Split(row.value, "\n")
		out.WriteString(pad(row.key, width) + "  " + lines[0] + "\n")
		for _, line := range lines[1:] {
			out.WriteString(pad("", width) + "  " + line + "\n")
		}
	}
	return out.String()
}

// describeProblems gives one kind one line; several get a total first, then
// the breakdown.
func describeProblems(problems []notebook.ProblemGroup) string {
	var out strings.Builder
	if len(problems) > 1 {
		total := 0
		for _, group := range problems {
			total += len(group.Subjects)
		}
		fmt.Fprintf(&out, "%d %s\n", total, plural(total, "problem", "problems"))
	}
	for _, group := range problems {
		out.WriteString(group.Kind.Describe(len(group.Subjects)) +
			style.Muted.Paint("  ("+elide(group.Subjects)+")") + "\n")
	}
	// Name the remedy.
	out.WriteString(style.Muted.Paint("run `noda doctor` to look at these"))
	return trimEnd(out.String())
}

// elide is the first few subjects, so a wholesale problem stays on one line.
func elide(subjects []string) string {
	const shown = 3
	listed := subjects[:min(shown, len(subjects))]
	if len(subjects) > shown {
		listed = append(append([]string(nil), listed...), "…")
	}
	return strings.Join(listed, "; ")
}

// Drifted is the drift phrased as what is left to do: one wording for every
// screen. `never synced`, not `never fetched`: a first push clears it as well
// as a fetch. Plain text, because one caller renders HTML.
func Drifted(drift *notebook.Drift) string {
	switch {
	case drift == nil:
		return "never synced"
	case drift.Ahead == 0 && drift.Behind == 0:
		return "in sync"
	case drift.Behind == 0:
		return fmt.Sprintf("%d to push", drift.Ahead)
	case drift.Ahead == 0:
		return fmt.Sprintf("%d to pull", drift.Behind)
	}
	return fmt.Sprintf("%d to push, %d to pull", drift.Ahead, drift.Behind)
}

// Standing is Drifted, or `no remote` where `never synced` would be a state it
// can never leave.
func Standing(hasRemote bool, drift *notebook.Drift) string {
	if !hasRemote {
		return "no remote"
	}
	return Drifted(drift)
}

// describeDrift is Drifted, painted, with the caveat that `status` reports the
// last sync.
func describeDrift(drift *notebook.Drift) string {
	words := Drifted(drift)
	if drift == nil {
		return style.Muted.Paint(words)
	}
	return words + " " + style.Muted.Paint("(as of the last sync)")
}
