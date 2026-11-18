package remote

import "testing"

func TestRedactHidesTheUserinfoThatMayBeASecret(t *testing.T) {
	cases := map[string]string{
		"https://token@example.com/a/b.git":         "https://***@example.com/a/b.git",
		"https://user:pw@example.com/a.git":         "https://***@example.com/a.git",
		"https://user:p@ss@example.com/a.git":       "https://***@example.com/a.git",
		"http://token@example.com":                  "http://***@example.com",
		"ssh://git@example.com/a.git":               "ssh://git@example.com/a.git",
		"ssh://git:secret@example.com/a.git":        "ssh://***@example.com/a.git",
		"git@example.com:a/b.git":                   "git@example.com:a/b.git",
		"https://example.com/a@b/c.git":             "https://example.com/a@b/c.git",
		"https://example.com?next=u@x":              "https://example.com?next=u@x",
		"/srv/notes.git":                            "/srv/notes.git",
		"https://token@example.com/a.git#frag@here": "https://***@example.com/a.git#frag@here",
	}
	for url, want := range cases {
		if got := Redact(url); got != want {
			t.Errorf("%s: %s, want %s", url, got, want)
		}
	}
}

func TestRedactTextHidesAPasswordThatHoldsAQuote(t *testing.T) {
	text := "fatal: unable to access 'https://u:ab'cd@h.example/r.git/': 403"
	want := "fatal: unable to access 'https://***@h.example/r.git/': 403"
	if got := RedactText(text); got != want {
		t.Fatalf("got  %q\nwant %q", got, want)
	}
}

func TestRedactTextFindsURLsInsideProse(t *testing.T) {
	text := "fatal: unable to access 'https://tok:en@example.com/a.git/': The requested URL returned error: 403\n" +
		"hint: see ssh://git@example.com/x and \"http://t@h/p\""
	want := "fatal: unable to access 'https://***@example.com/a.git/': The requested URL returned error: 403\n" +
		"hint: see ssh://git@example.com/x and \"http://***@h/p\""
	if got := RedactText(text); got != want {
		t.Fatalf("got  %q\nwant %q", got, want)
	}
}
