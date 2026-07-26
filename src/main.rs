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
    };
    cmd::print(&output)
}
