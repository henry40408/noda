# History and sync

*Everything that follows from a notebook being a git repository. Part of the [noda README](../README.md).*

## History (git-backed)

| Command | Description |
| --- | --- |
| `noda log [<note>] [-n <count>]` | Show commit history for the notebook, or one note; marks what the remote has not seen. |
| `noda blame <note>` | Show which commit put each line of a note where it is. |
| `noda diff [<note>] [--remote]` | Show uncommitted or last-commit changes; `--remote` shows what a push would carry. |
| `noda deleted [--notebook <name>] [--json]` | List notes the notebook no longer holds, with the commit to restore each from. |
| `noda restore <note> <commit> [--no-touch]` | Restore a note to an earlier version (new commit). |
| `noda snapshot [<name>] [-m <text>]` | Name the notebook as it stands. Without a name, list what has been named. |

`noda log <note>` follows a note across renames, because every commit records the filenames
and the id is one of them — no rename guessing involved. Nothing is capped: `-n` is there
when you want less.

**A commit the remote has not seen carries a `↑` in the margin.** `noda status` says how many
there are to push; this says which.

```
$ noda log
↑ 061f38a  2026-08-20 11:05  merge: origin/main
↑ 37f9a04  2026-08-20 11:05  add: localonly
  94acbd0  2026-08-20 11:05  add: fromother
  8d5f590  2026-08-20 11:05  add: gamma
```

The marks are that count enumerated rather than a second opinion, so the two cannot disagree.
Which matters most in exactly the listing above: after a pull that merged, **the unpushed
commits are no longer a run along the top of the log**. `fromother` came down in the merge and
the remote already has it, so it sits unmarked between two commits that are still waiting to
go out.

A notebook with no remote, or one that has never synced, carries no marks at all — with
nothing to compare against every commit is unpushed, and saying so on all of them says
nothing. And when `-n` cuts the listing above the oldest unpushed commit, a line below the
rows says how many marks are out of sight, so what is on screen is never a subset presenting
itself as the whole.

**The TUI's log screen carries the same mark in the same margin** — `l` on a note or the
listing, or `:log`. Its chrome has been saying `↑2 ↓0` all along; the marks are which two.

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

**`noda diff --remote` asks the other question: what a push would carry.** It is the third
of three answers about the same gap — `noda status` counts it, `noda log` marks which
commits, and this is what is inside them. It takes a note, so "what am I about to send"
can be asked about one as readily as about the notebook.

It is measured **from where the two histories parted**, which is the same diff a pull
request shows. That matters when the remote has commits you have not pulled: comparing
straight against the remote's tip would report every line *they* added as a line removed,
because it is missing from your tree — and since this diff needs rename detection (`noda mv`
renames a note whenever its title changes), git then pairs their new note with yours and
reports a rename that never happened:

```
c7pjk17v-theirnote.md => pt1a8xar-beta.md | 4 ++--
```

Two notes written on two machines, neither of which is the other renamed. So `--remote`
says only what you would send, and never anything about what you have yet to receive —
that is `noda pull`'s business. A notebook that has never synced gets an error rather than
an empty diff, because it differs from its remote by everything it holds.

Uncommitted work is not in it either: a push would not carry that, and plain `noda diff` is
the command that shows it.

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
push: main (2 commits, 1 snapshot) -> git@github.com:me/notes.git
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

## Remote sync (HTTPS / SSH)

| Command | Description |
| --- | --- |
| `noda remote set <url>` | Set the active notebook's remote. |
| `noda remote show` | Print the configured remote. |
| `noda sync` | Pull, then push (auto-commits pending changes first). |
| `noda push` / `noda pull` | One-directional sync. |

**Where you stand against the remote is said in one vocabulary, wherever it is said.** `in
sync`, `2 to push`, `3 to pull`, `never synced`, `no remote` — those are the words, and
`noda status`, the `noda notebook ls` column, the TUI's `↑2 ↓3` and the chip on the web bar
are all reading the same two numbers. None of them goes to the network: the counts are
measured against what the last sync left behind, which is what makes every one of them
instant and correct on a train. `never synced` covers a notebook whose remote it has not
spoken to yet, and a first `noda push` clears it as readily as a fetch does.

```
$ noda notebook ls
* work     git@github.com:me/work-notes.git  2 to push
  archive  git@github.com:me/archive.git     in sync
  scratch  no remote
```

`noda status` speaks only about the notebook you are in, so this column is where a notebook
you have not opened in a fortnight gets to say it is thirty commits behind.

**The two commands that act on the difference report it too.** A push used to print the same
line whether it carried twenty commits or nothing at all:

```
$ noda push
push: main (2 commits) -> git@github.com:me/notes.git
$ noda push
push: main matches git@github.com:me/notes.git — nothing to send
$ noda pull
pull: fast-forwarded 3 commits to cba91df
```

A first push is the one that gives no number — until something has been fetched, what the
remote holds is unknown, and a count taken off the local history would be a guess dressed as
a fact.

**The same gap, asked three ways.** Each answer is the one above it opened up, and all three
read the same refs without going anywhere:

| | |
| --- | --- |
| how many? | `noda status`, and the `noda notebook ls` column |
| which commits? | the `↑` margin in `noda log`, and on the TUI's log screen |
| what is in them? | `noda diff --remote` |

HTTPS and SSH are built in; no system git or OpenSSL is required at runtime. Credentials
are not noda's to keep: SSH keys come from `ssh-agent`, HTTPS from git's credential helper.
The helper is looked up in the notebook's own `.git/config` as well as `~/.gitconfig`,
`~/.config/git/config` and `/etc/gitconfig`, so one notebook can authenticate differently
from the rest. Those four are the whole list: noda carries its own libgit2 rather than
calling `git`, so a helper your `git` reads from its installation's own `etc/gitconfig` —
where a packaged build may well have put `credential.helper = osxkeychain` for you — is
invisible to noda and has to be repeated in one of the files above.

A remote can also carry its credentials in the URL — `https://<user>:<token>@host/notes.git`
— and where no helper can be *run* at all, that is what is left: the container image has no
shell to run one with. So noda assumes a remote is carrying a secret and never reads one
back to you. `noda status`, `noda remote show`, `noda notebook ls`, the TUI, the web status
page and the error a failed sync prints all say `https://***@host/notes.git`; the URL you
configured is untouched in `.git/config`, which is what push and fetch still open. The whole
userinfo is replaced rather than the password alone, because Gitea and Forgejo take the
token as the *username*. `git@github.com:me/notes.git` is left exactly as it is — over SSH
the key does the authenticating and the username is not a secret.

Carrying its own libgit2 has a second consequence, and it is the one that surprises people:
**noda runs no git hooks.** libgit2 does not run them at all, so a `pre-commit` in your
notebook fires under `git commit` and does nothing under `noda add` — same file, same
repository, different outcome. `noda doctor` says so when it finds one, and if you want a
hook to run, run the command through git: `cd "$(noda path)"` and commit there.

GPG signing has the same root cause and is the one case noda makes up for itself, by
calling gpg the way git would — see [Signing](../README.md#signing).

A pull fast-forwards when only the remote moved, and makes a merge commit when both sides
did. Two notebooks that each added a note produce two different filenames, so there is
nothing to conflict over and nothing for noda to reconcile afterwards. A conflict inside a
note — the same note edited on both sides — is yours: the merge is rolled back, the notebook
is left exactly as it was, and you can resolve it with git in the notebook directory.

`noda sync` commits the whole working tree and needs no guard before it. There is nothing
derived to fall out of step with the notes, so there is no state in which committing
everything would make a disagreement permanent and remote.

