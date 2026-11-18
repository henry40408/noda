package e2e

import (
	"fmt"
	"os"
	"strings"
	"testing"

	"github.com/cucumber/godog"

	"github.com/henry40408/noda/e2e/internal/harness"
)

const defaultFeatures = "features"

// onlyScripted marks a scenario about the shortcut itself (filter-as-you-type,
// polling), which runs only in the scripted pass: with scripts off there is
// nothing to observe. It may be about the shortcut, never the only account of
// a result: every claim about what an answer is must also be made by an
// untagged scenario that goes through the form.
const onlyScripted = "@scripted"

// TestE2E runs every scenario twice, with the page's scripts enabled and
// disabled, and each must pass both ways, so working without JavaScript is a
// property of the whole interface rather than of the scenarios somebody
// remembered. The scenarios share one server and notebook, one at a time.
func TestE2E(t *testing.T) {
	binary, err := harness.EnsureBinary()
	if err != nil {
		t.Fatal(err)
	}
	fmt.Fprintf(os.Stderr, "e2e: browser: %s\n", harness.BrowserChoice())
	server, err := harness.StartServer(binary, harness.SeedFixture)
	if err != nil {
		t.Fatal(err)
	}
	defer server.Stop()

	// Both passes run before failing: which of the two broke is the diagnosis.
	for _, pass := range []struct {
		name    string
		scripts bool
		tags    string
	}{
		{"scripts enabled", true, ""},
		{"scripts disabled", false, "~" + onlyScripted},
	} {
		t.Run(pass.name, func(t *testing.T) {
			suite := godog.TestSuite{
				Name: "noda",
				ScenarioInitializer: func(scenario *godog.ScenarioContext) {
					initializeScenario(scenario, server, pass.scripts)
				},
				Options: &godog.Options{
					Format:      "pretty",
					NoColors:    os.Getenv("CI") != "",
					Output:      os.Stdout,
					Paths:       featurePaths(),
					Tags:        pass.tags,
					Concurrency: 1,
					// Undefined and pending steps fail rather than skip.
					Strict: true,
				},
			}
			if status := suite.Run(); status != 0 {
				t.Fatalf("the suite failed with status %d", status)
			}
		})
	}
}

// featurePaths is E2E_FEATURES, a comma-separated list of feature paths, for
// running one file locally. It defaults to every feature.
func featurePaths() []string {
	value := os.Getenv("E2E_FEATURES")
	if value == "" {
		return []string{defaultFeatures}
	}
	return strings.Split(value, ",")
}
