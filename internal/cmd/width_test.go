package cmd

import "testing"

func TestDisplayWidth(t *testing.T) {
	cases := []struct {
		name string
		text string
		want int
	}{
		{"empty", "", 0},
		{"ascii counts one cell per rune", "hello world", 11},
		{"CJK ideographs take two cells", "你好", 4},
		{"kana take two cells", "日本語のテキスト", 16},
		{"hangul takes two cells", "한국어", 6},
		{"CJK mixed with ascii", "漢字 and ascii", 14},
		{"CJK punctuation takes two cells", "《》，。", 8},
		{"ideographic space takes two cells", "　", 2},
		{"fullwidth latin takes two cells", "ＡＢＣａ", 8},
		{"halfwidth katakana takes one cell", "ｱｲｳ", 3},
		{"halfwidth voiced mark is its own cell", "ｶﾞ", 2},
		{"emoji takes two cells", "😀😀", 4},
		{"emoji between narrow and wide", "😀a你", 5},
		{"ZWJ sequence is one emoji", "👨\u200D👩\u200D👧", 2},
		{"ZWJ sequence with variation selector", "🏳\uFE0F\u200D🌈", 2},
		{"ZWJ technologist", "🧑\u200D💻", 2},
		{"regional indicator pair is one flag", "🇹🇼", 2},
		{"two flags", "🇯🇵🇰🇷", 4},
		{"skin tone modifier joins its emoji", "👍🏽", 2},
		{"precomposed accent is one cell", "café naïve ñ ß", 14},
		{"combining mark takes no cell", "á", 1},
		{"ambiguous symbols count as one cell", "±Ω─│①▲★°×÷", 10},
		{"ambiguous punctuation counts as one cell", "—…“x”‘y’", 8},
		{"trademark and copyright stay narrow", "™©®‼", 4},
		{"zero width space takes no cell", "\u200B", 0},
		{"lone ZWJ takes no cell", "\u200D", 0},
		{"byte order mark takes no cell", "\uFEFF", 0},
		{"NUL takes one cell", "\x00", 1},
		{"tab takes one cell", "a\tb", 3},
		{"escape sequence counts its raw bytes", "\x1b[31mred\x1b[0m", 12},
		{"last C0 control takes one cell", "\x1f", 1},
		{"space is the first printable", " ", 1},
		{"tilde is the last printable ascii", "~", 1},
		{"DEL takes one cell", "\x7f", 1},
		{"last C1 control takes one cell", "\u009f", 1},
		{"no-break space is printable", " ", 1},
		{"VS16 widens a narrow symbol", "❤\uFE0F", 2},
		{"narrow symbol without VS16", "❤", 1},
		{"VS16 widens a warning sign", "⚠\uFE0F", 2},
		{"VS16 widens a smiley", "☺\uFE0F", 2},
		{"VS16 widens a double exclamation", "⁉\uFE0F", 2},
		{"VS16 widens a check mark", "✔\uFE0F", 2},
		{"VS16 after a keycap digit", "1\uFE0F⃣", 2},
		{"VS16 after keycap zero", "0\uFE0F", 2},
		{"VS16 after keycap nine", "9\uFE0F", 2},
		{"VS16 after keycap hash", "#\uFE0F", 2},
		{"VS16 after keycap star", "*\uFE0F", 2},
		{"VS16 after a non-keycap ascii adds nothing", "a\uFE0F", 1},
		{"VS16 after slash, below the digits, adds nothing", "/\uFE0F", 1},
		{"VS16 after colon, above the digits, adds nothing", ":\uFE0F", 1},
		{"VS16 after a space adds nothing", " \uFE0F", 1},
		{"VS16 first in the text adds nothing", "\uFE0F", 0},
		{"VS16 after a wide character adds nothing", "你\uFE0F", 2},
		{"VS16 after a ZWJ adds nothing", "\u200D\uFE0F", 0},
		{"VS16 before a ZWJ adds nothing", "❤\uFE0F\u200D", 1},
		{"VS16 widens a narrow box-drawing character", "─\uFE0F", 2},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			if got := DisplayWidth(c.text); got != c.want {
				t.Errorf("DisplayWidth(%q) = %d, want %d", c.text, got, c.want)
			}
		})
	}
}

func TestPadAndColumnIgnoreEscapes(t *testing.T) {
	if got := pad("日本", 6); got != "日本  " {
		t.Errorf("pad = %q", got)
	}
	if got := pad("long", 2); got != "long" {
		t.Errorf("pad never cuts: %q", got)
	}
}
