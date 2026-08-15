//! The `noda` binary: parse arguments, run one command, print its output.

use clap::{Parser, Subcommand, ValueEnum};

use noda::{Paths, cmd, tui, web};

/// What `noda ls --sort` accepts. `cmd::Sort` has a fourth variant for the
/// order a listing comes in when the flag is absent, which is not something to
/// ask for by name.
#[derive(Clone, Copy, ValueEnum)]
enum SortField {
    Created,
    Updated,
    Title,
}

#[derive(Parser)]
#[command(
    name = "noda",
    version,
    about = "A git-native notebook for your terminal"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Create the XDG directories and a default notebook.
    Init,
    /// Write the notebook's README.md, which a git host shows as its front page.
    /// Auto-commits.
    Readme {
        /// Replace a README.md the notebook already holds.
        #[arg(long)]
        force: bool,
    },
    /// Create a note. Opens $EDITOR when no content is given. Auto-commits.
    Add {
        /// Note title. Derived from the first line when omitted.
        title: Option<String>,
        /// Note body, instead of opening $EDITOR.
        ///
        /// Hyphen values are allowed through so a body that opens with a list —
        /// `- [ ] …`, which is what a note of nothing but action items looks
        /// like — reaches the command as content rather than being read as an
        /// option.
        #[arg(short = 'c', long, allow_hyphen_values = true)]
        content: Option<String>,
        /// Tag to attach; repeat for several.
        #[arg(long = "tag", value_name = "TAG")]
        tags: Vec<String>,
    },
    /// List what the notebook holds: notes as `id  title  [tags]`, then its files.
    Ls {
        /// Only notes carrying this tag. Anything more selective is `noda
        /// search`, which is where the query language lives.
        #[arg(long, conflicts_with = "files_only")]
        tag: Option<String>,
        /// List another notebook instead of the active one.
        #[arg(long)]
        notebook: Option<String>,
        /// Print one JSON object instead of a table.
        #[arg(long, conflicts_with = "quiet")]
        json: bool,
        /// Print one identifier per record and nothing else: a note's id, a
        /// file's name.
        #[arg(short, long)]
        quiet: bool,
        /// Separate what `--quiet` prints with NUL, for `xargs -0`. A file's
        /// name may contain a space.
        #[arg(short = '0', long, requires = "quiet")]
        null: bool,
        /// Leave out the notebook's files.
        #[arg(long, conflicts_with = "files_only")]
        notes_only: bool,
        /// Leave out the notes.
        #[arg(long)]
        files_only: bool,
        /// Show the whole row: the slug and both timestamps as well as the
        /// title. `--json` carries every field either way.
        #[arg(short, long)]
        long: bool,
        /// Order the notes. `created` and `updated` put the newest first;
        /// `title` is alphabetical.
        #[arg(long, value_name = "FIELD")]
        sort: Option<SortField>,
        /// Run the listing the other way. Reverses whatever `--sort` asked for,
        /// or the default order when it was not passed.
        #[arg(short, long)]
        reverse: bool,
    },
    /// Print a note to stdout, addressed by id or slug.
    Show {
        /// Note id (`k3f9m2p1`, or any prefix naming exactly one) or slug
        /// (`meeting-notes`).
        note: String,
    },
    /// Open a note in $EDITOR; auto-commits on save.
    Edit {
        /// Note id (`k3f9m2p1`, or any prefix naming exactly one) or slug
        /// (`meeting-notes`).
        note: String,
        /// Leave `updated` as it stands instead of setting it to now.
        #[arg(long)]
        no_touch: bool,
    },
    /// Retitle a note. The slug follows the title; the id is preserved.
    Mv {
        /// Note id (`k3f9m2p1`, or any prefix naming exactly one) or slug
        /// (`meeting-notes`).
        note: String,
        /// The new title.
        new_title: String,
        /// Rewrite the links that point at this note, instead of reporting them.
        /// Matched on its id, so a link written before an earlier retitle is
        /// caught too.
        #[arg(long)]
        update_links: bool,
        /// Leave `updated` as it stands instead of setting it to now.
        #[arg(long)]
        no_touch: bool,
    },
    /// Delete a note. The removal is a commit, so `git revert` undoes it.
    Rm {
        /// Note id (`k3f9m2p1`, or any prefix naming exactly one) or slug
        /// (`meeting-notes`).
        note: String,
    },
    /// Where the active notebook stands: notes, changes, drift from the remote.
    Status,
    /// Diagnose the notebook, and adopt the notes that only lack an id. Names any
    /// git hooks noda will never run, which needs no flag.
    Doctor {
        /// Report what would change without writing or committing anything.
        #[arg(long)]
        dry_run: bool,
        /// Also follow every link in every note: report the files no note links
        /// to, the links whose note was retitled, and the links that name
        /// nothing. Reads the whole notebook, so it is asked for rather than
        /// assumed.
        #[arg(long)]
        links: bool,
        /// Also check the timestamps: values that cannot be read, notes changed
        /// before they were created, and notes git has a newer commit for than
        /// their own `updated` claims. Walks all of history, so it is asked for
        /// rather than assumed.
        #[arg(long)]
        times: bool,
    },
    /// Print where something lives, for the tools noda does not wrap:
    /// `pandoc "$(noda path meeting-notes)"`.
    Path {
        /// A note (id or slug), or one of the notebook's files by name. Omit for
        /// the notebook's own directory.
        key: Option<String>,
    },
    /// Search the active notebook: `noda search tag:work OR tag:q3 budget`.
    Search {
        /// Terms, all of which must match. `field:value` narrows one to `tag`,
        /// `title`, `id` or `text`; `OR` between two terms takes either; `-` in
        /// front of one rules it out.
        ///
        /// Hyphen values are allowed through so `-tag:archived` reaches the
        /// command as a term rather than being read as an option.
        #[arg(
            required = true,
            num_args = 1..,
            allow_hyphen_values = true,
            value_name = "QUERY"
        )]
        query: Vec<String>,
    },
    /// Browse the notebook: the listing, the same query language `noda search`
    /// takes, and Enter to open what the cursor is on as a screen of its own.
    ///
    /// Every key that changes a note runs the command that changes it, and `:`
    /// runs the ones that have no key, under the names they already have — so
    /// what a change means is written down in exactly one place.
    Tui,
    /// Serve the notebooks in a browser, for reading them from a phone.
    ///
    /// There is no password on this. It is meant to be reached over a tailnet or
    /// from behind something that already does authentication, and the defaults
    /// are set for that: it listens on this machine only until told otherwise,
    /// and it answers to a hostname only when told to.
    Web {
        /// Address to listen on. Give `0.0.0.0:8080` to reach it from elsewhere
        /// — and read the paragraph above before you do.
        #[arg(short, long, default_value = "127.0.0.1:8080", value_name = "ADDR")]
        listen: String,
        /// A hostname to answer to, repeatable.
        ///
        /// Addresses and `localhost` need no permission. A *name* does, because
        /// a name is what an attacker needs for a DNS rebinding attack — so a
        /// reverse proxy or a tailnet name has to be named here.
        #[arg(long = "allow-host", value_name = "NAME")]
        allow_host: Vec<String>,
    },
    /// Show commit history for the notebook, or for one note.
    Log {
        /// Note id (`k3f9m2p1`, or any prefix naming exactly one) or slug
        /// (`meeting-notes`). Omit for the notebook.
        note: Option<String>,
        /// Show at most this many commits.
        #[arg(short = 'n', long = "max-count", value_name = "COUNT")]
        max: Option<usize>,
    },
    /// List the notes that link to a note or a file. Reads every note's body,
    /// so it costs what `doctor --links` costs.
    Backlinks {
        /// A note (id or slug), or one of the notebook's files by name.
        key: String,
        /// Print one JSON object instead of a table.
        #[arg(long, conflicts_with = "quiet")]
        json: bool,
        /// Print one note id per line and nothing else.
        #[arg(short, long)]
        quiet: bool,
    },
    /// List every unticked `- [ ]` in the notebook, soonest due first. Reads
    /// every note's body, so it costs what `search` costs.
    ///
    /// A due date is written into the item itself as `due:2026-08-10`; items
    /// without one come last, and anything else — `due:tomorrow` included —
    /// stays part of the text.
    Todo {
        /// Print one JSON object instead of a table.
        #[arg(long)]
        json: bool,
    },
    /// Show which commit put each line of a note where it is.
    Blame {
        /// Note id (`k3f9m2p1`, or any prefix naming exactly one) or slug
        /// (`meeting-notes`).
        note: String,
    },
    /// Show uncommitted changes, or what the last commit changed.
    Diff {
        /// Note id (`k3f9m2p1`, or any prefix naming exactly one) or slug
        /// (`meeting-notes`). Omit for the notebook.
        note: Option<String>,
    },
    /// List notes the notebook no longer holds, with the commit to restore each
    /// from. Walks all of history.
    Deleted {
        /// Look at another notebook instead of the active one.
        #[arg(long)]
        notebook: Option<String>,
        /// Print one JSON object instead of a table.
        #[arg(long)]
        json: bool,
    },
    /// Mark the notebook as it stands, so `restore` can name that moment later.
    /// Omit the name to list what has been marked.
    Snapshot {
        /// What to call it: `2026-q3`, `before-the-rewrite`.
        name: Option<String>,
        /// What it marks. Defaults to the name.
        #[arg(short = 'm', long, requires = "name", value_name = "TEXT")]
        message: Option<String>,
    },
    /// Restore a note to an earlier version, as a new commit.
    Restore {
        /// Note id (`k3f9m2p1`, or any prefix naming exactly one) or slug
        /// (`meeting-notes`).
        note: String,
        /// Anything git accepts: an id, an abbreviated id, `HEAD~3`, a tag.
        commit: String,
        /// Restore `updated` along with the rest of the note, instead of setting
        /// it to now.
        #[arg(long)]
        no_touch: bool,
    },
    /// Show or change settings: editor, author, notebook, sign.
    Config {
        /// Setting to read or write. Omit to show every setting.
        key: Option<String>,
        /// New value. Omit to read the setting instead of writing it.
        value: Option<String>,
        /// Remove the setting, going back to the default.
        #[arg(long, requires = "key", conflicts_with = "value")]
        unset: bool,
        /// Open config.toml in the editor.
        #[arg(long, conflicts_with_all = ["key", "unset"])]
        edit: bool,
    },
    /// Bring a notebook in from somewhere else.
    Import {
        #[command(subcommand)]
        command: ImportCommand,
    },
    /// Put files in the notebook, and take them out again.
    File {
        #[command(subcommand)]
        command: FileCommand,
    },
    /// Manage notebooks.
    Notebook {
        #[command(subcommand)]
        command: NotebookCommand,
    },
    /// Clone an existing remote notebook.
    Clone {
        /// Remote URL, over HTTPS or SSH.
        url: String,
        /// Local notebook name. Defaults to the repository's own name.
        name: Option<String>,
    },
    /// Show or set the active notebook's remote.
    Remote {
        #[command(subcommand)]
        command: RemoteCommand,
    },
    /// Pull, then push. Commits pending changes first.
    Sync,
    /// Send the active notebook's commits to its remote.
    Push,
    /// Bring in the remote's commits.
    Pull,
    /// Set the active notebook.
    Use {
        /// Notebook name.
        name: String,
    },
    /// Add and remove tags: `noda tag meeting-notes +work -q3`.
    Tag {
        /// Note id (`k3f9m2p1`, or any prefix naming exactly one) or slug
        /// (`meeting-notes`).
        note: String,
        /// `+tag` to add, `-tag` to remove; repeat as needed.
        ///
        /// Hyphen values are allowed through so `-q3` reaches the command as a
        /// tag rather than being read as an option.
        #[arg(
            required = true,
            num_args = 1..,
            allow_hyphen_values = true,
            value_name = "+TAG|-TAG"
        )]
        changes: Vec<String>,
        /// Leave `updated` as it stands instead of setting it to now. Goes
        /// before the tags, which take every argument after them.
        #[arg(long)]
        no_touch: bool,
    },
}

/// `--no-touch` on the commands that change a note, as `cmd` wants it.
fn touch(no_touch: bool) -> cmd::Touch {
    if no_touch {
        cmd::Touch::Keep
    } else {
        cmd::Touch::Stamp
    }
}

/// One subcommand per source. A format is named rather than sniffed: guessing
/// wrong would import somebody's notes as the wrong thing, quietly, which is
/// the one failure an import must not have.
#[derive(Subcommand)]
enum ImportCommand {
    /// Import a `TiddlyWiki` 5 export: the JSON `export all` writes, or a saved
    /// single-file wiki. Writes two commits — the notes as the wiki held them,
    /// then the conversion — so the originals stay in history either way.
    Tiddlywiki {
        /// The exported `.json`, or a saved `.html` wiki. Several are read as
        /// one import, so the links between them resolve.
        #[arg(required = true, num_args = 1.., value_name = "FILE")]
        files: Vec<std::path::PathBuf>,
        /// Bring the `WikiText` in as it stands instead of converting it to
        /// Markdown.
        #[arg(long)]
        no_convert: bool,
    },
}

#[derive(Subcommand)]
enum FileCommand {
    /// Copy files into the active notebook. Auto-commits.
    Add {
        /// Files to copy in.
        #[arg(required = true, num_args = 1.., value_name = "PATH")]
        paths: Vec<std::path::PathBuf>,
        /// Store it under this name instead of its own. One file at a time.
        #[arg(long = "as", value_name = "NAME")]
        rename: Option<String>,
    },
    /// Rename one of the notebook's files. Auto-commits.
    Mv {
        /// The file's current name in the notebook.
        old: String,
        /// What to call it instead.
        new: String,
        /// Rewrite the links that named it, instead of reporting them.
        #[arg(long)]
        update_links: bool,
    },
    /// Remove one of the notebook's files (as a revertible commit).
    Rm {
        /// The file's name in the notebook, as `noda ls` shows it.
        name: String,
    },
}

#[derive(Subcommand)]
enum NotebookCommand {
    /// Create a notebook (a new git repo).
    Add {
        /// Notebook name; it becomes the directory name.
        name: String,
        /// Remote to sync with, e.g. `git@github.com:me/notes.git`.
        #[arg(long)]
        remote: Option<String>,
    },
    /// List notebooks; marks the active one.
    Ls,
    /// Remove a notebook's local repository. This is not a commit and cannot be undone.
    Rm {
        /// Notebook name.
        name: String,
        /// Delete without asking.
        #[arg(short, long)]
        force: bool,
    },
    /// Rename a notebook.
    Rename {
        /// Current name.
        old: String,
        /// New name.
        new: String,
    },
    /// Print the active notebook.
    Current,
}

#[derive(Subcommand)]
enum RemoteCommand {
    /// Set the active notebook's remote.
    Set {
        /// Remote URL, e.g. `git@github.com:me/notes.git`.
        url: String,
    },
    /// Print the configured remote.
    Show,
}

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        // `noda log | head` closes the pipe on us. That is the pipeline working,
        // not a failure: leave quietly rather than shouting at a reader that has
        // already gone.
        Err(e) if e.is_broken_pipe() => std::process::ExitCode::SUCCESS,
        Err(e) => {
            // Through `anstream`, like every other stream noda writes: an error
            // may quote a command's own output, and a piped `noda sync` must not
            // spit escape sequences at whatever is reading it.
            anstream::eprintln!("noda: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run() -> noda::Result<()> {
    let cli = Cli::parse();
    let paths = Paths::from_env()?;
    let output = match &cli.command {
        Command::Init => cmd::init(&paths)?,
        Command::Readme { force } => cmd::readme(&paths, *force)?,
        Command::Add {
            title,
            content,
            tags,
        } => cmd::add(&paths, title.as_deref(), content.as_deref(), tags)?,
        Command::Ls {
            tag,
            notebook,
            json,
            quiet,
            null,
            notes_only,
            files_only,
            long,
            sort,
            reverse,
        } => cmd::ls(
            &paths,
            &cmd::List {
                notebook: notebook.as_deref(),
                tag: tag.as_deref(),
                long: *long,
                sort: match sort {
                    Some(SortField::Created) => cmd::Sort::Created,
                    Some(SortField::Updated) => cmd::Sort::Updated,
                    Some(SortField::Title) => cmd::Sort::Title,
                    None => cmd::Sort::Slug,
                },
                reverse: *reverse,
                format: match (json, quiet) {
                    (true, _) => cmd::Format::Json,
                    (_, true) => cmd::Format::Quiet,
                    _ => cmd::Format::Table,
                },
                only: match (notes_only, files_only) {
                    (true, _) => cmd::Only::Notes,
                    (_, true) => cmd::Only::Files,
                    _ => cmd::Only::Everything,
                },
                null: *null,
            },
        )?,
        Command::Show { note } => cmd::show(&paths, note)?,
        Command::Edit { note, no_touch } => cmd::edit(&paths, note, touch(*no_touch))?,
        Command::Mv {
            note,
            new_title,
            update_links,
            no_touch,
        } => cmd::mv(&paths, note, new_title, *update_links, touch(*no_touch))?,
        Command::Tag {
            note,
            changes,
            no_touch,
        } => cmd::tag(&paths, note, changes, touch(*no_touch))?,
        Command::Rm { note } => cmd::rm(&paths, note)?,
        Command::Status => cmd::status(&paths)?,
        Command::Doctor {
            dry_run,
            links,
            times,
        } => cmd::doctor(&paths, *dry_run, *links, *times)?,
        Command::Search { query } => cmd::search(&paths, query)?,
        Command::Tui => tui::run(&paths)?,
        Command::Web { listen, allow_host } => web::serve(&paths, listen, allow_host)?,
        Command::Log { note, max } => cmd::log(&paths, note.as_deref(), *max)?,
        Command::Backlinks { key, json, quiet } => cmd::backlinks(
            &paths,
            key,
            match (json, quiet) {
                (true, _) => cmd::Format::Json,
                (_, true) => cmd::Format::Quiet,
                _ => cmd::Format::Table,
            },
        )?,
        Command::Todo { json } => cmd::todo(&paths, *json)?,
        Command::Blame { note } => cmd::blame(&paths, note)?,
        Command::Diff { note } => cmd::diff(&paths, note.as_deref())?,
        Command::Deleted { notebook, json } => cmd::deleted(&paths, notebook.as_deref(), *json)?,
        Command::Snapshot { name, message } => match name {
            Some(name) => cmd::snapshot(&paths, name, message.as_deref())?,
            None => cmd::snapshot_ls(&paths)?,
        },
        Command::Restore {
            note,
            commit,
            no_touch,
        } => cmd::restore(&paths, note, commit, touch(*no_touch))?,
        Command::Config {
            key,
            value,
            unset,
            edit,
        } => match (edit, key.as_deref(), value.as_deref(), unset) {
            (true, ..) => cmd::config_edit(&paths)?,
            (_, Some(key), _, true) => cmd::config_unset(&paths, key)?,
            (_, Some(key), Some(value), _) => cmd::config_set(&paths, key, value)?,
            (_, Some(key), None, _) => cmd::config_get(&paths, key)?,
            (_, None, ..) => cmd::config_show(&paths)?,
        },
        Command::Use { name } => cmd::use_notebook(&paths, name)?,
        Command::Import { command } => match command {
            ImportCommand::Tiddlywiki { files, no_convert } => {
                cmd::import_tiddlywiki(&paths, files, !*no_convert)?
            }
        },
        Command::File { command } => match command {
            FileCommand::Add {
                paths: sources,
                rename,
            } => cmd::file_add(&paths, sources, rename.as_deref())?,
            FileCommand::Mv {
                old,
                new,
                update_links,
            } => cmd::file_mv(&paths, old, new, *update_links)?,
            FileCommand::Rm { name } => cmd::file_rm(&paths, name)?,
        },
        Command::Path { key } => cmd::path(&paths, key.as_deref())?,
        Command::Notebook { command } => match command {
            NotebookCommand::Add { name, remote } => {
                cmd::notebook_add(&paths, name, remote.as_deref())?
            }
            NotebookCommand::Ls => cmd::notebook_ls(&paths)?,
            NotebookCommand::Rm { name, force } => cmd::notebook_rm(&paths, name, *force)?,
            NotebookCommand::Rename { old, new } => cmd::notebook_rename(&paths, old, new)?,
            NotebookCommand::Current => cmd::notebook_current(&paths)?,
        },
        Command::Clone { url, name } => cmd::clone(&paths, url, name.as_deref())?,
        Command::Remote { command } => match command {
            RemoteCommand::Set { url } => cmd::remote_set(&paths, url)?,
            RemoteCommand::Show => cmd::remote_show(&paths)?,
        },
        Command::Sync => cmd::sync(&paths)?,
        Command::Push => cmd::push(&paths)?,
        Command::Pull => cmd::pull(&paths)?,
    };
    cmd::print(&output)
}
