# Browsing in the terminal

*One of noda's three interfaces — see the [README](../README.md) for the rest.*

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

## The other screens

A screen is the whole width and there is a stack of them, which is what lets a screen be
about something a listing cannot hold. `noda blame`, `noda log` and `noda diff` do not fit
beside a note at any width; given the width, each of them is a screen.

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

## Changing several notes at once

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

