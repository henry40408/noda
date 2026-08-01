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
an HTTPS remote with a token.

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

A file that will not parse does not lock you out of the commands that do not read it.
`restore`, `rm`, `log` and `diff` identify a note by its filename alone, so they work on one
whose frontmatter has gone — which is exactly when they are wanted. `mv` and `tag` rewrite
the frontmatter, so they still have to read it first and say so plainly.

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
jjvgqnrv  meeting-notes  Meeting notes
b60ccfw0  reading-log    Reading log

files
  diagram.png
  receipt.txt
```

`--time` adds the two timestamps, and `--sort created|updated|title` puts the listing in
order — the times newest first, the title alphabetically. Both are cheap: `ls` has already
read each note's frontmatter to get its title, so the times come with it.

```
$ noda ls --time --sort updated
b60ccfw0  reading-log    2019-03-14T08:21:00Z  2024-11-02T16:40:12Z  Reading log
jjvgqnrv  meeting-notes  2026-08-02T09:14:00Z  2026-08-02T09:14:00Z  Meeting notes
k3f9m2p1  imported       -                     -                     Imported
```

Sorting reads the stamps rather than comparing them as text, so a note imported with
`+08:00` lands where it belongs rather than where its digits fall. A note with no time to
sort by sorts last. `--json` carries `created` and `updated` whether or not `--time` was
passed — they are `null` when the note has neither — because what a program reads should
not depend on a flag about what fits on a terminal.

What is *not* free is the other question — which files are actually used, and which links
actually resolve. Answering it means reading every note's prose rather than its filename, so
it is a flag rather than the default:

```
$ noda doctor --links
1 file no note links to
  receipt.txt
1 broken link
  b60ccfw0-reading-log.md -> cover.jpg
```

Both are reported and neither is repaired. A file nothing links to may be an attachment
whose note was deleted, or a receipt you parked here on purpose — and the only repair
available is deleting something git cannot regenerate from anything else. A link that names
nothing may be a typo, or a file you have not added yet.

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
| `noda ls [--tag <t>] [--notebook <name>] [--json\|-q [-0]] [--notes-only\|--files-only] [--time] [--sort <field>]` | List what the notebook holds. |
| `noda show <note>` | Print a note to stdout. |
| `noda edit <note>` | Open a note in `$EDITOR`; auto-commits on save. |
| `noda rm <note>` | Delete a note (as a revertible commit). |
| `noda mv <note> <new-title>` | Rename a note (updates slug; id is preserved). |
| `noda tag <note> [+tag]... [-tag]...` | Add/remove tags. |
| `noda search <term>...` | Search the active notebook. Terms may name a field, be `OR`ed, or be negated. |

`<note>` accepts an id (`k3f9m2p1`, or any prefix naming exactly one note) or a slug
(`meeting-notes`, matched whole). Two notes may share a slug — the id in front of it keeps
their filenames apart — and then the slug alone is ambiguous and noda asks which you meant.

`noda tag` takes signed tags — `noda tag meeting-notes +q3 -work` adds `q3` and removes
`work`. Adding a tag a note already has is not an error; it just leaves nothing to commit.

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
s33wpe5y  q3-planning  Q3 planning  [work, q3]
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

### History (git-backed)

| Command | Description |
| --- | --- |
| `noda log [<note>] [-n <count>]` | Show commit history for the notebook, or one note. |
| `noda diff [<note>]` | Show uncommitted or last-commit changes. |
| `noda deleted [--notebook <name>] [--json]` | List notes the notebook no longer holds, with the commit to restore each from. |
| `noda restore <note> <commit>` | Restore a note to an earlier version (new commit). |

`noda log <note>` follows a note across renames, because every commit records the filenames
and the id is one of them — no rename guessing involved. Nothing is capped: `-n` is there
when you want less.

`noda diff` shows uncommitted changes when there are any, and otherwise what the last
commit changed — noda commits as it goes, so a clean notebook is the normal state and
"what just happened" is the useful answer. The output is a plain unified diff with nothing
wrapped around it, so `git apply` will take it.

`<commit>` is anything git accepts: a full or abbreviated id, `HEAD~3`, a tag, a branch.
A restore is a new commit, never a rewrite, and a note keeps the name it has now — only its
contents travel back. It also works on a note you removed: `noda restore <slug> HEAD~1`
brings it back with its id intact, which is the friendly face of "`noda rm` is a commit you
can revert".

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

There are three settings, and `noda init` leaves a `config.toml` with all of them commented
out so you can see what there is to change.

| Setting | What it does | Where it looks first |
| --- | --- | --- |
| `editor` | Editor for `add` and `edit`. | `config.toml`, `$VISUAL`, `$EDITOR`, `vi` |
| `author` | Who commits, as `Name <email>`. | `config.toml`, your git config, `noda <noda@localhost>` |
| `notebook` | Which notebook `init` creates, and which one stands in when none is active. | `config.toml`, `default` |

The config file beats `$VISUAL` and `$EDITOR`, the way git's `core.editor` does: the
environment is a blanket default for every program you use, while `config.toml` is a
decision about this one. `noda config <key> <value>` writes through a real TOML editor, so
the comments and layout you put in the file survive it.

### Output

Colour appears on a terminal and nowhere else: redirect or pipe any command and the escape
sequences are gone, so `noda show meeting-notes > backup.md` writes the file byte for byte.
`NO_COLOR=1` turns it off everywhere, `CLICOLOR_FORCE=1` keeps it through a pipe. Colour
marks structure — commit ids, timestamps, diff signs, a note's frontmatter — and never the
text of a note itself.

There is no built-in pager. `noda log | less -R` is a pager, and quitting it early is
handled quietly rather than reported as a broken pipe.

## Storage layout

noda follows the [XDG Base Directory Specification](https://specifications.freedesktop.org/basedir-spec/latest/).
Files are split by role across four base directories, and the `$XDG_*` environment
variables always take precedence over the defaults shown below.

```
$XDG_CONFIG_HOME/noda/          (default ~/.config/noda/)
└── config.toml                 # editor, author, default notebook

$XDG_DATA_HOME/noda/            (default ~/.local/share/noda/)
└── notebooks/
    ├── work/                   # a notebook = a git repo
    │   ├── .git/
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

- **Web UI** — `noda web` serves the active notebook in the browser, reading the same
  git-backed files. (v1 is CLI-only.)
- Note linking/backlinks and encrypted notebooks are under consideration.

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
