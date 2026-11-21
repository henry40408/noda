# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

noda is a git-native notebook CLI: notes are Markdown files in an ordinary git repository, one
notebook per repo. Everything is a library (`internal/`); `cmd/noda` is a thin shell, so commands
are tested without spawning a process.

**`docs/commands.md` is a contract, not a description.** It was written spec-first, before any
code, when it was the README's command reference. A change to user-facing behaviour updates it in the same change; where the
contract turned out wrong, correct it rather than leave it behind. README.md is the front page —
Features, Quick Start, a command overview — and changes when those do.

## Commands

```sh
go test -mod=vendor -race ./...            # the suite
go test -mod=vendor ./internal/cmd -run TestAddWritesTheId   # one test by name
golangci-lint fmt --diff ./...             # formatting; `run` does not check it
golangci-lint run ./...                    # errcheck, govet, ineffassign, staticcheck, unused
scripts/bench-coldstart.sh                 # times whole processes, not in-process code
```

Dependencies are vendored (`go mod vendor`; CI fails on drift) and `scripts/check-dep-age.sh`
rejects one younger than 168 hours.

Lint config lives in `.golangci.yml`. Suppress a finding with `//nolint:<linter> // reason`.
`golangci-lint run` does not check formatting, so `golangci-lint fmt --diff` is a separate command.
CI installs the linter through `golangci/golangci-lint-action` with no pinned version, so the linter
release comes from the action and dependabot tracks only the action.

`e2e/` is **its own Go module** (own `go.mod` and `vendor/`), so root `./...` never compiles it and
root lint does not cover it; run both `golangci-lint` commands inside `e2e/` too (CI does). Its `go` and
`toolchain` lines must equal the root's. It drives the real binary in Chrome through godog and
chromedp:

```sh
cd e2e && go test -mod=vendor -count=1 -timeout 25m ./...   # needs Chrome
```

The README's four web screenshots are generated, not taken by hand. A change to what `noda web`
renders (`internal/web/page*.go`, `theme.go`, the assets) is committed with the regenerated images:

```sh
cd e2e && go run -mod=vendor ./cmd/screenshots   # writes ../screenshots/
```

Test coverage: `NODA_COVERDIR=$PWD/covdata go test -mod=vendor -coverprofile=coverage.out ./...`
(the `cmd/noda` tests then build the binary with `-cover`); mutation testing is `gremlins` with
the threshold in `.gremlins.yaml`.

**Mutation testing is sharded.** One whole-module run exhausted the runner's memory (exit 143,
"runner received a shutdown signal"), so CI runs nine `mutation-shard (<name>)` matrix jobs
(`core`, `cmd-a`, `cmd-b`, `cmd-c`, `tui-app`, `tui-view`, `web-listing`, `web-forms`, `web-render`,
`web-rest`; each under ~15 minutes, because the push run waits for the slowest) and a `mutation` job
that `needs` all of them. Each shard runs `GOMAXPROCS=2 gremlins unleash --workers 2` under
`ulimit -v 4194304` with an `--exclude-files` regexp naming everything outside it (RE2 has no
lookahead, so the matrix lists the complement; the flag replaces `.gremlins.yaml`'s list, so every
entry repeats `e2e/` and `vendor/`). Each shard must reach the 95 threshold by itself. Never pass
`--test-cpu` (gremlins v0.6.0 builds a bad `-cpu 2` argument and every mutant then counts as
killed) or `--threshold-efficacy` (ignored on the command line). **A new package, or a new file
in `internal/cmd`, `internal/tui` or `internal/web` (split by file), needs a place in the matrix** in
`.github/workflows/ci.yml`: the shards must stay disjoint and complete, which `unleash --dry-run`
per shard summing to the whole module's `Runnable` count checks. Packages that scan text in loops
(`config`, `importer`, `link`, `note`, `query`, `tui`) have a `TestMain` watchdog (`main_test.go`) that
exits a test binary stuck in a loop or holding over 512 MiB, because gremlins kills `go test` on a
timeout but not the binary it started.

## Architecture

**`docs/ARCHITECTURE.md` is the reference** — the shape, three worked paths (a CLI command, a
browser write, a TUI keystroke), the concurrency model, and where to add a command / import source
/ screen / page. In short:

```
cmd/noda → internal/cmd → internal/notebook   the single façade over git
                          internal/note       the filename that carries a note's identity
                          internal/gitx       the only place that runs `git`
link · query · todo · markdown               read from the CommonMark tree, never grepped
paths · config · style · sign · remote · clock · version
```

`internal/tui`, `internal/web` and `internal/importer` sit on top; neither front end reaches past
`internal/cmd` to write a note, and `importer` writes only when `internal/cmd` calls it.

**git rules.** Every git operation goes through `internal/gitx`.

- Use plumbing, or porcelain with `-z` output; never parse output meant for people.
- Never run a command that triggers a hook.
- Adding a git call: decide whether it needs a setting in `fixedConfig` (`internal/gitx/env.go`),
  the list of user settings that would change the output noda parses.
- The oldest supported git is 2.39 (`gitx.MinVersion`); the `min-git` CI job runs on it.

## Invariants

These hold *between* packages, so no single file states them. Breaking one is how the two front
ends drift apart; `docs/ARCHITECTURE.md` explains each in context.

1. **A command returns its answer; it does not emit it.** Every `internal/cmd` entry point returns
   `(string, error)`. Command output reaches stdout only through `print` in `cmd/noda`, called
   once, so a redirected `noda show` is byte-for-byte the file; `noda web` and `noda tui` write
   their own terminal output.

2. **What a command returns is prose, not an interface.** A caller that parses it turns a message's
   wording into an API. To learn what just happened, ask the notebook — both front ends diff
   `TakenIDs()` around the call rather than reading an id out of the answer.

3. **Commands come in pairs: `Foo(env, …)` and `FooIn(notebook, …)`.** The `In` half takes an
   open notebook and never opens `$EDITOR`; `noda web` calls it, because a request must not open an
   editor.

4. **Nothing outside `internal/cmd` writes a note.** Validating a title, stamping `updated` and
   committing happen in one place, so a change means the same thing however it was asked for. The
   one exception is `importer`, reached only through `cmd.Import*`: it validates the same way and
   commits through `Notebook.Commit`, but keeps the source's times instead of stamping.

5. **The layer that produces a screen touches nothing.** `tui.App` returns `Action`s and opens no
   file, repository or terminal; the `internal/web` page functions return strings and know nothing
   about requests. So the interesting half is testable with no terminal and no socket.

6. **A web handler opens its own `Notebook` and takes the notebook's lock to write.** One write at
   a time per notebook; `internal/web/work.go` runs sync, pull and push on their own goroutine,
   because no request waits on them.

7. **Markdown is parsed, never grepped** — `link`, `todo`, `web/render.go`. `- [ ]` inside a
   code fence is not a checkbox and a link inside one is not a link; grepping would report a
   referenced file as an orphan.

8. **Colour is decided once, in `internal/style`.** `tui/theme.go` and `web/theme.go` translate
   that decision for their medium; they do not make one.

## Data model

A note is `<id>-<slug>.md`. **The filename is the identity and nothing else records it** — there is
no index file, and one should not be introduced. git forbids two entries under one path in a tree,
so uniqueness is structural, and two machines that each add a note write two different filenames
that merge without conflict.

The frontmatter carries `title`, `tags`, `created`, `updated` and `pinned` (absent rather than
`false` when not pinned). Its *presence* is what marks a file as a note (`notebook.Scan`). noda
interprets those fields only, but does not own the block: any other field survives a write-back
untouched.

## The web layer

Server-rendered and works with JavaScript off — the search box is a form, every row is a link.
The script is the only part allowed to be absent, and **the script may answer sooner or not at
all, never differently.** So the listing filter may be narrower than the server, never wider; it
stands aside entirely for a negated bare word, which it would widen (it cannot see bodies).
`internal/web/guard.go` holds the `Origin` and `Host` checks — with no accounts, those are the
whole defence against cross-site writes and DNS rebinding. `internal/web/log.go` names a request
only by the route template it matched, never by the path, because a path carries somebody's note
id or filename.

## Testing

Six layers: unit tests next to the code; the CLI tests in `internal/cmd` and `cmd/noda` for the
command layer; `internal/tui` Buffer tests for screens (a character buffer, no terminal); the pty
tests in `cmd/noda` for *layout* (a real pty and `vt10x`, catching what a character buffer is blind
to); `internal/web` and `cmd/noda` web tests for the real handlers and the real binary on a real
socket; and `e2e/` for a real browser.`docs/ARCHITECTURE.md` says what each catches; put a new test in the cheapest layer that can
actually fail on the bug.

Two things are not optional:

- **`sign = false` in every test notebook's `config.toml`, and `HOME`, `GIT_CONFIG_GLOBAL` and
  `XDG_CONFIG_HOME` pointed into `t.TempDir()`.** Git reads the developer's real config, and a
  machine with `commit.gpgsign = true` sends every test commit to gpg.
- **`paths.Rooted(<temp dir>)`, not process-wide environment variables.** Tests run in parallel
  and cannot safely mutate them (`t.Setenv` is for serial tests only).

## Conventions

- **Package comments are where design decisions are recorded** — read a package's header before
  changing it. Dependencies are few; adding one is a decision to justify.
- **A comment states the decision and its reason, and stops.** A rejected alternative, a
  measurement or the failure a rule prevents is worth a clause; restating them, or restating what
  the code plainly says, is not. Where a name and signature already answer the question, write no
  comment.
- **`cmd/noda/spec.go` holds the `--help` text.** Editing it changes what the CLI prints, which is
  user-facing behaviour and belongs in `docs/commands.md` with it.
- **Startup time is a feature.** A quick `noda ls` costs more in process startup than in work, so
  anything that grows the binary is measured, not assumed.
- Colour is emitted unconditionally; `internal/style` strips it when output is not a terminal, so a
  piped `noda show` emits exactly the bytes on disk. NUL-separated output (`ls -q0`) is written
  raw.
- **Environment variables.** `NODA_LOG_LEVEL` (`debug|info|warn|error`, default `info`) is env-only, with
  no flag; `--log-format` has the env fallback `NODA_LOG_FORMAT`. `RUST_LOG` is a retired variable
  and fails startup with a rename hint. `ensureNoLegacyEnvVars` (`cmd/noda/env.go`) does that:
  update `legacyEnvVars` there whenever an environment variable is added, renamed or removed — a
  silently ignored variable is worse than a startup failure.
- **Breaking change of the Go release: `RUST_LOG` is retired.** A deployment that sets it (compose,
  k8s, a shell profile) now exits at startup, and `restart: always` turns that into a crash loop.
  Release notes are generated, so the PR title carries it: mark it breaking (`feat!:`) and name
  `RUST_LOG -> NODA_LOG_LEVEL` in the body. The version number is chosen after the PR merges.
- **GitHub Actions are pinned by commit SHA**, with the tag as a trailing comment.
- Releases use `gh release create --generate-notes`; never edit a manifest version or add a
  `CHANGELOG.md`.

## Accepted differences from the Rust version

- **`noda blame` across a rename that also rewrites the file.** The Rust version followed a note by id, so
  it credited lines kept through a large rewrite to the note's first commit. Go delegates to
  `git blame`, which follows by content and credits them to the rewriting commit. The user accepted
  this and no boundary mapping is implemented. `TestRenameRewriteBlameDiffersFromRustAsKnown`
  pins the Go result and its comment records what Rust credited.
- **ssh may prompt in the CLI.** The command line sets `Interactive` when stdin is a terminal, so
  ssh can ask for a passphrase or host key; the web server and the TUI never do.

