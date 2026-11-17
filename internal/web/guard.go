package web

import (
	"fmt"
	"net/netip"
	"strings"
)

// Who is allowed to be talking to this server.
//
// There are no accounts: the server is meant for a tailnet or behind an
// authenticating proxy. These checks are the whole defence against the two
// attacks that borrow a browser you already have open:
//
// Cross-site requests. A form on any site can post to localhost:8080, and with
// no session cookie to be missing, only Origin stops it.
//
// DNS rebinding, where evil.example resolves to 127.0.0.1 and both headers
// agree. The attack needs a name, so a bare address always passes and a name
// must be one given with --allow-host (needed behind a proxy or on a tailnet).
// It fails closed and says what to add.

// Refusal is why a request was turned away, in the words the reader needs to
// fix it.
type Refusal string

func (r Refusal) Error() string { return string(r) }

// Guard holds the hostnames this server answers to.
type Guard struct {
	allowed []string
}

// NewGuard takes the values of --allow-host.
func NewGuard(extra []string) Guard {
	allowed := make([]string, len(extra))
	for i, name := range extra {
		allowed[i] = strings.ToLower(name)
	}
	return Guard{allowed: allowed}
}

// Admits decides a request by its Host and Origin headers. An empty host is a
// missing Host, which is refused (HTTP/1.1 requires it); a nil origin is not
// refused, as ordinary navigation omits it.
func (g Guard) Admits(host string, origin *string) error {
	if host == "" {
		return Refusal("the request carried no Host header")
	}
	if err := g.admitsHost(host); err != nil {
		return err
	}
	if origin == nil {
		return nil
	}
	// A sandboxed frame or file:// page sends "null"; it would fail the
	// comparison anyway, but deserves its own message.
	if *origin == "null" {
		return Refusal("the request came from an opaque origin, which cannot be checked")
	}
	if asciiEqualFold(authority(*origin), host) {
		return nil
	}
	return Refusal(fmt.Sprintf("the request says it came from %s, which is not %s — a page on another site cannot make changes here", *origin, host))
}

func (g Guard) admitsHost(host string) error {
	name := hostname(host)
	// An address cannot be rebound, and resolvers pin localhost.
	if isAddress(name) || asciiEqualFold(name, "localhost") {
		return nil
	}
	for _, allowed := range g.allowed {
		if asciiEqualFold(allowed, name) {
			return nil
		}
	}
	return Refusal(fmt.Sprintf("this server was not told to answer to the name %s — start it with `--allow-host %s` if that is where you meant to reach it", name, name))
}

func isAddress(name string) bool {
	address, err := netip.ParseAddr(name)
	return err == nil && address.Zone() == ""
}

// asciiEqualFold folds ASCII only: strings.EqualFold would also match a
// character such as the Kelvin sign to k, and a host check must not.
func asciiEqualFold(a, b string) bool {
	if len(a) != len(b) {
		return false
	}
	for i := 0; i < len(a); i++ {
		if lowerASCII(a[i]) != lowerASCII(b[i]) {
			return false
		}
	}
	return true
}

func lowerASCII(b byte) byte {
	if b >= 'A' && b <= 'Z' {
		return b + 'a' - 'A'
	}
	return b
}

// authority is the host:port out of an origin; the port is kept, since it is
// part of the origin.
func authority(origin string) string {
	if _, rest, found := strings.Cut(origin, "://"); found {
		origin = rest
	}
	return strings.TrimRight(origin, "/")
}

// hostname is the name out of a host:port, including a bracketed [::1]:8080.
func hostname(host string) string {
	if rest, bracketed := strings.CutPrefix(host, "["); bracketed {
		inside, _, _ := strings.Cut(rest, "]")
		return inside
	}
	name, _, _ := strings.Cut(host, ":")
	return name
}
