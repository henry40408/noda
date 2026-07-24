# noda

> A git-native notebook for your terminal. Your notes are plain Markdown in an ordinary
> git repository — versioned, syncable, and yours.

> **Status: design draft (spec-first).** This README describes the intended v1 behavior
> and is being written *before* implementation, AWS "working-backwards" style. Commands
> below are the target contract, not yet a shipped tool. See [docs/PRFAQ.md](docs/PRFAQ.md).

---

## Why noda

- **Just git.** Every notebook is a normal git repo of Markdown files. No lock-in, no
  proprietary format. Anything noda does, plain `git` can inspect and undo.
- **Automatic history.** Every change is committed for you. `noda log` shows a note's
  history; `noda restore` rewinds it.
- **Sync anywhere.** HTTPS and SSH are compiled into the binary, so `noda sync` talks to
  GitHub, GitLab, or any git host with nothing else to install.
- **Fast to reach.** Address a note by a short numeric id *or* a readable slug.
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
| `noda init` | Create `~/.noda` and a default notebook. |
| `noda notebook add <name> [--remote <url>]` | Create a notebook (a new git repo). |
| `noda notebook ls` | List notebooks; marks the active one. |
| `noda notebook rm <name>` | Remove a notebook (local repo). |
| `noda notebook rename <old> <new>` | Rename a notebook. |
| `noda use <name>` | Set the active notebook. |
| `noda notebook current` | Print the active notebook. |
| `noda clone <url> [name]` | Clone an existing remote notebook. |

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

### History (git-backed)

| Command | Description |
| --- | --- |
| `noda log [<note>]` | Show commit history for the notebook, or one note. |
| `noda diff [<note>]` | Show uncommitted or last-commit changes. |
| `noda restore <note> <commit>` | Restore a note to an earlier version (new commit). |

### Remote sync (HTTPS / SSH)

| Command | Description |
| --- | --- |
| `noda remote set <url>` | Set the active notebook's remote. |
| `noda remote show` | Print the configured remote. |
| `noda sync` | Pull, then push (auto-commits pending changes first). |
| `noda push` / `noda pull` | One-directional sync. |

HTTPS and SSH are built in; no system git or OpenSSL is required at runtime.

### Config

| Command | Description |
| --- | --- |
| `noda config` | Show/edit config (editor, author, default notebook). |

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

## License

[MIT](LICENSE.txt) © 2026 Heng-Yi Wu
