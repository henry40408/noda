package tui

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"unicode"

	"github.com/henry40408/noda/internal/cmd"
	"github.com/henry40408/noda/internal/tui/ui"
)

// The frames the Rust implementation drew on a fixed notebook, one per key
// sequence. Screens that print a commit time (log, blame, diff) are left out:
// a commit is stamped with the clock.

var goldenNotes = [][2]string{
	{"01abc234-budget-review.md", "---\ntitle: Budget review\ntags: [work]\ncreated: 2024-01-10T09:00:00Z\nupdated: 2024-03-01T10:00:00Z\n---\n\nthe q3 budget is late\n\n- [ ] pay the bill due:2024-01-02\n- [ ] plan ahead due:2099-01-01\n- [x] done already\n"},
	{"02def567-meeting-notes.md", "---\ntitle: Meeting notes\ntags: [work, q3]\ncreated: 2024-02-01T10:00:00+08:00\nupdated: 2024-02-02T10:00:00+08:00\npinned: true\n---\n\n# Agenda\n\nthe budget, again\n\nsee [the budget](01abc234-budget-review.md)\n"},
	{"03ghj890-reading-list.md", "---\ntitle: Reading list\ncreated: 2024-01-05T00:00:00Z\nupdated: 2024-01-06T00:00:00Z\n---\n\na book\n"},
	{"04kmn123-日本語メモ.md", "---\ntitle: 日本語メモ\ntags: [日本語, has space]\ncreated: 2024-01-01T00:00:00Z\nupdated: 2024-01-02T00:00:00Z\n---\n\n本文はここにあります。\n\n- [ ] 日本語の用事 due:2024-05-06\n"},
}

func goldenNotebook(t *testing.T) cmd.Env {
	t.Helper()
	env := emptyNotebook(t)
	active := notebookOf(t, env)
	for _, note := range goldenNotes {
		if err := os.WriteFile(filepath.Join(active.Path, note[0]), []byte(note[1]), 0o666); err != nil {
			t.Fatal(err)
		}
	}
	if err := os.WriteFile(filepath.Join(active.Path, "diagram.png"), []byte("not really a png"), 0o666); err != nil {
		t.Fatal(err)
	}
	if _, err := active.CommitAll("add notes"); err != nil {
		t.Fatal(err)
	}
	return env
}

type goldenCase struct {
	Name   string   `json:"name"`
	Width  int      `json:"width"`
	Height int      `json:"height"`
	Keys   []string `json:"keys"`
}

type goldenRow struct {
	Text   string `json:"text"`
	Styles []any  `json:"styles"`
}

type goldenFrame struct {
	Cursor []int       `json:"cursor"`
	Rows   []goldenRow `json:"rows"`
}

// describe is a cell's style as the golden frames write it.
func describe(cell *ui.Cell) string {
	parts := []string{"fg=" + cell.Fg.String(), "bg=" + cell.Bg.String()}
	if cell.Modifier != 0 {
		parts = append(parts, "add="+cell.Modifier.String())
	}
	return strings.Join(parts, " ")
}

// styleRuns are runs of cells that share a style: [start, length, "fg=Yellow ...].
func styleRuns(buffer *ui.Buffer, y int) []any {
	runs := []any{}
	for x := 0; x < buffer.Area.Width; {
		here := describe(buffer.At(x, y))
		end := x + 1
		for end < buffer.Area.Width && describe(buffer.At(end, y)) == here {
			end++
		}
		runs = append(runs, []any{x, end - x, here})
		x = end
	}
	return runs
}

func goldenFrameOf(frame *ui.Frame) goldenFrame {
	buffer := frame.Buffer
	out := goldenFrame{Cursor: []int{0, 0}}
	if frame.Cursor != nil {
		out.Cursor = []int{frame.Cursor.X, frame.Cursor.Y}
	}
	for y := 0; y < buffer.Area.Height; y++ {
		out.Rows = append(out.Rows, goldenRow{
			Text:   strings.TrimRightFunc(buffer.Row(y), unicode.IsSpace),
			Styles: styleRuns(buffer, y),
		})
	}
	return out
}

func readJSON(t *testing.T, name string, into any) {
	t.Helper()
	raw, err := os.ReadFile(filepath.Join("..", "..", "testdata", "golden", "tui", name))
	if err != nil {
		t.Fatal(err)
	}
	if err := json.Unmarshal(raw, into); err != nil {
		t.Fatal(err)
	}
}

func TestEveryPageMatchesTheFrameRustDrew(t *testing.T) {
	var cases []goldenCase
	var expected []struct {
		Name string      `json:"name"`
		Out  goldenFrame `json:"out"`
	}
	readJSON(t, "cases.json", &cases)
	readJSON(t, "expected.json", &expected)
	if len(cases) != 75 || len(expected) != len(cases) {
		t.Fatalf("%d cases and %d frames, want 75 of each", len(cases), len(expected))
	}
	env := goldenNotebook(t)
	for i, c := range cases {
		t.Run(c.Name, func(t *testing.T) {
			if expected[i].Name != c.Name {
				t.Fatalf("frame %d is %q", i, expected[i].Name)
			}
			app := load(t, env)
			for _, token := range c.Keys {
				pressToken(t, app, token)
			}
			got := goldenFrameOf(drawAt(env, app, c.Width, c.Height))
			compareFrames(t, got, expected[i].Out)
		})
	}
}

func compareFrames(t *testing.T, got, want goldenFrame) {
	t.Helper()
	if fmt.Sprint(got.Cursor) != fmt.Sprint(want.Cursor) {
		t.Errorf("cursor %v, want %v", got.Cursor, want.Cursor)
	}
	if len(got.Rows) != len(want.Rows) {
		t.Fatalf("%d rows, want %d", len(got.Rows), len(want.Rows))
	}
	for y := range got.Rows {
		if got.Rows[y].Text != want.Rows[y].Text {
			t.Errorf("row %d text\n got %q\nwant %q", y, got.Rows[y].Text, want.Rows[y].Text)
			continue
		}
		gotStyles, _ := json.Marshal(got.Rows[y].Styles)
		wantStyles, _ := json.Marshal(want.Rows[y].Styles)
		if string(gotStyles) != string(wantStyles) {
			t.Errorf("row %d (%q) styles\n got %s\nwant %s", y, got.Rows[y].Text, gotStyles, wantStyles)
		}
	}
}
