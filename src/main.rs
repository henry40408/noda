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
    /// Manage notebooks.
    Notebook {
        #[command(subcommand)]
        command: NotebookCommand,
    },
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

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("noda: {e}");
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
        Command::Use { name } => cmd::use_notebook(&paths, name)?,
        Command::Notebook { command } => match command {
            NotebookCommand::Add { name, remote } => {
                cmd::notebook_add(&paths, name, remote.as_deref())?
            }
            NotebookCommand::Ls => cmd::notebook_ls(&paths)?,
            NotebookCommand::Rm { name } => cmd::notebook_rm(&paths, name)?,
            NotebookCommand::Rename { old, new } => cmd::notebook_rename(&paths, old, new)?,
            NotebookCommand::Current => cmd::notebook_current(&paths)?,
        },
    };
    cmd::print(&output)
}
