package note

import (
	"reflect"
	"strings"
	"testing"
)

func TestSlugifyTable(t *testing.T) {
	cases := []struct{ name, title, want string }{
		{"lowercases and joins words", "Hello World", "hello-world"},
		{"trims surrounding space", "  Leading and trailing  ", "leading-and-trailing"},
		{"collapses a punctuation run", "Weird -- punctuation!!!", "weird-punctuation"},
		{"collapses adjacent dashes", "a--b", "a-b"},
		{"collapses adjacent spaces", "a  b", "a-b"},
		{"drops a trailing dash", "Trailing-", "trailing"},
		{"drops a leading dash", "-Leading", "leading"},
		{"tab separates", "tab\tseparated", "tab-separated"},
		{"underscore separates", "under_score", "under-score"},
		{"dot separates", "dots.in.name", "dots-in-name"},
		{"path separators never survive", "a/b\\c", "a-b-c"},
		{"traversal becomes plain words", "../../etc/passwd", "etc-passwd"},
		{"digits are kept", "100% sure", "100-sure"},
		{"symbols collapse", "C++ & C#", "c-c"},
		{"empty falls back", "", "note"},
		{"blank falls back", "   ", "note"},
		{"punctuation only falls back", "!!!", "note"},
		{"CJK stays legible", "中文標題", "中文標題"},
		{"kana with a space", "日本語 タイトル", "日本語-タイトル"},
		{"emoji is a separator", "emoji 😀 title", "emoji-title"},
		{"accents are kept", "ünïcödé wörds", "ünïcödé-wörds"},
		{"dotted capital I expands to two characters", "İstanbul", "i̇stanbul"},
		{"titlecase letter lowercases", "ǅ", "ǆ"},
		{"ligature is not expanded", "ﬁne", "ﬁne"},
		{"99 bytes then a word cuts at the dash", strings.Repeat("x", 99) + " y", strings.Repeat("x", 99)},
		{"100 bytes then a word cuts at the cap", strings.Repeat("x", 100) + " y", strings.Repeat("x", 100)},
		{"early dash is not worth cutting at", strings.Repeat("a", 30) + " " + strings.Repeat("b", 120), strings.Repeat("a", 30) + "-" + strings.Repeat("b", 69)},
		{"cut at a dash ends on a whole word", strings.Repeat("ab cd ", 30), strings.Repeat("ab-cd-", 16) + "ab"},
		{"cap inside a 3 byte character backs up", strings.Repeat("日", 34), strings.Repeat("日", 33)},
		{"dash after multibyte text cuts at the dash", strings.Repeat("日本", 21) + " end", strings.Repeat("日本", 16) + "日"},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			if got := Slugify(c.title); got != c.want {
				t.Errorf("Slugify(%q):\n got  %q\n want %q", c.title, got, c.want)
			}
		})
	}
}

func TestNormalizeIDTable(t *testing.T) {
	cases := []struct{ name, input, want string }{
		{"plain id is unchanged", "01abc234", "01abc234"},
		{"uppercase folds", "ABCDEFGH", "abcdefgh"},
		{"i l o fold in both cases", "LlIi0Oo1", "11110001"},
		{"u is not folded", "u", "u"},
		{"empty stays empty", "", ""},
		{"text past the id is kept", "12345678extra", "12345678extra"},
		{"dash is kept", "0-1", "0-1"},
		{"whitespace is kept", " 01ab ", " 01ab "},
		{"non ASCII uppercase lowercases", "Ü", "ü"},
		{"fullwidth o is not folded", "ｏ", "ｏ"},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			if got := NormalizeID(c.input); got != c.want {
				t.Errorf("NormalizeID(%q) = %q, want %q", c.input, got, c.want)
			}
		})
	}
}

func TestFileNameTable(t *testing.T) {
	cases := []struct{ name, slug, want string }{
		{"joins id slug and extension", "hello", "01abc234-hello.md"},
		{"empty slug keeps the dash", "", "01abc234-.md"},
		{"a long slug is not cut", strings.Repeat("日", 100), "01abc234-" + strings.Repeat("日", 100) + ".md"},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			if got := FileName("01abc234", c.slug); got != c.want {
				t.Errorf("FileName = %q, want %q", got, c.want)
			}
		})
	}
}

func TestSplitStemTable(t *testing.T) {
	cases := []struct {
		name, stem, id, slug string
		ok                   bool
	}{
		{"id and slug", "01abc234-hello", "01abc234", "hello", true},
		{"slug keeps later dashes", "01abc234-a-b-c", "01abc234", "a-b-c", true},
		{"second dash belongs to the slug", "01abc234--x", "01abc234", "-x", true},
		{"uppercase id is kept as written", "01ABC234-x", "01ABC234", "x", true},
		{"CJK slug", "01abc234-日本語", "01abc234", "日本語", true},
		{"empty slug is no note", "01abc234-", "", "", false},
		{"no dash is no note", "01abc234", "", "", false},
		{"id too short", "01abc23-hello", "", "", false},
		{"no dash after a long run", "01abc234hello", "", "", false},
		{"empty id", "-hello", "", "", false},
		{"empty stem", "", "", "", false},
		{"o is outside the id alphabet", "O1abc234-x", "", "", false},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			id, slug, ok := SplitStem(c.stem)
			if id != c.id || slug != c.slug || ok != c.ok {
				t.Errorf("SplitStem(%q) = %q, %q, %v", c.stem, id, slug, ok)
			}
		})
	}
}

func TestNamesANoteTable(t *testing.T) {
	cases := []struct {
		name string
		file string
		want bool
	}{
		{"id slug md", "01abc234-hello.md", true},
		{"other extension", "01abc234-hello.txt", false},
		{"extension is case sensitive", "01abc234-hello.MD", false},
		{"README", "README.md", false},
		{"id without slug", "01abc234.md", false},
		{"id with empty slug", "01abc234-.md", false},
		{"bare extension", ".md", false},
		{"short stem", "a.md", false},
		{"a path is not a name", "x/01abc234-hello.md", false},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			if got := NamesANote(c.file); got != c.want {
				t.Errorf("NamesANote(%q) = %v", c.file, got)
			}
		})
	}
}

func TestIsIDShapedTable(t *testing.T) {
	cases := []struct {
		name string
		text string
		want bool
	}{
		{"exactly the id length", "01abc234", true},
		{"longer is shaped", "01abc2345", true},
		{"one short", "01abc23", false},
		{"empty", "", false},
		{"confusable letters are outside the alphabet", "OIlo0000", false},
		{"u is outside the alphabet", "uuuuuuuu", false},
		{"dash is outside the alphabet", "01-bc234", false},
		{"non ASCII is outside the alphabet", "01abc23é", false},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			if got := IsIDShaped(c.text); got != c.want {
				t.Errorf("IsIDShaped(%q) = %v", c.text, got)
			}
		})
	}
}

func TestValidateTitleTable(t *testing.T) {
	const refused = "a title has to fit on one line"
	cases := []struct{ name, title, err string }{
		{"plain", "ok", ""},
		{"empty", "", ""},
		{"colon and hash are fine", "a: b # c", ""},
		{"tab is fine", "tab\there", ""},
		{"NUL is fine", "\x00", ""},
		{"LF refused", "multi\nline", refused},
		{"CR refused", "a\rb", refused},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			if got := errText(ValidateTitle(c.title)); got != c.err {
				t.Errorf("ValidateTitle(%q) = %q, want %q", c.title, got, c.err)
			}
		})
	}
}

func TestValidateTagTable(t *testing.T) {
	cases := []struct{ name, tag, err string }{
		{"plain", "ok", ""},
		{"inner space", "q3 plan", ""},
		{"only a space", " ", ""},
		{"colon, hash and leading dash", "-a:b#c", ""},
		{"CJK", "日本語", ""},
		{"empty", "", "a tag needs a name"},
		{"comma", "a,b", "a tag cannot contain `,`: a,b"},
		{"trailing comma", "a,", "a tag cannot contain `,`: a,"},
		{"opening bracket names the first offender", "[x]", "a tag cannot contain `[`: [x]"},
		{"closing bracket", "a]", "a tag cannot contain `]`: a]"},
		{"LF", "a\nb", "a tag cannot contain a line break: a\nb"},
		{"CR", "a\rb", "a tag cannot contain a line break: a\rb"},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			if got := errText(ValidateTag(c.tag)); got != c.err {
				t.Errorf("ValidateTag(%q) = %q, want %q", c.tag, got, c.err)
			}
		})
	}
}

func errText(err error) string {
	if err == nil {
		return ""
	}
	return err.Error()
}

func TestParseAndRenderTable(t *testing.T) {
	cases := []struct {
		name     string
		text     string
		want     Note
		pinned   bool
		rendered string
	}{
		{
			name:     "every known field",
			text:     "---\ntitle: T\ntags: [a, b]\ncreated: 2024-01-01T00:00:00Z\nupdated: 2024-01-02T00:00:00Z\n---\n\nbody\n",
			want:     Note{Title: "T", Tags: []string{"a", "b"}, Created: ptr("2024-01-01T00:00:00Z"), Updated: ptr("2024-01-02T00:00:00Z"), Body: "body\n"},
			rendered: "---\ntitle: T\ntags: [a, b]\ncreated: 2024-01-01T00:00:00Z\nupdated: 2024-01-02T00:00:00Z\n---\n\nbody\n",
		},
		{
			name:     "a body without a final newline gains one",
			text:     "---\ntitle: T\n---\n\nbody",
			want:     Note{Title: "T", Body: "body"},
			rendered: "---\ntitle: T\n---\n\nbody\n",
		},
		{
			name:     "no body",
			text:     "---\ntitle: T\n---\n",
			want:     Note{Title: "T"},
			rendered: "---\ntitle: T\n---\n\n",
		},
		{
			name:     "empty frontmatter",
			text:     "---\n---\n",
			want:     Note{},
			rendered: "---\ntitle: \n---\n\n",
		},
		{
			name:     "blank lines before the body are dropped",
			text:     "---\n---\n\n\n\nbody\n",
			want:     Note{Body: "body\n"},
			rendered: "---\ntitle: \n---\n\nbody\n",
		},
		{
			name:     "a body may start right after the fence",
			text:     "---\ntitle: T\n---\nimmediately\n",
			want:     Note{Title: "T", Body: "immediately\n"},
			rendered: "---\ntitle: T\n---\n\nimmediately\n",
		},
		{
			name:     "a fence inside the body stays in the body",
			text:     "---\ntitle: T\n---\n\n---\nnot frontmatter\n---\n",
			want:     Note{Title: "T", Body: "---\nnot frontmatter\n---\n"},
			rendered: "---\ntitle: T\n---\n\n---\nnot frontmatter\n---\n",
		},
		{
			name:     "body indentation and inner blank lines survive",
			text:     "---\ntitle: T\n---\n\n\n\n  indented body\n\nlast",
			want:     Note{Title: "T", Body: "  indented body\n\nlast"},
			rendered: "---\ntitle: T\n---\n\n  indented body\n\nlast\n",
		},
		{
			name:     "a fence with trailing space closes the block",
			text:     "---\ntitle: T\n--- \n---\n\nb\n",
			want:     Note{Title: "T", Body: "---\n\nb\n"},
			rendered: "---\ntitle: T\n---\n\n---\n\nb\n",
		},
		{
			name:     "pinned true",
			text:     "---\ntitle: T\npinned: true\n---\n\nb\n",
			want:     Note{Title: "T", Pinned: ptr("true"), Body: "b\n"},
			pinned:   true,
			rendered: "---\ntitle: T\npinned: true\n---\n\nb\n",
		},
		{
			name:     "pinned TRUE counts",
			text:     "---\ntitle: T\npinned: TRUE\n---\n\nb\n",
			want:     Note{Title: "T", Pinned: ptr("TRUE"), Body: "b\n"},
			pinned:   true,
			rendered: "---\ntitle: T\npinned: TRUE\n---\n\nb\n",
		},
		{
			name:     "pinned yes is kept but not a pin",
			text:     "---\ntitle: T\npinned: yes\n---\n\nb\n",
			want:     Note{Title: "T", Pinned: ptr("yes"), Body: "b\n"},
			rendered: "---\ntitle: T\npinned: yes\n---\n\nb\n",
		},
		{
			name:     "pinned false is kept but not a pin",
			text:     "---\ntitle: T\npinned: false\n---\n\nb\n",
			want:     Note{Title: "T", Pinned: ptr("false"), Body: "b\n"},
			rendered: "---\ntitle: T\npinned: false\n---\n\nb\n",
		},
		{
			name:     "unknown fields move below the known ones and keep their order; an offset is not respelled",
			text:     "---\ntitle: T\nauthor: someone\naliases: [x]\ncreated: 2024-01-01T00:00:00+08:00\n---\n\nb\n",
			want:     Note{Title: "T", Created: ptr("2024-01-01T00:00:00+08:00"), Extra: []string{"author: someone", "aliases: [x]"}, Body: "b\n"},
			rendered: "---\ntitle: T\ncreated: 2024-01-01T00:00:00+08:00\nauthor: someone\naliases: [x]\n---\n\nb\n",
		},
		{
			name:     "a line without a colon is carried as extra",
			text:     "---\ntitle: T\nnot a pair\n---\n\nb\n",
			want:     Note{Title: "T", Extra: []string{"not a pair"}, Body: "b\n"},
			rendered: "---\ntitle: T\nnot a pair\n---\n\nb\n",
		},
		{
			name:     "an indented line is extra, not a field",
			text:     "---\ntitle: T\n  indented: x\n---\n\nb\n",
			want:     Note{Title: "T", Extra: []string{"  indented: x"}, Body: "b\n"},
			rendered: "---\ntitle: T\n  indented: x\n---\n\nb\n",
		},
		{
			name:     "known fields are written in a fixed order",
			text:     "---\nupdated: u\ncreated: c\ntitle: T\ntags: [z]\n---\n\nb\n",
			want:     Note{Title: "T", Tags: []string{"z"}, Created: ptr("c"), Updated: ptr("u"), Body: "b\n"},
			rendered: "---\ntitle: T\ntags: [z]\ncreated: c\nupdated: u\n---\n\nb\n",
		},
		{
			name:     "the last duplicate wins",
			text:     "---\ntitle: T\ntitle: U\ntags: [a]\ntags: [b]\n---\n\nb\n",
			want:     Note{Title: "U", Tags: []string{"b"}, Body: "b\n"},
			rendered: "---\ntitle: U\ntags: [b]\n---\n\nb\n",
		},
		{
			name:     "a colon in the title stays in the title",
			text:     "---\ntitle: A: B\n---\n\nb\n",
			want:     Note{Title: "A: B", Body: "b\n"},
			rendered: "---\ntitle: A: B\n---\n\nb\n",
		},
		{
			name:     "CRLF inside the block is read as LF",
			text:     "---\ntitle: T\r\ntags: [a]\r\n---\nbody\n",
			want:     Note{Title: "T", Tags: []string{"a"}, Body: "body\n"},
			rendered: "---\ntitle: T\ntags: [a]\n---\n\nbody\n",
		},
		{
			name:     "values are trimmed and tags tolerate a missing space",
			text:     "---\ntitle:   spaced   \ntags:[a,b,  c ]\n---\n\nb\n",
			want:     Note{Title: "spaced", Tags: []string{"a", "b", "c"}, Body: "b\n"},
			rendered: "---\ntitle: spaced\ntags: [a, b, c]\n---\n\nb\n",
		},
		{
			name:     "empty tags write no line",
			text:     "---\ntitle: T\ntags: []\n---\n\nb\n",
			want:     Note{Title: "T", Body: "b\n"},
			rendered: "---\ntitle: T\n---\n\nb\n",
		},
		{
			name:     "tags without brackets",
			text:     "---\ntitle: T\ntags: a, b\n---\n\nb\n",
			want:     Note{Title: "T", Tags: []string{"a", "b"}, Body: "b\n"},
			rendered: "---\ntitle: T\ntags: [a, b]\n---\n\nb\n",
		},
		{
			name:     "an empty tag entry is dropped",
			text:     "---\ntitle: T\ntags: [a, , b]\n---\n\nb\n",
			want:     Note{Title: "T", Tags: []string{"a", "b"}, Body: "b\n"},
			rendered: "---\ntitle: T\ntags: [a, b]\n---\n\nb\n",
		},
		{
			name:     "unicode title tags and body",
			text:     "---\ntitle: 日本語\ntags: [中文, emoji😀]\n---\n\n本文\n",
			want:     Note{Title: "日本語", Tags: []string{"中文", "emoji😀"}, Body: "本文\n"},
			rendered: "---\ntitle: 日本語\ntags: [中文, emoji😀]\n---\n\n本文\n",
		},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			got := mustParse(t, c.text)
			if !reflect.DeepEqual(got, c.want) {
				t.Errorf("Parse:\n got  %+v\n want %+v", got, c.want)
			}
			if got.IsPinned() != c.pinned {
				t.Errorf("IsPinned = %v", got.IsPinned())
			}
			if rendered := got.Render(); rendered != c.rendered {
				t.Errorf("Render:\n got  %q\n want %q", rendered, c.rendered)
			}
		})
	}
}

func TestParseWithoutAFrontmatterBlockFails(t *testing.T) {
	cases := []struct{ name, text string }{
		{"plain text", "no frontmatter\n"},
		{"block never closed", "---\ntitle: T\n"},
		{"CRLF opening fence", "---\r\ntitle: T\r\n---\r\n\r\nbody\r\n"},
		{"four dashes do not open", "----\ntitle: T\n---\n"},
		{"indented fence does not open", " ---\ntitle: T\n---\n"},
		{"byte order mark before the fence", "\ufeff---\ntitle: T\n---\n\nb\n"},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			if _, err := Parse(c.text); err != errNoFrontmatter {
				t.Errorf("err = %v", err)
			}
		})
	}
}

const stampedNote = "---\ntitle: T\ntags: [a, b]\ncreated: 2024-01-01T00:00:00Z\nupdated: 2024-01-02T00:00:00Z\n---\n\nbody\n"

func TestSetFieldTable(t *testing.T) {
	cases := []struct {
		name, text, key, value, want string
		ok                           bool
	}{
		{"replaces in place", stampedNote, "title", "New",
			"---\ntitle: New\ntags: [a, b]\ncreated: 2024-01-01T00:00:00Z\nupdated: 2024-01-02T00:00:00Z\n---\n\nbody\n", true},
		{"replaces the last field", stampedNote, "updated", "2025-01-01T00:00:00Z",
			"---\ntitle: T\ntags: [a, b]\ncreated: 2024-01-01T00:00:00Z\nupdated: 2025-01-01T00:00:00Z\n---\n\nbody\n", true},
		{"appends a known field that is absent", stampedNote, "pinned", "true",
			"---\ntitle: T\ntags: [a, b]\ncreated: 2024-01-01T00:00:00Z\nupdated: 2024-01-02T00:00:00Z\npinned: true\n---\n\nbody\n", true},
		{"appends an unknown field", stampedNote, "missing", "x",
			"---\ntitle: T\ntags: [a, b]\ncreated: 2024-01-01T00:00:00Z\nupdated: 2024-01-02T00:00:00Z\nmissing: x\n---\n\nbody\n", true},
		{"value is written as given", stampedNote, "tags", "[x]",
			"---\ntitle: T\ntags: [x]\ncreated: 2024-01-01T00:00:00Z\nupdated: 2024-01-02T00:00:00Z\n---\n\nbody\n", true},
		{"appending leaves a bodyless note bodyless", "---\ntitle: T\n---\n", "created", "c",
			"---\ntitle: T\ncreated: c\n---\n", true},
		{"replaces an unknown field in the middle", "---\ntitle: T\nauthor: a\n---\n\nb\n", "author", "z",
			"---\ntitle: T\nauthor: z\n---\n\nb\n", true},
		{"a duplicate is collapsed", "---\ntags: [a]\ntitle: T\ntags: [b]\n---\n\nb\n", "tags", "[c]",
			"---\ntags: [c]\ntitle: T\n---\n\nb\n", true},
		{"a line without a colon is left alone", "---\nnot a pair\ntitle: T\n---\n", "title", "U",
			"---\nnot a pair\ntitle: U\n---\n", true},
		{"a value is not escaped", "---\ntitle: T\n---\n\nb\n", "title", "multi\nline",
			"---\ntitle: multi\nline\n---\n\nb\n", true},
		{"no frontmatter", "no frontmatter", "title", "x", "", false},
		{"CRLF opening fence is no frontmatter", "---\r\ntitle: T\r\n---\r\n\r\nb\r\n", "title", "U", "", false},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			got, ok := SetField(c.text, c.key, c.value)
			if got != c.want || ok != c.ok {
				t.Errorf("SetField:\n got  %q, %v\n want %q, %v", got, ok, c.want, c.ok)
			}
		})
	}
}

func TestSetBodyTable(t *testing.T) {
	const head = "---\ntitle: T\ntags: [a, b]\ncreated: 2024-01-01T00:00:00Z\nupdated: 2024-01-02T00:00:00Z\n---\n\n"
	cases := []struct {
		name, text, body, want string
		ok                     bool
	}{
		{"adds the final newline", stampedNote, "new body", head + "new body\n", true},
		{"keeps an existing final newline", stampedNote, "new body\n", head + "new body\n", true},
		{"empty body leaves the blank line", stampedNote, "", head, true},
		{"a bodyless note gains the blank line", "---\ntitle: T\n---\n", "b", "---\ntitle: T\n---\n\nb\n", true},
		{"leading blank lines are dropped and CRLF becomes LF", "---\ntitle: T\n---\n\nold\n", "\r\n\r\na\r\nb\r\n", "---\ntitle: T\n---\n\na\nb\n", true},
		{"a body may look like frontmatter", "---\ntitle: T\n---\n\n\nold\n", "---\nlooks like fm\n---\n", "---\ntitle: T\n---\n\n---\nlooks like fm\n---\n", true},
		{"no frontmatter", "no frontmatter", "x", "", false},
		{"CRLF opening fence is no frontmatter", "---\r\ntitle: T\r\n---\r\n\r\nb\r\n", "n\r\n", "", false},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			got, ok := SetBody(c.text, c.body)
			if got != c.want || ok != c.ok {
				t.Errorf("SetBody:\n got  %q, %v\n want %q, %v", got, ok, c.want, c.ok)
			}
		})
	}
}
