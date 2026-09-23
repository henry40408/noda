# Architecture

Every module in `src/` opens with a `//!` header recording its decisions — read it before changing
the module. This document covers what no single header can: the shape, the paths through it, and
the rules that hold *between* modules.

## The shape

```
main.rs        clap definitions, one match arm per command, and the only call to cmd::print.
    |
cmd.rs         one function per command. Takes what it needs, returns a String.
    |
notebook.rs    `Notebook` — the single façade over git2. Open, scan, resolve, commit,
               pull/push, log, blame, diff, snapshots, drift against a remote.
note.rs        one note: the filename that carries its identity, and the frontmatter.
    |
link · query · todo        derived from the CommonMark event stream, never from a regex
paths · config · error · style · sign · remote
```

`tui/`, `web/` and `import/` sit on top. None of them reaches past `cmd` to write a note.

## Three paths through it

### `noda add "Meeting notes"`

```
main.rs           Command::Add            → cmd::add(&paths, title, content, tags)
cmd::add          Notebook::open_active   → reads $XDG_STATE_HOME/noda/active, opens the repo
                  note::validate_title    → checked *before* $EDITOR opens: nobody should
                  clean_tags                compose a note only to be told the title is illegal
                  compose_in_editor       → the scratch buffer in $XDG_CACHE_HOME
                  → cmd::add_in(&notebook, …)
cmd::add_in       validates again         → it is reachable on its own; see below
                  note::slugify(title)
                  note::mint_id(notebook.taken_ids()?)
                  note::now()             → created and updated get the same value
                  fs::write(<id>-<slug>.md)
                  notebook.commit(&[file], "add: {slug}")
main.rs           cmd::print(&output)     → the only write to stdout in the crate
```

The commit is part of the command. Nothing in noda writes a note and leaves committing to somebody
else.

### A write from the browser

```
web::new_note     answer(move || { … }).await
                  |
                  └─ inside spawn_blocking, synchronous from here down:
                     server.writing.of(&book).lock()   one write at a time, per notebook
                     let before = notebook.taken_ids()?
                     cmd::add_in(&notebook, …)          ← the same function the CLI calls
                     let after  = notebook.taken_ids()?
                     after.difference(&before)          which note is new
                     → Answer::Elsewhere("/nb/{book}/n/{id}")   303, so a reload is a GET
```

- **It calls `add_in`, not `add`.** The handler already has the notebook open, and `add_in` cannot
  open `$EDITOR`, which a request must never do. Most of `cmd` comes in this pair.
- **It does not read the new id out of what `add_in` returned** — see below.

### One keystroke in the TUI

```
tui::run          loop {
                     terminal.draw(|frame| view::draw(frame, app))   state → frame, a pure function
                     match app.on_key(key) { … }                     state machine → Option<Action>
                  }
                     |
                     └─ perform(…, action)   the only place in tui/ that touches the world
                        Action::Edit    → cmd::edit(&paths, &key, touch)   terminal handed back
                        Action::Tag     → cmd::tag(…)
                        Action::Remove  → cmd::rm(…)
                        Action::Send    → cmd::bulk(…)   the queue, in one commit
                        → whatever the command returned goes in the status band, verbatim
```

`app.rs` opens no file, repository or terminal; everything it wants from the world leaves as an
`Action`, so the whole interaction is testable with no terminal attached.

## One core, two front ends

**A command's return value is prose, not an interface.** A caller that parses it turns the wording
of a message into an API. When a caller needs a fact about what just happened, it asks the
notebook:

```rust
// web/mod.rs                                  // tui/mod.rs
let before = notebook.taken_ids()?;            let before = app.ids().collect();
cmd::add_in(&notebook, …)?;                    cmd::add(paths, …)?;
let after  = notebook.taken_ids()?;            // then reload and diff
after.difference(&before)
```

**Resolving a key is `Notebook::resolve`'s job and nobody else's.** An id prefix naming two notes
must be refused. The TUI holds every note in memory but `Action::Open` still asks the notebook.

**The layer that produces a screen touches nothing.** `tui/app.rs` returns `Action`s; `web/page.rs`
takes what a page is about and returns a string, opening no repository and knowing nothing about
requests. Both exist so what goes on screen can be tested without one.

**One palette, translated twice.** `style.rs` decides what an id looks like; `tui/theme.rs` hands
that to ratatui and `web/theme.rs` restates it in CSS, for light and dark both, because a browser
brings no theme of its own. An id is the same yellow in `noda ls`, the TUI and a browser.

## Concurrency, which exists only in the web server

**`git2::Repository` is `!Send`**, so it cannot be held across an await:

```rust
async fn handler(…) -> Response {
    answer(move || {          // spawn_blocking: the only place a !Send Repository
        …                     // can be created and dropped without crossing an await
    }).await
}
```

Every handler has that shape, and `answer` turns what the closure decided — `Page`, `Elsewhere`,
`Missing`, `Held` — into a response. Opening the notebook per request is what a `!Send` handle
requires.

**Writes take a lock, one per notebook.** Two commits racing in one repository collide at
`index.lock`, and libgit2's "file exists" is no help to somebody who pressed Save. It was once one
global lock, until a notebook with an unresponsive remote froze Save on every other notebook;
`index.lock` is per repository, so the lock is too.

It is a `std::sync::Mutex`, not tokio's, because it is only taken off the async threads. It does
not guard against another process — a terminal in another window can always write. That is what
the per-note fingerprint on every edit form is for. Being a git blob id, it also addresses the
version the edit began from: on a mismatch `web::merge` fetches that version and three-way merges
with `git2::merge_file`, so only an overlap is refused.

**Network errands do not run in a request.** A `POST` to `sync`, `pull` or `push` answers `303`
immediately; `web/work.rs` runs the errand on a plain `std::thread` (the blocking pool is for work
a request waits on), and keeps its outcome, because a page that says nothing after a sync looks like
one that ignored the button.

**`web/watch.rs` is one plain thread for all watches.** An editor with JavaScript holds an SSE
connection to hear that its note moved; a thread per connection could not notice its browser go
away. The one thread walks a registry, and every tick prunes senders whose readers have gone,
since a note nobody edits again is never sent to.

**Shutdown.** `SIGINT` or `SIGTERM` makes `axum::serve` stop accepting and finish what is in
flight. Two things need more. A watch stream never finishes, so `watch::Watch::stop` drops every
sender as shutdown begins. An errand outlives its request, so `serve` ends with
`work::Errands::settle`, a condvar the errand thread wakes on its way out. The wait has no
deadline, because an errand killed mid-commit leaves `index.lock` behind; a second signal ends the
wait (not the errand) and the process exits non-zero.

## Adding to it

**A new command.** In order: a variant in `main.rs`'s `Command` enum, a match arm calling into
`cmd`, the function in `cmd.rs` returning `Result<String>`, its row in README.md's command table,
and a test in `tests/cli.rs`. If a front end will call it with a notebook already open, write the
`foo_in(notebook, …)` half and have `foo(paths, …)` open the active notebook and delegate.

**A new import source.** One parser producing `Incoming`, nothing else. Minting ids, writing files,
resolving links and committing are the shared back end in `import/mod.rs`.

**A new TUI screen.** The chrome is `frame.rs`; `view.rs` draws only the middle band. A screen is
pushed onto the stack in `app.rs` with its own cursor, query and scroll, so going back lands where
you left.

**A new web page.** Markup in `page.rs` as a function returning a string, the route in
`web/mod.rs`'s router, and a handler wrapping its work in `answer(move || …)`. If it writes, take
the notebook's lock first. If it is reachable without a script it must work without one —
`script.rs` may make an answer arrive sooner, never differently.

**Something every page needs.** It goes in `web/asset.rs`, linked rather than inlined: the content
hash in the name, served for a year. The pages are `no-cache` so they always name addresses this
build wrote; the two halves are one decision, and either alone serves a page whose stylesheet 404s.

**A part of a page.** When the script fetches a page to take one region out of it, that region gets
a name in `web::Part`, its own function in `page.rs`, and a branch in the handler; the fetch sends
`x-noda-fragment: <name>`. One route may answer several parts, chosen by what the reader is doing,
never by the route. Two rules: the whole page is *built from* the part (asserted by containment in
`page.rs`, never two renderings that look alike), and the whole page stays a correct answer for an
unknown name or a missing header. The script queries what arrives for the element it wants, so a
server ignoring the header would still be answering — the header is an optimisation, not a
protocol.

## Testing

Seven layers, each catching what the ones above it cannot:

| | what it exists to catch |
| --- | --- |
| `#[cfg(test)]` in `src/**` | units, next to the code |
| `tests/cli.rs` | the command layer, each test in its own XDG root |
| `tests/tui.rs` | what is on a screen — ratatui's test backend, a character buffer |
| `tests/pty.rs` | *layout*: a real pty and `vt100`. A padding on the wrong side, a column sliding left, a card outgrowing 24 rows — each passed every assertion in `tui.rs` |
| `tests/web.rs` | the real binary on a real socket, requests written by hand, because the guard tests need a `Host` that lies |
| `tests/version.rs` | what `build.rs` stamped into the binary at compile time, which no library test can call |
| `e2e/` | a real browser over Gherkin features. Its own workspace, so the root suite never compiles it |

Two harness rules are not optional. **`sign = false` in every test notebook**: the XDG roots are
per-test but git's are not, so libgit2 reads the developer's real `~/.config/git/config`, and
`commit.gpgsign = true` there sends every test commit to gpg. **`Paths::rooted(<temp>)`, not
environment variables**: tests run in parallel and cannot safely mutate process-wide env.

## Where the reasoning lives

- **Module `//!` headers** — why a module is the way it is. Start here.
- **`build.rs`** — where `--version` comes from, and why it is not `Cargo.toml`'s `version`.
- **`Cargo.toml`** — why each dependency is present, what was rejected, and the measurements
  behind it.
- **`README.md`** — the user-facing contract. The reasoning behind it is in [tui.md](tui.md),
  [web.md](web.md), [history.md](history.md) and [importing.md](importing.md).
