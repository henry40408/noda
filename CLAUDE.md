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

```
main.rs      clap definitions and argument parsing. Prints; holds no logic.
   |
cmd.rs       one function per command. Returns a String — never prints.
   |
notebook.rs  `Notebook`: the single façade over git2. Status, drift, scan,
             commit, pull/push, log, blame, diff, snapshots.
note.rs      one note: frontmatter parse / write-back, and the filename identity.
link · query · todo    features derived from the CommonMark event stream.
paths · config · error · style · sign · remote
```

Two front ends sit on top of `cmd`, and neither reimplements it:

| | modules | reaches for |
| --- | --- | --- |
| `tui/` | `app` `view` `frame` `field` `command` `theme` | `cmd::edit`, `cmd::tag`, `cmd::rm`, `cmd::bulk`, … |
| `web/` | `mod` `page` `render` `script` `log` `guard` `theme` `work` | `cmd::rewrite_in`, `cmd::tag_in`, `cmd::rm_in`, … |
| `import/` | `wikitext` `tiddlywiki` | one parser per source, producing `Incoming`; one shared back end |

## Invariants

These hold across modules, so no single file states them. Breaking one is how the two front ends
drift apart.

1. **A command returns its answer; it does not emit it.** Every entry point in `cmd` is
   `-> Result<String>`, which is what lets the TUI show a command's own output in its status band
   and the web handlers reuse it. There is exactly one write to stdout in the crate —
   `cmd::print`, called once from `main.rs` — and it goes through `anstream`, so a redirected
   `noda show` writes the file byte for byte. Adding a second one breaks both front ends.

2. **Commands come in pairs: `foo(paths, …)` and `foo_in(notebook, …)`.** The first opens the
   active notebook; the second takes one already open. `noda web` opens a notebook per request and
   calls the `_in` half — a second handle on the same repository would defeat the point — and the
   `_in` half never opens `$EDITOR`, because a browser arrives with the body already written.

3. **Nothing outside `cmd` writes a note.** Validating a title, stamping `updated` and committing
   happen in one place. `tui/mod.rs` says it outright: nothing in that module writes a note itself.

4. **The layer that produces a screen touches nothing else.** `tui/app.rs` takes keystrokes and
   returns `Action`s — it opens no file, repository or terminal. `web/page.rs` takes what a page is
   about and returns a string — it opens no repository and does not know what a request is. Both
   exist so the interesting half can be tested with no terminal and no socket; `tui/mod.rs` is the
   only place in the TUI that touches the world.

5. **`git2::Repository` is `!Send`** and cannot be held across an await. Every web handler works
   inside `spawn_blocking` and opens its own `Notebook`. `web/work.rs` uses a plain `std::thread`
   instead: the blocking pool is for work a request waits on, and sync is precisely the work no
   request waits on.

6. **Markdown is parsed, never grepped.** `link.rs`, `todo.rs` and `web/render.rs` all go through
   `pulldown-cmark`. `- [ ]` inside a code fence is not a checkbox, a link inside one is not a
   link, and a reference-style link keeps its destination at the bottom of the file — getting any
   of that wrong reports a referenced file as an orphan.

7. **Colour is decided once, in `style.rs`.** `tui/theme.rs` and `web/theme.rs` translate that
   decision for their medium; they do not make one. An id is the same yellow in `noda ls`, in the
   TUI and in a browser.

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

| layer | what it catches |
| --- | --- |
| `#[cfg(test)]` in `src/**` (~369 tests) | units, next to the code |
| `tests/cli.rs` | the command layer, each test with its own XDG root |
| `tests/tui.rs` | screens, drawn into ratatui's test backend — no terminal |
| `tests/pty.rs` | layout bugs a character buffer is blind to: a real pty, `vt100` on the other end |
| `tests/web.rs` | the real binary on a real socket, requests written by hand (the guard tests need a `Host` that lies) |
| `e2e/` | Gherkin features driving a real browser |

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
