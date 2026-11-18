package cmd

import (
	"fmt"
	"os"

	"github.com/henry40408/noda/internal/importer"
)

// ImportTiddlywiki reads TiddlyWiki 5 exports into the active notebook.
//
// Several files are one import, so links between the pieces resolve: every
// file is read before anything is written, and an unreadable one stops the
// import before it touches the notebook.
func ImportTiddlywiki(e Env, files []string, convert bool) (string, error) {
	request := importer.Request{Source: "tiddlywiki"}
	for _, file := range files {
		export, err := readExport(file)
		if err != nil {
			return "", err
		}
		request.Incoming = append(request.Incoming, export.Notes...)
		request.Skipped = append(request.Skipped, export.Skipped...)
	}
	if convert {
		request.Convert = importer.Convert
	}
	active, err := e.openActive()
	if err != nil {
		return "", err
	}
	return importer.Write(active, request)
}

func readExport(file string) (importer.Export, error) {
	text, err := os.ReadFile(file)
	if err != nil {
		return importer.Export{}, fmt.Errorf("%s: %s", file, importer.Reason(err))
	}
	export, err := importer.ReadTiddlyWiki(string(text))
	if err != nil {
		return importer.Export{}, fmt.Errorf("%s: %w", file, err)
	}
	return export, nil
}
