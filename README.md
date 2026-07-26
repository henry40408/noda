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
in different places.

**Note.** A Markdown file inside a notebook. It has:
- a **slug** — a human-readable name derived from the title; it's also the filename
  (`meeting-notes.md`) and changes if you rename the note;
- an **id** — a short, stable code (Crockford base32, e.g. `k3f9`) stored in the note's
  frontmatter. It's unique within the notebook and never changes, even across renames.
  Ids are lowercase and matched case-insensitively; Crockford maps the easily-confused
  `I`/`L` to `1` and `O` to `0`, so a mistyped id still resolves to the right note.

Anywhere a command takes `<note>`, pass either the id or the slug. Both are matched
**exactly** — there is no prefix guessing and there are no positional numbers to reshuffle,
so `noda show k3f9` always resolves to one specific note or reports "not found". It never
silently hits the wrong one.

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
| `noda clone <url> [name]` | Clone an existing remote notebook. |

`noda rm` (a note) is a commit you can revert. `noda notebook rm` is not — it deletes the
repository and its whole history from disk. The active notebook is refused outright; switch
with `noda use` first. Everything else is confirmed at the terminal, and `--force` skips the
question. With no terminal to ask at — piped, or in a script — the deletion is refused
rather than assumed, so `--force` is how a script says it meant it.

`noda status` answers "where do I stand" without going to the network — the push/pull
counts are measured against what the last sync left behind, so it works offline and
returns instantly. It is also the one command that reports a `.md` file it cannot read as a
note instead of failing on it, because finding that out is why you ran it.

```
notebook  work  (main)
notes     42
changes   1 file uncommitted
remote    git@github.com:me/work-notes.git
sync      2 to push (as of the last sync)
```

### Notes

| Command | Description |
| --- | --- |
| `noda add [title] [-c <content>] [--tag <t>]...` | Create a note. Opens `$EDITOR` if no `-c`. Auto-commits. |
| `noda ls [--tag <t>] [--notebook <name>]` | List notes: id, slug, title, tags. |
| `noda show <note>` | Print a note to stdout. |
| `noda edit <note>` | Open a note in `$EDITOR`; auto-commits on save. |
| `noda rm <note>` | Delete a note (as a revertible commit). |
| `noda mv <note> <new-title>` | Rename a note (updates slug; id is preserved). |
| `noda tag <note> [+tag]... [-tag]...` | Add/remove tags. |
| `noda search <query>...` | Full-text search across the active notebook. |

`<note>` accepts an id (`k3f9`) or a slug (`meeting-notes`), matched exactly.

`noda tag` takes signed tags — `noda tag meeting-notes +q3 -work` adds `q3` and removes
`work`. Adding a tag a note already has is not an error; it just leaves nothing to commit.

`noda search` looks through every note's title, tags and body in the active notebook. It
matches case-insensitively and by substring rather than by word — Chinese and Japanese have
no spaces to split on, and a word-based search would simply find nothing in them. Several
terms mean all of them, in any order. Results are listed the way `ls` lists them, and a hit
in the body quotes the line it was found on.

`noda add` and `noda edit` open `$VISUAL`, falling back to `$EDITOR` and then to `vi`.
`edit` opens the real file, frontmatter included, but refuses to commit an edit that
breaks the frontmatter or rewrites the id — the file is left as you saved it so you can
fix it or throw it away with `git checkout`.

### History (git-backed)

| Command | Description |
| --- | --- |
| `noda log [<note>] [-n <count>]` | Show commit history for the notebook, or one note. |
| `noda diff [<note>]` | Show uncommitted or last-commit changes. |
| `noda restore <note> <commit>` | Restore a note to an earlier version (new commit). |

`noda log <note>` follows a note across renames, because the committed index records which
file the note lived in at every commit — no rename guessing involved. Nothing is capped:
`-n` is there when you want less.

`noda diff` shows uncommitted changes when there are any, and otherwise what the last
commit changed — noda commits as it goes, so a clean notebook is the normal state and
"what just happened" is the useful answer. `.noda/index.tsv` is left out of the output;
it changes on nearly every commit and is rebuildable from the notes. The output is a plain
unified diff with nothing wrapped around it, so `git apply` will take it.

`<commit>` is anything git accepts: a full or abbreviated id, `HEAD~3`, a tag, a branch.
A restore is a new commit, never a rewrite, and a note keeps the name it has now — only its
contents travel back. It also works on a note you removed: `noda restore <slug> HEAD~1`
brings it back with its id intact, which is the friendly face of "`noda rm` is a commit you
can revert".

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
did. Two notebooks that each added a note both appended to `.noda/index.tsv`, so it
conflicts almost every time — noda settles that one itself by rebuilding the index from the
notes, because the index is derived data. A conflict inside a note is yours: the merge is
rolled back, the notebook is left exactly as it was, and you can resolve it with git in the
notebook directory.

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
    │   ├── .noda/index.tsv     # id ↔ slug lookup (committed; rebuildable from frontmatter)
    │   ├── meeting-notes.md
    │   └── reading-log.md
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
works exactly as you'd expect. Only your notes live in `XDG_DATA_HOME` — config, the
active-notebook pointer, and the editor's scratch buffer are kept out of your synced data
on purpose.

**Platform note.** noda honors the XDG variables on **every** platform, including macOS
(it does not use `~/Library/Application Support`). If a variable is unset, the standard
`~/.config`, `~/.local/share`, `~/.local/state`, and `~/.cache` defaults apply.

## Roadmap

- **Web UI** — `noda web` serves the active notebook in the browser, reading the same
  git-backed files. (v1 is CLI-only.)
- Attachments, note linking/backlinks, and encrypted notebooks are under consideration.

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
