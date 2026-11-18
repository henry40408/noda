package gitx

import (
	"bytes"
	"fmt"
	"strings"
	"time"
)

// Ident is who did something and when, as it stands in a commit object.
type Ident struct {
	Name, Email string
	When        time.Time
}

// String is `Name <email> <unix seconds> <±hhmm>`.
func (i Ident) String() string {
	return fmt.Sprintf("%s <%s> %s", i.Name, i.Email, i.RawDate())
}

// RawDate is `<unix seconds> <±hhmm>`, the form git takes in
// GIT_COMMITTER_DATE as well as writes in an object.
func (i Ident) RawDate() string {
	_, offset := i.When.Zone()
	direction := '+'
	if offset < 0 {
		direction = '-'
		offset = -offset
	}
	return fmt.Sprintf("%d %c%02d%02d", i.When.Unix(), direction, offset/3600, offset%3600/60)
}

// CommitObject is a commit before it is stored. noda builds the object itself
// so it can have gpg sign the exact bytes and so no hook runs.
type CommitObject struct {
	Tree      string
	Parents   []string
	Author    Ident
	Committer Ident
	// Message is written as given: no newline is added.
	Message string
}

// Encode is the object git stores: headers, a blank line, the message.
func (c CommitObject) Encode() []byte {
	var out bytes.Buffer
	out.WriteString("tree " + c.Tree + "\n")
	for _, parent := range c.Parents {
		out.WriteString("parent " + parent + "\n")
	}
	out.WriteString("author " + c.Author.String() + "\n")
	out.WriteString("committer " + c.Committer.String() + "\n")
	out.WriteString("\n")
	out.WriteString(c.Message)
	return out.Bytes()
}

// WithSignature puts a `gpgsig` header after the others. Every line of the
// signature after the first continues the header, so it starts with a space;
// the signature ends in a newline, which leaves a line holding one space
// before the blank line. That is the shape git writes and libgit2 copies.
func WithSignature(encoded []byte, armored []byte) []byte {
	headerEnd := bytes.Index(encoded, []byte("\n\n"))
	if headerEnd < 0 {
		return encoded
	}
	headers, rest := encoded[:headerEnd+1], encoded[headerEnd+1:]
	var out bytes.Buffer
	out.Write(headers)
	out.WriteString("gpgsig ")
	out.WriteString(strings.ReplaceAll(string(armored), "\n", "\n "))
	out.WriteString("\n")
	out.Write(rest)
	return out.Bytes()
}
