//! `noda tui` — the notebook as a screen you can go into and come back out of.
//!
//! It exists for the one thing a command cannot do, which is stay: the listing
//! keeps its place while a note is read, and a query narrows it as it is typed.
//!
//! A screen is the whole width and there is a stack of them, which is what lets
//! a note be read at the width it was written at — the pane it used to share
//! with the listing was never wide enough for either.
//!
//! It changes notes by asking the commands to: `e` runs `noda edit`, `Ctrl-d`
//! runs `noda rm`, and what comes back is the line that command would have
//! printed. Nothing here writes a note itself.
//!
//! The parts are kept apart so most of this can be tested with no terminal:
//! [`app`] is the state, [`field`] the line typed into it, [`view`] and
//! [`frame`] the drawing, and this module is the only place that opens a
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

/// The empty string comes back: everything it had to say was said on a screen
/// that no longer exists.
pub fn run(paths: &Paths) -> Result<String> {
    // Both ends: `noda tui | less` is a screenful of escape sequences, and a TUI
    // with no keyboard cannot be quit.
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        return Err(Error::msg(
            "noda tui needs a terminal at both ends; \
             `noda ls`, `noda search` and `noda show` are the ones to redirect",
        ));
    }

    // Before the screen is taken over, so a notebook that cannot be opened says
    // so at the prompt.
    let mut app = load(paths)?;

    // ratatui's own, hook included — which matters more than usual here: this
    // crate aborts on panic in release, so a `Drop` guard would never run and
    // the terminal would be left in raw mode with no echo.
    let mut terminal = ratatui::try_init()?;
    let outcome = browse(paths, &mut terminal, &mut app);
    let restored = ratatui::try_restore();

    // A terminal that could not be put back is reported only when there is
    // nothing else to say.
    outcome?;
    restored?;
    Ok(String::new())
}

/// Every note is held for the session, bodies and all: what `noda search` pays
/// on every invocation, paid once instead.
pub fn load(paths: &Paths) -> Result<App> {
    let notebook = Notebook::open_active(paths)?;
    let session = read(paths, &notebook)?;
    Ok(App::new(notebook.name, notebook.path, session))
}

/// `inventory` rather than `notes`, so one walk answers both: the files screen
/// would otherwise be a second walk for a list the first went past.
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

/// What `r` asks for. noda watches no files: a browser rearranging itself under
/// a reader mid-sentence is worse than one that waits to be asked.
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
        // would keep a laptop awake to find that out. A resize needs no arm —
        // the top of this loop draws to whatever the size is by then.
        if let Event::Key(key) = event::read()? {
            match app.on_key(key) {
                Some(Action::Quit) => return Ok(()),
                Some(Action::Reload) => reload(paths, app)?,
                Some(action) => {
                    // Long enough that the last frame would look like nothing
                    // had happened, so draw once before handing over.
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

/// The same command the shell would have run, called the same way. Nothing is
/// decided here about what a change means — only which command means it.
fn perform(
    paths: &Paths,
    terminal: &mut DefaultTerminal,
    app: &mut App,
    action: Action,
) -> Result<()> {
    // Before the command runs, so a note `a` just made can be told from the
    // ones already there — `add`'s answer is a sentence written for a person.
    let before: Option<HashSet<String>> =
        matches!(action, Action::Add(_)).then(|| app.ids().map(str::to_string).collect());

    let outcome = match action {
        // Named rather than left to a wildcard, so a later action cannot be
        // quietly swallowed.
        Action::Quit | Action::Reload => return Ok(()),
        // `Notebook::resolve`'s question, asked only here: a prefix naming two
        // notes has one answer. Read again first, because a note written from
        // another window is exactly what somebody opens by name.
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
        // A different notebook is a different session, built fresh rather than
        // reloaded: `reload` keeps precisely what does not survive the move.
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
            // Reporting only: a keystroke that rewrote a directory is not
            // something to discover afterwards.
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
        // `--update-links` edits the prose of notes nobody is looking at, which
        // is not a thing a browser should do.
        Action::Retitle { key, title, touch } => cmd::mv(paths, &key, &title, false, touch),
        Action::Tag {
            key,
            changes,
            touch,
        } => cmd::tag(paths, &key, &changes, touch),
        Action::Remove(key) => cmd::rm(paths, &key),
        Action::Restore { key, rev, touch } => cmd::restore(paths, &key, &rev, touch),
        // The whole queue in one commit: `bulk` runs the same code the keys
        // above run, and only the commit boundary moved.
        Action::Send(steps) => {
            let sent = cmd::bulk(paths, &steps);
            // A refused queue is a queue you still have.
            if sent.is_ok() {
                app.sent();
            }
            sent
        }
    };
    app.report(outcome);

    // Whatever happened, the notebook on screen is now a guess.
    reload(paths, app)?;

    // The one note they are certainly looking for is the one just made.
    let made = before.and_then(|ids| app.ids().find(|id| !ids.contains(*id)).map(str::to_string));
    if let Some(id) = made {
        app.select_id(&id);
    }
    Ok(())
}

/// Hands the terminal back for `$EDITOR`, the only such command noda runs.
///
/// The alternate screen and raw mode go, so the editor starts as it would from
/// the shell. Coming back the screen is cleared rather than redrawn: ratatui's
/// record describes a frame gone for as long as the edit took.
///
/// crossterm's own calls rather than `ratatui::try_init`/`try_restore`, for two
/// reasons. `try_init` installs a panic hook around the one already there, and
/// twenty edits would leave twenty. And the terminal is *replaced* rather than
/// cleared: `Terminal::clear` asks where the cursor is and waits for a reply
/// some terminals never send — under a pty that wait ends the session with "the
/// cursor position could not be read".
fn in_the_foreground<T>(terminal: &mut DefaultTerminal, run: impl FnOnce() -> T) -> Result<T> {
    disable_raw_mode()?;
    execute!(std::io::stdout(), LeaveAlternateScreen, cursor::Show)?;

    let out = run();

    enable_raw_mode()?;
    // Switching is specified to clear it, but the fresh terminal below believes
    // the screen is blank and that belief is cheap to make true.
    execute!(
        std::io::stdout(),
        EnterAlternateScreen,
        Clear(ClearType::All),
        cursor::Hide
    )?;
    *terminal = Terminal::new(CrosstermBackend::new(std::io::stdout()))?;
    Ok(out)
}

/// Fetches whatever the screen just opened is a screen of.
///
/// Once per screen rather than per keystroke: [`App::wanted`] answers `None` as
/// soon as the screen has what it is about, so the ordinary frame opens no
/// repository.
///
/// A note's file is read rather than re-rendered from memory, for `noda show`'s
/// reason. The rest come from `notebook` — a second reader of the same answers
/// rather than a second source of them.
///
/// What cannot be fetched closes the screen and says why, rather than leaving an
/// empty one behind once the card is dismissed.
///
/// Public because it is the one step between a keystroke and a frame the state
/// cannot take for itself: a test that draws a screen takes it too.
pub fn refresh(paths: &Paths, app: &mut App) {
    let Some(need) = app.wanted() else {
        return;
    };
    // Before the fetch, so a slow blame does not land on whatever screen the
    // reader has moved to.
    let asked = app.view().clone();

    // The one thing needing no repository, and the one whose failure is not
    // worth closing a screen over: the reason belongs where the note would be.
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
        // Answered by the caller. Said rather than panicked over: this crate
        // aborts on panic, and ending a session over an impossible case is a
        // worse answer than a card.
        Need::Note { .. } => {
            return Err(Error::msg("a note's file is read without the repository"));
        }
        // Two refs beside a walk that was happening anyway, and no network.
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
        // The working tree's diff, not the remote's. `:diff` is a screen and a
        // screen takes no flags; asking for the other one would be a second
        // command name to invent, and that is a decision of its own.
        Need::Diff => {
            Content::Diff(anstream::adapter::strip_str(&cmd::diff(paths, None, false)?).to_string())
        }
    })
}
