//! The `noda` binary: parse arguments, run one command, print its output.

use clap::{Parser, Subcommand};

use noda::{Paths, cmd};

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
    /// Create a note. Opens $EDITOR when no content is given. Auto-commits.
    Add {
        /// Note title. Derived from the first line when omitted.
        title: Option<String>,
        /// Note body, instead of opening $EDITOR.
        #[arg(short = 'c', long)]
        content: Option<String>,
        /// Tag to attach; repeat for several.
        #[arg(long = "tag", value_name = "TAG")]
        tags: Vec<String>,
    },
    /// List notes: id, slug, title, tags.
    Ls {
        /// Only notes carrying this tag.
        #[arg(long)]
        tag: Option<String>,
        /// List another notebook instead of the active one.
        #[arg(long)]
        notebook: Option<String>,
    },
    /// Print a note to stdout, addressed by id or slug.
    Show {
        /// Note id (`k3f9`) or slug (`meeting-notes`).
        note: String,
    },
    /// Open a note in $EDITOR; auto-commits on save.
    Edit {
        /// Note id (`k3f9`) or slug (`meeting-notes`).
        note: String,
    },
    /// Retitle a note. The slug follows the title; the id is preserved.
    Mv {
        /// Note id (`k3f9`) or slug (`meeting-notes`).
        note: String,
        /// The new title.
        new_title: String,
    },
    /// Delete a note. The removal is a commit, so `git revert` undoes it.
    Rm {
        /// Note id (`k3f9`) or slug (`meeting-notes`).
        note: String,
    },
    /// Where the active notebook stands: notes, changes, drift from the remote.
    Status,
    /// Diagnose the notebook, and adopt the notes that only lack an id.
    Doctor {
        /// Report what would change without writing or committing anything.
        #[arg(long)]
        dry_run: bool,
        /// Also follow every link in every note: report the files no note links
        /// to, and the links that name nothing. Reads the whole notebook, so it
        /// is asked for rather than assumed.
        #[arg(long)]
        links: bool,
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
    /// Show commit history for the notebook, or for one note.
    Log {
        /// Note id (`k3f9`) or slug (`meeting-notes`). Omit for the notebook.
        note: Option<String>,
        /// Show at most this many commits.
        #[arg(short = 'n', long = "max-count", value_name = "COUNT")]
        max: Option<usize>,
    },
    /// Show uncommitted changes, or what the last commit changed.
    Diff {
        /// Note id (`k3f9`) or slug (`meeting-notes`). Omit for the notebook.
        note: Option<String>,
    },
    /// Restore a note to an earlier version, as a new commit.
    Restore {
        /// Note id (`k3f9`) or slug (`meeting-notes`).
        note: String,
        /// Anything git accepts: an id, an abbreviated id, `HEAD~3`, a tag.
        commit: String,
    },
    /// Show or change settings: editor, author, notebook.
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
        /// Note id (`k3f9`) or slug (`meeting-notes`).
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
        Command::Add {
            title,
            content,
            tags,
        } => cmd::add(&paths, title.as_deref(), content.as_deref(), tags)?,
        Command::Ls { tag, notebook } => cmd::ls(&paths, notebook.as_deref(), tag.as_deref())?,
        Command::Show { note } => cmd::show(&paths, note)?,
        Command::Edit { note } => cmd::edit(&paths, note)?,
        Command::Mv { note, new_title } => cmd::mv(&paths, note, new_title)?,
        Command::Tag { note, changes } => cmd::tag(&paths, note, changes)?,
        Command::Rm { note } => cmd::rm(&paths, note)?,
        Command::Status => cmd::status(&paths)?,
        Command::Doctor { dry_run, links } => cmd::doctor(&paths, *dry_run, *links)?,
        Command::Search { query } => cmd::search(&paths, query)?,
        Command::Log { note, max } => cmd::log(&paths, note.as_deref(), *max)?,
        Command::Diff { note } => cmd::diff(&paths, note.as_deref())?,
        Command::Restore { note, commit } => cmd::restore(&paths, note, commit)?,
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
