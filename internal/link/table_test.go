package link

import (
	"slices"
	"testing"
)

func TestTargetsTable(t *testing.T) {
	cases := []struct {
		name string
		body string
		want []string
	}{
		{"inline link", "[a](01abc234-hello.md)", []string{"01abc234-hello.md"}},
		{"fragment and dot-slash are normalised", "[a](01abc234-hello.md) and [b](./02abc234-other.md#frag)", []string{"01abc234-hello.md", "02abc234-other.md"}},
		{"image counts, external link does not", "![img](pic.png) [x](https://example.com/a.md)", []string{"pic.png"}},
		{"full reference", "[ref][r]\n\n[r]: 03abc234-ref.md\n", []string{"03abc234-ref.md"}},
		{"shortcut reference", "[r]\n\n[r]: 03abc234-ref.md \"title\"\n", []string{"03abc234-ref.md"}},
		{"collapsed reference", "[a][] \n\n[a]: shortcut.md\n", []string{"shortcut.md"}},
		{"angle bracket definition keeps its space", "[r]: <03 abc234-ref.md>\n\n[r]\n", []string{"03 abc234-ref.md"}},
		{"unused definition", "[x]: unused.md\n", nil},
		{"two definitions of one file", "[a] [b]\n\n[a]: x.md\n[b]: x.md\n", []string{"x.md"}},
		{"reference case is kept", "[a]\n\n[a]: Shortcut.MD\n", []string{"Shortcut.MD"}},
		{"fenced code", "```\n[a](code.md)\n```\n", nil},
		{"inline code", "`[a](inline.md)`", nil},
		{"raw html", "<a href=\"raw.md\">raw</a>", nil},
		{"autolink", "<https://example.com/x.md>", nil},
		{"bare url", "https://example.com/bare.md", nil},
		{"angle bracket destination", "[a](<with space.md>)", []string{"with space.md"}},
		{"percent escape", "[a](with%20space.md)", []string{"with space.md"}},
		{"unicode name", "[日本語](日本語.md)", []string{"日本語.md"}},
		{"unicode percent escape", "[a](日本語%E6%96%87.md)", []string{"日本語文.md"}},
		{"subdirectory", "[a](sub/dir/file.md)", []string{"sub/dir/file.md"}},
		{"parent directory", "[a](../up.md)", nil},
		{"fragment only", "[a](#only-fragment)", nil},
		{"mailto", "[a](mailto:x@y.z)", nil},
		{"javascript", "[a](javascript:alert(1))", nil},
		{"upper case scheme", "[a](HTTP://UP.example/x.md)", nil},
		{"data", "[a](data:text/plain,x)", nil},
		{"protocol relative", "[a](//proto-relative.md)", nil},
		{"absolute path", "[a](/abs/path.md)", nil},
		{"query is dropped", "[a](file.md?query=1)", []string{"file.md"}},
		{"duplicates collapse, order kept", "[a](a.md) [b](a.md) [c](b.md)", []string{"a.md", "b.md"}},
		{"empty destination", "[a]()", nil},
		{"empty angle destination", "[a](<>)", nil},
		{"nested brackets in the text", "[nested [brackets]](x.md)", []string{"x.md"}},
		{"double quoted title", "[a](x.md \"title\")", []string{"x.md"}},
		{"single quoted title", "[a](x.md 'title')", []string{"x.md"}},
		{"parenthesised title", "[a](x.md (title))", []string{"x.md"}},
		{"space in an unbracketed image destination", "![alt](img one.png)", nil},
		{"balanced parentheses in the name", "[a](x(1).md)", []string{"x(1).md"}},
		{"escaped parentheses in the name", "[a](x\\(1\\).md)", []string{"x(1).md"}},
		{"several lines", "line1\n[a](x.md)\nline3 [b](y.md)\n", []string{"x.md", "y.md"}},
		{"blockquote", "> [a](quoted.md)\n", []string{"quoted.md"}},
		{"list items", "- [a](list.md)\n- [b](list2.md)\n", []string{"list.md", "list2.md"}},
		{"table cell", "| t |\n|---|\n| [a](cell.md) |\n", []string{"cell.md"}},
		{"adjacent links", "[a](x.md)[b](y.md)", []string{"x.md", "y.md"}},
		{"escaped opening bracket", "\\[a](escaped.md)", nil},
		{"empty body", "", nil},
		{"no links", "no links at all", nil},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			if got := Targets(c.body); !slices.Equal(got, c.want) {
				t.Errorf("Targets(%q) = %q, want %q", c.body, got, c.want)
			}
		})
	}
}

func TestRewriteTable(t *testing.T) {
	cases := []struct {
		name, body, old, new, want string
		changed                    bool
	}{
		{"plain destination", "[a](01abc234-hello.md)", "01abc234-hello.md", "01abc234-new.md", "[a](01abc234-new.md)", true},
		{"dot-slash spelling", "[a](./01abc234-hello.md)", "01abc234-hello.md", "01abc234-new.md", "[a](01abc234-new.md)", true},
		{"fragment survives", "[a](01abc234-hello.md#frag)", "01abc234-hello.md", "01abc234-new.md", "[a](01abc234-new.md#frag)", true},
		{"query survives", "[a](01abc234-hello.md?x=1)", "01abc234-hello.md", "01abc234-new.md", "[a](01abc234-new.md?x=1)", true},
		{"every occurrence", "[a](01abc234-hello.md) [b](01abc234-hello.md)", "01abc234-hello.md", "01abc234-new.md", "[a](01abc234-new.md) [b](01abc234-new.md)", true},
		{"other links are left alone", "[a](01abc234-hello.md) [b](other.md)", "01abc234-hello.md", "x.md", "[a](x.md) [b](other.md)", true},
		{"fragments on adjacent links", "[a](x.md#f)[b](x.md#g)", "x.md", "y.md", "[a](y.md#f)[b](y.md#g)", true},
		{"definition", "[r]\n\n[r]: 01abc234-hello.md\n", "01abc234-hello.md", "01abc234-new.md", "[r]\n\n[r]: 01abc234-new.md\n", true},
		{"angle definition with title", "[r]\n\n[r]: <01abc234-hello.md> \"t\"\n", "01abc234-hello.md", "01abc234-new.md", "[r]\n\n[r]: <01abc234-new.md> \"t\"\n", true},
		{"angle destination", "[a](<with space.md>)", "with space.md", "plain.md", "[a](<plain.md>)", true},
		{"escaped old name", "[a](with%20space.md)", "with space.md", "plain.md", "[a](plain.md)", true},
		{"new name is escaped", "[a](01abc234-hello.md)", "01abc234-hello.md", "with space.md", "[a](with%20space.md)", true},
		{"image", "![i](pic.png)", "pic.png", "new pic.png", "![i](new%20pic.png)", true},
		{"unicode old name written escaped", "[a](%E6%97%A5.md)", "日.md", "x.md", "[a](x.md)", true},
		{"new name parentheses", "[a](x.md)\n[b](x.md)", "x.md", "y(1).md", "[a](y%281%29.md)\n[b](y%281%29.md)", true},
		{"new name close paren", "[a](x.md)", "x.md", "a)b.md", "[a](a%29b.md)", true},
		{"new name open paren", "[a](x.md)", "x.md", "a(b.md", "[a](a%28b.md)", true},
		{"new name angle brackets", "[a](x.md)", "x.md", "a<b>.md", "[a](a%3Cb%3E.md)", true},
		{"double quoted title survives", "[a](x.md \"title\")", "x.md", "y.md", "[a](y.md \"title\")", true},
		{"single quoted title survives", "[a](x.md 'title')", "x.md", "y.md", "[a](y.md 'title')", true},
		{"same name", "[a](x.md)", "x.md", "x.md", "[a](x.md)", true},
		{"subdirectory old name", "[a](sub/x.md)", "sub/x.md", "y.md", "[a](y.md)", true},
		{"surrounding spaces survive", "[a]( x.md )", "x.md", "y.md", "[a]( y.md )", true},
		{"blockquote", "> [a](x.md)\n> [b](x.md)", "x.md", "y.md", "> [a](y.md)\n> [b](y.md)", true},
		{"fenced code", "```\n[a](01abc234-hello.md)\n```\n", "01abc234-hello.md", "x.md", "", false},
		{"inline code", "`[a](01abc234-hello.md)`", "01abc234-hello.md", "x.md", "", false},
		{"name never linked", "[a](01abc234-hello.md)", "nothing.md", "x.md", "", false},
		{"parent directory is not the notebook file", "[a](../x.md)", "x.md", "y.md", "", false},
		{"raw html", "<a href=\"x.md\">raw</a>", "x.md", "y.md", "", false},
		{"names are case sensitive", "[a](X.md)", "x.md", "y.md", "", false},
		{"wiki link is not a link", "[[x.md]]", "x.md", "y.md", "", false},
		{"unicode names stay literal", "[a](日本語.md)", "日本語.md", "中文.md", "[a](中文.md)", true},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			got, ok := Rewrite(c.body, c.old, c.new)
			if ok != c.changed || got != c.want && c.changed {
				t.Errorf("Rewrite(%q, %q, %q) = %q, %v; want %q, %v", c.body, c.old, c.new, got, ok, c.want, c.changed)
			}
		})
	}
}

func TestTargetTable(t *testing.T) {
	cases := []struct {
		name, dest, want string
		ok               bool
	}{
		{"plain", "x.md", "x.md", true},
		{"dot-slash", "./x.md", "x.md", true},
		{"fragment", "x.md#f", "x.md", true},
		{"empty fragment", "x.md#", "x.md", true},
		{"query", "x.md?q", "x.md", true},
		{"empty query", "x.md?", "x.md", true},
		{"fragment after query", "x.md?q#f", "x.md", true},
		{"query after fragment", "x.md#f?g", "x.md", true},
		{"percent escape", "x%20y.md", "x y.md", true},
		{"unicode percent escape", "%E6%97%A5.md", "日.md", true},
		{"invalid escape is kept", "%zz.md", "%zz.md", true},
		{"nul escape", "%00.md", "\x00.md", true},
		{"subdirectory", "sub/x.md", "sub/x.md", true},
		{"unicode", "日本語.md", "日本語.md", true},
		{"space", "a b.md", "a b.md", true},
		{"trailing slash is trimmed", "x.md/", "x.md", true},
		{"note name", "01abc234-hello.md", "01abc234-hello.md", true},
		{"parent", "../x.md", "", false},
		{"fragment only", "#f", "", false},
		{"empty", "", "", false},
		{"http", "http://a/b.md", "", false},
		{"mailto", "mailto:a@b", "", false},
		{"absolute", "/x.md", "", false},
		{"protocol relative", "//x.md", "", false},
		{"dot-slash only", "./", "", false},
		{"dot", ".", "", false},
		{"dot dot", "..", "", false},
		{"windows drive", "C:\\x.md", "", false},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			got, ok := Target(c.dest)
			if got != c.want || ok != c.ok {
				t.Errorf("Target(%q) = %q, %v; want %q, %v", c.dest, got, ok, c.want, c.ok)
			}
		})
	}
}

func TestSchemeTable(t *testing.T) {
	cases := []struct {
		name, dest, want string
		ok               bool
	}{
		{"http", "http://x", "http", true},
		{"upper case is kept", "HTTP://x", "HTTP", true},
		{"mailto", "mailto:a@b", "mailto", true},
		{"single letter", "a:b", "a", true},
		{"drive letter looks like a scheme", "C:\\x", "C", true},
		{"scheme punctuation", "a+b.c-d:e", "a+b.c-d", true},
		{"nothing after the colon", "x:", "x", true},
		{"plus in the number", "tel:+886", "tel", true},
		{"no colon", "x.md", "", false},
		{"digit first", "1a:b", "", false},
		{"colon first", ":x", "", false},
		{"empty", "", "", false},
		{"protocol relative", "//x", "", false},
		{"fragment", "#f", "", false},
		{"query", "?q", "", false},
		{"space inside", "a b:c", "", false},
		{"full width letter", "ａ:b", "", false},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			got, ok := Scheme(c.dest)
			if got != c.want || ok != c.ok {
				t.Errorf("Scheme(%q) = %q, %v; want %q, %v", c.dest, got, ok, c.want, c.ok)
			}
		})
	}
}
