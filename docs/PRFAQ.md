# noda — PR/FAQ

> **Working Backwards artifact.** Written *as if noda had already shipped*, to pin down who it is
> for and what problem it solves before any product code existed. Kept as the record of those
> decisions; README.md is the current contract.

---

## Press Release

**noda 1.0 — your notes are just a git repo, and the terminal is the fastest way in**

*2026-07-25* — Today we're releasing **noda**, a git-native notebook for the command line. Every
note is a plain Markdown file in an ordinary git repository, so your knowledge base is versioned,
diffable, syncable, and yours — no proprietary format, no lock-in, no cloud account.

Terminal users have faced a bad trade-off for notes. Cloud note apps are fast to write in but hold
your data in opaque formats and sync through someone else's server. Plain files in a folder are
portable but have no history, no search, and no easy sync. Existing git-backed tools either bury
you in raw `git` commands or wrap everything in a heavy GUI.

noda closes the gap. `noda add "meeting notes"` opens your editor and commits the note when you
save. `noda ls` lists everything by a short id *or* a readable slug — use either. `noda sync`
pushes and pulls to GitHub, GitLab, or any git host over HTTPS or SSH, with the transport compiled
into the single static binary — nothing to install.

Because a notebook *is* a git repo, you get per-note history (`noda log`), point-in-time restore
(`noda restore`), branching, and honest offline-first behaviour for free. Multiple notebooks — one
repo each — keep `work` and `personal` separate and pointed at different remotes.

"I wanted my notes to outlive any app I happen to be using this year," said the author. "Git
already solved durable, syncable, versioned text. noda is just the smallest possible layer that
makes git feel like a notebook."

noda builds to a single self-contained binary — statically linked musl on Linux, native on macOS —
and is distributed as a container image on `ghcr.io` for `linux/amd64` and `linux/arm64`.
Anywhere else, `cargo build --release` produces the same one file. It is open source.

---

## Customer FAQ

**Q: What exactly is a "notebook"?**
A git repository. `noda notebook add work` creates one; `noda use work` makes it active. Each
notebook has its own remote, so `work` can live on your company GitLab and `personal` on GitHub.

**Q: Do I need to know git to use it?**
No. Day to day you use `noda add / ls / edit / sync`. Nothing is hidden — it's a normal repo, so
`cd` in and run `git log` anytime. noda never does anything you couldn't inspect or undo with git.

**Q: How do I refer to a note?**
By a short **id** or by its **slug**. A note's filename is `<id>-<slug>.md`: the id is a stable
Crockford base32 code that never changes, even across renames, and the slug comes from the title.
A slug is matched whole; an id by any prefix naming exactly one note, as git does with object ids —
so `noda show k3f9` works. An ambiguous key is an error listing the candidates, never a guess, and
there are no positional numbers to reshuffle.

**Q: Will ids get scrambled when I sync across machines?**
No. The id is the filename, so it is part of the synced state by construction, and nothing derived
can drift from it. Two machines that each add a note write two different filenames, so the merge is
clean.

If two machines mint the same id offline, git still merges them — the slugs differ, so the
filenames do — and `noda status` reports the collision. noda will not settle it: both files are
real notes, and renaming one is a person's call.

**Q: Does it work offline?**
Always. Writing, editing, searching and history are local git operations. Only the commands that
talk to a remote — `sync`, `push`, `pull`, `clone` — touch the network, and only when you run them.

**Q: Can I use my existing notes repo?**
Yes. `noda clone <url>` pulls an existing remote, and pointing noda at a directory of Markdown
files adopts them in place.

**Q: Can I keep images or PDFs in a notebook?**
Yes. `noda file add ~/Downloads/diagram.png` puts one in the active notebook and commits it, and
`noda file rm diagram.png` takes it out — you never need to know where the notebook lives. The file
syncs like anything else, and a note points at it with an ordinary Markdown link, so it renders in
any Markdown reader. `noda ls` lists files under their own heading and `noda status` counts them.
`noda doctor --links` reports files no note uses and links that name nothing; it never deletes.

**Q: How do I use a note with pandoc, or open an attachment in something else?**
`noda path` prints where it lives: `pandoc "$(noda path meeting-notes)" -o notes.pdf`,
`open "$(noda path diagram.png)"`, `cd "$(noda path)"`. Rather than a verb per tool, noda gives the
one thing those tools need. The argument resolves as a note first (id prefix or slug), then as a
file by name; a key that means both is an error naming both.

**Q: Is there anything more interactive than one command at a time?**
`noda tui` opens the notebook as a screen: the listing, narrowed as you type in the query language
`noda search` takes, and `Enter` to open what the cursor is on. Keys that change a note run the
commands — `e` runs `noda edit`, `#` runs `noda tag` — so validating, stamping `updated` and
committing have exactly one implementation. Nothing in the browser writes a note itself.

**Q: What about a web UI?**
`noda web` serves the notebooks over HTTP, for reading and writing from a phone. It renders on the
server and needs no JavaScript, works on the same files the CLI does, and every change runs the
command the terminal would have run, landing as its own commit. Notes are rendered, attachments
served, and Sync starts at once and answers straight away, with the landing page reporting progress
until it stops. Scripting is only an enhancement — filtering as you type, over pages that already
work without it.

An edit carries the note's git blob id. A stale one is merged with what changed meanwhile, and
refused only where the two overlap, with both versions handed back so nothing typed is lost. The blob id and not the `updated` stamp, because `--no-touch` exists so
content can change without that stamp moving.

There is deliberately no password: it listens on your own machine unless told otherwise, and the
way to reach it from elsewhere is a tailnet or an authenticating proxy. It carries the two
protections that need no account — it refuses requests that say they came from another site, and
answers to a hostname only when started with that name.

---

## Internal FAQ

**Q: Why compile HTTPS *and* SSH transport into every binary instead of making them
optional features?**
The hosts users actually target — GitHub and GitLab — are reached over HTTPS or SSH. A build
without them cannot sync to what everyone uses, which is a support trap, not a saving. The cost is
accepted: the binary grows from ~1.0 MB to ~5.6 MB and build time roughly triples, because
libgit2, OpenSSL and libssh2 are vendored and compiled from source. Validated by cross-compiling to
`x86_64` and `aarch64` `-unknown-linux-musl` via cargo-zigbuild.

**Q: Why git2/libgit2 rather than shelling out to the `git` binary?**
A single static binary with no runtime dependency on a system `git` is the whole distribution
story (one file, musl, arm64). Shelling out would bring back that dependency and fragile output
parsing. If we ever need a transport libgit2 lacks, we can shell out for that one operation.

**Q: Why is a note's id in its filename rather than in its frontmatter?**
Because that is where git can enforce it. An earlier design put the id in the frontmatter and kept
a committed `id ↔ slug` index beside the notes; this replaced both.

git's conventions decided it. git names changing things (branches, tags) and gives content an
identity by hash; a note changes, so it gets a name. git commits none of its own bookkeeping —
refs, the index and the reflog live outside the tree, which is why git never merges them. And
where git keeps a mutable map, it is one file per name, so two new branches are two files, not two
edits to one.

The old design broke those rules. Two notebooks that each added a note both appended to the index,
which conflicted on nearly every divergent sync and needed a special case to rebuild. The
frontmatter could claim an id the index never minted, so `edit` needed a guard, `sync` a refusal,
and `mv`, `rm` and `restore` rules for which entry to move. The id in the path deletes all of it:
uniqueness is structural (git forbids duplicate paths in a tree), two concurrently-added notes
cannot collide into one file, and there is no second copy to keep in step.

History got simpler too. `noda log <note>` used to follow a rename through the index committed
with each commit; now it looks for the tree entry carrying that id. Every commit already records
the map.

**Q: Why is the container image the only distribution channel?**
Because it is the only one that can be kept honest. crates.io and Homebrew are promises to keep
publishing — a formula to maintain, a version to bump, a name to defend — and an earlier draft of
this document made all three before any existed. The image is built by the workflow that already
cross-compiles the binary, so it costs nothing beyond the push and cannot quietly go stale. The
binary itself is still one `cargo build --release` away; running a CLI through a container is an
inconvenience an alias absorbs.

**Q: Why does `search` have no index?**
Measured on 5000 notes totalling 12.4 MiB: `noda search` takes 68 ms for a term almost nothing
matches and 82 ms when nearly everything does, against 67 ms for `noda ls`, which already opens
and parses every note. ripgrep takes 56 ms and `git grep` 55 ms on the same tree. The cost is
opening five thousand files, not matching bytes, so searching costs about what listing does, and
both are imperceptible at the hundreds of notes that are common. An index would save maybe 50 ms at
5000 notes and cost a staleness story, a `reindex` command, invalidation after every `pull`, and a
corruption path. v1 declines that trade. A cache can be added later without changing anything the
repository holds.

**Q: Why is the command `noda file add <path>` rather than `noda attach <note> <file>`?**
Because the file goes into the notebook, not into a note.

An earlier draft argued for no command at all: `cp` already puts a file in a directory. That was
wrong, and the README sentence it produced showed it —
`cp ~/Downloads/diagram.png ~/.local/share/noda/notebooks/work/` sent you to find noda's storage
and operate it by hand, and was only correct with `XDG_DATA_HOME` unset and a notebook called
`work`. A command that saves someone from knowing where their data lives is not ceremony.

The *note* argument stayed rejected. Which note uses a file is written in that note's prose as a
Markdown link; a command that also took a note would record the relationship twice, and the two
would disagree at the first edit.

Naming an attachment `<note-id>-diagram.png` was rejected too. Ownership would be structural and
cheap to check, but it makes a person encode and maintain a relationship by hand for a saving the
machine enjoys. It also breaks the id-prefix bargain: `k3f9m2p1-diagram.md` shares a prefix with
its note, so `noda show k3f9` becomes ambiguous.

What is left: a file is used if a note links to it, read with a CommonMark parser rather than a
text search. A reference-style link keeps its destination at the bottom of the file, `%20` in a
destination is a space on disk, and a link in a fenced code block is prose about a link; each
would turn a used file into a reported one, and an unused-file report that cries wolf goes unread.
The cost is reading every note — `search`'s cost, not `ls`'s — which is why it sits behind
`--links` rather than running on every `status`.

**Q: Why do `noda file mv` and `noda mv` edit notes only when asked, when they know exactly
which links they just broke?**
Because it would be the first time noda changed prose the command was not pointed at. Every other
write is to the thing named on the command line. A rename that reached into three other notes and
rewrote their bodies is a different kind of act, however correct, and should be asked for.

Reporting follows the orphan check's rule — say what is true, let the person decide — and is what
makes `--update-links` safe to offer: the rewrite is checked by re-reading the notes afterwards, so
a destination written with backslash escapes, which cannot be located in the source, is reported as
still pointing at the old name rather than assumed fixed.

The two renames differ only in what they match. An attachment's name is its whole identity, so
`file mv` looks for the name it just left. A note keeps its id across retitles, so `mv` looks for
the id — which also catches a destination written two renames ago.

**Q: Why does the search grammar have `OR` but no parentheses, and why does `OR` bind
tighter than a space?**
Because those are the same choice. A query language compounds — `tag:` invites `OR`, `OR` invites
parentheses, parentheses invite precedence rules nobody remembers — so the grammar is fixed at one
shape: an AND of ORs, four lines, written into the README.

Binding `OR` tighter than the space makes that shape sufficient. `a OR b c OR d` reads as
`(a OR b) AND (c OR d)`, and an AND of ORs is conjunctive normal form, which expresses every
query — so parentheses would add notation, not power. Boolean convention would make
`budget tag:x OR tag:y` mean `(budget AND tag:x) OR tag:y`, which is not what anybody listing two
acceptable tags meant.

The grammar cannot write `(a AND b) OR (c AND d)` directly. That is two searches, which is cheaper
than a language nobody can predict.

**Q: Why hand-write the JSON instead of adding serde?**
Because it is one object with five string fields, and the alternative is two crates and a derive
macro in a tool whose distribution story is one small static binary. The part that must be right is
the escaping: thirty lines with tests. The project made the same trade for percent-decoding and for
having no error-handling crate.

**Q: If serde was too much dependency for five string fields, why is a whole UI library not
too much for one screen?**
Because the problems differ in size. The JSON was thirty lines whose hard part was escaping; a
terminal UI must hold raw mode, restore the terminal on the way out *including* on panic, survive a
resize, measure wide characters to lay out a column, and redraw only changed cells. Getting that
subtly wrong leaves somebody's terminal unusable.

The cost was measured on the cold-start harness: the release binary grows 243 KiB
(6069 → 6312 KiB) and `noda ls` 0.10 ms (1.95 → 2.05 ms of its own time, both binaries in one run).
It is one dependency, not two — ratatui re-exports crossterm — and its default features are off,
dropping a calendar widget, a macro crate and a colour-space converter a two-pane reader does not
need.

**Q: Why does `noda ls -0` exist when `-q` already prints one record per line?**
Because `noda file add` allows a space in a filename, so newline-separated output is not safe for
`xargs`, and a listing that is *nearly* safe is worse than one that is obviously not.

It was also the one thing the test suite could not see. Other tests call the command functions
directly, but `-0` is a promise about the bytes leaving the process, and the layer between ate
them: the colour handling that makes a piped `noda show` byte-exact stripped NUL along with escape
sequences, and the trailing newline other commands want arrived after the last terminator.
Machine-separated output now bypasses both, and one test runs the real binary to prove it.

**Q: What's explicitly *out* of scope for v1?**
Web UI, real-time collaboration, encryption-at-rest, mobile, and plugin systems. v1 is: multiple
git-backed notebooks, add/ls/show/edit/rm, id+slug addressing, full-text search, per-note
history/restore, and HTTPS/SSH sync. (`noda web`, answered above, came later.)

**Q: How do we know it's working backwards and not feature-driven?**
This document is the contract. A feature that doesn't serve a promise in the press release or
answer a customer FAQ above does not go into v1.
