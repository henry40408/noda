# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

noda is a git-native notebook CLI: notes are Markdown files in an ordinary git repository, one
notebook per repo. Everything is exposed as a library (`src/lib.rs`); the binary is a thin shell,
so commands are tested without spawning a process.

**README.md is a contract, not a description.** It was written spec-first — the whole v1 surface
before there was any code (`docs/PRFAQ.md` is the working-backwards artifact behind it). A change
to user-facing behaviour belongs in README.md in the same commit; where the contract turned out to
be wrong, the convention is to correct it rather than quietly leave it behind.

## Commands

```sh
cargo nextest run                          # the suite — not `cargo test`
cargo nextest run <substring>              # one test by name
cargo nextest run --test cli               # one integration file
cargo fmt                                  # before committing
cargo clippy --all-targets -- -D warnings
cargo deny check                           # supply chain, as CI runs it
scripts/bench-coldstart.sh                 # times whole processes, not in-process code
```

`e2e/` is **deliberately its own workspace**, so the root suite never compiles it — running the
browser tests is a separate decision, taken by running them:

```sh
cargo build                                              # e2e drives the built binary
cargo test --manifest-path e2e/Cargo.toml --test e2e     # needs Chrome; chromedriver is fetched
```

The toolchain is pinned (`rust-toolchain.toml`, 1.97.1) so a local build, a CI run and a release
build are the same compiler, and the musl targets are declared there so the cross-compile is
reproducible locally. `unsafe_code = "deny"`. Clippy runs `all` + `pedantic` with an itemised
opt-out list in `Cargo.toml`.

## Architecture

**`docs/ARCHITECTURE.md` is the reference** — it has the shape, three worked paths through it (a
CLI command, a write from the browser, a TUI keystroke), the concurrency model, and where to add a
command / import source / screen / page. The short version:

```
main.rs → cmd.rs → notebook.rs   the single façade over git2
                   note.rs       the filename that carries a note's identity
link · query · todo              read from the CommonMark event stream, never grepped
paths · config · error · style · sign · remote
```

`tui/`, `web/` and `import/` sit on top, and neither front end reaches past `cmd` to write a note.

## Invariants

These hold *between* modules, so no single file states them — which is why they are here. Breaking
one is how the two front ends drift apart; `docs/ARCHITECTURE.md` explains each in context.

1. **A command returns its answer; it does not emit it.** Every `cmd` entry point is
   `-> Result<String>`. There is exactly one write to stdout in the crate — `cmd::print`, called
   once from `main.rs`, through `anstream` so a redirected `noda show` is byte-for-byte the file.

2. **What a command returns is prose, not an interface.** It is a sentence written for a person,
   and a caller that parses it has turned the wording of a message into an API. When a caller needs
   a fact about what just happened it asks the notebook — both front ends diff `taken_ids()` around
   the call rather than reading an id out of the answer.

3. **Commands come in pairs: `foo(paths, …)` and `foo_in(notebook, …)`.** The `_in` half takes an
   already-open notebook and never opens `$EDITOR` — it is the half `noda web` calls, because a
   request must not open an editor and a second handle on one repository defeats the point.

4. **Nothing outside `cmd` writes a note.** Validating a title, stamping `updated` and committing
   happen in one place, so a change means the same thing however it was asked for.

5. **The layer that produces a screen touches nothing.** `tui/app.rs` returns `Action`s and opens
   no file, repository or terminal; `web/page.rs` returns strings and knows nothing about requests.
   Both exist so the interesting half can be tested with no terminal and no socket.

6. **`git2::Repository` is `!Send`** and is never held across an await. Every web handler works
   inside `spawn_blocking` with its own `Notebook`; `web/work.rs` uses a plain `std::thread`,
   because the blocking pool is for work a request waits on and sync is the work none waits on.

7. **Markdown is parsed, never grepped** — `link.rs`, `todo.rs`, `web/render.rs`. `- [ ]` inside a
   code fence is not a checkbox and a link inside one is not a link; getting that wrong reports a
   referenced file as an orphan.

8. **Colour is decided once, in `style.rs`.** `tui/theme.rs` and `web/theme.rs` translate that
   decision for their medium; they do not make one.

## Data model

A note is `<id>-<slug>.md`. **The filename is the identity and nothing else records it** — there is
no index file, and one should not be introduced. git forbids two entries under one path in a tree,
so uniqueness is structural rather than something noda polices, and two machines that each add a
note write two different filenames that merge without a conflict.

The frontmatter carries `title`, `tags`, `created` and `updated`. Its *presence* is what marks a
file as a note (`notebook::Scan`). noda interprets those fields and no others, but does not own the
block: any other field survives a write-back untouched.

## The web layer

Server-rendered and works with JavaScript off — the search box is a form, every row is a link.
`web/script.rs` is the only part allowed to be absent, and the rule it is written against is that
**the script may answer sooner or not at all, never differently.** The listing filter is therefore
allowed to be narrower than the server and never wider; it stands aside entirely for a negated bare
word, which it would otherwise widen (it cannot see bodies). `web/guard.rs` holds the `Origin` and
`Host` checks — there are no accounts, so those are the whole defence against cross-site writes and
DNS rebinding. `web/log.rs` may name a request only by the route template it matched, never by the
path asked for, because an address here carries somebody's note id or filename.

## Testing

Six layers: `#[cfg(test)]` units in `src/**` (~369), `tests/cli.rs` for the command layer,
`tests/tui.rs` for screens (ratatui's test backend — no terminal), `tests/pty.rs` for *layout*
(a real pty and `vt100`, catching what a character buffer is blind to), `tests/web.rs` for the real
binary on a real socket, and `e2e/` for a real browser. `docs/ARCHITECTURE.md` says what each one
exists to catch; put a new test in the cheapest layer that can actually fail on the bug.

Two things that are not optional:

- **`sign = false` in every test notebook's `config.toml`.** The XDG roots are per-test but git's
  are not — libgit2 reads the developer's real `~/.config/git/config`, so a machine with
  `commit.gpgsign = true` sends every test commit to gpg.
- **`Paths::rooted(<temp dir>)`, not environment variables.** Tests run in parallel and cannot
  safely mutate process-wide env.

The harness is restated in each integration file rather than shared: an integration test is its own
crate.

## Conventions

- **Module `//!` comments are where design decisions are recorded**, and they are thorough — read a
  module's header before changing it. `Cargo.toml` does the same for every dependency, including
  the measurements behind a choice (e.g. `tracing-subscriber`'s `env-filter` was dropped for
  `Targets` after measuring it at 355 KB, 69% of the whole of the logging).
- **Startup time is a feature.** A quick `noda ls` costs more in process startup than in work, so
  the release profile is tuned for size and anything that grows the binary is measured, not
  assumed.
- Colour is emitted unconditionally; `anstream` strips it when output is not a terminal, so a piped
  `noda show` emits exactly the bytes on disk.
