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

`noda log <note>` follows a note across renames by its id, which every commit records in the
filename — no rename guessing. Nothing is capped; `-n` is there when you want less.

**A commit the remote has not seen carries a `↑` in the margin.** `noda status` says how many
there are to push; this says which.

```
$ noda log
↑ 061f38a  2026-08-20 11:05  merge: origin/main
↑ 37f9a04  2026-08-20 11:05  add: localonly
  94acbd0  2026-08-20 11:05  add: fromother
  8d5f590  2026-08-20 11:05  add: gamma
```

The marks enumerate that same count, so the two cannot disagree. After a pull that merged, **the
unpushed commits are not necessarily a run along the top**: above, `fromother` came down in the
merge, so the remote has it and it sits unmarked between two commits still waiting to go out.

A notebook with no remote, or one that has never synced, carries no marks — with nothing to compare
against, marking every commit says nothing. When `-n` cuts the listing above the oldest unpushed
commit, a line below the rows says how many marks are out of sight.

**The TUI's log screen carries the same mark in the same margin** — `l` on a note or the listing,
or `:log`.

`noda blame <note>` answers "when did I write *this*":

```
$ noda blame q3-planning
8abf00e  2026-08-02 11:47  # Meeting notes
8abf00e  2026-08-02 11:47
8abf00e  2026-08-02 11:47  - Q3 budget signed off
89bb210  2026-08-02 11:47  - hire two engineers
0000000  not committed     - draft still open
```

Two things it does that `git blame` on the same file will not:

**It reaches past a rename.** The note above was `meeting-notes` when its first lines were written;
`noda mv` renamed the file when the title changed, yet those lines are still credited to the commit
that wrote them. git's own blame follows renames by guessing at content similarity, and libgit2's
rename-tracking options are all documented as not implemented. noda picks the note out of each
commit **by id**, so a rename never comes up.

**It marks lines not committed yet** as `0000000` — what a note edited outside noda looks like
before anything picks the change up.

Only the body is blamed: `updated` is rewritten on every edit, so blaming the frontmatter would make
every note look written all at once. There are no line numbers; in prose the unit you look for is a
paragraph.

`noda diff` shows uncommitted changes when there are any, and otherwise what the last commit
changed — noda commits as it goes, so a clean notebook is normal. The output is a plain unified
diff that `git apply` will take.

**`noda diff --remote` shows what a push would carry.** `noda status` counts the gap, `noda log`
marks which commits, and this shows what is inside them. It takes a note too.

It is measured **from where the two histories parted**, as a pull request is. Comparing against
the remote's tip would report every line *they* added and you have not pulled as a line removed —
and since this diff needs rename detection (`noda mv` renames a note whenever its title changes),
git would pair their new note with yours and report a rename that never happened:

```
c7pjk17v-theirnote.md => pt1a8xar-beta.md | 4 ++--
```

So `--remote` says only what you would send; what you have yet to receive is `noda pull`'s
business. A notebook that has never synced gets an error rather than an empty diff, because it
differs from its remote by everything it holds. Uncommitted work is not included — a push would not
carry it.

`<commit>` is anything git accepts: a full or abbreviated id, `HEAD~3`, a tag, a branch. A restore
is a new commit, never a rewrite, and a note keeps its current name — only its contents travel
back. It also works on a removed note: `noda restore <slug> HEAD~1` brings it back with its id
intact.

`noda snapshot` gives a moment a name to cite later instead of counting back to it:

```
$ noda snapshot 2026-q3 -m 'end of quarter'
snapshot: 2026-q3 -> 4953133
$ noda snapshot
2026-q3   2026-08-02 10:14  4953133  end of quarter
$ noda restore meeting-notes 2026-q3
```

It is a git annotated tag, so it records who took it and when (a lightweight tag would list as an
empty row). It commits the working tree first, as `noda sync` does, so the snapshot includes what is
on disk. It never moves an existing snapshot, because a name that can be reassigned cannot be
cited; `git tag -d <name>` in the notebook is there if you meant to.

Snapshots travel with the notebook — `noda push` sends them, `noda pull` brings them down. If the
remote already has that name for another commit, the snapshot is held back and the notes go anyway:

```
$ noda push
push: main (2 commits, 1 snapshot) -> git@github.com:me/notes.git
snapshot `q3` was not sent — the remote already has that name for another commit; rename
yours, or drop it with `git tag -d q3`
```

`noda deleted` lists what there is to bring back, most recently lost first:

```
$ noda deleted
2kpas2d8  meeting-notes  2026-08-02 02:40  ff9062f  Meeting notes
qzdt88kk  old-draft      2026-08-02 02:40  6918cec  Old draft
`noda restore <note> <commit>` with the commit above brings one back
```

The revision in each row is the last commit that still held the note, not the one that deleted
it, so `restore` takes it as it stands. The slug and title are read from that commit too.

It compares trees, not commit messages: a note's identity is its filename, so the notes at any
commit are read off its tree without opening a blob. So a rename is not a deletion (`noda mv`
keeps the id), a note deleted and later restored is not listed, and a plain `git rm` is found like
`noda rm`. It walks all of history, which is why it is its own command rather than a flag on
`noda ls`, which reads a directory.

`--json` carries object ids in full, because an abbreviation can stop being unique later:

```sh
noda deleted --json | jq -r '.deleted[] | "noda restore \(.slug) \(.restore_from)"'
```
```
noda restore old-draft 4953133a9f2e154d8bcc11672de7503c77862c71
noda restore meeting-notes a40b843e5d29a008fe8a3124cd9a1b7b705570d2
```

`removed_at` is RFC 3339 UTC there, as a note's `created` and `updated` are; the table shows the
time in the zone the commit was made in. Unlike the table, `--json` prints a document even when
nothing has been deleted. `--notebook` looks at one you are not currently in.

## Remote sync (HTTPS / SSH)

| Command | Description |
| --- | --- |
| `noda remote set <url>` | Set the active notebook's remote. |
| `noda remote show` | Print the configured remote. |
| `noda sync` | Pull, then push (auto-commits pending changes first). |
| `noda push` / `noda pull` | One-directional sync. |

**Where you stand against the remote is said in one vocabulary everywhere**: `in sync`,
`2 to push`, `3 to pull`, `never synced`, `no remote`. `noda status`, the `noda notebook ls`
column, the TUI's `↑2 ↓3` and the chip on the web bar all read the same two numbers. None goes to
the network: the counts are measured against what the last sync left behind, so they are instant
and work offline. `never synced` means noda has not yet spoken to the remote; a first `noda push`
clears it as a fetch does.

```
$ noda notebook ls
* work     git@github.com:me/work-notes.git  2 to push
  archive  git@github.com:me/archive.git     in sync
  scratch  no remote
```

`noda status` speaks only about the notebook you are in, so this column is where a notebook you
have not opened in a fortnight says it is thirty commits behind.

**Push and pull report the difference too:**

```
$ noda push
push: main (2 commits) -> git@github.com:me/notes.git
$ noda push
push: main matches git@github.com:me/notes.git — nothing to send
$ noda pull
pull: fast-forwarded 3 commits to cba91df
```

A first push gives no number: until something has been fetched, what the remote holds is unknown.

**The same gap, asked three ways**, all read from local refs:

| | |
| --- | --- |
| how many? | `noda status`, and the `noda notebook ls` column |
| which commits? | the `↑` margin in `noda log`, and on the TUI's log screen |
| what is in them? | `noda diff --remote` |

HTTPS and SSH are built in; no system git or OpenSSL is required at runtime. SSH keys come from
`ssh-agent`, HTTPS credentials from git's credential helper. The helper is looked up in the
notebook's own `.git/config` as well as `~/.gitconfig`, `~/.config/git/config` and
`/etc/gitconfig`, so one notebook can authenticate differently from the rest. Those four are the
whole list: noda carries its own libgit2 rather than calling `git`, so a helper set in your `git`
installation's own `etc/gitconfig` (where a packaged build may have put
`credential.helper = osxkeychain`) is invisible to noda and has to be repeated in one of them.

A remote can also carry credentials in the URL — `https://<user>:<token>@host/notes.git` — which is
what is left where no helper can be run, as in the container image, which has no shell. So noda
never prints the userinfo back: `noda status`, `noda remote show`, `noda notebook ls`, the TUI, the
web status page and a failed sync's error all say `https://***@host/notes.git`, while `.git/config`
keeps the URL you configured. The whole userinfo is replaced, not just the password, because Gitea
and Forgejo take the token as the *username*. `git@github.com:me/notes.git` is left as is — over
SSH the key authenticates and the username is no secret.

The same libgit2 means **noda runs no git hooks**: a `pre-commit` in your notebook fires under
`git commit` and does nothing under `noda add`. `noda doctor` says so when it finds one; to run a
hook, commit through git: `cd "$(noda path)"` and commit there.

GPG signing has the same root cause and is the one case noda makes up for, by calling gpg the way
git would — see [Signing](../README.md#signing).

A pull fast-forwards when only the remote moved, and makes a merge commit when both sides did. Two
notebooks that each added a note produce two different filenames, so there is nothing to conflict
over. A conflict inside a note — the same note edited on both sides — is yours: the merge is rolled
back, the notebook is left as it was, and you resolve it with git in the notebook directory.

`noda sync` commits the whole working tree with no guard: nothing derived is stored beside the
notes, so there is no state that committing everything could make permanently inconsistent.
