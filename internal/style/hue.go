package style

// Hues is how many colours an identity is drawn in. A medium keeps one colour
// per index; style only says which index.
const Hues = 12

// Hue is which of the Hues colours an identity wears: a note's id, a
// notebook's name. It is a pure function of the text, with nothing stored, so
// a note is the same colour in every list, link and margin on every machine,
// for as long as its id lives.
//
// The sum is kept under 9973 so the arithmetic cannot overflow, and 31 spreads
// ids that differ in a single trailing character over different colours.
func Hue(identity string) int {
	const modulus = 9973
	sum := 0
	for _, char := range identity {
		sum = (sum*31 + int(char)) % modulus
	}
	return sum % Hues
}
