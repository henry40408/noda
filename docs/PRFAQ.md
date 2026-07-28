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
By a short **id** or by its **slug**. A note's filename is `<id>-<slug>.md`: the id is a
stable Crockford base32 code that never changes, even across renames, and the slug is
derived from the title. A slug is matched whole; an id is matched by any prefix that names
exactly one note, the same bargain git makes with object ids — so `noda show k3f9` works
without you typing all eight characters. An ambiguous key is an error listing the
candidates, never a guess, and there are no positional numbers to reshuffle.

**Q: Will ids get scrambled when I sync across machines?**
No. The id is the filename, so it is part of the synced state by construction — machine A
and machine B always agree that `k3f9m2p1` is the same note, and there is nothing derived
that could drift away from it. Two machines that each add a note write two different
filenames, so the merge is clean and noda has nothing to reconcile afterwards.

In the rare event two machines mint the same id offline, git still merges them without
complaint — the filenames differ, because the slugs do — and `noda status` reports the
collision. noda will not settle it: both files are real notes, and keeping one identity
means discarding the other's. Renaming one of the files is a person's call.

**Q: Does it work offline?**
Always. Writing, editing, searching, and history are 100% local git operations. `noda
sync` is the only command that touches the network, and only when you run it.

**Q: Can I use my existing notes repo?**
Yes. `noda clone <url>` pulls an existing remote, and pointing noda at a directory of
Markdown files adopts them in place.

**Q: Can I keep images or PDFs in a notebook?**
Yes. `noda file add ~/Downloads/diagram.png` puts one in the active notebook and commits it,
and `noda file rm diagram.png` takes it out again — you never need to know where on disk the
notebook lives. The file is synced like anything else, and a note points at it with an
ordinary Markdown link, so the note still renders correctly in any other Markdown reader.
`noda ls` lists those files under their own heading and `noda status` counts them.
`noda doctor --links` follows every link and tells you which files no note uses and which
links name nothing — it only reports, and it never deletes.

**Q: How do I use a note with pandoc, or open an attachment in something else?**
`noda path` prints where it lives: `pandoc "$(noda path meeting-notes)" -o notes.pdf`,
`open "$(noda path diagram.png)"`, `cd "$(noda path)"`. noda does not wrap your toolchain,
so rather than growing a verb per tool it tells you the one thing those tools need. The
argument is resolved as a note first — by id prefix or slug — and then as a file by name; a
key that means both is an error naming both.

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

**Q: Why is a note's id in its filename rather than in its frontmatter?**
Because that is where git can enforce it. An earlier design put the id in the frontmatter
and kept a committed `id ↔ slug` index beside the notes; this replaced both.

git's own conventions decided it. git gives unchanging things an identity from their content
(a blob's hash) and changing things a name (a branch, a tag) — a note is a changing thing,
so it gets a name. git commits no bookkeeping of its own: refs, the staging index and the
reflog all live outside the tree, which is precisely why git never has to merge them. And
where git does keep a mutable map, it is one file per name — two people creating two branches
create two files, not two edits to one.

Every problem the old design had came from breaking those rules. Two notebooks that each
added a note both appended to the index, so it conflicted on nearly every divergent sync and
noda needed a special case to rebuild it. The frontmatter could be edited to claim an id the
index never minted, so `edit` needed a guard, `sync` needed a refusal, and `mv`, `rm` and
`restore` each needed rules for which entry to move. Putting the id in the path deletes all
of it: uniqueness is structural (git forbids duplicate paths in a tree), the ids of two
concurrently-added notes cannot collide into one file, and there is no second copy of
anything to keep in step.

It also made history simpler rather than harder. `noda log <note>` followed a rename by
reading the index committed alongside each commit; now it looks for the tree entry carrying
that id prefix. Every commit records the filenames, so every commit already records the map.

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
the user's repository holds.

**Q: Why is the command `noda file add <path>` rather than `noda attach <note> <file>`?**
Because the file goes into the notebook, not into a note.

An earlier draft of this document argued there should be no command at all: a notebook is a
directory, `cp` already puts a file in one, and a verb wrapping `cp` would be ceremony the
filesystem already provides. That was wrong, and the sentence it produced in the README is
what showed it — `cp ~/Downloads/diagram.png ~/.local/share/noda/notebooks/work/`. Every
other thing you can do to a notebook, noda does; that line sent you to find noda's own
directory and operate the storage by hand, and it was only correct when `XDG_DATA_HOME` was
unset and the notebook happened to be called `work`. A command that saves someone from
knowing where their data lives is not ceremony.

What stayed rejected is the *note* argument. Which note uses a file is written in that
note's prose as a Markdown link; a command that also took a note would record the same
relationship in two places, and they would disagree the first time anyone edited one of
them.

Tying an attachment to a note by naming it `<note-id>-diagram.png` was rejected for a
different reason again. It is nearly free to check — ownership would be structural, readable
from a directory listing without opening a single note — but it asks a person to encode a
relationship in a filename by hand, and to keep encoding it. That is a mental burden the
model puts on the user in exchange for a saving the machine enjoys, which is the wrong way
round. It also breaks the id-prefix bargain: `k3f9m2p1-diagram.md` and its owning note share
a prefix, so `noda show k3f9` becomes ambiguous.

What is left is the honest reading — a file is used if a note links to it — and that has to
be read with a CommonMark parser rather than a text search. A reference-style link holds its
destination at the bottom of the file, so the paragraph using it never contains the
filename; `%20` in a destination is a space on disk; and a link inside a fenced code block
is prose about a link. Each of those turns a used file into a reported one, and a report
about unused files that cries wolf is a report nobody reads. The cost is a read of every
note — `search`'s cost, not `ls`'s — which is why it sits behind `--links` rather than
running on every `status`.

**Q: Why does `noda file mv` edit notes only when asked, when it knows exactly which links
it just broke?**
Because it would be the first time noda changed prose the command was not pointed at. Every
other write is to the thing named on the command line: `tag` rewrites one note's
frontmatter, `mv` renames one note's file. A rename that silently reached into three other
notes and rewrote their bodies is a different kind of act, however correct each individual
edit is, and it should be asked for.

Reporting is not a lesser answer either. It is the same rule the orphan check already
follows — say what is true, let the person decide — and it is what makes `--update-links`
safe to offer at all: the rewrite is checked by re-reading the notes afterwards, so a
destination written with backslash escapes, which cannot be located in the source, is
reported as still pointing at the old name rather than assumed fixed.

**Q: What's explicitly *out* of scope for v1?**
Web UI, real-time collaboration, encryption-at-rest, mobile, and plugin systems. v1 is:
multiple git-backed notebooks, add/ls/show/edit/rm, id+slug addressing, full-text search,
per-note history/restore, and HTTPS/SSH sync.

**Q: How do we know it's working backwards and not feature-driven?**
This document is the contract. A feature that doesn't serve a promise made in the press
release or answer a customer FAQ above does not go into v1.
