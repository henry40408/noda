# Architecture

Every module in `src/` opens with a `//!` header explaining the decisions inside it, and they are
thorough — read the header before changing a module. What none of them can state is the rules that
hold *between* modules, because each one only speaks for itself.

That is what this document is for: the shape, the paths through it, and the places where two
modules agree on something neither can enforce alone.

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

The layering is easiest to see by following something all the way down.

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

The commit is part of the command, not a step after it. Nothing in noda writes a note to disk and
leaves committing to somebody else.

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

Two things in that are not obvious:

- **It calls `add_in`, not `add`.** The handler already has the notebook open — a second handle on
  the same repository is the thing the `_in` half exists to avoid — and `add_in` cannot open
  `$EDITOR`, which a request must never do. Most of `cmd` comes in this pair.
- **It does not read the new id out of what `add_in` returned.** See below; the TUI does the same
  thing the same way.

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
`Action`. That is what lets the whole interaction be tested with no terminal attached — which
matters more here than anywhere else in noda, because every other command is a function returning a
string and this one is a loop.

## One core, two front ends

Both front ends are built out of `cmd`, and the rules that keep them from drifting are rules
neither can enforce on its own.

**A command's return value is prose, not an interface.** It is a sentence written for a person, and
a caller that parses it has quietly turned the wording of a message into an API. So when a caller
needs a fact about what just happened, it asks the notebook — and both front ends independently
arrived at the same shape for the same question:

```rust
// web/mod.rs                                  // tui/mod.rs
let before = notebook.taken_ids()?;            let before = app.ids().collect();
cmd::add_in(&notebook, …)?;                    cmd::add(paths, …)?;
let after  = notebook.taken_ids()?;            // then reload and diff
after.difference(&before)
```

**Resolving a key is `Notebook::resolve`'s job and nobody else's.** An id prefix that names two
notes has exactly one right answer, and it is a refusal. The TUI holds every note in memory and
could plausibly answer from there — so `Action::Open` deliberately does not, and asks the notebook
instead.

**The layer that produces a screen touches nothing.** `tui/app.rs` and `web/page.rs` follow the
same rule for the same reason: `page.rs` takes what a page is about and returns a string, opening
no repository and knowing nothing about requests. The interesting half of an interface is what it
puts on the screen, and that is worth being able to test without one.

**One palette, translated twice.** `style.rs` decides what an id looks like. `tui/theme.rs` hands
that decision to ratatui, and `web/theme.rs` restates it in CSS — twice over, because a terminal
brings its own theme and a browser does not, so light and dark both have to be written down. An id
is the same yellow in `noda ls`, in the TUI and in a browser because it is the same thing being
named.

## Concurrency, which exists only in the web server

Everywhere else noda is one process doing one thing. `noda web` is the exception, and its whole
model follows from one fact: **`git2::Repository` is `!Send`.** It cannot be held across an await,
so it cannot live in an async handler at all.

```rust
async fn handler(…) -> Response {
    answer(move || {          // spawn_blocking: the only place a !Send Repository
        …                     // can be created and dropped without crossing an await
    }).await
}
```

Every handler has that shape, and `answer` turns what the closure decided — `Page`, `Elsewhere`,
`Missing`, `Held` — into a response. Opening the notebook per request is not overhead worked
around; it is what a `!Send` handle requires.

**Writes take a lock, one per notebook.** Two commits racing in one repository meet at
`index.lock`, and what comes back is libgit2 saying a file exists — a true statement about a lock
file and no help to somebody who pressed Save. It was a single lock over everything until a network
errand held one for as long as a network takes: a notebook whose remote had gone quiet froze Save
on every *other* notebook too. `index.lock` is a file inside one repository, so the lock belongs
where the collision is.

It is a `std::sync::Mutex`, not tokio's, because it is only ever taken off the async threads. And
it does not lock the notebook against the world — a terminal in another window is writing to the
same repository and always could be. That is what the per-note fingerprint (the optimistic lock on
every edit form) is for.

**Network errands do not run in a request at all.** `sync`, `pull` and `push` take as long as
somebody's tailnet does. A `POST` starts one and answers `303` immediately; `web/work.rs` runs it
on a plain `std::thread` — the blocking pool is for work a request is waiting on, and this is
precisely the work no request waits on — and the outcome outlives the errand, because a page that
says nothing after a sync looks exactly like a page that ignored the button.

## Adding to it

**A new command.** Five places, in this order: a variant in `main.rs`'s `Command` enum (clap
derives the parsing and the help), a match arm calling into `cmd`, the function in `cmd.rs`
returning `Result<String>`, its row in README.md's command table, and a test in `tests/cli.rs`. If
a front end will call it with a notebook already open, write it as the `foo_in(notebook, …)` half
with `foo(paths, …)` opening the active notebook and delegating.

**A new import source.** One parser producing `Incoming`, and nothing else. Minting ids, writing
files, resolving links between them and committing are the shared back end in `import/mod.rs` —
they are the same work whatever the notes came from.

**A new TUI screen.** The chrome is `frame.rs` and applies to every screen; `view.rs` draws only
the middle band. A screen is pushed onto the stack in `app.rs`, keeping its own cursor, query and
scroll, so going back lands where you left.

**A new web page.** The markup goes in `page.rs` as a function returning a string, the route in the
router in `web/mod.rs`, and the handler wraps its work in `answer(move || …)`. If it writes, take
the notebook's lock first. If it is reachable without a script, it must work without one — the
enhancement layer in `script.rs` may make an answer arrive sooner, never differently.

**Something every page needs.** It belongs in `web/asset.rs`, linked rather than written into the
markup: one address per thing, the content's own hash in the name, served for a year and never
asked for twice. The pages are `no-cache` so that they always name addresses this build wrote — the
two halves are one decision, and either alone serves somebody a page whose stylesheet is a 404.

**A part of a page.** When the script fetches a page to take one region out of it, that region gets
a name in `web::Part`, a function of its own in `page.rs`, and a branch in the handler; the fetch
sends `x-noda-fragment: <name>`. One route may answer several — the listing sends its column to a
search and both of its panes to a press of back — and which is asked for is decided by what the
reader is doing, never by the route. Two rules hold it together. The whole page must be *built from* the
part — one rendering, asserted by containment in `page.rs`, never two that look alike — and the
whole page must stay a correct answer, because an unknown name, a missing header and a reader typing
the address all get it. That is what keeps the header an optimisation rather than a protocol: the
script parses what arrives and queries it for the element it wants, so a server that ignored the
header entirely would still be answering.

## Testing

Six layers, each catching what the ones above it cannot:

| | what it exists to catch |
| --- | --- |
| `#[cfg(test)]` in `src/**` | units, next to the code |
| `tests/cli.rs` | the command layer, each test in its own XDG root |
| `tests/tui.rs` | what is on a screen — ratatui's test backend, a character buffer |
| `tests/pty.rs` | *layout*: a real pty and `vt100`. A padding on the wrong side, a column sliding left, a card outgrowing 24 rows — each passed every assertion in `tui.rs` |
| `tests/web.rs` | the real binary on a real socket, requests written by hand, because the guard tests need a `Host` that lies |
| `e2e/` | a real browser over Gherkin features. Its own workspace, so the root suite never compiles it |

Two harness facts that are not optional. **`sign = false` in every test notebook**: the XDG roots
are per-test but git's are not, so libgit2 reads the developer's real `~/.config/git/config` and a
machine with `commit.gpgsign = true` sends every test commit to gpg. **`Paths::rooted(<temp>)`
rather than environment variables**: tests run in parallel and cannot safely mutate process-wide
env.

## Where the reasoning lives

- **Module `//!` headers** — why a module is the way it is. Start here.
- **`Cargo.toml`** — why each dependency is present, what was rejected, and the measurements
  behind it (`env-filter` was dropped for `Targets` after measuring it at 355 KB, 69% of the whole
  of the logging).
- **`README.md`** — the user-facing contract, written spec-first before the code existed. It
  says what each command does; the four documents beside this one carry the reasoning:
  [tui.md](tui.md), [web.md](web.md), [history.md](history.md), [importing.md](importing.md).
- **`docs/PRFAQ.md`** — the working-backwards document behind the README: who this is for, and
  what problem it solves.
