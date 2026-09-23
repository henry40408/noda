# noda

> A git-native notebook for your terminal. Your notes are plain Markdown in an ordinary
> git repository — versioned, syncable, and yours.

---

**Start here** — [Why noda](#why-noda) · [Install](#install) · [Quickstart](#quickstart) · [Concepts](#concepts)

**The commands** — [Notebooks](#notebooks) · [Notes](#notes) · [Attachments](#attachments) · [Paths](#paths) · [Action items](#action-items) · [Backlinks](#backlinks) · [History](#history-git-backed) · [Remote sync](#remote-sync-https--ssh) · [Config](#config) · [Signing](#signing) · [Output](#output) · [Importing](#importing) · [Storage layout](#storage-layout) · [Roadmap](#roadmap) · [Building](#building-from-source)

**The two other ways in** — [In the terminal](#browsing) · [In a browser](#in-a-browser)

**In full** — [Browsing in the terminal](docs/tui.md) · [In a browser](docs/web.md) · [History and sync](docs/history.md) · [Importing](docs/importing.md) · [Architecture](docs/ARCHITECTURE.md)

This page says what each command does. The reasoning behind a decision sits behind a ▸ or in
one of the documents above.

---

## Why noda

- **Just git.** Every notebook is a normal git repo of Markdown files. Anything noda does,
  plain `git` can inspect and undo.
- **Automatic history.** Every change is committed for you; `noda log` shows a note's history
  and `noda restore` rewinds it.
- **Sync anywhere.** HTTPS and SSH are compiled in, so `noda sync` talks to any git host with
  nothing else to install.
- **Fast to reach.** Address a note by a short id *or* a readable slug.
- **One static binary.** Self-contained for macOS and Linux (incl. arm64/musl), and a
  container image.

## Install

A container image on GitHub's registry, for `linux/amd64` and `linux/arm64`. Notebooks live in
a volume, and the image runs `noda` directly:

```sh
docker pull ghcr.io/henry40408/noda:main
alias noda='docker run --rm -it -v noda:/data ghcr.io/henry40408/noda:main'
noda init
noda add "Meeting notes" -c "agenda"
```

Otherwise build it — there is no crates.io package and no Homebrew formula:

```sh
cargo build --release        # target/release/noda
```

<details>
<summary>The three things a container cannot do for you</summary>

`add` and `edit` open an editor, which the image does not carry — write notes with `-c`, or
mount one in. `sync` over SSH needs a key: pass your agent through with
`-v "$SSH_AUTH_SOCK:/ssh-agent" -e SSH_AUTH_SOCK=/ssh-agent`, or use an HTTPS remote with a
token. `noda tui` needs the `-it` the alias already carries; without a terminal, the command
says so.

</details>

## Quickstart

```sh
noda init                       # create XDG config/data dirs and a default notebook
noda add "Meeting notes"        # opens $EDITOR; auto-commits on save
noda ls                         # list notes in the current notebook
noda show k3f9                  # or: noda show meeting-notes
noda edit meeting-notes         # re-open in $EDITOR, auto-commits the change
noda tui                        # or browse the lot: list, filter, open, come back

noda notebook add work --remote git@github.com:me/work-notes.git
noda use work                   # switch active notebook
noda sync                       # pull + push over SSH/HTTPS
```

## Concepts

**Notebook.** A git repository under `$XDG_DATA_HOME/noda/notebooks/<name>/`. You can have
many; one is active at a time, and each has its own remote. A new notebook starts on the
branch your `init.defaultBranch` names.

**Note.** A Markdown file named `<id>-<slug>.md`. The **id** is a short code (Crockford base32,
e.g. `k3f9m2p1`) that never changes, even across renames. The **slug** is derived from the
title, follows a retitle, and is cut to 100 bytes at a word boundary. The filename is the
identity; there is no index.

Anywhere a command takes `<note>`, pass the id or the slug. A slug is matched whole; an id by
any prefix that names exactly one note, as git does with object ids, so `noda show k3f9` works.
Ambiguity is an error listing the candidates, never a guess.

<details>
<summary>Why the filename carries the identity, and how a mistyped id still lands</summary>

git will not put two entries under one path in a tree, so uniqueness is structural, and two
machines that each add a note write two different filenames that merge without a conflict.

Ids are lowercase and matched case-insensitively, and Crockford maps `I`/`L` to `1` and `O` to
`0`, so a mistyped id still resolves. Two notes may share a slug — the id keeps their filenames
apart — and then the slug alone is ambiguous, which is also why cutting a long slug cannot cause
a collision.

A filename gets 255 bytes, of which the id and `.md` spend 12. The cut is at 100 rather than
243 because `noda ls -l` pads the slug column to the widest slug. The full title stays in the
frontmatter.

</details>

**Frontmatter.** Having one is what marks a file as a note. noda reads these five fields and
leaves anything else in the block alone:

```
---
title: Reading notes on TAOCP
tags: [books, algorithms]
created: 2019-03-14T08:21:00Z
updated: 2024-11-02T16:40:12Z
pinned: true
---
```

`pinned` is normally absent: `noda pin` writes it and `noda unpin` removes the line, rather than
writing `false`.

`created` is set once; `updated` follows every change noda makes. `--no-touch` opts one command
out of that — on `edit`, `tag`, `pin`, `unpin`, `mv` and `restore` — for example on a note
imported with its own dates:

```
$ noda edit imported --no-touch          # the 2019 date it came with is still the date it has
$ noda tag imported --no-touch +archived
```

<details>
<summary>Writing your own dates, where <code>--no-touch</code> goes, and what noda will not invent</summary>

Anything RFC 3339 is read and kept as written, offset and all (`2019-03-14T16:21:00+08:00`
stays). noda writes UTC when it writes one itself. A note without the fields keeps not having
them: filesystem times do not survive a `clone`, and git only knows when a note reached *this*
notebook, so neither is a true `created`.

Renaming an attachment does not touch `updated` on the notes whose links it rewrites.

On `tag` the flag goes *before* the tags, which take every argument after them (so `-q3` is a
tag to remove); written after them, noda says where it belongs. On `restore`, `updated` comes
back with the rest of the version, so the note is byte for byte the copy asked for. `add` has no
such flag.

</details>

**History.** Every add/edit/rm is a commit, so `noda rm` is a commit you can revert.

## Command reference

### Notebooks

| Command | Description |
| --- | --- |
| `noda init` | Create the XDG directories and a `default` notebook. |
| `noda notebook add <name> [--remote <url>]` | Create a notebook (a new git repo). |
| `noda notebook ls` | List notebooks; marks the active one and says where each stands against its remote. |
| `noda notebook rm <name> [--force]` | Remove a notebook (local repo). Asks first. |
| `noda notebook rename <old> <new>` | Rename a notebook. |
| `noda use <name>` | Set the active notebook. |
| `noda notebook current` | Print the active notebook. |
| `noda status` | Where the active notebook stands: notes, changes, drift from the remote. |
| `noda doctor [--dry-run] [--links] [--times]` | Report what noda will not settle on its own, and adopt notes that only lack an id. |
| `noda clone <url> [name]` | Clone an existing remote notebook. |
| `noda readme [--force]` | Write the notebook's `README.md`, which a git host shows as its front page. |

**`noda notebook rm` cannot be undone** — it deletes the repository and its history from disk.
The active notebook is refused outright; any other is confirmed at the terminal. With no
terminal, it is refused unless `--force` is given.

`noda status` does not touch the network: push/pull counts are measured against what the last
sync left behind, so it works offline and instantly.

```
notebook  work  (main)
notes     42
changes   1 file uncommitted
remote    git@github.com:me/work-notes.git
sync      2 to push (as of the last sync)
files     2
problems  2 problems
          1 note has no id in its filename  (hand-written.md)
          1 file is named like a note but has no frontmatter  (abcdefgh-hello.md)
          run `noda doctor` to look at these
```

`noda doctor` lists everything `status` elides. It makes one repair: a file with frontmatter
that only lacks an id is given one, as a revertible commit (`--dry-run` shows it without
changing anything). Everything else it only reports, including git hooks that will never run
because noda uses its own libgit2 and never calls git.

```
$ noda doctor --dry-run
1 note has no id in its filename
  hand-written.md
would adopt 1 note — nothing was changed
```

<details>
<summary>What counts as a note, the two problems that are yours to settle, which hooks are reported, and what works on a note that will not parse</summary>

A `*.md` file with a **frontmatter block** and an **id in its filename** is a note; with neither
it is an ordinary file, counted on `files`. The other two combinations are `problems`, counted
by kind so a directory copied in at once still fits on one screen.

**One id on two notes** (two machines can produce it): rename one of the files. **A name that
claims an id over a file with no frontmatter**: add the `---` block back, or rename it so it no
longer starts with an id. Only you know which.

Hooks are reported the way git would find them — `core.hooksPath` when set, the executable bit,
never `*.sample` — and only by `doctor`, not `status`.

`restore`, `rm`, `log` and `diff` identify a note by its filename, so they work on one whose
frontmatter will not parse. `mv` and `tag` rewrite the frontmatter, so they refuse and say why.

</details>

**`noda readme`** writes a `README.md` for a git host's front page: what the filenames and
frontmatter fields mean, that none of it needs noda to read, and how to clone it. It is not an
index of the notes. Everything under its trailing comment is yours; a second run refuses to
overwrite it, and `--force` replaces the file as a revertible commit.

### Notes

| Command | Description |
| --- | --- |
| `noda add [title] [-c <content>] [--tag <t>]...` | Create a note. Opens `$EDITOR` if no `-c`. Auto-commits. |
| `noda ls [--tag <t>] [--notebook <name>] [--json\|-q [-0]] [--notes-only\|--files-only] [-l] [--sort <field>] [-r]` | List what the notebook holds. |
| `noda show <note>` | Print a note to stdout. |
| `noda edit <note> [--no-touch]` | Open a note in `$EDITOR`; auto-commits on save. |
| `noda rm <note>` | Delete a note (as a revertible commit). |
| `noda mv <note> <new-title> [--update-links] [--no-touch]` | Rename a note (updates slug; id is preserved). |
| `noda tag <note> [--no-touch] [+tag]... [-tag]...` | Add/remove tags. |
| `noda pin <note> [--no-touch]` | Float a note to the top of every listing. Auto-commits. |
| `noda unpin <note> [--no-touch]` | Let a pinned note back down among the rest. |
| `noda search <term>...` | Search the active notebook. Terms may name a field, be `OR`ed, or be negated. |
| `noda tui` | Browse the notebook on a screen — see [Browsing](#browsing). |
| `noda todo [--json]` | List every unticked `- [ ]` in the notebook, soonest due first. |
| `noda backlinks <note\|file> [--json\|-q]` | List the notes that link to a note or a file. |

`add` and `edit` open `$VISUAL`, then `$EDITOR`, then `vi`. `edit` opens the real file,
frontmatter included, and refuses to commit an edit that breaks the frontmatter — the file is
left as you saved it, to fix or discard with `git checkout`. An edit cannot change which note it
is: the id is in the filename.

`noda tag meeting-notes +q3 -work` adds `q3` and removes `work`; adding a tag a note already has
just leaves nothing to commit. A title must fit on one line, and a tag cannot contain `,`, `[`,
`]` or a line break; noda refuses rather than write a note it cannot read.

`noda mv` retitles a note and renames the file, which leaves links to it naming a path that is
gone. It lists them, and `--update-links` rewrites them:

```
$ noda mv meeting-notes "Weekly sync"
jjvgqnrv  weekly-sync
1 note links to jjvgqnrv by an older name
  k3f9m2p1-imported.md

$ noda mv weekly-sync "Weekly sync" --update-links
jjvgqnrv  weekly-sync
updated  1 note
```

The flag means *make the links to this note use its current name*, so, as above, it also repairs
links an earlier rename left behind. It is opt-in because it edits other notes' prose.

#### Listing

`noda ls` prints the id and the title, then the notebook's other files under a heading of their
own. `-l` adds the slug and both timestamps.

```
$ noda ls
jjvgqnrv  Meeting notes  [work, q3]
b60ccfw0  Reading log

files
  diagram.png

$ noda ls -l --sort updated
b60ccfw0  Reading log    reading-log    2019-03-14T08:21:00Z  2024-11-02T16:40:12Z
jjvgqnrv  Meeting notes  meeting-notes  2026-08-02T09:14:00Z  2026-08-02T09:14:00Z  [work, q3]
k3f9m2p1  Imported       imported       -                     -
```

`--sort created|updated|title` orders the listing — times newest first, titles alphabetically;
without it, notes are in slug order. `-r` reverses whichever order is in force, files included.

`noda pin` floats a note above any order, marked `pinned` at the end of its row; `noda unpin`
undoes it. `-r` reverses pins too, so pinned notes come last. `pinned:true` and `pinned:false`
narrow a search.

```
$ noda pin meeting-notes
jjvgqnrv  meeting-notes  pinned

$ noda ls
jjvgqnrv  Meeting notes  [work, q3]  pinned
b60ccfw0  Reading log
```

<details>
<summary>Why a pin is a frontmatter field and not a tag</summary>

A tag says what a note is *about*; a pin says how a listing treats it, so it would not belong in
your tag namespace. An unpinned note has no `pinned` line, so pinning and unpinning leaves the
file byte for byte as it was. A value noda cannot read (`pinned: yes`) is not a pin, and is kept,
like a malformed `created`.

</details>

<details>
<summary>The columns, and where a note with no timestamps sorts</summary>

The title, not the slug, names a note — in `ls`, `search` and `backlinks` — since the slug is the
same words again. `-l` extends the row without rearranging it, so `noda ls | cut -c1-8` gives the
ids either way. Tags come last because a note may have none.

Each column is coloured: the id yellow, like a commit id in `log`; the slug a step down from it;
timestamps grey; tags a hue of their own; the title uncoloured.

Sorting compares instants, not text, so a `+08:00` stamp lands where it belongs. A note with no
time sorts last, and first under `-r`.

</details>

Two other shapes are for programs: `--json`, and `-q` for one identifier per record.

```sh
noda ls --json | jq -r '.notes[] | select(.tags[]? == "work") | .file'
noda ls -q0 --files-only | xargs -0 -n1 file
```

<details>
<summary>What each program shape carries</summary>

`--json` is one object on one line, with every field whether or not `-l` was passed (`created`
and `updated` are `null` when absent). Each note carries its filename as well as its id and slug.

`-q` prints a note's id and a file's name. `-0` separates them with NUL, which matters: a file
name may contain a space.

`--notes-only` and `--files-only` narrow any of the three shapes. Filtering beyond one `--tag`
is `noda search`.

</details>

#### Search

`noda search` matches every note's title, tags and body, case-insensitively and by substring
rather than by word (so Chinese and Japanese work). A body hit quotes its line. A bare word
searches all three; a term can also name one field, be `OR`ed with the next, or be negated with
a leading `-`:

```
$ noda search budget tag:work OR tag:q3 -tag:archived
s33wpe5y  Q3 planning  [work, q3]
          the budget and the hiring plan
```

The grammar:

```text
query := term-group…                 every group must match
group := term ('OR' term)…           any term in the group will do
term  := ['-'] [field ':'] value
field := tag | title | id | pinned | text
```

`OR` binds tighter than the space between groups, so the example reads `budget AND (work OR q3)
AND NOT archived`. One shell argument is one term, so there is no escape syntax.

<details>
<summary>What each field matches, the uppercase <code>OR</code>, negation, and what cannot be expressed</summary>

`tag:` compares a tag whole, like `ls --tag`. `id:` takes any prefix and folds confusable
characters, like `noda show`. `title:` and `text:` are case-insensitive substrings. Any other
prefix is plain text, so `noda search https://example.com` works. `pinned:` takes only `true` or
`false` and refuses anything else, since `pinned:ture` would otherwise match every note.

`OR` must be uppercase, so `noda search or` finds the word. A leading `-` always negates; write
`text:--flag` to search for a leading hyphen. Quote a phrase in the shell:
`noda search "title:Q3 budget"`.

Any query in conjunctive normal form can be written (`a OR b c OR d` is `(a OR b) AND (c OR d)`).
`(a AND b) OR (c AND d)` cannot; it is two searches.

</details>

### Attachments

| Command | Description |
| --- | --- |
| `noda file add <path>... [--as <name>]` | Copy files into the active notebook. Auto-commits. |
| `noda file mv <old> <new> [--update-links]` | Rename one of the notebook's files. Auto-commits. |
| `noda file rm <name>` | Remove one of the notebook's files (a revertible commit). |

A notebook can hold files that are not notes: an image, a PDF, a receipt.

```
$ noda file add ~/Downloads/diagram.png
added  diagram.png
$ noda edit meeting-notes        # write: ![the shape of it](diagram.png)
```

A note uses a file through an ordinary Markdown link. Adding never overwrites a file the
notebook already holds; `--as <name>` stores it under another name (one file at a time). A name
longer than 255 bytes is refused, not cut. `noda file rm` refuses a note and points at `noda rm`.
Renaming lists which notes linked to the old name, and `--update-links` rewrites them as
`noda mv` does.

`noda ls` and `noda status` count files. `noda doctor --links` reads every note to report unused
files and stale or broken links; `--times` asks git whether a note was changed outside noda.
Neither repairs anything.

<details>
<summary>What those two checks print, what a rewrite touches, and how links are read</summary>

```
$ noda doctor --links
1 file no note links to
  receipt.txt
1 stale link
  k3f9m2p1-imported.md -> jjvgqnrv-meeting-notes.md
    now jjvgqnrv-weekly-sync.md
1 broken link
  b60ccfw0-reading-log.md -> cover.jpg

$ noda doctor --times
1 time cannot be read
  k3f9m2p1-imported.md created: last tuesday
1 note was changed outside noda
  b60ccfw0-reading-log.md
  git has a commit newer than the note's own `updated`
```

A **stale** link names a path a retitle moved, but still the id; fix it with
`noda mv <note> <its current title> --update-links`. An **unused** file may be a deliberate
receipt, and a **broken** link a typo or a file not yet added — only you know, so noda does not
guess. `README.md` is never counted as unused.

A rewrite — here or in `file mv` — changes only the destination; link text, title and a trailing
`#page=2` stay. The rename and rewrites are one commit, and notes are re-read afterwards, so a
destination that could not be located is reported rather than assumed fixed.

`--times` also reports a note changed before it was created and a value that cannot be read,
rather than refusing to work with them. A note changed with `--no-touch` is reported too — git
does have a newer commit than the note claims.

Links are read with a CommonMark parser, so reference-style links, `%20` in a destination and
links inside code fences are all handled correctly. A destination in raw HTML (`<img src=…>`) is
not followed, and only files at the notebook's root can be reported as unused, though a link
into a subdirectory resolves normally.

</details>

### Paths

| Command | Description |
| --- | --- |
| `noda path [<note-or-file>]` | Print where something lives. Omit the argument for the notebook itself. |

For the tools noda does not wrap. The argument resolves as a note first (id prefix or slug),
then as a file by name; a key that names both is an error listing both.

```sh
pandoc "$(noda path meeting-notes)" -o notes.pdf
cd "$(noda path)" && git log --stat
```

## Browsing

`noda tui` is the notebook on screen: nine screens, a query that narrows the listing as you type,
a `:` prompt taking noda's own subcommand names, and a queue for changing several notes in one
commit. **Every key that changes a note runs the command that does it** — `e` is `noda edit`,
`#` is `noda tag`.

**[Browsing in the terminal →](docs/tui.md)** — every key, every screen, and why the tag card
replaced typing `+work -q3`.

## In a browser

`noda web` serves the notebooks over HTTP, so a phone can read them. It renders on the server
and works with JavaScript off: the search box is a form, every row is a link.

```
$ noda web
noda is at http://127.0.0.1:8080
```

The listing is searched and ordered from one bar, a note's links work as they do on disk, and
times are shown in your own zone. The status screen says what `noda status` prints — `2 to
push`, `in sync` — and ends with the version of the build answering.

**There is no password on it, and there is not going to be one** — reach it over a tailnet or
behind something that authenticates. It listens on **this machine only** until `--listen` says
otherwise, refuses a request whose `Origin` is another site, and answers to a hostname only when
`--allow-host` names it (needed behind a reverse proxy).

**Ctrl-C or `SIGTERM` stops it cleanly**: it stops accepting, answers what is in flight, then
waits for a running `sync`. A second signal stops the waiting.

<details>
<summary>The order chips, links, whose day a date is, two people editing at once, and why a stop waits</summary>

Under the search field are four order chips — the default `slug` order and the three `--sort`
accepts. Pressing the one in force reverses it, like `-r`. The order rides in the address
(`?sort=updated`); the default writes nothing.

A relative link to another note opens that note, and a bare `https://` address is a link too. A
link off the notebook opens in a new tab and is sent no referrer, since the address holds a note
id.

Stamps are shown as the file spells them, and restated in your zone when JavaScript is on — the
server cannot know your zone.

An edit onto a note that changed underneath is merged against the version you started from, so
edits in different parts both land. Where you both changed the same lines, the page comes back
with git's `<<<<<<<` markers in one box to settle. The tags form removes only what you unticked,
never a tag added while the page was open. With JavaScript on, an open editor says when the file
changes — including from `noda edit` at a terminal.

A stop waits for a sync because a push killed halfway leaves git's lock file behind, and the
next write from anywhere fails on it.

</details>

**[In a browser →](docs/web.md)** — every screen, the security model in full, logging, and the
script layer that nothing depends on.

## Action items

A todo is a GFM checkbox in a note's body, readable anywhere Markdown is. `due:2026-08-10`
(todo.txt's `key:value` shape) is the only thing noda reads from an item's text.

```markdown
- [ ] send the revised contract due:2026-08-10
- [x] confirm the legal contact
```

```
$ noda todo
rgy2cwtw  q3-planning    2026-07-20  chase legal on the terms
r571tmze  meeting-notes  2026-08-10  send the revised contract
v69raz2x  reading-log                sort out the chapter-three notes
```

Soonest first; items with no date come last. **A date that has passed is coloured.** Ticked
items are not listed, and nothing is truncated. `--json` carries `id`, `slug`, `file`, `text`
and `due`.

<details>
<summary>Which day counts as passed, how a box is recognised, and why there is no <code>noda done</code></summary>

"Passed" means passed where you are. noda carries no timezone database, so it takes the offset
from git — the same one on every commit and every time noda prints. In a container, set `TZ` as
you would for `git`. `--json` has no "overdue" field; a program has its own clock.

Boxes are read with a CommonMark parser, so `- [ ]` inside a code fence is not an item and a
nested list still is.

**There is no `noda done`**: an item inside a note has no address, and giving each one an id
would make the file a noda-only format. `noda edit <note>`, type one `x`, and it auto-commits.
noda never moves a ticked item.

</details>

## Backlinks

`noda backlinks` lists what links *to* a note — the half the note itself cannot show you:

```
$ noda backlinks meeting-notes
mj8ajges  Q3 budget
2bn13xn0  Reading log
```

**It survives a retitle.** After `noda mv`, a link like `[the meeting](mj8ajges-meeting-notes.md)`
names a path that is gone, but it still names the id `mj8ajges`, which never moves — so noda still
counts it. It takes a file as readily as a note, like `noda path`.

<details>
<summary>What counts as a link</summary>

A link as CommonMark understands one: inline, reference-style or image, with anchors trimmed.
Not a `[[wiki-link]]`, a filename in prose, or a link inside a code fence. Three links from one
note are one backlink, and a note linking to itself is listed.

`-q` prints one note id per line, for `noda backlinks x -q | xargs -n1 noda show`. There is no
`-0`: an id has no spaces.

</details>

## History (git-backed)

| Command | Description |
| --- | --- |
| `noda log [<note>] [-n <count>]` | Show commit history for the notebook, or one note; marks what the remote has not seen. |
| `noda blame <note>` | Show which commit put each line of a note where it is. |
| `noda diff [<note>] [--remote]` | Show uncommitted or last-commit changes; `--remote` shows what a push would carry. |
| `noda deleted [--notebook <name>] [--json]` | List notes the notebook no longer holds, with the commit to restore each from. |
| `noda restore <note> <commit> [--no-touch]` | Restore a note to an earlier version (new commit). |
| `noda snapshot [<name>] [-m <text>]` | Name the notebook as it stands. Without a name, list what has been named. |

`log` follows a note across renames and marks with `↑` what the remote has not seen; `blame`
follows it too, picking the note out of each commit by its id. `restore` is always a new commit,
never a rewrite. `deleted` says which commit to bring a removed note back from.

**[History and sync →](docs/history.md)**

## Remote sync (HTTPS / SSH)

| Command | Description |
| --- | --- |
| `noda remote set <url>` | Set the active notebook's remote. |
| `noda remote show` | Print the configured remote. |
| `noda sync` | Pull, then push (auto-commits pending changes first). |
| `noda push` / `noda pull` | One-directional sync. |

HTTPS and SSH are compiled in; no system git, OpenSSL or libssh2 is needed. Where you stand is
always said in one vocabulary — `in sync`, `2 to push`, `3 to pull`, `never synced`,
`no remote` — measured against what the last sync left behind, so it never goes to the network.

**[Remote sync →](docs/history.md#remote-sync-https--ssh)** — credentials over both transports,
what a conflict looks like, and the tokens noda will not let leak.

## Config

| Command | Description |
| --- | --- |
| `noda config` | Show every setting, its value, and where that value came from. |
| `noda config <key>` | Print one setting's effective value. |
| `noda config <key> <value>` | Set it. |
| `noda config <key> --unset` | Remove it, going back to the default. |
| `noda config --edit` | Open `config.toml` in the editor. |

There are four settings; `noda init` writes a `config.toml` with all of them commented out.

| Setting | What it does | Where it looks first |
| --- | --- | --- |
| `editor` | Editor for `add` and `edit`. | `config.toml`, `$VISUAL`, `$EDITOR`, `vi` |
| `author` | Who commits, as `Name <email>`. | `config.toml`, your git config, `noda <noda@localhost>` |
| `notebook` | Which notebook `init` creates, and which one stands in when none is active. | `config.toml`, `default` |
| `sign` | Whether commits are GPG-signed. | `config.toml`, git's `commit.gpgsign`, off |

Setting a key edits the TOML in place, keeping your comments and layout.

### Signing

If your git config says `commit.gpgsign = true`, noda signs too, reading the same settings `git
commit` does. `noda config sign true|false|--unset` decides it for noda alone. **A commit that
cannot be signed is not made** — the command stops, leaving the note on disk and the history
untouched.

<details>
<summary>OpenPGP only, and the agent you will want</summary>

`gpg.format = ssh` or `x509` is refused at the commit rather than silently producing an unsigned
one. noda reads `user.signingkey` for the key and `gpg.openpgp.program`, then `gpg.program`, for
the binary, as `git commit` does. gpg runs once per commit, so keep an agent unlocked.

</details>

## Output

Colour appears on a terminal and nowhere else, so `noda show meeting-notes > backup.md` writes
the file byte for byte. `NO_COLOR=1` turns it off, `CLICOLOR_FORCE=1` keeps it through a pipe. It
marks structure — commit ids, timestamps, diff signs, a listing's columns — never a note's text.
There is no built-in pager: use `noda log | less -R`; quitting it early is not reported as an
error.

## Importing

| Command | Description |
| --- | --- |
| `noda import tiddlywiki <file>... [--no-convert]` | Import a TiddlyWiki 5 export: the JSON `export all` writes, or a saved single-file wiki. |

The format is named, never guessed. An import writes **two commits** — the original as the wiki
wrote it, then the conversion — so a bad conversion cannot lose the original. What could not be
converted stays as WikiText, listed in the note's `unconverted:` field.

**[Importing →](docs/importing.md)** — several files as one import, what converts, and how
times, tags and fields carry over.

## Storage layout

noda follows the [XDG Base Directory Specification](https://specifications.freedesktop.org/basedir-spec/latest/)
on **every** platform, macOS included; the `$XDG_*` variables always beat the defaults below.

```
$XDG_CONFIG_HOME/noda/          (default ~/.config/noda/)
└── config.toml                 # editor, author, default notebook, signing

$XDG_DATA_HOME/noda/            (default ~/.local/share/noda/)
└── notebooks/
    ├── work/                   # a notebook = a git repo
    │   ├── .git/
    │   ├── README.md           # the front page, written by `noda readme`
    │   ├── diagram.png         # a file put there by `noda file add`
    │   ├── k3f9m2p1-meeting-notes.md
    │   └── q7x2rstv-reading-log.md
    └── personal/

$XDG_STATE_HOME/noda/           (default ~/.local/state/noda/)
└── active                      # name of the currently active notebook
                                # (losing it falls back to config's `notebook`)

$XDG_CACHE_HOME/noda/           (default ~/.cache/noda/)
└── NOTE_EDITMSG.md             # scratch buffer while a note is open in $EDITOR
```

Only what you put in a notebook is committed; config, the active-notebook pointer and the
editor's scratch buffer stay out of your synced data.

## Roadmap

- **The web UI reads, writes and syncs**, with a script layer that the scriptless forms never
  depend on. See [In a browser](#in-a-browser).
- Encrypted notebooks are under consideration.

## Building from source

```sh
cargo build --release
# Cross-compile static Linux binaries (requires zig + cargo-zigbuild):
cargo zigbuild --release --target x86_64-unknown-linux-musl
cargo zigbuild --release --target aarch64-unknown-linux-musl

cargo nextest run
scripts/bench-coldstart.sh                # times whole processes, not in-process code
```

`noda --version` reports the tag the build came from via `git describe` at compile time (`0.2.0`,
or `0.2.0-3-gabc1234` three commits later), not `Cargo.toml`'s placeholder `0.1.0`. The container
build has no `.git`, so it is passed a `NODA_VERSION` build argument instead.

libgit2, OpenSSL and libssh2 are vendored into one static binary. Startup time is a feature, so
the release profile is tuned for size and cold start is measured. How the crate fits together:
[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## License

[MIT](LICENSE.txt) © 2026 Heng-Yi Wu
