# Architecture

Every package in `internal/` opens with a package comment recording its decisions — read it before
changing the package. This document covers what no single comment can: the shape, the paths
through it, and the rules that hold *between* packages.

## The shape

```
cmd/noda          flag parsing (spec.go, cli.go), one case per command (dispatch.go), the env
    |             checks, and the only write of command output to stdout (print.go).
    |
internal/cmd      one function per command. Takes what it needs, returns a string.
    |
internal/notebook `Notebook` — the single façade over git. Open, scan, resolve, commit,
                  pull/push, log, blame, diff, snapshots, drift against a remote.
internal/note     one note: the filename that carries its identity, and the frontmatter.
internal/gitx     the only code that runs `git` (and `ssh` through it): a fixed environment,
    |             fixed config, plumbing or `-z` output only, no hooks.
    |
link · query · todo · markdown   derived from the CommonMark tree, never from a regex
paths · config · style · sign · remote · clock · version
```

`internal/tui`, `internal/web` and `internal/importer` sit on top. Neither front end reaches past
`internal/cmd` to write a note. `importer` is the one other writer, and only `cmd.Import*` calls
it: it validates titles and tags as `cmd` does and commits through `Notebook.Commit`, but keeps
each note's times from the source rather than stamping `updated`.

## Three paths through it

### `noda add "Meeting notes"`

```
cmd/noda          dispatch                → cmd.Add(env, opts)
cmd.Add           notebook.OpenActive     → reads $XDG_STATE_HOME/noda/active, opens the repo
                  note.ValidateTitle      → checked *before* $EDITOR opens: nobody should
                  clean tags                compose a note only to be told the title is illegal
                  compose in the editor   → the scratch buffer in $XDG_CACHE_HOME
                  → cmd.AddIn(notebook, opts)
cmd.AddIn         validates again         → it is reachable on its own; see below
                  note.Slugify(title)
                  note.MintID(notebook.TakenIDs())
                  clock                   → created and updated get the same value
                  os.WriteFile(<id>-<slug>.md)
                  notebook.Commit(files, "add: <slug>")
cmd/noda          print(output)           → the only write to stdout for a command
```

The commit is part of the command. Nothing in noda writes a note and leaves committing to somebody
else.

### A write from the browser

```
web.newNote       s.aim(r, …)             opens this request's own Notebook
                  s.locks.of(book).Lock() one write at a time, per notebook
                  before := notebook.TakenIDs()
                  cmd.AddIn(notebook, …)  ← the same function the CLI calls
                  after  := notebook.TakenIDs()
                  difference(after, before)   which note is new
                  → elsewhere("/nb/<book>/n/<id>")   303, so a reload is a GET
```

- **It calls `AddIn`, not `Add`.** The handler already has the notebook open, and `AddIn` cannot
  open `$EDITOR`, which a request must never do. Most of `cmd` comes in this pair.
- **It does not read the new id out of what `AddIn` returned** — see below.

### One keystroke in the TUI

```
tui.Browse        loop {
                     draw(frame, app)                      state → frame, a pure function
                     action := app.OnKey(key)              state machine → an Action
                  }
                     |
                     └─ Perform(env, foreground, app, action)   the only place in tui that
                        ActionEdit    → cmd.Edit(env, key, touch)  touches the world;
                        ActionTag     → cmd.Tag(…)                 the terminal is handed back
                        ActionRemove  → cmd.Rm(…)                  for $EDITOR
                        ActionSend    → cmd.Bulk(…)   the queue, in one commit
                        → whatever the command returned goes in the status band, verbatim
```

`App` opens no file, repository or terminal; everything it wants from the world leaves as an
`Action`, so the whole interaction is testable with no terminal attached. The drawing is a
`ui.Buffer` of cells flushed to a tcell screen, so tests read the buffer.

## One core, two front ends

**A command's return value is prose, not an interface.** A caller that parses it turns the wording
of a message into an API. When a caller needs a fact about what just happened, it asks the
notebook:

```go
// web                                         // tui
before := notebook.TakenIDs()                  before := app.IDs()
cmd.AddIn(notebook, …)                         cmd.Add(env, …)
after := notebook.TakenIDs()                   // then reload and diff
difference(after, before)
```

**Resolving a key is `Notebook.Resolve`'s job and nobody else's.** An id prefix naming two notes
must be refused. The TUI holds every note in memory but opening one still asks the notebook.

**The layer that produces a screen touches nothing.** `tui.App` returns `Action`s; the
`internal/web` page functions take what a page is about and return a string, opening no repository
and knowing nothing about requests. Both exist so what goes on screen can be tested without one.

**One palette, translated twice.** `style` decides what an id looks like; `tui/theme.go` hands that
to the screen and `web/theme.go` restates it in CSS, for light and dark both, because a browser
brings no theme of its own. An id is the same yellow in `noda ls`, the TUI and a browser.

## Concurrency, which exists only in the web server

**A handler opens its own `Notebook`.** A `Notebook` is a directory and a few cached reads, with no
shared handle, so every request opens one; nothing is held between requests. `internal/gitx` runs
`git` as a child process under the request's context, and a write uses `workContext`, which cannot
be cancelled by a reader who hangs up: a commit that has begun is finished, never abandoned
half-way.

**Writes take a lock, one per notebook.** Two commits racing in one repository collide at
`index.lock`, and git's "file exists" is no help to somebody who pressed Save. It was once one
global lock, until a notebook with an unresponsive remote froze Save on every other notebook;
`index.lock` is per repository, so the lock is too (`internal/web/locks.go`).

It does not guard against another process — a terminal in another window can always write. That is
what the per-note fingerprint on every edit form is for. Being a git blob id, it also addresses the
version the edit began from: on a mismatch the handler fetches that version and three-way merges
with `gitx.MergeFile` (`git merge-file`), so only an overlap is refused.

**Network errands do not run in a request.** A `POST` to `sync`, `pull` or `push` answers `303`
immediately; `internal/web/work.go` runs the errand on its own goroutine and keeps its outcome,
because a page that says nothing after a sync looks like one that ignored the button.

**`internal/web/watch.go` is one goroutine for all watches.** An editor with JavaScript holds an
SSE connection to hear that its note moved; one goroutine walks a registry, and every tick prunes
subscribers whose readers have gone, since a note nobody edits again is never sent to. A slow
reader is dropped messages rather than holding the watcher.

**Shutdown.** `SIGINT` or `SIGTERM` makes the server stop accepting and finish what is in flight.
Two things need more. A watch stream never finishes, so `Watch.Stop` ends every subscription as
shutdown begins. An errand outlives its request, so `Serve` ends with `Errands.Settle`, which
waits for the errand to finish. The wait has no deadline, because an errand killed mid-commit
leaves `index.lock` behind; a second signal ends the wait (not the errand) and the process exits
non-zero.

## Adding to it

**A new command.** In order: a variant in `cmd/noda/spec.go`'s command table, a case in
`dispatch.go` calling into `internal/cmd`, the function there returning `(string, error)`, its row
in docs/commands.md's command table, and a test in `internal/cmd`. If a front end will call it with
a notebook already open, write the `FooIn(notebook, …)` half and have `Foo(env, …)` open the
active notebook and delegate.

**A new import source.** One parser producing `Incoming`, nothing else. Minting ids, writing files,
resolving links and committing are the shared back end in `internal/importer/importer.go`.

**A new TUI screen.** The chrome is `frame.go`; `view.go` draws only the middle band. A screen is
pushed onto the stack in `app.go` with its own cursor, query and scroll, so going back lands where
you left.

**A new web page.** Markup in `page*.go` as a function returning a string, the route in
`routes.go`, and a handler returning an `answer`. If it writes, take the notebook's lock first. If
it is reachable without a script it must work without one — the scripts may make an answer arrive
sooner, never differently.

**Something every page needs.** It goes in `internal/web/asset.go`, linked rather than inlined: the content
hash in the name, served for a year. The pages are `no-cache` so they always name addresses this
build wrote; the two halves are one decision, and either alone serves a page whose stylesheet 404s.

**A part of a page.** When the script fetches a page to take one region out of it, that region gets
a name as a `fragment` in `internal/web/answer.go`, its own function in `page*.go`, and a branch in the handler; the fetch sends
`x-noda-fragment: <name>`. One route may answer several parts, chosen by what the reader is doing,
never by the route. Two rules: the whole page is *built from* the part (asserted by containment in the
page tests, never two renderings that look alike), and the whole page stays a correct answer for an
unknown name or a missing header. The script queries what arrives for the element it wants, so a
server ignoring the header would still be answering — the header is an optimisation, not a
protocol.

## Testing

Six layers, each catching what the ones above it cannot:

| | what it exists to catch |
| --- | --- |
| unit tests next to the code | units, each case with its input and expected output written inline |
| `internal/cmd` and `cmd/noda` CLI tests | the command layer, each test in its own XDG root |
| `internal/tui` Buffer tests | what is on a screen — a character buffer, no terminal |
| pty tests in `cmd/noda` | *layout*: a real pty and `vt10x`. A padding on the wrong side, a column sliding left, a card outgrowing 24 rows — each passed every assertion on the buffer |
| `internal/web` and `cmd/noda` web tests | the handlers through `httptest`, and the real binary on a real socket, requests written by hand because the guard tests need a `Host` that lies |
| `e2e/` | a real browser over Gherkin features. Its own module, so the root suite never compiles it |

Two harness rules are not optional. **`sign = false` in every test notebook, and `HOME`,
`GIT_CONFIG_GLOBAL` and `XDG_CONFIG_HOME` inside `t.TempDir()`**: git reads the developer's real
config, and `commit.gpgsign = true` there sends every test commit to gpg. **`paths.Rooted(<temp>)`,
not environment variables**: tests run in parallel and cannot safely mutate process-wide env.

## Where the reasoning lives

- **Package comments** — why a package is the way it is. Start here.
- **`internal/gitx/env.go`** — which user settings are neutralised, and why each one.
- **`go.mod` and `scripts/check-dep-age.sh`** — what the dependencies are, and the 168-hour rule.
- **`docs/commands.md`** — the user-facing contract. The reasoning behind it is in [tui.md](tui.md),
  [web.md](web.md), [history.md](history.md) and [importing.md](importing.md).
