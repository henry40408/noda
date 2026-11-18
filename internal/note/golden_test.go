package note

import (
	"encoding/json"
	"os"
	"reflect"
	"testing"
)

type goldenCase struct {
	Name  string `json:"name"`
	Op    string `json:"op"`
	Title string `json:"title"`
	Input string `json:"input"`
	ID    string `json:"id"`
	Slug  string `json:"slug"`
	Stem  string `json:"stem"`
	Text  string `json:"text"`
	Tag   string `json:"tag"`
	Key   string `json:"key"`
	Value string `json:"value"`
	Body  string `json:"body"`
}

func outcome(err error, ok any) any {
	if err != nil {
		return map[string]any{"err": err.Error()}
	}
	return map[string]any{"ok": ok}
}

func optional(text string, ok bool) any {
	if !ok {
		return nil
	}
	return text
}

func strOrNil(value *string) any {
	if value == nil {
		return nil
	}
	return *value
}

func run(c goldenCase) any {
	switch c.Op {
	case "slugify":
		return Slugify(c.Title)
	case "normalize_id":
		return NormalizeID(c.Input)
	case "file_name":
		return FileName(c.ID, c.Slug)
	case "split_stem":
		id, slug, ok := SplitStem(c.Stem)
		if !ok {
			return nil
		}
		return []any{id, slug}
	case "names_a_note":
		return NamesANote(c.Name)
	case "is_id_shaped":
		return IsIDShaped(c.Text)
	case "validate_title":
		return outcome(ValidateTitle(c.Title), true)
	case "validate_tag":
		return outcome(ValidateTag(c.Tag), true)
	case "roundtrip":
		n, err := Parse(c.Text)
		if err != nil {
			return outcome(err, nil)
		}
		return outcome(nil, map[string]any{
			"title": n.Title, "tags": tagsOrEmpty(n.Tags), "created": strOrNil(n.Created),
			"updated": strOrNil(n.Updated), "pinned": strOrNil(n.Pinned),
			"extra": tagsOrEmpty(n.Extra), "body": n.Body,
			"is_pinned": n.IsPinned(), "rendered": n.Render(),
		})
	case "set_field":
		return optional(SetField(c.Text, c.Key, c.Value))
	case "set_body":
		return optional(SetBody(c.Text, c.Body))
	}
	panic("unknown op " + c.Op)
}

func tagsOrEmpty(values []string) []any {
	out := []any{}
	for _, v := range values {
		out = append(out, v)
	}
	return out
}

func TestTheNoteModelMatchesTheRustGolden(t *testing.T) {
	var cases []goldenCase
	var expected []struct {
		Name string `json:"name"`
		Out  any    `json:"out"`
	}
	readJSON(t, "../../testdata/golden/note/cases.json", &cases)
	readJSON(t, "../../testdata/golden/note/expected.json", &expected)
	if len(cases) != len(expected) || len(cases) < 100 {
		t.Fatalf("%d cases, %d expected", len(cases), len(expected))
	}
	for i, c := range cases {
		if expected[i].Name != c.Name {
			t.Fatalf("case %d is %s, expected %s", i, c.Name, expected[i].Name)
		}
		if got := run(c); !reflect.DeepEqual(got, expected[i].Out) {
			t.Errorf("%s (%s):\n got  %#v\n want %#v\n input %+v", c.Name, c.Op, got, expected[i].Out, c)
		}
	}
}

func readJSON(t *testing.T, path string, into any) {
	t.Helper()
	raw, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	if err := json.Unmarshal(raw, into); err != nil {
		t.Fatal(err)
	}
}
