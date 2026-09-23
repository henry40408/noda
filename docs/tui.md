# Browsing in the terminal

*One of noda's three interfaces — see the [README](../README.md) for the rest.*

`noda tui` puts the notebook on a screen you can go into and come back out of. The listing keeps
its place while you read, and a query narrows it as you type.

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

Every screen is the same five bands: where the notebook stands and what the keys do here, what
this screen is of, the screen itself, how far down you are, and what was last said. A listing row
is the one every other listing prints — id, title, tags.

Columns are named along the top, because `Ctrl-w` puts `created` and `updated` side by side and
they look alike. The cursor is a bar down the left edge rather than a reversed row, because
reversing would invert the id's yellow and the tags' cyan. A screen with more than fits gets a bar
down its right.

`Enter` opens what the cursor is on as a screen of its own, and `Esc` closes it. The listing keeps
its cursor meanwhile, so coming back lands where you left.

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

A note is `noda show`: the frontmatter dimmed, your text left alone. The one thing painted over
your prose is the search match, as `noda search` does when it quotes a hit, and it carries into the
note you open.

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
| `#` | tags: a card of every tag the notebook has, `Tab` to choose, `Enter` to apply. Type to narrow it, or to name one it does not have yet |
| `p` | pin, or unpin what is already pinned: a pinned note is above every order, marked `pinned` at the end of its row. `:pin` and `:unpin` say which way rather than toggling |
| `Ctrl-d` | delete, once you have said `y`. With notes marked, `#` and `Ctrl-d` are aimed at the marked set and go into the queue instead |
| `T` | `--no-touch` for the rest of the session: changes stop moving `updated`. The title band says `keeping updated` for as long as it is on |
| `t`, `l` | the notebook's unticked boxes; commits — this note's, or the notebook's from the listing |
| `b`, `B` | what links to this note; who wrote each of its lines |
| `S`, `R` | cycle the walk's own order and the three `--sort` orders; `-r`, which reverses whichever is in force |
| `Ctrl-w` | the whole row — `ls -l`'s columns, in `ls -l`'s places |
| `1` – `9` | narrow to one of the commonest tags; `0` lets go again. The tags screen numbers its first nine rows with these keys |
| `Ctrl-g` | the crumb trail on and off, for a terminal that would rather have the row |
| `r` | read the notebook again |
| `?`, `q` / `Ctrl-C` | keys, quit |

`e`, `m`, `#`, `p` and `Ctrl-d` aim at whatever the screen is about — the row under the cursor on
the listing, the note itself once opened. `p` is not in the key grid in the header, because the
grid is full and `p` can be found another way (`?`, `:pin`). Delete is behind a modifier because it
is the one key that cannot be undone by pressing something else.

**`S`, `R` and `Ctrl-w` are `noda ls`'s flags as session settings**, since a screen has no command
line to write them on. The title band says which are in force, because all three rearrange rows
without otherwise saying why:

```
Notes  all  128  by updated reversed wide ─────────────────────── 3 marks  1 queued
```

They survive `r`. Re-sorting keeps the cursor on the note it was on, not the row.

`Ctrl-w` extends the row with `ls -l`'s columns rather than rearranging it. On a narrow terminal
columns give way from the right, one whole column at a time; the id and title always stay.

`1`–`9` narrow the listing to one of the nine commonest tags from any screen, and `0` lets go. A
notebook's tags are a short head and a long tail: the head is worth a key apiece, the tail is what
`/` is for.

## The other screens

Screens are full-width and stacked, so `noda blame`, `noda log` and `noda diff`, which do not fit
beside a note, each get one.

| | |
| --- | --- |
| `t` / `:todo` | every unticked box in the notebook, soonest due first, with a missed date in red. `Enter` reads the note it is in |
| `:tags` | every tag, commonest first, and how many notes carry it, with the first nine numbered. `Enter` narrows the listing to it rather than opening a screen — the notes are already down there |
| `b` / `:backlinks` | what links to the note in front of you. `Enter` reads the note that was found |
| `l` / `:log` | commits, newest first: the note's on a note screen, the notebook's on the listing. A `↑` marks what the remote has not seen, the same mark in the same margin `noda log` uses |
| `B` / `:blame` | which commit put each line of a note where it is — the body only, because `updated` moves on every edit |
| `:diff` | what is uncommitted, or what the last commit did |
| `:deleted` | the notes history holds that the notebook no longer does |
| `:files` | what the notebook holds that is not a note. `Enter` asks what links to one, which is the question worth asking about an attachment |
| `:notebooks` | every notebook there is. `Enter` moves the whole session to one |

The ones about the note in front of you get a letter; the rest are named.

**A row that cannot be taken back writes the command instead of running it.** `Enter` on a
deleted note, or on a commit in a note's own history, puts `restore <note> <rev>` on the prompt
and stops:

```
Deleted  1 ─────────────────────────────────────────────────────────────────────
  ID        SLUG       DELETED           FROM     TITLE
▌ v62b8rfa  trip-plan  2026-08-04 09:12  2a8715b  Trip plan

 notes   deleted
:restore v62b8rfa 2a8715b
```

Press `Enter` again and it runs. Landing on a row is not agreeing to overwrite the note. Nothing is
rewritten either way: `restore` puts the old text back as a new commit.

Moving to another notebook is refused while the queue has anything in it: a queued change names
notes by id, and an id belongs to the notebook it was minted in.

**`:` is how the rest of noda gets in**, under the subcommands' own names:

```
:open meeting-notes          # a note by id or by slug, without going to find it
:tag reading-list +urgent    # naming the note, which the `#` key has no way to do
:log budget-review           # a screen about a note you are not looking at
:status                      # and everything else that was never going to get a key
:doctor --links
:sync
```

`Ctrl-a` lists them with what each takes, searching descriptions as well as names — `remote` finds
`push` and `pull`. Names resolve through the same code as the CLI, so `:open k3f` matching two
notes is refused in the same words `noda show k3f` would use.

Two things `:` deliberately does not do. `:rm` takes no note: it removes the one on screen, and
`:open` is how you put another there. `:doctor` only reports; adopting files and rewriting is left
to the prompt.

**Everywhere you type, the keys are readline's.** The query, the prompt and the `:` line are one
field: `Ctrl-a` / `Ctrl-e` for the ends of the line, `Ctrl-b` / `Ctrl-f` and `Alt-b` / `Alt-f` for
a character and a word, `Ctrl-w`, `Ctrl-u` and `Ctrl-k` to cut, `Ctrl-y` to paste the last cut,
`Ctrl-d` and `Delete` forwards. A chord the field does not bind does nothing rather than typing its
letter — `Ctrl-d` arrives as `d` with a modifier, and taken at face value would put a `d` in
somebody's title.

Two deliberate departures from readline: `Ctrl-c` leaves the browser (`Esc` abandons a line), and
`Ctrl-p` / `Ctrl-n` walk command history at `:` but the list of notes while a query is typed.

**Every key that changes a note runs the command that changes it.** `e` is `noda edit`, `#` is
`noda tag`, `Ctrl-d` is `noda rm` — so a change is validated, stamped and committed exactly as at
the prompt, and the status line shows what that command printed. `$EDITOR` gets the terminal to
itself while it runs, and a refusal is shown on a card, because the reason (say, where an edit with
a broken frontmatter block was left) is worth reading.

`--no-touch` is a session setting here, since there is nowhere to qualify one keystroke. `T` turns
it on, `e`, `m` and `#` follow it, and the header carries `keeping updated` until you turn it off.

## Tags

`#` opens a card of every tag the notebook has, commonest first. A box in front of each says where
the note stands with it, and `Tab` cycles it through the states that would change something:

```
╭ tags: budget-review ───────────────────────────────────────────╮
│[x] work     37 notes                                           │
│[-] q3       4 notes                                            │
│[ ] archive  11 notes                                           │
│[+] urgent   2 notes                                            │
│                                                                │
│type to narrow      Tab  choose      Enter  apply      Esc  back│
╰────────────────────────────────────────────────────────────────╯
```

**The card asks which tags the note should end up with**, not which `+`s and `-`s to apply — as the
browser's tag form does. The `+`s and `-`s are worked out from the boxes and handed to `noda tag`.
With the notation, a `-` aimed at a misspelled tag removes nothing and reports success; here the tag
is a row, so there is nothing to misspell.

Typing narrows the list, and a tag the notebook does not have appears as the last row. Choosing it
takes a keystroke of its own, so a new tag is never a typo's side effect — and when it is one edit
away from an existing tag, the row says so:

```
╭ tags: budget-review / Work ────────────────────────────────────╮
│[x] work  37 notes                                              │
│[ ] Work  new — close to work, 37 notes                         │
│                                                                │
│type to narrow      Tab  choose      Enter  apply      Esc  back│
╰────────────────────────────────────────────────────────────────╯
```

**`Tab` chooses, not `Space`**, because this is a field naming a tag that may not exist yet, and a
tag may contain a space (imports leave tags like `24.04 Dark patterns`).

With notes marked, the card is about the set: the number is how many of them carry the tag, an
empty box means "leave each as it is", `[+]` and `[-]` mean all of them, and a tick means all of
them have it already.

```
╭ tags: 12 notes ───────────────────────────────────────────────────╮
│[-] q3       8 of 12                                               │
│[x] work     12 of 12                                              │
│[+] archive  0 of 12                                               │
│                                                                   │
│type to narrow      Tab  choose      Enter  queue it      Esc  back│
╰───────────────────────────────────────────────────────────────────╯
```

## Changing several notes at once

Marking and searching are separate. `Space` marks the note under the cursor and `*` marks
everything the query shows, so several searches can build one selection. A marked note the query
now hides is still marked and still changed.

With notes marked, `#` and `Ctrl-d` stop acting on the note under the cursor and add to a queue:
one entry per change, aimed at the notes marked when it was added. The title band counts both, and
the tag card's `Enter` reads `queue it` rather than `apply`.

```
Notes  all  128 ─────────────────────────────────────────────── 12 marks  2 queued
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

**A queue arrives in the history as one commit**, because it is one intention:

```
$ noda log -n 1
  8f2a1c9  2026-08-06 22:31  bulk: 2 changes over 12 notes
```

`noda tui` hands the queue to the same code `noda tag` and `noda rm` use, with the commit boundary
moved out one level. Every tag is parsed before anything is written, so one bad change leaves the
notebook untouched; a note deleted from another window meanwhile is reported under what did go
through. Sending asks first only if the queue deletes something — queueing a delete deletes
nothing, so the question comes at the send.

`q` asks before leaving with a queue in it, since the queue is written down nowhere. `Ctrl-C`
leaves without asking — a program that argues with `Ctrl-C` is one you kill from another window.

It deliberately **does not watch the filesystem**: a note written from another window arrives when
you press `r`, rather than rearranging the list mid-read.

It needs a terminal at both ends and says so rather than filling a pipe with escape sequences:

```
$ noda tui | less
noda: noda tui needs a terminal at both ends; `noda ls`, `noda search` and `noda show` are the ones to redirect
```
