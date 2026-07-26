# noda

> A git-native notebook for your terminal. Your notes are plain Markdown in an ordinary
> git repository — versioned, syncable, and yours.

> **Status: in progress (spec-first).** This README is the v1 contract, written *before*
> implementation, AWS "working-backwards" style. The note commands (`add`, `ls`, `show`,
> `edit`, `mv`, `tag`, `rm`) and the notebook commands (`init`, `notebook add/ls/rm/rename`,
> `use`, `notebook current`) work today. Search, history, `config`, and everything that
> touches the network — `clone`, `remote`, `sync`, `push`, `pull` — are still the target
> contract, not shipped features.
> See [docs/PRFAQ.md](docs/PRFAQ.md).

---

## Why noda

- **Just git.** Every notebook is a normal git repo of Markdown files. No lock-in, no
  proprietary format. Anything noda does, plain `git` can inspect and undo.
- **Automatic history.** Every change is committed for you. `noda log` shows a note's
  history; `noda restore` rewinds it.
- **Sync anywhere.** HTTPS and SSH are compiled into the binary, so `noda sync` talks to
  GitHub, GitLab, or any git host with nothing else to install.
- **Fast to reach.** Address a note by a short id *or* a readable slug.
- **One static binary.** Ships self-contained for macOS and Linux (incl. arm64/musl).

## Install

```sh
cargo install noda            # from crates.io
brew install noda             # macOS / Linux (Homebrew)
```

Or download a prebuilt static binary for your platform from the releases page
(`x86_64`/`aarch64`, macOS & Linux-musl).

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
| `noda clone <url> [name]` | Clone an existing remote notebook. |

`noda rm` (a note) is a commit you can revert. `noda notebook rm` is not — it deletes the
repository and its whole history from disk. The active notebook is refused outright; switch
with `noda use` first. Everything else is confirmed at the terminal, and `--force` skips the
question. With no terminal to ask at — piped, or in a script — the deletion is refused
rather than assumed, so `--force` is how a script says it meant it.

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
| `noda search <query>` | Full-text search across the active notebook. |

`<note>` accepts an id (`k3f9`) or a slug (`meeting-notes`), matched exactly.

`noda tag` takes signed tags — `noda tag meeting-notes +q3 -work` adds `q3` and removes
`work`. Adding a tag a note already has is not an error; it just leaves nothing to commit.

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
| `noda config` | Show/edit config (editor, author, default notebook). |

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

$XDG_CACHE_HOME/noda/           (default ~/.cache/noda/)
└── search-index/               # rebuildable full-text search index
```

Each notebook is a normal git repo; `cd "$XDG_DATA_HOME/noda/notebooks/work" && git log`
works exactly as you'd expect. Only your notes live in `XDG_DATA_HOME` — config, the
active-notebook pointer, and the search cache are kept out of your synced data on purpose.

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
