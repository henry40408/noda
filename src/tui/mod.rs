//! `noda tui` — the notebook as a screen you can go into and come back out of:
//! the listing, whatever the cursor was on, and the query language `noda search`
//! takes in the line along the bottom.
//!
//! It exists for the one thing a command cannot do, which is stay. Reading a
//! notebook is `ls`, then `show`, then `ls` again to find where you were; here
//! the listing keeps its place while a note is read, and a query narrows it as
//! it is typed rather than once it is finished.
//!
//! A screen is the whole width and there is a stack of them. That is what lets a
//! note be read at the width it was written at, and what leaves room for a
//! screen to be about something a listing cannot hold — the pane the note used
//! to share with the listing was never going to be wide enough for either.
//!
//! It changes notes by asking the commands to. Every command that changes a
//! note validates it, stamps it and commits it, and there must not be a second
//! implementation of what a change means — so `e` runs `noda edit`, `Ctrl-d`
//! runs `noda rm`, and what comes back is the line that command would have
//! printed. Nothing in this module writes a note itself.
//!
//! The parts are kept apart so that most of this can be tested with no terminal
//! in the room: [`app`] is the state and takes no input but keystrokes, [`field`]
//! is the one line at a time that is typed into it, [`view`] and [`frame`] turn
//! that state into a frame, and this module is the only place that opens a
//! repository, reads a file, runs a command or touches a terminal.

pub mod app;
mod command;
mod field;
mod frame;
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
use crate::notebook::Notebook;
use crate::paths::Paths;
use crate::{Error, Result};

pub use app::{Action, App, Content, Look, Need, Run};

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
    let session = read(paths, &notebook)?;
    Ok(App::new(notebook.name, notebook.path, session))
}

/// Everything a session holds that does not depend on which screen is on top.
///
/// One walk of the directory answers both the notes and the files, which is why
/// `inventory` is asked rather than `notes`: the files screen would otherwise be
/// a second walk for a list the first one already went past.
fn read(paths: &Paths, notebook: &Notebook) -> Result<app::Session> {
    let status = notebook.status()?;
    let (notes, files) = notebook.inventory()?;
    Ok(app::Session {
        status,
        notes,
        files,
        notebooks: Notebook::list(paths)?,
        today: cmd::today()?,
    })
}

/// Reads the notebook again into a session already under way, keeping the query
/// and — where the note is still there — the cursor.
///
/// What `r` asks for. noda watches no files: a note written from another window
/// is somebody else's edit, and a browser that rearranged itself underneath a
/// reader mid-sentence would be worse than one that waits to be asked.
pub fn reload(paths: &Paths, app: &mut App) -> Result<()> {
    let notebook = Notebook::open_active(paths)?;
    let session = read(paths, &notebook)?;
    app.replace(session);
    Ok(())
}

fn browse(paths: &Paths, terminal: &mut DefaultTerminal, app: &mut App) -> Result<()> {
    loop {
        refresh(paths, app);
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
                Some(action) => {
                    // A command that goes to the network takes long enough that
                    // the last frame would sit there looking like nothing had
                    // happened. Draw once, saying what is being waited for,
                    // before handing over.
                    if let Some(said) = action.working() {
                        app.working = Some(said);
                        terminal.draw(|frame| view::draw(frame, app))?;
                        app.working = None;
                    }
                    perform(paths, terminal, app, action)?;
                }
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
        // What a key names is `Notebook::resolve`'s question, and this is the
        // only place that may ask it: an id prefix that names two notes has one
        // answer, and it is the refusal the prompt would have printed.
        //
        // Read again first, because the note being asked for may be newer than
        // the listing — a note written from another window is exactly the sort
        // of thing somebody opens by name.
        Action::Open(key) => {
            let notebook = Notebook::open_active(paths)?;
            match notebook.resolve(&key) {
                Ok((id, _)) => {
                    reload(paths, app)?;
                    app.open_note(id);
                }
                Err(e) => app.report(Err(e)),
            }
            return Ok(());
        }
        // The same question, asked for a screen about the note rather than a
        // screen of it.
        Action::Show { key, look } => {
            let notebook = Notebook::open_active(paths)?;
            match notebook.resolve(&key) {
                Ok((id, _)) => app.look_at(look, id),
                Err(e) => app.report(Err(e)),
            }
            return Ok(());
        }
        // A different notebook is a different session: the name, the directory,
        // the notes and every screen in the stack were all about the last one.
        // Built fresh rather than reloaded, because `reload` keeps precisely the
        // things that do not survive the move.
        Action::Use(name) => {
            match cmd::use_notebook(paths, &name) {
                Ok(said) => {
                    *app = load(paths)?;
                    app.report(Ok(said));
                }
                Err(e) => app.report(Err(e)),
            }
            return Ok(());
        }
        Action::Run(run) => match run {
            // Reporting only. `doctor` writes when it is asked to at the prompt;
            // from a browser it says what it found, because a keystroke that
            // rewrote a directory is not something to discover afterwards.
            Run::Doctor { links, times } => cmd::doctor(paths, true, links, times),
            Run::Status => cmd::status(paths),
            Run::Readme => cmd::readme(paths, false),
            Run::Snapshot(Some(name)) => cmd::snapshot(paths, &name, None),
            Run::Snapshot(None) => cmd::snapshot_ls(paths),
            Run::Sync => cmd::sync(paths),
            Run::Push => cmd::push(paths),
            Run::Pull => cmd::pull(paths),
        },
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
        Action::Restore { key, rev, touch } => cmd::restore(paths, &key, &rev, touch),
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

/// Fetches whatever the screen that has just been opened is a screen of.
///
/// Once per screen rather than once per keystroke: [`App::wanted`] answers
/// `None` as soon as the screen has what it is about, so the ordinary frame
/// opens no repository at all. Moving a cursor reads nothing.
///
/// A note's file is read rather than the note re-rendered from memory, for the
/// reason `noda show` reads it: what is on screen should be what is on disk,
/// down to a frontmatter field noda does not interpret. The rest come from
/// `notebook`, which is where a browser reads from — `cmd` is the layer that
/// turns them into text for a pipe, and this is a second reader of the same
/// answers rather than a second source of them.
///
/// What cannot be fetched closes the screen and says why. Leaving an empty
/// screen up with the reason on a card over the top of it would leave the reader
/// somewhere with nothing on it once the card was dismissed.
///
/// Public because it is the one step between a keystroke and a frame that the
/// state cannot take for itself: a test that draws a screen has to take it too.
pub fn refresh(paths: &Paths, app: &mut App) {
    let Some(need) = app.wanted() else {
        return;
    };
    // Taken before the fetch, so what comes back can be checked against the
    // screen that asked for it: a slow blame must not land on whatever screen
    // the reader has moved to in the meantime.
    let asked = app.view().clone();

    // A note's file is the one thing here that needs no repository, and it is
    // also the one whose failure is not worth closing a screen over — a note
    // deleted from another window is more likely than anything else, and the
    // reason belongs where the note would have been.
    if let Need::Note { id: _, path } = &need {
        let text =
            std::fs::read_to_string(path).unwrap_or_else(|e| format!("{}: {e}\n", path.display()));
        app.supply(&asked, Content::Note(text));
        return;
    }

    match fetch(paths, need) {
        Ok(content) => app.supply(&asked, content),
        Err(e) => {
            app.give_up();
            app.report(Err(e));
        }
    }
}

fn fetch(paths: &Paths, need: Need) -> Result<Content> {
    let notebook = Notebook::open_active(paths)?;
    Ok(match need {
        // Answered by the caller, which reads a file rather than opening a
        // repository. Said rather than panicked over if it ever gets here: a
        // browser is a loop, and this crate aborts on panic — ending somebody's
        // session over an impossible case is a worse answer than a card.
        Need::Note { .. } => {
            return Err(Error::msg("a note's file is read without the repository"));
        }
        // Two refs compared beside the walk that was happening anyway, and
        // nothing on the network — the same bargain the chrome's `↑2 ↓3` makes.
        Need::Log(id) => Content::Log(
            notebook.log(id.as_deref(), None)?,
            notebook.unpushed(&notebook.branch()?)?,
        ),
        Need::Blame { id, slug } => Content::Blame(notebook.blame(&id, &slug)?),
        Need::Deleted => Content::Deleted(notebook.deleted()?),
        // Built by `cmd`, and then stripped of the colour `cmd` painted it for a
        // pipe. The patch itself is the part worth having written down once; what
        // colour a `+` line is, is the drawing's business here as it is for every
        // other listing on screen.
        Need::Diff => {
            Content::Diff(anstream::adapter::strip_str(&cmd::diff(paths, None)?).to_string())
        }
    })
}
