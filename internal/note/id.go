package note

import (
	"crypto/rand"
	"strings"
)

// crockford is Crockford base32: no `i`, `l`, `o` or `u`, so an id can't be
// misread.
const crockford = "0123456789abcdefghjkmnpqrstvwxyz"

// IDLen is 40 bits, minted against what the notebook holds.
const IDLen = 8

// charsPerDraw is the base32 characters one 60-bit draw supplies.
const charsPerDraw = 12

// IsIDShaped reports whether text is long enough and entirely from the id
// alphabet.
func IsIDShaped(text string) bool {
	if len(text) < IDLen {
		return false
	}
	for i := 0; i < len(text); i++ {
		if !strings.ContainsRune(crockford, rune(asciiLower(text[i]))) {
			return false
		}
	}
	return true
}

func asciiLower(b byte) byte {
	if b >= 'A' && b <= 'Z' {
		return b + 'a' - 'A'
	}
	return b
}

// NormalizeID folds Crockford's interchangeable characters: `I` and `L` are
// `1`, `O` is `0`, case never matters.
func NormalizeID(input string) string {
	var out strings.Builder
	for _, ch := range input {
		for _, lower := range lowerCase(ch) {
			switch lower {
			case 'i', 'l':
				lower = '1'
			case 'o':
				lower = '0'
			}
			out.WriteRune(lower)
		}
	}
	return out.String()
}

// MintID returns an id not in taken (which holds folded ids), lengthened if
// the space fills.
func MintID(taken map[string]struct{}) string {
	for length := IDLen; ; length++ {
		for range 64 {
			candidate := randomID(length)
			if _, used := taken[NormalizeID(candidate)]; !used {
				return candidate
			}
		}
	}
}

func randomID(length int) string {
	id := make([]byte, 0, length)
	var bits uint64
	for n := range length {
		// Redraw every 12 characters, or the shift feeds in zeros.
		if n%charsPerDraw == 0 {
			bits = randomBits()
		}
		id = append(id, crockford[bits&0x1f])
		bits >>= 5
	}
	return string(id)
}

func randomBits() uint64 {
	var raw [8]byte
	// crypto/rand does not return an error on supported platforms: it crashes
	// the program instead, which is what an unusable RNG deserves.
	_, _ = rand.Read(raw[:])
	var bits uint64
	for _, b := range raw {
		bits = bits<<8 | uint64(b)
	}
	return bits
}
