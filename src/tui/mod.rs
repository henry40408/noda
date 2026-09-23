//! `noda tui`: a stack of full-width screens, so the listing keeps its place
//! while a note is read.
//!
//! Notes are changed only through `cmd` (`e` runs `noda edit`, `Ctrl-d` runs
//! `noda rm`), and the status line shows what that command returned.
//!
//! [`app`] is the state, `field` the line typed into, [`view`] and `frame`
//! the drawing; only this module opens a repository, reads a file, runs a
//! command or touches a terminal, so the rest is tested without one.

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

/// Returns the empty string: everything was said on screen.
pub fn run(paths: &Paths) -> Result<String> {
    // Both ends: a piped stdout gets escape sequences, and no keyboard means no
    // way to quit.
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        return Err(Error::msg(
            "noda tui needs a terminal at both ends; \
             `noda ls`, `noda search` and `noda show` are the ones to redirect",
        ));
    }

    // Before taking over the screen, so an error is printed at the prompt.
    let mut app = load(paths)?;

    // ratatui's panic hook restores the terminal; a `Drop` guard would not run
    // because release builds abort on panic.
    let mut terminal = ratatui::try_init()?;
    let outcome = browse(paths, &mut terminal, &mut app);
    let restored = ratatui::try_restore();

    // A failed restore is reported only if the session itself succeeded.
    outcome?;
    restored?;
    Ok(String::new())
}

/// Every note, bodies included, is read once and held for the session.
pub fn load(paths: &Paths) -> Result<App> {
    let notebook = Notebook::open_active(paths)?;
    let session = read(paths, &notebook)?;
    Ok(App::new(notebook.name, notebook.path, session))
}

/// `inventory` rather than `notes`: one walk yields the notes and the files
/// screen.
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

/// What `r` asks for. No file watching, so the screen never shifts under a
/// reader.
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
        // Blocking rather than polling, which would keep a laptop awake. A
        // resize needs no arm: the next draw uses the new size.
        if let Event::Key(key) = event::read()? {
            match app.on_key(key) {
                Some(Action::Quit) => return Ok(()),
                Some(Action::Reload) => reload(paths, app)?,
                Some(action) => {
                    // A slow action: show that it is working first.
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

/// Maps an action to the `cmd` call the shell would make; nothing else is
/// decided here.
fn perform(
    paths: &Paths,
    terminal: &mut DefaultTerminal,
    app: &mut App,
    action: Action,
) -> Result<()> {
    // Diffed afterwards to find the new note, rather than parsing `add`'s
    // prose.
    let before: Option<HashSet<String>> =
        matches!(action, Action::Add(_)).then(|| app.ids().map(str::to_string).collect());

    let outcome = match action {
        // No wildcard, so a new action cannot be silently swallowed.
        Action::Quit | Action::Reload => return Ok(()),
        // Reload first: a note written from another window is exactly what
        // somebody opens by name.
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
        Action::Show { key, look } => {
            let notebook = Notebook::open_active(paths)?;
            match notebook.resolve(&key) {
                Ok((id, _)) => app.look_at(look, id),
                Err(e) => app.report(Err(e)),
            }
            return Ok(());
        }
        // Built fresh: `reload` keeps exactly the state that must not survive
        // a change of notebook.
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
            // Dry run: a keystroke should not rewrite the notebook.
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
        // No `--update-links`: it edits notes nobody is looking at.
        Action::Retitle { key, title, touch } => cmd::mv(paths, &key, &title, false, touch),
        Action::Tag {
            key,
            changes,
            touch,
        } => cmd::tag(paths, &key, &changes, touch),
        Action::Pin { key, pinned, touch } => cmd::pin(paths, &key, pinned, touch),
        Action::Remove(key) => cmd::rm(paths, &key),
        Action::Restore { key, rev, touch } => cmd::restore(paths, &key, &rev, touch),
        // One commit for the whole queue, through the same code as the keys
        // above.
        Action::Send(steps) => {
            let sent = cmd::bulk(paths, &steps);
            // A refused queue is kept.
            if sent.is_ok() {
                app.sent();
            }
            sent
        }
    };
    app.report(outcome);

    reload(paths, app)?;

    let made = before.and_then(|ids| app.ids().find(|id| !ids.contains(*id)).map(str::to_string));
    if let Some(id) = made {
        app.select_id(&id);
    }
    Ok(())
}

/// Hands the terminal to `$EDITOR`, then takes it back with a fresh
/// `Terminal`, since ratatui's record of the last frame is stale.
///
/// crossterm calls rather than `ratatui::try_init`/`try_restore`: `try_init`
/// stacks another panic hook on every edit. The terminal is replaced rather than
/// `Terminal::clear`ed, which queries the cursor position and, under a pty, can
/// fail with "the cursor position could not be read".
fn in_the_foreground<T>(terminal: &mut DefaultTerminal, run: impl FnOnce() -> T) -> Result<T> {
    disable_raw_mode()?;
    execute!(std::io::stdout(), LeaveAlternateScreen, cursor::Show)?;

    let out = run();

    enable_raw_mode()?;
    // The fresh terminal below assumes a blank screen; clear to make sure.
    execute!(
        std::io::stdout(),
        EnterAlternateScreen,
        Clear(ClearType::All),
        cursor::Hide
    )?;
    *terminal = Terminal::new(CrosstermBackend::new(std::io::stdout()))?;
    Ok(out)
}

/// Fetches what a newly opened screen shows. [`App::wanted`] is `None` once it
/// has it, so an ordinary frame opens no repository.
///
/// A note is read from disk, as `noda show` does, not re-rendered from memory.
/// A failed fetch closes the screen and says why. Public so tests that draw a
/// screen can take this step too.
pub fn refresh(paths: &Paths, app: &mut App) {
    let Some(need) = app.wanted() else {
        return;
    };
    // Captured before the fetch, so a slow result cannot land on another screen.
    let asked = app.view().clone();

    // Needs no repository; a read error is shown in place of the note rather
    // than closing the screen.
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
        // Handled by `refresh`. An error, not a panic, since panics abort.
        Need::Note { .. } => {
            return Err(Error::msg("a note's file is read without the repository"));
        }
        // Unpushed is two local refs, no network.
        Need::Log(id) => Content::Log(
            notebook.log(id.as_deref(), None)?,
            notebook.unpushed(&notebook.branch()?)?,
        ),
        Need::Blame { id, slug } => Content::Blame(notebook.blame(&id, &slug)?),
        Need::Deleted => Content::Deleted(notebook.deleted()?),
        // `cmd::diff` stripped of its pipe colours; `view` colours it. Always
        // the working tree's diff: a screen takes no flags.
        Need::Diff => {
            Content::Diff(anstream::adapter::strip_str(&cmd::diff(paths, None, false)?).to_string())
        }
    })
}
