//! `noda tui` — the notebook as one screen: the listing on the left, the note
//! under the cursor on the right, and the query language `noda search` takes in
//! the line along the bottom.
//!
//! It exists for the one thing a command cannot do, which is stay. Reading a
//! notebook is `ls`, then `show`, then `ls` again to find where you were; here
//! the listing does not go away while the note is read, and a query narrows it
//! as it is typed rather than once it is finished.
//!
//! It changes notes by asking the commands to. Every command that changes a
//! note validates it, stamps it and commits it, and there must not be a second
//! implementation of what a change means — so `e` runs `noda edit`, `d` runs
//! `noda rm`, and what comes back is the line that command would have printed.
//! Nothing in this module writes a note itself.
//!
//! The three parts are kept apart so that most of this can be tested with no
//! terminal in the room: [`app`] is the state and takes no input but keystrokes,
//! [`view`] turns that state into a frame, and this module is the only place
//! that opens a repository, reads a file, runs a command or touches a terminal.

pub mod app;
mod theme;
pub mod view;

use std::collections::HashSet;
use std::io::IsTerminal;

use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::event::{self, Event};
use ratatui::crossterm::terminal::{
    Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::crossterm::{cursor, execute};
use ratatui::{DefaultTerminal, Terminal};

use crate::cmd;
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
                Some(action) => perform(paths, terminal, app, action)?,
                None => {}
            }
        }
    }
}

/// Runs the command a keystroke asked for, and puts the screen back in step with
/// the notebook afterwards.
///
/// The command is the same one the shell would have run, called the same way.
/// Nothing is decided here about what the change means — only which command
/// means it, and where its answer goes.
fn perform(
    paths: &Paths,
    terminal: &mut DefaultTerminal,
    app: &mut App,
    action: Action,
) -> Result<()> {
    // Taken before the command runs, so a note that `a` has just made can be
    // told apart from the ones that were already there. The alternative is
    // reading it out of `add`'s answer, and that answer is a sentence written
    // for a person.
    let before: Option<HashSet<String>> =
        matches!(action, Action::Add(_)).then(|| app.ids().map(str::to_string).collect());

    let outcome = match action {
        // Both are the caller's, and named rather than left to a wildcard so
        // that an action added later cannot be quietly swallowed here.
        Action::Quit | Action::Reload => return Ok(()),
        Action::Edit { key, touch } => {
            in_the_foreground(terminal, || cmd::edit(paths, &key, touch))?
        }
        Action::Add(title) => {
            in_the_foreground(terminal, || cmd::add(paths, title.as_deref(), None, &[]))?
        }
        // Links are left alone: `--update-links` edits the prose of notes the
        // command was not pointed at, and a browser is not the place to do that
        // to a note nobody is looking at. `noda mv --update-links` still is.
        Action::Retitle { key, title, touch } => cmd::mv(paths, &key, &title, false, touch),
        Action::Tag {
            key,
            changes,
            touch,
        } => cmd::tag(paths, &key, &changes, touch),
        Action::Remove(key) => cmd::rm(paths, &key),
        // The whole queue, in one commit. Nothing is decided here about what any
        // of it means — `bulk` runs the same code the keys above run, and the
        // only thing that moved is where the commit falls.
        Action::Send(steps) => {
            let sent = cmd::bulk(paths, &steps);
            // Only when it went through: a queue that was refused is a queue you
            // still have, which is the difference between an error you can fix
            // and an afternoon's work you have to remember.
            if sent.is_ok() {
                app.sent();
            }
            sent
        }
    };
    app.report(outcome);

    // Whatever happened, the notebook on screen is now a guess: a change that
    // was committed, an edit that was rejected and left on disk, or a file
    // another window wrote while the editor had the terminal.
    reload(paths, app)?;

    // The one note the reader is certainly looking for is the one they have just
    // made. Anything else keeps the cursor `reload` already kept.
    let made = before.and_then(|ids| app.ids().find(|id| !ids.contains(*id)).map(str::to_string));
    if let Some(id) = made {
        app.select_id(&id);
    }
    Ok(())
}

/// Hands the terminal back for the length of a command that wants one of its
/// own — which means `$EDITOR`, the only kind noda runs.
///
/// The alternate screen goes away and raw mode with it, so the editor starts on
/// a terminal in the state it would have found had it been run from the shell.
/// Coming back the screen is cleared rather than redrawn from what was there:
/// the alternate screen is a fresh buffer, and ratatui's record of what is on it
/// describes a frame that has been gone for as long as the edit took.
///
/// Done with crossterm's own calls rather than by `ratatui::try_init` and
/// `try_restore`, for two reasons. `try_init` installs a panic hook around the
/// one already there, and an editor opened twenty times would leave twenty of
/// them. And the terminal is replaced rather than cleared: `Terminal::clear`
/// asks the terminal where its cursor is and waits for the reply, which is a
/// question the alternate screen has just made pointless and which some
/// terminals answer slowly or not at all — under a pty with nothing to answer
/// it, that wait ends the session with "the cursor position could not be read".
/// A terminal built fresh has two empty buffers, which is exactly what a screen
/// that has just been switched back to needs anyway.
fn in_the_foreground<T>(terminal: &mut DefaultTerminal, run: impl FnOnce() -> T) -> Result<T> {
    disable_raw_mode()?;
    execute!(std::io::stdout(), LeaveAlternateScreen, cursor::Show)?;

    let out = run();

    enable_raw_mode()?;
    // Cleared as well as entered. Switching to the alternate screen is specified
    // to clear it, but the fresh terminal below believes the screen is blank,
    // and that belief is cheap to make true rather than to rely on.
    execute!(
        std::io::stdout(),
        EnterAlternateScreen,
        Clear(ClearType::All),
        cursor::Hide
    )?;
    *terminal = Terminal::new(CrosstermBackend::new(std::io::stdout()))?;
    Ok(out)
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
