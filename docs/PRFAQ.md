# noda — PR/FAQ

> **Working Backwards artifact.** This is written *as if noda has already shipped*, to
> pin down who it is for and what problem it solves before a line of product code exists.
> Status: design draft — not yet implemented.

---

## Press Release

**noda 1.0 — your notes are just a git repo, and the terminal is the fastest way in**

*2026-07-25* — Today we're releasing **noda**, a git-native notebook for the command
line. Every note you take is a plain Markdown file in an ordinary git repository, so your
knowledge base is versioned, diffable, syncable, and yours — no proprietary format, no
lock-in, no cloud account required.

People who live in the terminal have always faced a bad trade-off for notes. Cloud
note apps are fast to write in but hold your data hostage in opaque formats and sync
through someone else's server. Plain files in a folder are portable but give you no
history, no search, and no easy way to sync across machines. Existing git-backed tools
either bury you in raw `git` commands or wrap everything in a heavy GUI.

noda closes the gap. `noda add "meeting notes"` drops you into your editor and, the moment
you save, commits the note to git automatically. `noda ls` lists everything by a short
id *or* a human-readable slug — use whichever your fingers reach for first.
`noda sync` pushes and pulls to GitHub, GitLab, or any git host over HTTPS or SSH, with
the transport compiled straight into the single static binary — nothing to install, no
system libraries to chase down.

Because a notebook *is* a git repo, you get things other note apps charge for, for free:
full per-note history (`noda log`), point-in-time restore (`noda restore`), branching,
and honest offline-first behavior. And because noda keeps multiple notebooks — one repo
each — you can keep `work` and `personal` cleanly separated and pointed at different
remotes.

"I wanted my notes to outlive any app I happen to be using this year," said the author.
"Git already solved durable, syncable, versioned text. noda is just the smallest possible
layer that makes git feel like a notebook."

noda builds to a single self-contained binary — statically linked musl on Linux, native on
macOS — and is distributed as a container image on `ghcr.io` for `linux/amd64` and
`linux/arm64`. Anywhere else, `cargo build --release` produces the same one file. It is
open source.

---

## Customer FAQ

**Q: What exactly is a "notebook"?**
A git repository. `noda notebook add work` creates one; `noda use work` makes it the
active notebook. Each notebook can point at its own remote, so `work` can live on your
company GitLab while `personal` lives on GitHub.

**Q: Do I need to know git to use it?**
No. Day to day you use `noda add / ls / edit / sync` and never touch git. But nothing is
hidden — it's a normal repo, so `cd` in and run `git log` anytime. noda never does
anything to the repo you couldn't inspect or undo with git.

**Q: How do I refer to a note?**
By a short **id** or by its **slug** — both matched exactly. Every note carries a stable id
(a short Crockford base32 code like `k3f9`) that never changes, even across renames, plus a slug
derived from its title. `noda show k3f9` and `noda show meeting-notes` resolve to the same
note, or report "not found" — they never silently hit the wrong one. There are no
positional numbers to reshuffle.

**Q: Will ids get scrambled when I sync across machines?**
No. An id is written into the note's frontmatter and committed, so it's part of the synced
state, not a positional guess — machine A and machine B always agree that `k3f9` is the
same note. In the rare event two machines mint the same id offline, `noda sync` detects it
and regenerates one, leaving the durable slug untouched.

**Q: Does it work offline?**
Always. Writing, editing, searching, and history are 100% local git operations. `noda
sync` is the only command that touches the network, and only when you run it.

**Q: Can I use my existing notes repo?**
Yes. `noda clone <url>` pulls an existing remote, and pointing noda at a directory of
Markdown files adopts them in place.

**Q: What about a web UI?**
Planned. The v1 focus is the CLI. A `noda web` local server that serves the same notebook
over a browser is on the roadmap; because storage is just git, the web UI reads the exact
same files.

---

## Internal FAQ

**Q: Why compile HTTPS *and* SSH transport into every binary instead of making them
optional features?**
Because the two hosts every user actually targets — GitHub and GitLab — are reached over
HTTPS or SSH. A build that omits them produces a notebook that can't sync to the services
100% of users use, which is a support trap, not a saving. We accept the cost: the binary
grows from ~1.0 MB to ~5.6 MB and build time roughly triples, because libgit2, OpenSSL,
and libssh2 are all vendored and compiled from source. This was validated by
cross-compiling to `x86_64` and `aarch64` `-unknown-linux-musl` via cargo-zigbuild.

**Q: Why git2/libgit2 rather than shelling out to the `git` binary?**
A single static binary with no runtime dependency on a system `git` is the whole
distribution story (one file, musl, arm64). Shelling out would reintroduce a runtime
dependency and fragile output parsing. Trade-off accepted; if we ever need a transport
libgit2 lacks, we can selectively shell out for that one operation.

**Q: Why is the committed `id ↔ slug` index a TSV?**
Because of where this particular file sits: it is written by noda, read by noda, never
edited by hand, and fully rebuildable from the notes' frontmatter. Both of its fields are
constrained by construction — an id is Crockford base32, and a slug keeps only alphanumeric
characters — so a tab cannot appear inside a value. That buys a parser that is one
`split_once('\t')` with no escaping rules to get wrong, and a file that `cut -f2` reads
straight out of a pipe. CSV earns its ubiquity on a different problem: its quoting rules let
a value carry the delimiter itself, which is what you want when fields are arbitrary user
text — a spreadsheet export, say. noda's index isn't that, so it pays TSV's price instead:
the writer must keep tabs and newlines out of the fields, which stays cheap as the index
grows to carry titles because the index is derived data, and a rebuild is always available.
Interchange is a separate concern from storage; if noda ever needs to hand this data to
another tool, that is an output format on `ls`, not a change to what sits in the repo.

**Q: Why is the container image the only distribution channel?**
Because it is the only one that can be kept honest. crates.io and Homebrew are promises to
keep publishing — a formula to maintain, a version to bump, a name to defend — and the
earlier draft of this document made all three before any of them existed. The image is
built by the same workflow that already cross-compiles the binary, so distribution costs
nothing beyond the push, and there is no channel that can quietly go stale. Anyone who
wants the binary itself still gets it from `cargo build --release`; running a CLI through
a container is a real inconvenience, and one an alias absorbs.

**Q: Why does `search` have no index?**
Measured on 5000 notes totalling 12.4 MiB: `noda search` takes 68 ms for a term almost
nothing matches and 82 ms when nearly everything does, against 67 ms for `noda ls`, which
already opens and parses every note. Ripgrep over the same tree takes 56 ms and `git grep`
55 ms, so a plain scan is already within a whisker of tools built for this. The cost is
dominated by opening five thousand files, not by matching bytes — which is why searching
costs about what listing costs, and why both are imperceptible at the hundreds-of-notes
sizes that are actually common. An index would buy
maybe 50 ms at 5000 notes and cost a staleness story, a `reindex` command, invalidation
after every `pull`, and a corruption path. v1 declines that trade. If notebooks in the tens
of thousands turn up, a cache is a cache: it can be added later without changing anything
the user's repository holds. Note that the committed `id ↔ slug` index cannot serve search
either way — it carries metadata, and search reads bodies.

**Q: What's explicitly *out* of scope for v1?**
Web UI, real-time collaboration, encryption-at-rest, mobile, and plugin systems. v1 is:
multiple git-backed notebooks, add/ls/show/edit/rm, id+slug addressing, full-text search,
per-note history/restore, and HTTPS/SSH sync.

**Q: How do we know it's working backwards and not feature-driven?**
This document is the contract. A feature that doesn't serve a promise made in the press
release or answer a customer FAQ above does not go into v1.
