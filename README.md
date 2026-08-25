# noda

> A git-native notebook for your terminal. Your notes are plain Markdown in an ordinary
> git repository — versioned, syncable, and yours.

> **Written spec-first.** This README was the v1 contract before there was any code, AWS
> "working-backwards" style. Every command described below is now implemented and covered by
> tests; where the contract turned out to be wrong, the contract was corrected rather than
> quietly left behind. See [docs/PRFAQ.md](docs/PRFAQ.md).

---

**Start here** — [Why noda](#why-noda) · [Install](#install) · [Quickstart](#quickstart) · [Concepts](#concepts)

**The commands** — [Notebooks](#notebooks) · [Notes](#notes) · [Attachments](#attachments) · [Paths](#paths) · [Config](#config) · [Output](#output)

**The two other ways in** — [In the terminal](#browsing) · [In a browser](#in-a-browser)

**What a note can carry** — [Action items](#action-items) · [Backlinks](#backlinks)

**Because it is git** — [History](#history-git-backed) · [Remote sync](#remote-sync-https--ssh) · [Signing](#signing) · [Storage layout](#storage-layout)

**Coming from elsewhere** — [Importing](#importing) · [Building from source](#building-from-source) · [Roadmap](#roadmap)

**In full** — [Browsing in the terminal](docs/tui.md) · [In a browser](docs/web.md) · [History and sync](docs/history.md) · [Importing](docs/importing.md) · [Architecture](docs/ARCHITECTURE.md)

Each section here says what a command does; the four documents above carry the reasoning
behind them. A ▸ folds away a decision's rationale where it is short enough to keep in place.

---

## Why noda

- **Just git.** Every notebook is a normal git repo of Markdown files. No lock-in, no
  proprietary format. Anything noda does, plain `git` can inspect and undo.
- **Automatic history.** Every change is committed for you. `noda log` shows a note's
  history; `noda restore` rewinds it.
- **Sync anywhere.** HTTPS and SSH are compiled into the binary, so `noda sync` talks to
  GitHub, GitLab, or any git host with nothing else to install.
- **Fast to reach.** Address a note by a short id *or* a readable slug.
- **One static binary.** Builds self-contained for macOS and Linux (incl. arm64/musl), and
  ships as a container image.

## Install

noda is distributed as a container image on GitHub's registry, for `linux/amd64` and
`linux/arm64`:

```sh
docker pull ghcr.io/henry40408/noda:main
```

Your notebooks live in a volume, and the image runs `noda` directly, so anything the CLI
does works through it:

```sh
docker run --rm -v noda:/data ghcr.io/henry40408/noda:main init
docker run --rm -v noda:/data ghcr.io/henry40408/noda:main add "Meeting notes" -c "agenda"
docker run --rm -v noda:/data ghcr.io/henry40408/noda:main ls
```

<details>
<summary><b>Living with the image</b> — an alias, and the three things a container cannot do for you</summary>

That is a mouthful to type, so it is worth an alias:

```sh
alias noda='docker run --rm -it -v noda:/data ghcr.io/henry40408/noda:main'
```

Two things to know. `noda add` and `noda edit` open an editor, which the image does not
carry — write notes with `-c`, or mount one in. And `noda sync` over SSH needs a key: pass
your agent through with `-v "$SSH_AUTH_SOCK:/ssh-agent" -e SSH_AUTH_SOCK=/ssh-agent`, or use
an HTTPS remote with a token. `noda tui` needs the `-it` the alias above already carries;
without it there is no terminal on the other end and the command says so.

</details>

Otherwise, build it:

```sh
cargo build --release        # target/release/noda
```

There is no crates.io package and no Homebrew formula.

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

**Notebook.** A git repository under `$XDG_DATA_HOME/noda/notebooks/<name>/`. You can have many; one is
"active" at a time. Each notebook has its own remote, so `work` and `personal` can live
in different places. A new notebook starts on the branch your `init.defaultBranch` names,
the same one `git init` would have picked, so it agrees with the remote you push it to.

**Note.** A Markdown file inside a notebook, named `<id>-<slug>.md`:
- the **id** — a short, stable code (Crockford base32, e.g. `k3f9m2p1`). It is unique
  within the notebook and never changes, even across renames. Ids are lowercase and
  matched case-insensitively; Crockford maps the easily-confused `I`/`L` to `1` and `O`
  to `0`, so a mistyped id still resolves to the right note;
- the **slug** — a human-readable name derived from the title, which changes if you
  retitle the note.

The identity is the filename, and nothing else records it. git will not put two entries
under one path in a tree, so uniqueness is structural rather than something noda has to
police — and two machines that each add a note write two different filenames, which git
merges without asking anyone to resolve anything. The frontmatter carries what you wrote —
the title and the tags — and when the note was written:

```
---
title: Reading notes on TAOCP
tags: [books, algorithms]
created: 2019-03-14T08:21:00Z
updated: 2024-11-02T16:40:12Z
---
```

`created` is set once and never moves again. `updated` follows every change noda makes:
`add`, `edit`, `tag`, `mv` and `restore`. Renaming an attachment does not count — it
rewrites links in notes you did not point the command at, and dating them all today would
flatten the order you read them in.

`--no-touch` opts one command out of it, on `edit`, `tag`, `mv` and `restore`. The change
still lands and still commits; only the note's own claim about itself is left alone. It is
for the changes that are not the note being rewritten — a typo, a tag, a title that was
wrong from the start — and, above all, for a note that arrived carrying dates from
somewhere else:

```
$ noda edit imported --no-touch          # the 2019 date it came with is still the date it has
$ noda tag imported --no-touch +archived
```

<details>
<summary>Where <code>--no-touch</code> goes on <code>tag</code>, what it means on <code>restore</code>, and why <code>add</code> has none</summary>

On `tag` the flag goes *before* the tags, which take every argument after them so that
`-q3` reads as a tag to remove rather than an option. Written after them it would arrive as
one more tag, so noda says where it belongs rather than accepting a command that did
nothing.

On `restore` it means something slightly stronger: `updated` comes back with the rest of
the version rather than being held aside, so the note ends up byte for byte the copy that
was asked for. `add` has no such flag — `created` and `updated` are both written the moment
a note is made, and a note nobody has changed was last changed when it was made. Write the
block yourself if it should say otherwise.

</details>

<details>
<summary>Why the timestamps live in the frontmatter — and how to write your own, for notes that arrived with dates</summary>

They live in the file because that is the only place that survives a `clone`. The
filesystem's own timestamps do not: git does not record them, so a fresh checkout stamps
every note with the moment you cloned it. git's history does survive, but it only knows
when a note reached *this* notebook — import a note written in 2019 and history will
truthfully tell you it arrived today.

Which is the other half of why they live there: **you can write them yourself**. A note
imported from somewhere else keeps the times you give it. Anything RFC 3339 is read, offset
and all, and noda does not restate it — write `2019-03-14T16:21:00+08:00` and that is what
stays in the file. noda writes UTC when it writes one itself.

The same goes for every other field. noda reads `title`, `tags`, `created` and `updated`,
and leaves the rest of the block alone: a note that came from somewhere else keeps whatever
came with it rather than losing it to the first command that rewrites the note.

A note without the fields keeps not having them. noda will not invent a `created` it was
never told — the only sources it could invent one from are the two that just failed.

</details>

Anywhere a command takes `<note>`, pass either the id or the slug. A slug is matched whole;
an id is matched by any prefix that names exactly one note, the same bargain git makes with
object ids — so `noda show k3f9` works. Ambiguity is an error listing the candidates, never
a guess.

**History.** Because storage is git, every add/edit/rm is a commit. Nothing is a
destructive surprise — `noda rm` is a commit you can revert.

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

`noda rm` (a note) is a commit you can revert. `noda notebook rm` is not — it deletes the
repository and its whole history from disk. The active notebook is refused outright; switch
with `noda use` first. Everything else is confirmed at the terminal, and `--force` skips the
question. With no terminal to ask at — piped, or in a script — the deletion is refused
rather than assumed, so `--force` is how a script says it meant it.

`noda status` answers "where do I stand" without going to the network — the push/pull
counts are measured against what the last sync left behind, so it works offline and
returns instantly.

```
notebook  work  (main)
notes     42
changes   1 file uncommitted
remote    git@github.com:me/work-notes.git
sync      2 to push (as of the last sync)
```

It also walks the notebook for what noda will not settle on its own. Two things decide what
a `*.md` file is, and they are independent: a **frontmatter block** is the file saying "I am
a note", and an **id in the filename** is it having been adopted. A file with both is a
note. A file with neither is just a file — an attachment, a `README` — and it is counted on
the `files` row, never reported as a problem. The other two combinations are what the
`problems` row reports, and it is only there when there is something to say:

```
problems  1 note has no id in its filename  (hand-written.md)
```

Problems are counted by kind rather than listed one at a time. A directory of notes copied
in at once makes every one of them a problem together — `status` has to stay one screen
through that, and "201 notes have no id in their filenames" tells you what happened where
201 filenames would not. Where more than one kind turns up, the total comes first:

```
problems  2 problems
          1 note has no id in its filename  (hand-written.md)
          1 file is named like a note but has no frontmatter  (abcdefgh-hello.md)
          run `noda doctor` to look at these
```

`noda doctor` is where the full list that `status` elides can be seen, and it performs
the one repair that cannot lose anything: a file that already declared itself a note and
only lacks an id is given one. That is a commit like any other change, so `git revert`
undoes it, and `--dry-run` shows what would happen without touching anything.

```
$ noda doctor --dry-run
1 note has no id in its filename
  hand-written.md
would adopt 1 note — nothing was changed
```

The other two it reports and leaves alone, because either answer loses something that
cannot be minted again. **One id on two notes** — which two machines can produce without
ever meeting — means keeping one identity by discarding the other; rename one of the files
to settle it. **A name that claims an id over a file with no frontmatter** might be a note
that lost its frontmatter or a file that was never one: add the `---` block back, or rename
it so it no longer starts with an id. Only their author knows which.

It also reports the **git hooks that will never run**, which needs no flag because it costs
one directory read:

```
$ noda doctor
1 git hook will never run
  pre-commit
noda carries its own libgit2 and never calls git, which is what would run them
```

Exactly the hooks git itself would reach for: `core.hooksPath` when it is set, the
executable bit, and never the `*.sample` files a fresh repository ships. A hook that git
would not run either is not noda's doing and is not reported. This stays out of `noda
status` on purpose — a script left in `.git` is not something the notebook holds.

A file that will not parse does not lock you out of the commands that do not read it.
`restore`, `rm`, `log` and `diff` identify a note by its filename alone, so they work on one
whose frontmatter has gone — which is exactly when they are wanted. `mv` and `tag` rewrite
the frontmatter, so they still have to read it first and say so plainly.

**`noda readme`** writes the one file a notebook needs for a reader who is not you. Push a
notebook to GitHub, GitLab, Codeberg or Gitea and the front page is whatever `README.md`
says — without one it is a wall of `k3f9m2p1-*.md` with nothing to explain them. It is a
separate command rather than a flag on `notebook add` because the day a notebook wants a
README is the day it goes somewhere people can see, which is rarely the day it was created.

```
$ noda readme
wrote README.md in `work`
```

What it writes is fixed prose: what the filenames mean, what the frontmatter fields are,
that none of it needs noda to be read, and how to clone it back. Every line stays true
however many notes arrive. It deliberately does **not** write an index of the notes — that
would be wrong from the next `noda add` onward, and `noda ls` is that list, always current.
Everything under the trailing comment is yours; a second run refuses rather than overwrite
it, and `--force` replaces the file as a commit you can revert.

Because it is addressed to a reader outside the notebook, `doctor --links` never counts it
as a file no note links to. The only way to clear such a report would be to link the front
page from a note, which reads backwards.

### Attachments

| Command | Description |
| --- | --- |
| `noda file add <path>... [--as <name>]` | Copy files into the active notebook. Auto-commits. |
| `noda file mv <old> <new> [--update-links]` | Rename one of the notebook's files. Auto-commits. |
| `noda file rm <name>` | Remove one of the notebook's files (a revertible commit). |

A notebook holds files that are not notes: an image a note shows, a PDF you want kept with
what you wrote about it, a receipt parked where you will find it again. `noda file add` puts
one there and commits it; nothing about a notebook requires knowing where on disk it lives.

```
$ noda file add ~/Downloads/diagram.png
added  diagram.png
$ noda edit meeting-notes        # write: ![the shape of it](diagram.png)
```

The command says nothing about notes, and takes no note as an argument. Which note uses a
file is written in that note's prose, as an ordinary Markdown link — which is also what
makes the note render correctly in anything else that reads Markdown.

Adding a file never overwrites one the notebook already holds; `--as <name>` stores it under
a different name instead. `noda file rm` refuses a note and points at `noda rm`, because a
note has an identity to lose and a file does not.

Renaming a file leaves every link that named it pointing at nothing, so `noda file mv`
always says which notes those are:

```
$ noda file mv IMG_4821.png diagram.png
renamed  IMG_4821.png -> diagram.png
2 notes link to IMG_4821.png
  3fmwhh8y-meeting-notes.md
  qs6vpx2s-reading-log.md
```

`--update-links` rewrites them instead. It is opt-in because it edits the prose of notes the
command was not pointed at, which nothing else in noda does. Only the destination's bytes
change — the link text, the title and a trailing `#page=2` are left where they were — and
the notes are re-read afterwards, so a destination that could not be located is reported
rather than assumed fixed.

```
$ noda file mv IMG_4821.png diagram.png --update-links
renamed  IMG_4821.png -> diagram.png
updated  2 notes
```

`noda ls` lists these under their own heading, and `noda status` counts them on the `files`
row. Both are free: the walk that finds the notes passes them anyway.

```
$ noda ls
jjvgqnrv  Meeting notes  [work, q3]
b60ccfw0  Reading log

files
  diagram.png
  receipt.txt
```

The id and the title, because the title is the answer to "which note is this". The slug is
the same words with the spaces taken out, so a column of it beside the title says everything
twice. `search` and `backlinks` name a note the same way, for the same reason — there is one
shape for naming a note.

`-l` shows the whole row: the slug and both timestamps as well. One flag rather than one per
column — `ls(1)` settled that a long format is a density, not a selection, and there is no
syntax to invent. Nothing here costs anything to read: `ls` has already parsed the
frontmatter to get the title, and the slug is in the filename.

```
$ noda ls -l --sort updated
b60ccfw0  Reading log    reading-log    2019-03-14T08:21:00Z  2024-11-02T16:40:12Z
jjvgqnrv  Meeting notes  meeting-notes  2026-08-02T09:14:00Z  2026-08-02T09:14:00Z  [work, q3]
k3f9m2p1  Imported       imported       -                     -
```

<details>
<summary>Why <code>-l</code> only extends the row, and what each column's colour is doing</summary>

**`-l` extends the row, it does not rearrange it.** The id and the title are the first two
columns either way, so `noda ls | cut -c1-8` and anything else that reads off the front says
the same thing with the flag as without it. Tags are last in both, and for the reason they
have to be: they are the one thing a note may not have, so anywhere but the end and their
absence would shift every column behind them.

Each column is coloured, so a row can be told apart without counting fields. The id takes the
same yellow `log` gives a commit id — both are the short string you copy out of a listing and
hand to the next command — and the slug takes that colour a step down, because the two side
by side are the note's filename. The timestamps are grey, as they are everywhere else in
noda; the tags, the one column that groups notes rather than naming one, get a hue of their
own. The title is left uncoloured, which is what makes it the column the eye lands on — and
it is the note's own words, which noda does not paint anywhere. `NO_COLOR=1` turns all of it
off, and a pipe has none of it to begin with.

</details>

`--sort created|updated|title` puts the listing in order — the times newest first, the title
alphabetically.

`-r` runs the listing the other way. It is applied after the sort, so it turns whichever
order was asked for — `--sort title -r` is Z to A, `--sort updated -r` is oldest first — and
on its own it turns the default one, which is what `ls(1)` means by `-r` and why it does not
require `--sort`. The notebook's files turn with the notes: it is one listing on one screen,
and a table whose top half runs newest-first while its bottom half runs A-to-Z is not an
order anyone asked for.

Sorting reads the stamps rather than comparing them as text, so a note imported with
`+08:00` lands where it belongs rather than where its digits fall. A note with no time to
sort by sorts last — and first under `-r`, since reversing an order reverses all of it.
`--json` carries every field whether or not `-l` was passed — `created` and `updated` are
`null` when the note has neither — because what a program reads should not depend on a flag
about what fits on a terminal.

What is *not* free is the other question — which files are actually used, and which links
actually resolve. Answering it means reading every note's prose rather than its filename, so
it is a flag rather than the default:

```
$ noda doctor --links
1 file no note links to
  receipt.txt
1 stale link
  k3f9m2p1-imported.md -> jjvgqnrv-meeting-notes.md
    now jjvgqnrv-weekly-sync.md
1 broken link
  b60ccfw0-reading-log.md -> cover.jpg
```

Nothing here is repaired, and the three lines are three different questions. A file nothing
links to may be an attachment whose note was deleted, or a receipt you parked here on purpose
— and the only repair available is deleting something git cannot regenerate from anything
else. A **broken** link names nothing at all: a typo, or a file you have not added yet, and
only you know which. `README.md` is the one file never counted here: it is written for a
reader outside the notebook, so no note was ever supposed to link to it.

A **stale** link is the one noda can answer. `noda mv` moves the slug half of a note's
filename, so a destination written before the retitle names a path that is gone — and still
names the id, which never moves, which is still exactly one note. The report says which note
it now is, rather than filing it with the links nobody can resolve. Repairing one is
`noda mv <note> <its current title> --update-links`, which is where the rewrite lives — see
below.

`--times` is the other check that has to be asked for, and it exists because `updated` has
one break it cannot avoid: a note edited outside noda changes without noda getting to
record that it did. git is the only witness, and asking it means walking all of history.

```
$ noda doctor --times
1 time cannot be read
  k3f9m2p1-imported.md created: last tuesday
1 note was changed outside noda
  b60ccfw0-reading-log.md
  git has a commit newer than the note's own `updated`
```

It also catches a note changed before it was created, and a value nothing can read — which
is reported rather than refused, because a typo in a date must not come between you and
your own prose. Nothing here is repaired: the only thing noda could do about a stale
`updated` is overwrite your record of your own work with a guess.

A note you changed with `--no-touch` is reported here too, and correctly: git does have a
commit newer than what the note claims. That is the flag working, not a fault — the check
says what git knows, and you are the one who decided the note's own date should not move.

The links are read with a CommonMark parser rather than searched for as text, because the
alternative reports files as unused when they are not: a reference-style link keeps its
destination at the bottom of the file, so the paragraph using it never contains the
filename; `%20` in a destination is a space in a filename; and a link inside a fenced code
block is prose about a link, not a link. Two limits are worth knowing: a destination written
as raw HTML (`<img src="...">`) is passed through by CommonMark and is not followed, and
only files at the notebook's root can be reported as unused, though a link *into* a
subdirectory resolves normally.

### Paths

| Command | Description |
| --- | --- |
| `noda path [<note-or-file>]` | Print where something lives. Omit the argument for the notebook itself. |

noda does not wrap the rest of your toolchain, so it tells you where things are and gets out
of the way. The argument is resolved the way noda resolves anything: as a note first, by id
prefix or slug, and then as one of the notebook's files by name.

```sh
pandoc "$(noda path meeting-notes)" -o notes.pdf
open "$(noda path diagram.png)"
cd "$(noda path)" && git log --stat
```

A key that names both a note and a file is an error listing both, never a guess.

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
| `noda search <term>...` | Search the active notebook. Terms may name a field, be `OR`ed, or be negated. |
| `noda tui` | Browse the notebook: the listing, that same query filtering as you type, and `Enter` to open what the cursor is on as a screen of its own. `e`, `a`, `m`, `#` and `Ctrl-d` run `edit`, `add`, `mv`, `tag` and `rm` on whatever the screen is about — `#` a card of every tag the notebook has, a box apiece, rather than a line to write `+work -q3` on; mark several and the tag and delete keys queue up instead, to be sent as one commit. The view-shaped commands get screens of their own — `todo`, `tags`, `backlinks`, `log`, `blame`, `diff`, `deleted`, `files`, `notebooks` — the first four of those on a letter. `S`, `R` and `Ctrl-w` are `--sort`, `-r` and `-l` asked for from the other end, and `1`–`9` narrow to the commonest tags. `:` runs the commands that have no key, under the names they already have (`Ctrl-a` lists them). |
| `noda todo [--json]` | List every unticked `- [ ]` in the notebook, soonest due first. |
| `noda backlinks <note\|file> [--json\|-q]` | List the notes that link to a note or a file. |

`<note>` accepts an id (`k3f9m2p1`, or any prefix naming exactly one note) or a slug
(`meeting-notes`, matched whole). Two notes may share a slug — the id in front of it keeps
their filenames apart — and then the slug alone is ambiguous and noda asks which you meant.

`noda tag` takes signed tags — `noda tag meeting-notes +q3 -work` adds `q3` and removes
`work`. Adding a tag a note already has is not an error; it just leaves nothing to commit.

`noda mv` retitles a note and the filename follows, so the notes that linked to it are left
naming a path that is gone. It says which, and `--update-links` rewrites them instead:

```
$ noda mv meeting-notes "Weekly sync"
jjvgqnrv  weekly-sync
1 note links to jjvgqnrv by an older name
  k3f9m2p1-imported.md

$ noda mv weekly-sync "Weekly sync" --update-links
jjvgqnrv  weekly-sync
updated  1 note
```

The second command retitles nothing, which is the point: the flag means *make the links to
this note say the name it has*, so it repairs what an earlier rename left behind as readily as
what this one would. The match is on the id rather than on the filename just left, so a link
two renames behind is caught too — the same fact `noda backlinks` is built on.

It is opt-in for the reason `noda file mv --update-links` is: it edits the prose of notes the
command was not pointed at, which nothing else in noda does. The rename and the rewrites land
in one commit. Nothing is assumed fixed either — the notes are read back afterwards, and one
whose link could not be rewritten is reported rather than counted. The retitled note is a note
like any other here, so a link it makes to itself is rewritten too.

A title has to fit on one line, and a tag cannot contain `,`, `[`, `]` or a line break —
the frontmatter writes both verbatim, and a value carrying its punctuation would read back
as something else. noda says so when you try, rather than writing a note it cannot read.

`noda search` looks through every note's title, tags and body in the active notebook. It
matches case-insensitively and by substring rather than by word — Chinese and Japanese have
no spaces to split on, and a word-based search would simply find nothing in them. Results
are listed the way `ls` lists them, and a hit in the body quotes the line it was found on.

A bare word searches the title, the tags and the body together, and several of them mean all
of them, in any order. A term can also name one field, be `OR`ed with the next, or be ruled
out with a leading `-`:

```
$ noda search budget tag:work OR tag:q3 -tag:archived
s33wpe5y  Q3 planning  [work, q3]
          the budget and the hiring plan
```

The grammar is four lines and stays that way:

```text
query := term-group…                 every group must match
group := term ('OR' term)…           any term in the group will do
term  := ['-'] [field ':'] value
field := tag | title | id | text
```

`OR` binds tighter than the space between groups, so the example above reads as
`budget AND (work OR q3) AND NOT archived` — the way somebody listing alternatives for one
field means it. That precedence is what makes parentheses unnecessary rather than missing:
`a OR b c OR d` already says `(a OR b) AND (c OR d)`, which is any query at all in
conjunctive normal form. What it cannot say is `(a AND b) OR (c AND d)`; that is two
searches, and it is the price of a grammar you can hold in your head.

Four details worth knowing:

- **Each field matches the way noda already matches that thing.** `tag:` compares a tag
  whole, like `ls --tag`. `id:` takes any prefix and folds the confusable characters, like
  `noda show k3f9`. `title:` and `text:` are case-insensitive substrings, like the rest of
  search.
- **`OR` must be uppercase**, so that `noda search or` can still find the English word.
- **The shell does the quoting.** One argument is one term, so `noda search "title:Q3
  budget"` searches that title for that phrase, and no escape syntax had to be invented.
- **A leading `-` is always a negation**, so a term that really starts with one is written
  `text:--flag`. The field prefix is the escape.

An unknown prefix is not an error: `noda search https://example.com` looks for that text,
because only `tag`, `title`, `id` and `text` are fields and everything else is punctuation
in a word.

`noda ls` prints an aligned table for a person to read. Two other shapes are for programs.
`--json` is one object on one line, and each note carries its filename as well as its id and
slug — that is what a script needs next, and deriving it means knowing noda's naming rule:

```
$ noda ls --json | jq -r '.notes[] | select(.tags[]? == "work") | .file'
k7n5qz9k-q3-planning.md
```

`-q` prints one identifier per record and nothing else — a note's id, a file's name, because
those are what the commands taking them expect. `-0` separates those with NUL instead of a
newline, which is not decoration: `noda file add` allows a space in a name, so
newline-separated output is not safe to hand to `xargs`.

```sh
noda ls -q0 --files-only | xargs -0 -n1 file
noda ls -q --notes-only | xargs -n1 noda show
```

`--notes-only` and `--files-only` narrow any of the three shapes to one half of the
notebook. Filtering beyond a single `--tag` belongs to `noda search`, which is where the
query language lives — one language in one command beats two commands nobody can tell apart.

`noda add` and `noda edit` open `$VISUAL`, falling back to `$EDITOR` and then to `vi`.
`edit` opens the real file, frontmatter included, but refuses to commit an edit that breaks
the frontmatter — the file is left as you saved it so you can fix it or throw it away with
`git checkout`. An edit cannot change *which* note it is editing: the id is in the filename,
and the editor is handed the file.

## Browsing

`noda tui` puts the notebook on a screen you can go into and come back out of. The listing
keeps its place while you read a note, a query narrows it as you type rather than once you
have finished, and **every key that changes a note runs the command that changes it** — `e`
is `noda edit`, `#` is `noda tag` — so there is no second implementation of what a change
means. Tags are chosen from a card of the notebook's own, a box apiece, because `+work -q3`
is a notation and a notation is a thing to misspell. Nine screens, a `:` prompt that takes
noda's own subcommand names, readline's bindings in every field, and a queue for changing
several notes in one commit.

**[Browsing in the terminal →](docs/tui.md)**

## In a browser

`noda web` serves the notebooks over HTTP, so a phone can read them. It renders on the
server and works with JavaScript turned off: the search box is a form, every row is a link.

```
$ noda web
noda is at http://127.0.0.1:8080
```

A note's links work the way they do on disk — a relative link to another note opens that
note — and an address a note only mentions is a link as well, which CommonMark says it is not
and every other Markdown you read says it is. Anything pointing off the notebook opens in a
tab of its own and is told nothing about where it was pressed: an address here holds a note's
id, and it does not travel.

A note also says when it was made and when it last changed, and the browser says both again in
the zone you are standing in — a listing's day too. The page itself carries what the file
carries, offset and all, so with JavaScript off you get the frontmatter's own spelling rather
than a guess: an instant is not a day until somebody says where they are, and a request does
not say.

**There is no password on it, and there is not going to be one.** It is meant to be reached
over a tailnet or from behind something that already authenticates. So it listens on **this
machine only** until `--listen` says otherwise, refuses a request whose `Origin` is another
site, and answers to a hostname only when `--allow-host` has named it — which is the
DNS-rebinding half, and what you will need behind a reverse proxy.

**[In a browser →](docs/web.md)** — every screen, the security model in full, what it logs
and how to turn it up, and the script layer that nothing depends on.

## Action items

A todo is a GFM checkbox in a note's body — not a note, and not a file of its own:

```markdown
## Action items

- [ ] send the revised contract due:2026-08-10
- [x] confirm the legal contact
- [ ] align on timing with Alice
```

That syntax was chosen for the same reason attachments are plain Markdown links: it renders
as a checkbox in anything else that reads Markdown, and it stays readable in the file when
nothing does. `due:2026-08-10` is todo.txt's `key:value` shape — plain text to every other
parser, and the only thing noda reads out of an item's prose.

```
$ noda todo
rgy2cwtw  q3-planning    2026-07-20  chase legal on the terms
r571tmze  meeting-notes  2026-08-10  send the revised contract
r571tmze  meeting-notes              align on timing with Alice
v69raz2x  reading-log                sort out the chapter-three notes
```

Soonest first; items with no date come last, because a date is a claim about when something
has to happen and an item without one has made no claim. **A date that has passed is
coloured** — it is the one thing anybody scans a todo list for. "Passed" means passed where
you are: nobody writes `due:2026-08-10` meaning UTC. noda carries no timezone database, so
it asks git for the offset instead — the same one stamped on every commit, and the same one
every time noda prints is rendered in. In a container, set `TZ` as you would for `git`.

Ticked items are not listed, and nothing is ever truncated — a list that cuts the sentence
off is a list you have to open the note to read anyway. `--json` carries `id`, `slug`,
`file`, `text` and `due`, and prints a document even when there is nothing to do. It does
not carry "overdue": a program has its own clock.

<details>
<summary>How a box is recognised, and why there is no <code>noda done</code></summary>

The boxes are read with a CommonMark parser, not searched for as text, for the same reason
`doctor --links` is — `- [ ]` inside a fenced code block is prose *about* a checkbox, and a
list nested three deep is still a list.

**There is no `noda done`.** Ticking a box needs an address noda does not have: a note is
addressed by its id or its slug, and an item inside one by nothing. Line numbers move, text
prefixes collide, and giving every item an id would turn the file into a format only noda
can read — which is the one thing choosing checkboxes was meant to avoid. `noda edit <note>`
types one `x` and auto-commits. Nor does noda ever move a finished item: a ticked line stays
where its author wrote it.

</details>

## Backlinks

What a note points *at* is in the note — `noda show` prints it, and every Markdown reader
renders it. What points at the note is the half nothing could tell you:

```
$ noda backlinks meeting-notes
mj8ajges  Q3 budget
2bn13xn0  Reading log
```

**It survives a retitle.** `noda mv` moves the slug half of a note's filename, so
`[the meeting](mj8ajges-meeting-notes.md)` is left naming a path that no longer exists unless
the rename was asked to rewrite it. Every Markdown renderer calls that a broken link. noda does
not have to: the destination still names `mj8ajges`, and the id is the half that never moves —
the same fact `log`, `blame`, `deleted` and `mv --update-links` are built on. Matching on the
whole filename would have been the easier build and the wrong feature, because backlinks would
go quiet after every retitle, which is exactly when you are looking for what points at a note.

It takes a file as readily as a note, like `noda path` — "which notes use this diagram" and
"which notes link to this note" are one question asked of two kinds of thing.

A link is a link as CommonMark understands one: inline, reference-style, image, and anchors
trimmed off. A `[[wiki-link]]` is not one (noda has no such syntax, and it would not render
anywhere else either), a filename written in prose is not one, and neither is a link inside a
fenced code block. A note that links to the same place three times is one backlink, and a note
that links to itself is listed — that is what the file says.

`-q` prints one note id per line, for `noda backlinks x -q | xargs -n1 noda show`. There is no
`--null` beside it: what it prints is an id, and an id has no spaces to protect.

## History (git-backed)

| Command | Description |
| --- | --- |
| `noda log [<note>] [-n <count>]` | Show commit history for the notebook, or one note; marks what the remote has not seen. |
| `noda blame <note>` | Show which commit put each line of a note where it is. |
| `noda diff [<note>] [--remote]` | Show uncommitted or last-commit changes; `--remote` shows what a push would carry. |
| `noda deleted [--notebook <name>] [--json]` | List notes the notebook no longer holds, with the commit to restore each from. |
| `noda restore <note> <commit> [--no-touch]` | Restore a note to an earlier version (new commit). |
| `noda snapshot [<name>] [-m <text>]` | Name the notebook as it stands. Without a name, list what has been named. |

Because storage is git, every add, edit and rm is a commit, and nothing is a destructive
surprise. `log` follows a note across renames and marks with `↑` what the remote has not
seen; `blame` reaches past a rename because a note is picked out of each commit by its id
rather than its path; `restore` is always a new commit, never a rewrite. A note you removed
is still in there — `deleted` says which commit to bring it back from.

**[History and sync →](docs/history.md)**

## Remote sync (HTTPS / SSH)

| Command | Description |
| --- | --- |
| `noda remote set <url>` | Set the active notebook's remote. |
| `noda remote show` | Print the configured remote. |
| `noda sync` | Pull, then push (auto-commits pending changes first). |
| `noda push` / `noda pull` | One-directional sync. |

HTTPS and SSH are compiled into the binary, so this works with no system git, OpenSSL or
libssh2 installed. **Where you stand against the remote is said in one vocabulary wherever
it is said** — `in sync`, `2 to push`, `3 to pull`, `never synced`, `no remote` — and none
of those goes to the network: they are measured against what the last sync left behind,
which is what makes them instant and correct on a train.

**[Remote sync →](docs/history.md#remote-sync-https--ssh)** — credentials over both
transports, what a conflict looks like, and the tokens noda will not let leak.

## Config

| Command | Description |
| --- | --- |
| `noda config` | Show every setting, its value, and where that value came from. |
| `noda config <key>` | Print one setting's effective value. |
| `noda config <key> <value>` | Set it. |
| `noda config <key> --unset` | Remove it, going back to the default. |
| `noda config --edit` | Open `config.toml` in the editor. |

There are four settings, and `noda init` leaves a `config.toml` with all of them commented
out so you can see what there is to change.

| Setting | What it does | Where it looks first |
| --- | --- | --- |
| `editor` | Editor for `add` and `edit`. | `config.toml`, `$VISUAL`, `$EDITOR`, `vi` |
| `author` | Who commits, as `Name <email>`. | `config.toml`, your git config, `noda <noda@localhost>` |
| `notebook` | Which notebook `init` creates, and which one stands in when none is active. | `config.toml`, `default` |
| `sign` | Whether commits are GPG-signed. | `config.toml`, git's `commit.gpgsign`, off |

The config file beats `$VISUAL` and `$EDITOR`, the way git's `core.editor` does: the
environment is a blanket default for every program you use, while `config.toml` is a
decision about this one. `noda config <key> <value>` writes through a real TOML editor, so
the comments and layout you put in the file survive it.

### Signing

If your git config says `commit.gpgsign = true`, noda signs too — `noda add`, `noda edit`,
`noda mv`, the merge commit a `noda pull` makes, all of them. It reads the same settings
`git commit` does: `user.signingkey` for the key, `gpg.openpgp.program` then `gpg.program`
for the binary, and `gpg.format` to decide it is being asked for OpenPGP at all.

```sh
noda config sign true      # sign notes even if nothing else you commit is signed
noda config sign false     # leave notes unsigned even though everything else is
noda config sign --unset   # go back to whatever commit.gpgsign says
```

Two caveats. **OpenPGP only:** `gpg.format = ssh` or `x509` is refused by name at the
commit rather than quietly producing an unsigned one — a commit that was asked to be signed
and is not is indistinguishable afterwards from one nobody asked about. And **a commit that
cannot be signed is not made**: if gpg fails or is not installed, the command stops and
says so, leaving the note on disk and the history untouched.

Signing runs gpg once per commit, so a notebook you write to constantly will want an
unlocked agent — the same arrangement `git commit` needs.

## Output

Colour appears on a terminal and nowhere else: redirect or pipe any command and the escape
sequences are gone, so `noda show meeting-notes > backup.md` writes the file byte for byte.
`NO_COLOR=1` turns it off everywhere, `CLICOLOR_FORCE=1` keeps it through a pipe. Colour
marks structure — commit ids, timestamps, diff signs, a listing's columns, a note's
frontmatter — and never the text of a note itself.

There is no built-in pager. `noda log | less -R` is a pager, and quitting it early is
handled quietly rather than reported as a broken pipe.

## Importing

| Command | Description |
| --- | --- |
| `noda import tiddlywiki <file>... [--no-convert]` | Import a TiddlyWiki 5 export: the JSON `export all` writes, or a saved single-file wiki. |

The format is named rather than sniffed — guessing wrong would import somebody's notes as
the wrong thing, quietly, which is the one failure an import must not have. An import writes
**two commits**: the first holds every note exactly as the wiki wrote it, the second is the
conversion, so nothing a converter gets wrong can take the original with it. What it could
not convert stays as WikiText and is named in the note's own `unconverted:` field.

**[Importing →](docs/importing.md)** — several files as one import, what converts and what
does not, and how times, tags and fields carry over.

## Storage layout

noda follows the [XDG Base Directory Specification](https://specifications.freedesktop.org/basedir-spec/latest/).
Files are split by role across four base directories, and the `$XDG_*` environment
variables always take precedence over the defaults shown below.

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
        ├── .git/
        └── ...

$XDG_STATE_HOME/noda/           (default ~/.local/state/noda/)
└── active                      # name of the currently active notebook
                                # (losing it falls back to config's `notebook`)

$XDG_CACHE_HOME/noda/           (default ~/.cache/noda/)
└── NOTE_EDITMSG.md             # scratch buffer while a note is open in $EDITOR
```

Each notebook is a normal git repo; `cd "$XDG_DATA_HOME/noda/notebooks/work" && git log`
works exactly as you'd expect. Nothing but what you put there is committed — noda keeps no
bookkeeping file of its own, which is why there is none in the listing above. Only your
notes and the files beside them live in `XDG_DATA_HOME` too: config, the active-notebook
pointer, and the editor's scratch buffer are kept out of your synced data on purpose.

**Platform note.** noda honors the XDG variables on **every** platform, including macOS
(it does not use `~/Library/Application Support`). If a variable is unset, the standard
`~/.config`, `~/.local/share`, `~/.local/state`, and `~/.cache` defaults apply.

## Roadmap

- **The web UI reads, writes and syncs**, and the enhancement layer over it has landed: a
  listing that filters as you type, a network screen that fetches instead of reloading whole,
  backlinks beside the prose on a wide screen, and a search and a press of back that replace
  the rows rather than the page — none of which gives up the form that works with no script
  at all. See [In a browser](#in-a-browser) and
  [the script layer](docs/web.md#and-a-script-on-top-that-nothing-depends-on).
- Encrypted notebooks are under consideration.

## Building from source

```sh
cargo build --release
# Cross-compile static Linux binaries (requires zig + cargo-zigbuild):
cargo zigbuild --release --target x86_64-unknown-linux-musl
cargo zigbuild --release --target aarch64-unknown-linux-musl
```

libgit2, OpenSSL, and libssh2 are vendored and compiled from source, producing a single
static binary with HTTPS/SSH sync built in.

Startup time is a feature — a quick `noda ls` costs more in process startup than in work
— so the release profile is tuned for size and cold start is measured, not assumed:

```sh
cargo nextest run
scripts/bench-coldstart.sh                # times whole processes, not in-process code
```

How the crate is put together — the layering, three worked paths through it, the concurrency the
web server needs, and where to add a command or a screen — is
[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## License

[MIT](LICENSE.txt) © 2026 Heng-Yi Wu
