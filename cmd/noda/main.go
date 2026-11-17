// Command noda is a git-native notebook.
package main

import (
	"fmt"
	"io"
	"os"

	"github.com/henry40408/noda/internal/version"
)

func main() {
	os.Exit(run(os.Args[1:], os.Stdout, os.Stderr))
}

// run is separate from main so tests can call it without a process.
func run(args []string, stdout, stderr io.Writer) int {
	if len(args) == 1 && (args[0] == "--version" || args[0] == "-V") {
		fmt.Fprintf(stdout, "noda %s\n", version.String())
		return 0
	}
	fmt.Fprintln(stderr, "noda: this Go build only supports --version so far")
	return 1
}
