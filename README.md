# noda

> A git-native notebook for your terminal. Your notes are plain Markdown in an ordinary
> git repository — versioned, syncable, and yours.

> **Written spec-first.** This README was the v1 contract before there was any code, AWS
> "working-backwards" style. Every command described below is now implemented and covered by
> tests; where the contract turned out to be wrong, the contract was corrected rather than
> quietly left behind. See [docs/PRFAQ.md](docs/PRFAQ.md).

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

That is a mouthful to type, so it is worth an alias:

```sh
alias noda='docker run --rm -it -v noda:/data ghcr.io/henry40408/noda:main'
```

Two things to know. `noda add` and `noda edit` open an editor, which the image does not
carry — write notes with `-c`, or mount one in. And `noda sync` over SSH needs a key: pass
your agent through with `-v "$SSH_AUTH_SOCK:/ssh-agent" -e SSH_AUTH_SOCK=/ssh-agent`, or use
an HTTPS remote with a token. `noda tui` needs the `-it` the alias above already carries;
without it there is no terminal on the other end and the command says so.

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

On `tag` the flag goes *before* the tags, which take every argument after them so that
`-q3` reads as a tag to remove rather than an option. Written after them it would arrive as
one more tag, so noda says where it belongs rather than accepting a command that did
nothing.

On `restore` it means something slightly stronger: `updated` comes back with the rest of
the version rather than being held aside, so the note ends up byte for byte the copy that
was asked for. `add` has no such flag — `created` and `updated` are both written the moment
a note is made, and a note nobody has changed was last changed when it was made. Write the
block yourself if it should say otherwise.

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
| `noda notebook ls` | List notebooks; marks the active one. |
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
| `noda tui` | Browse the notebook: the listing, that same query filtering as you type, and `Enter` to open what the cursor is on as a screen of its own. `e`, `a`, `m`, `#` and `Ctrl-d` run `edit`, `add`, `mv`, `tag` and `rm` on whatever the screen is about; mark several and the tag and delete keys queue up instead, to be sent as one commit. The view-shaped commands get screens of their own — `todo`, `tags`, `backlinks`, `log`, `blame`, `diff`, `deleted`, `files`, `notebooks` — the first four of those on a letter. `S`, `R` and `Ctrl-w` are `--sort`, `-r` and `-l` asked for from the other end, and `1`–`9` narrow to the commonest tags. `:` runs the commands that have no key, under the names they already have (`Ctrl-a` lists them). |
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

### Browsing

`noda tui` puts the notebook on a screen you can go into and come back out of. Reading a
notebook from the shell is `ls`, then `show`, then `ls` again to find your place; here the
listing keeps its place while you read, and a query narrows it as you type rather than once
you have finished typing it.

```
$ noda tui
Notebook: personal    <enter>  read       <e>  edit         <space>  mark        noda
Branch:   main        </>  filter         <a>  new          <*>  mark shown
Remote:   origin      <:>  command        <m>  retitle      <Q>  queue
Notes:    128 notes   <ctrl-a>  commands  <#>  tags         <T>  keep updated
Changes:  2 uncommitted <?>  keys         <ctrl-d>  delete  <q>  quit
Notes  tag:work budget  2 ───────────────────────────────────────── 3 marks  1 queued
    ID        TITLE                                                    TAGS
▌ • k3f9m2p1  Budget review                                            [work]
    7bqx4t20  Meeting notes                                            [work, q3]

 notes
/tag:work budget
```

Every screen is the same five bands: where the notebook stands and what the keys do here,
what this screen is of, the screen itself, how far down you are, and what was last said. The
row in the listing is the row every other listing prints — the id, the title, then the tags —
because a note is named the same way wherever noda names it.

Every screen names its columns along the top, the row under the cursor carries a bar down its
left-hand edge, and a screen with more on it than fits gets a bar down its right. The names
are there because `Ctrl-w` puts `created` and `updated` side by side and they are the same
twenty characters twice; the cursor is a bar rather than a reversed row because reversing one
inverts the id's yellow and the tags' cyan along with it, and the row you are looking at
should not be the one row whose columns have stopped being told apart by colour.

`Enter` opens what the cursor is on as a screen of its own, and `Esc` closes it again. The
listing keeps its cursor while you are down there, so coming back lands where you left.

```
Note  7bqx4t20  Meeting notes ─────────────────────────────────────────────────────
  ---
  title: Meeting notes
  tags: [work, q3]
  ---

  # Agenda
  - [ ] budget due:2026-08-10

 notes   7bqx4t20
```

A note is `noda show`: the frontmatter dimmed, your own text left alone. The one thing
painted over your prose is the search match, which is the exception `noda search` already
makes when it quotes a hit — and it survives the way down, so the word you searched for is
still marked in the note you opened to read it in.

| Key | |
| --- | --- |
| `j` / `k`, `↓` / `↑` | move the cursor, or scroll the note |
| `Ctrl-f` / `Ctrl-b`, `g` / `G` | half a screen, first / last |
| `Enter` | open what the cursor is on; while a query is being typed, keep it and put the keyboard back on the list |
| `Esc` | leave a prompt; close the screen you are on; otherwise drop the query, and once there is no query, the marks |
| `/` | filter, in the language `noda search` takes. There is no shell in front of the field, so it quotes like one: `tag:"12.34 foo bar"` |
| `:` | run a command by name: `:open meeting-notes`, `:tag reading-list +urgent`, `:status`, `:sync`. `Up` / `Down` — or `Ctrl-p` / `Ctrl-n` — walk what you have already typed |
| `Ctrl-a` | what `:` accepts, narrowed as you type — by what a command *does* as well as by its name, so `remote` finds `push` and `pull`. `Enter` puts one on the prompt |
| in a field | readline's keys, on readline's bindings: `Ctrl-a` / `Ctrl-e` and `Alt-b` / `Alt-f` to move, `Ctrl-w` / `Ctrl-u` / `Ctrl-k` to take a word or an end of the line out, `Ctrl-y` to put the last of those back. `Ctrl-p` / `Ctrl-n` are `Up` / `Down` |
| `Space`, `*` | mark the note under the cursor; mark everything the filter is showing (or take the marks off it) |
| `Q` | the queue: what is waiting to be sent, `d` to drop an entry, `Enter` to send |
| `e` | edit in `$EDITOR` |
| `a` | new note: a title along the bottom, then `$EDITOR` for the body (`Enter` on an empty title takes it from the body, as `noda add` does) |
| `m` | retitle, starting from the title it has |
| `#` | tags, written the way `noda tag` takes them: `+work -q3`. A tag may contain a space, so the prompt quotes like a shell: `-"24.04 Dark patterns"` |
| `Ctrl-d` | delete, once you have said `y`. With notes marked, `#` and `Ctrl-d` are aimed at the marked set and go into the queue instead |
| `T` | `--no-touch` for the rest of the session: changes stop moving `updated`. The title band says `keeping updated` for as long as it is on |
| `t`, `l` | the notebook's unticked boxes; commits — this note's, or the notebook's from the listing |
| `b`, `B` | what links to this note; who wrote each of its lines |
| `S`, `R` | the four orders `--sort` names, one press apiece; `-r`, which turns whichever one is in force |
| `Ctrl-w` | the whole row — `ls -l`'s columns, in `ls -l`'s places |
| `1` – `9` | narrow to one of the commonest tags; `0` lets go again. The tags screen numbers its first nine rows with these very keys |
| `Ctrl-g` | the crumb trail on and off, for a terminal that would rather have the row |
| `r` | read the notebook again |
| `?`, `q` / `Ctrl-C` | keys, quit |

`e`, `m`, `#` and `Ctrl-d` aim at whatever the screen is about — the row under the cursor on
the listing, and the note itself once you have opened it — so they read the same on either.
The delete is behind a modifier because it is the one key here that cannot be taken back by
pressing something else.

**`S`, `R` and `Ctrl-w` are the flags `noda ls` already has**, asked for from the other end.
At a prompt an order is written on the one `ls` it applies to; on a screen there is nothing
to write it on, so they are session settings like `T` — and the title band says which ones
are in force, because all three rearrange rows and leave nothing else behind to say why:

```
Notes  all  128  by updated reversed wide ─────────────────────── 3 marks  1 queued
```

They survive `r`. A read brings the notebook back in the walk's own order, and a setting that
came off every time you refreshed would not be a setting. Re-sorting keeps the cursor on the
note it was on rather than the row — re-sorting is asking where *this* note falls in a new
order, and being thrown to the top to find out would be a reason not to press the key.

`Ctrl-w` is `ls -l`: the same columns in the same places, extending the row rather than
rearranging it. When the terminal is too narrow for all of them they give way from the right,
one whole column at a time, because the id and the title are what name a note and everything
behind them is a density.

`1`–`9` narrow the listing to one of the commonest tags from wherever you are, and `0` lets
go. Nine because a notebook's tags are a long tail with a short head: the handful it actually
runs on are worth a keystroke apiece, and the hundred one-offs are what `/` is for. They are
the short version of what the tags screen's `Enter` does, which is why that screen numbers
its first nine rows with the very digits that reach them.

#### The other screens

A screen is the whole width and there is a stack of them, which is what lets a screen be
about something a listing cannot hold. `noda blame`, `noda log` and `noda diff` do not fit
beside a note at any width; given the width, each of them is a screen.

| | |
| --- | --- |
| `t` / `:todo` | every unticked box in the notebook, soonest due first, with a missed date in red. `Enter` reads the note it is in |
| `:tags` | every tag, commonest first, and how many notes carry it, with the first nine numbered. `Enter` narrows the listing to it rather than opening a screen — the notes are already down there |
| `b` / `:backlinks` | what links to the note in front of you. `Enter` reads the note that was found |
| `l` / `:log` | commits, newest first: the note's on a note screen, the notebook's on the listing |
| `B` / `:blame` | which commit put each line of a note where it is — the body only, because `updated` moves on every edit |
| `:diff` | what is uncommitted, or what the last commit did |
| `:deleted` | the notes history holds that the notebook no longer does |
| `:files` | what the notebook holds that is not a note. `Enter` asks what links to one, which is the question worth asking about an attachment |
| `:notebooks` | every notebook there is. `Enter` moves the whole session to one |

Four have a letter and five do not, on the same rule the keys already follow: the ones about
the note in front of you are worth a keystroke, because naming a note you are already looking
at is the thing a browser exists to avoid. The rest are named.

**A row that cannot be taken back writes the command instead of running it.** `Enter` on a
deleted note, or on a commit in a note's own history, puts `restore <note> <rev>` on the
prompt and stops there:

```
Deleted  1 ─────────────────────────────────────────────────────────────────────
  ID        SLUG       DELETED           FROM     TITLE
▌ v62b8rfa  trip-plan  2026-08-04 09:12  2a8715b  Trip plan

 notes   deleted
:restore v62b8rfa 2a8715b
```

Press `Enter` again and it runs. Landing on a row is not agreeing to write over the note it
names — the same bargain `Ctrl-a` makes, for a stronger reason. Nothing is rewritten either
way: `restore` puts the old text back as a new commit, so the version you moved away from is
still there to move back to.

Moving to another notebook is refused while the queue has anything in it. A queued change
names notes by id, and an id belongs to the notebook it was minted in; sent against another
one it would find nothing, or find the wrong thing.

**`:` is how the rest of noda gets in.** There are about a dozen letters worth spending on
keys and rather more subcommands than that, so the ones that do not get a letter are named
instead — under the names they already have:

```
:open meeting-notes          # a note by id or by slug, without going to find it
:tag reading-list +urgent    # naming the note, which the `#` key has no way to do
:log budget-review           # a screen about a note you are not looking at
:status                      # and everything else that was never going to get a key
:doctor --links
:sync
```

The names are noda's own subcommands; a browser that invented a second vocabulary for the
same commands would be a second thing to learn. `Ctrl-a` lists them with what each one takes,
and searches their descriptions as well as their names — type `remote` and you get `push` and
`pull`. What a name refers to is the notebook's question, not the browser's: `:open k3f` is
resolved by the same code `noda show k3f` uses, so an id prefix that matches two notes is
refused here in the same words it would be refused at the prompt.

Two things `:` deliberately does not do. `:rm` takes no note — a delete is only worth asking
about for a note you can see, so it removes the one on screen and `:open` is how you put
another one there. And `:doctor` reports and stops: it will adopt files and write to the
notebook when you ask it to at the prompt, but a browser is not where you want to find out
that a keystroke rewrote a directory.

**Everywhere you type, the keys are readline's.** The query, the prompt and the `:` line are
one field wearing three labels, and it answers the bindings a shell prompt answers: `Ctrl-a`
and `Ctrl-e` for the ends of the line, `Ctrl-b` / `Ctrl-f` and `Alt-b` / `Alt-f` for a
character and a word, `Ctrl-w`, `Ctrl-u` and `Ctrl-k` for taking a word or an end of it out,
`Ctrl-y` for putting the last of those back, `Ctrl-d` and `Delete` forwards. They are not a
feature to learn — they are there so that a hand which has typed at shell prompts for twenty
years does not have to stop and find out this field is different. A chord the field does not
bind does nothing at all rather than typing its own letter, which is the trap here: `Ctrl-d`
arrives as `d` with a modifier on it, and a field that took it at face value would put a `d`
in the middle of somebody's title.

Two places where the browser and readline part company, both deliberate. `Ctrl-c` leaves the
browser instead of abandoning the line — `Esc` is what abandons a line here, and a program
that argues with `Ctrl-c` is one you end up killing from another window. And `Ctrl-p` /
`Ctrl-n` walk the command history at `:` where there is one, and the list of notes while a
query is being typed, where there is not.

**Every key that changes a note runs the command that changes it.** `e` is `noda edit`, `#`
is `noda tag`, `Ctrl-d` is `noda rm` — so a change made here is validated, stamped and committed
exactly as one made at the prompt, and the line along the bottom afterwards is the line that
command would have printed. There is no second implementation of what a change means, which
is the whole reason the keys are wired this way rather than to a writer of their own. Two
consequences worth knowing: `$EDITOR` gets the terminal to itself while it runs, as it would
from the shell; and a command that refuses says why on a card, because the reason — where an
edit with a broken frontmatter block was left, say — is the part worth reading.

`--no-touch` is a setting here rather than something said per change. At a prompt you write it
on the one command it applies to; on a screen there is nowhere to qualify a single keystroke,
and the reason for wanting it — a sitting of small corrections to notes whose dates came from
somewhere else — outlasts one keystroke anyway. `T` turns it on for the session, `e`, `m` and
`#` follow it, and the header carries `keeping updated` until you turn it off.

#### Changing several notes at once

Marking and searching are separate, and neither undoes the other. `Space` marks the note under
the cursor and `*` marks everything the query is showing, so "narrow to what I mean, take the
lot, search again" builds a selection out of several searches. A note the query is currently
hiding is still marked and still gets changed — otherwise marking would only ever mean "what
is on screen right now", which is what the query already means.

With notes marked, `#` and `Ctrl-d` stop acting on the note under the cursor and start filling a
queue: one entry per change, each aimed at the notes that were marked when it was added. The
header counts both, because a key that means two things has to say which one it means.

```
personal  (main)  128 notes  12 marked  2 queued
```

`Q` reads the queue back, `d` drops an entry, and `Enter` sends it:

```
╭ queued ─────────────────────────────────────────────────────────╮
│tag: -q3 (12 notes)                                              │
│tag: +archive (12 notes)                                         │
│                                                                 │
│Enter  send, in one commit       d  drop this one       Esc  back│
╰─────────────────────────────────────────────────────────────────╯
```

**A queue arrives in the history as one commit**, because a queue is one intention: "these
twelve notes are no longer q3" is a thing you did, and twelve commits saying so is a history
that buries the fact under the work of carrying it out.

```
$ noda log -n 1
8f2a1c9  2026-08-06 22:31  bulk: 2 changes over 12 notes
```

What a change *means* is not restated to make that possible — `noda tui` hands the queue to
the same code `noda tag` and `noda rm` use, with the commit boundary moved out one level. Every
tag is parsed before anything is written, so a queue with one bad change in it leaves the
notebook untouched; a note that disappeared from another window while the queue was being
built is reported underneath what did go through. Nothing is asked before sending unless the
queue deletes something, and then it is asked once — queueing a delete deletes nothing, so
the question belongs at the last moment it can still be answered no.

`q` asks before leaving with a queue still in it. The queue is the one thing a session holds
that is written down nowhere: a query can be retyped and a mark remade, but an afternoon of
queued changes goes with the process. `Ctrl-C` is the exception and leaves without asking —
a program that argues with `Ctrl-C` is one you end up killing from another window.

It deliberately **does not watch the filesystem**: a note written from another window arrives
when you press `r`, rather than rearranging the list under a reader mid-sentence.

It needs a terminal at both ends and says so rather than filling a pipe with escape
sequences:

```
$ noda tui | less
noda: noda tui needs a terminal at both ends; `noda ls`, `noda search` and `noda show` are the ones to redirect
```

### In a browser

`noda web` serves the notebooks over HTTP, so a phone can read them. It renders on the
server and needs no JavaScript at all: the search box is a form, every row is a link.

```
$ noda web
noda is at http://127.0.0.1:8080
```

Every notebook is named in the URL — `/nb/work` for the listing, `/nb/work/n/k3f9m2p1` for a
note — and the active-notebook pointer is never consulted. That pointer belongs to a shell
session, and a browser tab that quietly changed which notebook it was showing because
something happened in a terminal would be worse than a longer address. A note's slug and any
prefix of its id both redirect to the id, so a bookmark survives a retitle.

**There is no password on it, and there is not going to be one.** It is meant to be reached
over a tailnet or from behind something that already does authentication, and both of those
do that job better than a notebook could. What follows from that is the whole of its security
model:

- It listens on **this machine only** until told otherwise. `--listen 0.0.0.0:8080` opens it
  to the network, and says so on the way up.
- It refuses a request whose `Origin` is another site. There is no session to be missing, so
  without that check a page on any other site could make your browser commit to your
  notebook.
- It answers to **addresses and `localhost` freely, and to a hostname only when told**.
  That is the DNS-rebinding half: an attacker who points `evil.example` at `127.0.0.1` sends
  requests your browser considers same-origin — `Origin` and `Host` agree, because both say
  `evil.example` — and the only thing that gives it away is that a rebinding attack needs a
  *name*. So a name has to be asked for:

```sh
noda web --listen 0.0.0.0:8080 --allow-host noda.tail1234.ts.net
```

  Behind a reverse proxy, name whatever the browser puts in the address bar. The refusal says
  exactly what to add.

It writes as well as reads: a note can be started, its body rewritten, its title changed, its
tags ticked on and off, and it can be deleted. Every one of those runs the command that does
it — the same `add`, `mv`, `tag` and `rm` the terminal calls — so what a change *means* has
one implementation, and each lands as its own commit with the same message it would have had.

The tags form ticks off what should go and takes the new ones in one field, separated by
spaces — `ops docs "24.04 Dark patterns"` adds three, quoted the way the `:` prompt and the
search box quote, and the whole change is one commit.

**An edit carries the note's fingerprint, and a stale one is refused.** The form remembers
what the file hashed to when the page was drawn; if it hashes to something else by the time
you save, nothing is written and the page comes back with both versions on it — what is saved
now, above what you typed, still in a box you can edit. That is the whole reason for the check:
an edit begun on a phone at breakfast must not flatten one made at a terminal at lunch, and a
refusal that threw away what you had just written would only be a politer way of losing it.

The fingerprint is the file's git blob id and not its `updated` stamp, because `--no-touch`
exists — a note's content can change without its stamp moving, which is exactly the case that
version marker would be wrong in.

**A note is rendered, and its links go where they went on disk.** A relative link to another
note — `[the plan](k3f9m2p1-the-plan.md)`, the spelling that works on a git host and in any
editor — becomes a link to that note's page, coloured the way an id is coloured everywhere
else in noda, so a link that stays inside the notebook looks different from one that leaves.
A link to anything else the notebook holds becomes a download, and an image is shown where it
stands.

`/nb/<book>/files` lists everything that is not a note: how big it is, what it will arrive as,
and how many notes point at it — a file nothing points at is exactly what `doctor --links`
calls an orphan.

Two things a note's body cannot do, both on purpose. **Raw HTML is shown as a code block**
rather than rendered: `noda import tiddlywiki` deliberately leaves markup it could not convert
in the body, so that markup is the only copy of what the note said, and dropping it would lose
it. **A destination carrying a scheme noda does not serve — `javascript:` first among them —
keeps its words and loses its link.** Files are served with `nosniff` and a content policy that
loads nothing, and only the formats that cannot carry a script are shown in place: SVG is a
document that runs script, so it arrives as a download.

Syncing from the browser is not there yet.

### Action items

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

The boxes are read with a CommonMark parser, not searched for as text, for the same reason
`doctor --links` is — `- [ ]` inside a fenced code block is prose *about* a checkbox, and a
list nested three deep is still a list.

**There is no `noda done`.** Ticking a box needs an address noda does not have: a note is
addressed by its id or its slug, and an item inside one by nothing. Line numbers move, text
prefixes collide, and giving every item an id would turn the file into a format only noda
can read — which is the one thing choosing checkboxes was meant to avoid. `noda edit <note>`
types one `x` and auto-commits. Nor does noda ever move a finished item: a ticked line stays
where its author wrote it.

### Backlinks

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

### History (git-backed)

| Command | Description |
| --- | --- |
| `noda log [<note>] [-n <count>]` | Show commit history for the notebook, or one note. |
| `noda blame <note>` | Show which commit put each line of a note where it is. |
| `noda diff [<note>]` | Show uncommitted or last-commit changes. |
| `noda deleted [--notebook <name>] [--json]` | List notes the notebook no longer holds, with the commit to restore each from. |
| `noda restore <note> <commit> [--no-touch]` | Restore a note to an earlier version (new commit). |
| `noda snapshot [<name>] [-m <text>]` | Name the notebook as it stands. Without a name, list what has been named. |

`noda log <note>` follows a note across renames, because every commit records the filenames
and the id is one of them — no rename guessing involved. Nothing is capped: `-n` is there
when you want less.

`noda blame <note>` answers the other question about a note's past — not "what happened to
it" but "when did I write *this*":

```
$ noda blame q3-planning
8abf00e  2026-08-02 11:47  # Meeting notes
8abf00e  2026-08-02 11:47
8abf00e  2026-08-02 11:47  - Q3 budget signed off
89bb210  2026-08-02 11:47  - hire two engineers
0000000  not committed     - draft still open
```

Two things it does that `git blame` on the same file will not.

**It reaches past a rename.** The note above was called `meeting-notes` when its first lines
were written, and `noda mv` renamed the file when the title changed — yet those lines are
still credited to the commit that wrote them, not to the rename. git's own blame can follow
a rename by guessing at content similarity; libgit2's cannot at all, since every one of its
rename-tracking options is documented as not implemented. noda needs neither: the note is
picked out of each commit **by id** rather than by path, so a rename never comes up. Line
history is followed through the diffs, and a filename is never part of the question.

**It says which lines are not committed yet**, marking them `0000000` — what a note edited
outside noda looks like before anything picks the change up.

Only the body is blamed. `updated` is rewritten on every edit, so blaming the frontmatter
would put a block of identical commits above the prose and make every note look as though it
was written all at once. There are no line numbers: nothing else in noda prints one, and in
prose the unit you are looking for is a paragraph.

`noda diff` shows uncommitted changes when there are any, and otherwise what the last
commit changed — noda commits as it goes, so a clean notebook is the normal state and
"what just happened" is the useful answer. The output is a plain unified diff with nothing
wrapped around it, so `git apply` will take it.

`<commit>` is anything git accepts: a full or abbreviated id, `HEAD~3`, a tag, a branch.
A restore is a new commit, never a rewrite, and a note keeps the name it has now — only its
contents travel back. It also works on a note you removed: `noda restore <slug> HEAD~1`
brings it back with its id intact, which is the friendly face of "`noda rm` is a commit you
can revert".

`noda snapshot` is how you get a name worth passing to it. It marks the notebook as it
stands, so a moment can be cited later instead of counted back to:

```
$ noda snapshot 2026-q3 -m 'end of quarter'
snapshot: 2026-q3 -> 4953133
$ noda snapshot
2026-q3   2026-08-02 10:14  4953133  end of quarter
$ noda restore meeting-notes 2026-q3
```

It is a git annotated tag, so it records who took it and when — a lightweight tag is a bare
pointer and would list as an empty row. It commits the working tree first, on the same terms
as `noda sync`: a snapshot that quietly left out what is on disk would be a snapshot of
something nobody has. And it never moves one that already exists, because a name that can be
reassigned is not one anything else can cite; `git tag -d <name>` in the notebook is there if
you meant to.

Snapshots travel with the notebook — `noda push` sends them, `noda pull` brings them down —
so a name means the same thing on every machine. When it cannot, noda says so instead of
choosing: if the remote already has that name for another commit, the snapshot is held back
and the notes go anyway.

```
$ noda push
push: main -> git@github.com:me/notes.git
snapshot `q3` was not sent — the remote already has that name for another commit; rename
yours, or drop it with `git tag -d q3`
```

`noda deleted` is how you find out what there is to bring back, most recently lost first:

```
$ noda deleted
2kpas2d8  meeting-notes  2026-08-02 02:40  ff9062f  Meeting notes
qzdt88kk  old-draft      2026-08-02 02:40  6918cec  Old draft
`noda restore <note> <commit>` with the commit above brings one back
```

The revision in each row is not the commit that did the deleting — it is the one before it,
the last that still held the note, so it is what `restore` takes as it stands. The slug and
title are read from that commit too; there is no file left to read them from.

It works by comparing trees, not by reading commit messages. A commit's tree is a complete
list of filenames, and a note's identity *is* its filename — so the notes that existed at
any commit are read straight off it without opening a single blob. Three things follow. A
rename is not a deletion, because `noda mv` changes the slug and leaves the id alone. A note
deleted and later restored is not listed, because the comparison is against what is on disk
now. And a deletion made with plain `git rm` is found exactly like one made with `noda rm`,
because nothing here cares what the commit message said.

It walks all of history, which is why it is a command of its own rather than a flag on
`noda ls` — that one reads a directory, and the two costs should not share a name.

`--json` makes the whole thing scriptable, and carries the object ids in full because an
abbreviation is a thing that can stop being unique later:

```sh
noda deleted --json | jq -r '.deleted[] | "noda restore \(.slug) \(.restore_from)"'
```
```
noda restore old-draft 4953133a9f2e154d8bcc11672de7503c77862c71
noda restore meeting-notes a40b843e5d29a008fe8a3124cd9a1b7b705570d2
```

`removed_at` is RFC 3339 UTC there, the same spelling a note's own `created` and `updated`
use, so a script never meets two ways of writing a time. The table shows it in the zone the
commit was made in, which is a question a person asks and a program should not have to.
Unlike the table, `--json` prints a document even when nothing has been deleted — an empty
list is an answer. `--notebook` looks at one you are not currently in.

### Remote sync (HTTPS / SSH)

| Command | Description |
| --- | --- |
| `noda remote set <url>` | Set the active notebook's remote. |
| `noda remote show` | Print the configured remote. |
| `noda sync` | Pull, then push (auto-commits pending changes first). |
| `noda push` / `noda pull` | One-directional sync. |

HTTPS and SSH are built in; no system git or OpenSSL is required at runtime. Credentials
are not noda's to keep: SSH keys come from `ssh-agent`, HTTPS from git's credential helper.
The helper is looked up in the notebook's own `.git/config` as well as `~/.gitconfig`,
`~/.config/git/config` and `/etc/gitconfig`, so one notebook can authenticate differently
from the rest. Those four are the whole list: noda carries its own libgit2 rather than
calling `git`, so a helper your `git` reads from its installation's own `etc/gitconfig` —
where a packaged build may well have put `credential.helper = osxkeychain` for you — is
invisible to noda and has to be repeated in one of the files above.

Carrying its own libgit2 has a second consequence, and it is the one that surprises people:
**noda runs no git hooks.** libgit2 does not run them at all, so a `pre-commit` in your
notebook fires under `git commit` and does nothing under `noda add` — same file, same
repository, different outcome. `noda doctor` says so when it finds one, and if you want a
hook to run, run the command through git: `cd "$(noda path)"` and commit there.

GPG signing has the same root cause and is the one case noda makes up for itself, by
calling gpg the way git would — see [Signing](#signing).

A pull fast-forwards when only the remote moved, and makes a merge commit when both sides
did. Two notebooks that each added a note produce two different filenames, so there is
nothing to conflict over and nothing for noda to reconcile afterwards. A conflict inside a
note — the same note edited on both sides — is yours: the merge is rolled back, the notebook
is left exactly as it was, and you can resolve it with git in the notebook directory.

`noda sync` commits the whole working tree and needs no guard before it. There is nothing
derived to fall out of step with the notes, so there is no state in which committing
everything would make a disagreement permanent and remote.

### Config

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

### Output

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

The format is named rather than sniffed. Guessing wrong would import somebody's notes as the
wrong thing, quietly, which is the one failure an import must not have.

```
$ noda import tiddlywiki notes.json
imported  1693 notes from tiddlywiki
converted 1678 notes

left as WikiText, and named in each note's `unconverted:` field:
  915 notes macro
  239 notes transclusion
  29 notes table

not imported:
  337 system tiddler
  12 not text (image/webp)
```

### A wiki exported in pieces

Several files are one import rather than several, because a wiki taken in pieces has links
running between the pieces:

```
$ noda import tiddlywiki part1.json part2.json part3.json
```

Every file is read before anything is written, so one that will not parse stops the import
before it has touched the notebook and says which file it was. Exports taken in pieces
overlap, and a note given twice arrives once — the first copy lands, the second is reported.

Bringing a wiki in over several sittings works too: the link rewriting starts from what the
notebook already holds, so a note imported today can link to one that arrived last week. What
cannot resolve is a link to a tiddler no import has brought in yet, and that is left as
WikiText and named, like everything else that could not be finished.

### Two commits, so nothing can be lost

An import writes **two** commits: the first holds every note exactly as the wiki wrote it,
the second holds the conversion.

```
$ noda log -n 2
7d1016e  2026-08-02 22:50  import: convert 1678 notes from tiddlywiki
bb81bb7  2026-08-02 22:50  import: 1693 notes from tiddlywiki
```

So `noda diff` shows you the whole conversion before it goes anywhere, and
`noda restore <note> HEAD~1` brings any note back to the text the export actually contained.
The original is not copied into the frontmatter, because git already keeps it and keeps it
better — the same reasoning behind every other command here being a commit.

### What converts, and what does not

`''bold''`, `//italic//`, `!` headings, `*`/`#` lists, `<<<` quotes, `[[links]]`, `[img[…]]`
and fenced code all have a Markdown form, so they get one. A link's target is a tiddler
*title* and noda's is a *filename*, which is why the rewrite is the second pass: the ids do
not exist until the notes do.

Anything Markdown has no word for — a transclusion, a macro, a widget, a table with a footer
or a merged cell — is **copied through as WikiText, exactly as it was written**, and named in
the note's own frontmatter:

```
---
title: Some note
source_key: Some Note
unconverted: macro, table
---
```

Unconverted WikiText is findable and fixable. Markdown that looks right and says something
else is neither, so nothing here is guessed.

`noda doctor` is the handle on that field, and needs no flag for it — the frontmatter is
already parsed:

```
$ noda doctor
3 notes carry text an importer did not convert
  3 notes macro
  1 note table
  for example:
    k3f9m2p1-some-note.md
```

It is a frontmatter field rather than a tag because tags belong to whoever writes the notes;
filing noda's paperwork among them would be noda using your drawer. Delete the field once a
note is dealt with and the count goes down.

`--no-convert` writes the first commit and stops, leaving the WikiText for you.

### Times, tags and fields

TiddlyWiki's `created` and `modified` are `YYYYMMDDhhmmssXXX` in UTC; they become RFC 3339
with their milliseconds intact, and noda never restates them again. A `tags` field is a title
list, so `[[26.04 Occam's razor]]` arrives as one tag with its spaces. Every other field the
wiki had — `creator`, `modifier`, whatever you invented — is carried into the frontmatter
untouched, and `source_key` records what the wiki called the note, which is what makes a
second import say "already imported" instead of making a second copy.

What is not a note is reported rather than dropped: system tiddlers under `$:/`, pictures and
other binaries, empty tiddlers, and anything carrying a title or a tag noda's own files
cannot spell.

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

- **The web UI reads and writes.** `noda web` is here — see [In a browser](#in-a-browser).
  Rendering Markdown, showing attachments, and syncing from the browser are still to come.
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

## License

[MIT](LICENSE.txt) © 2026 Heng-Yi Wu
