package remote

import "testing"

func TestSplitCredentialsTakesTheUserinfoOutOfAnHTTPURL(t *testing.T) {
	cases := []struct {
		raw, clean  string
		credentials Credentials
	}{
		{"https://tok@example.com/a.git", "https://example.com/a.git", Credentials{Username: "tok"}},
		{"https://u:p%40ss@example.com/a.git", "https://example.com/a.git", Credentials{Username: "u", Password: "p@ss"}},
		{"http://u:p@ss@example.com:8080/a@b.git?x=1", "http://example.com:8080/a@b.git?x=1", Credentials{Username: "u", Password: "p@ss"}},
		{"https://u:bad%zz@example.com/", "https://example.com/", Credentials{Username: "u", Password: "bad%zz"}},
	}
	for _, c := range cases {
		clean, credentials, found := SplitCredentials(c.raw)
		if !found || clean != c.clean || credentials != c.credentials {
			t.Errorf("%s: %q %+v %v", c.raw, clean, credentials, found)
		}
	}
}

func TestSplitCredentialsLeavesWhatHoldsNoSecretOverHTTP(t *testing.T) {
	for _, raw := range []string{
		"https://example.com/a@b.git", "ssh://git@example.com/a.git", "git@example.com:a/b.git", "/srv/repo.git", "",
	} {
		clean, credentials, found := SplitCredentials(raw)
		if found || clean != raw || credentials != (Credentials{}) {
			t.Errorf("%q: %q %+v %v", raw, clean, credentials, found)
		}
	}
}

func TestAnEmptyUserinfoIsStillTakenOut(t *testing.T) {
	clean, credentials, found := SplitCredentials("https://@example.com/a")
	if !found || clean != "https://example.com/a" || credentials != (Credentials{}) {
		t.Errorf("%q %+v %v", clean, credentials, found)
	}
}

func TestAnAtSignInThePathOfAHostlessURLIsNotUserinfo(t *testing.T) {
	const raw = "https:///x@y"
	if clean, _, found := SplitCredentials(raw); found || clean != raw {
		t.Errorf("%q %v", clean, found)
	}
}
