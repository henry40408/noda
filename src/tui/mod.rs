//! `noda tui` — the notebook as one screen: the listing on the left, the note
//! under the cursor on the right, and the query language `noda search` takes in
//! the line along the bottom.
//!
//! It exists for the one thing a command cannot do, which is stay. Reading a
//! notebook is `ls`, then `show`, then `ls` again to find where you were; here
//! the listing does not go away while the note is read, and a query narrows it
//! as it is typed rather than once it is finished.
//!
//! Read-only, deliberately. Every command that changes a note validates it,
//! stamps it and commits it, and there must not be a second implementation of
//! what a change means — so nothing in here writes, and the keys that would are
//! not bound.
//!
//! The three parts are kept apart so that most of this can be tested with no
//! terminal in the room: [`app`] is the state and takes no input but keystrokes,
//! [`view`] turns that state into a frame, and this module is the only place
//! that opens a repository, reads a file or touches a terminal.

pub mod app;
mod theme;
pub mod view;

use std::io::IsTerminal;

use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, Event};

use crate::notebook::{NoteFile, Notebook, Status};
use crate::paths::Paths;
use crate::{Error, Result};

pub use app::{Action, App};

/// Opens the active notebook in the browser, and returns when it is closed.
///
/// The empty string is what comes back: everything this command had to say was
/// said on a screen that no longer exists, and printing an epitaph for it under
/// the shell prompt would only be noise.
pub fn run(paths: &Paths) -> Result<String> {
    // Checked before anything is read, and on both ends. `noda tui | less` can
    // only produce a screenful of escape sequences, and a TUI with no keyboard
    // is a program that cannot be quit — better to say so than to be killed
    // from another window.
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        return Err(Error::msg(
            "noda tui needs a terminal at both ends; \
             `noda ls`, `noda search` and `noda show` are the ones to redirect",
        ));
    }

    // Before the screen is taken over: a notebook that cannot be opened should
    // say so at the prompt, not in the corner of an alternate screen that is
    // about to be torn down.
    let mut app = load(paths)?;

    // ratatui's own: raw mode, the alternate screen, and a panic hook that
    // undoes both before the panic is reported. The hook matters more here than
    // the usual — this crate aborts on panic in release, so a `Drop` guard would
    // never run and the terminal would be left in raw mode with no echo.
    let mut terminal = ratatui::try_init()?;
    let outcome = browse(paths, &mut terminal, &mut app);
    let restored = ratatui::try_restore();

    // The session's own failure is the one worth reporting; a terminal that
    // could not be put back is reported only when there is nothing else to say.
    outcome?;
    restored?;
    Ok(String::new())
}

/// Reads the active notebook into a session.
///
/// Every note is held for as long as the session lasts, bodies and all. That is
/// what a query costs `noda search` on every invocation, paid once instead —
/// and it is the difference between a filter that narrows as you type and one
/// that walks the notebook per keystroke.
pub fn load(paths: &Paths) -> Result<App> {
    let notebook = Notebook::open_active(paths)?;
    let (status, notes) = read(&notebook)?;
    Ok(App::new(notebook.name, notebook.path, status, notes))
}

fn read(notebook: &Notebook) -> Result<(Status, Vec<NoteFile>)> {
    Ok((notebook.status()?, notebook.notes()?))
}

/// Reads the notebook again into a session already under way, keeping the query
/// and — where the note is still there — the cursor.
///
/// What `r` asks for. noda watches no files: a note written from another window
/// is somebody else's edit, and a browser that rearranged itself underneath a
/// reader mid-sentence would be worse than one that waits to be asked.
pub fn reload(paths: &Paths, app: &mut App) -> Result<()> {
    let notebook = Notebook::open_active(paths)?;
    let (status, notes) = read(&notebook)?;
    app.replace(status, notes);
    Ok(())
}

fn browse(paths: &Paths, terminal: &mut DefaultTerminal, app: &mut App) -> Result<()> {
    loop {
        refresh_preview(app);
        terminal.draw(|frame| view::draw(frame, app))?;
        // Blocking: a browser has nothing to do between keystrokes, and polling
        // to find that out would keep a laptop awake for the privilege.
        //
        // A resize needs no arm of its own. The top of this loop draws, and
        // `terminal.draw` fits the frame to whatever the size is by then.
        if let Event::Key(key) = event::read()? {
            match app.on_key(key) {
                Some(Action::Quit) => return Ok(()),
                Some(Action::Reload) => reload(paths, app)?,
                None => {}
            }
        }
    }
}

/// Brings the preview into step with the cursor.
///
/// The file is read rather than the note re-rendered from memory, for the reason
/// `noda show` reads it: what is on screen should be what is on disk, down to a
/// frontmatter field noda does not interpret.
///
/// A file that cannot be read puts the reason in the pane instead of ending the
/// session. It is one note out of a notebook, and it will more often be a note
/// deleted from another window than anything worth quitting over.
///
/// Public because it is the one step between a keystroke and a frame that the
/// state cannot take for itself: a test that draws a screen has to take it too.
pub fn refresh_preview(app: &mut App) {
    let Some((id, path)) = app.preview_wanted() else {
        return;
    };
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| format!("{}: {e}\n", path.display()));
    app.set_preview(id, text);
}
