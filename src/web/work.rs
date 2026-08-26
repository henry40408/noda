//! The three commands that talk to the network, and the fact that they take
//! time.
//!
//! Everything else finishes while the browser waits. `sync` is a fetch, a merge
//! and a push over somebody's tailnet, and a phone showing a white screen for
//! eleven seconds has said nothing about whether it is working.
//!
//! So the request does not wait: a `POST` starts the errand and answers `303`,
//! and the page it lands on says what is going on. Three things follow, and each
//! is why this is a file rather than a `spawn_blocking` in a handler:
//!
//! - **A reload must not start it again.** Only a `POST` begins anything, and
//!   what the reader holds after the redirect is a `GET`.
//!
//! - **One notebook, one errand.** Two pushes meet in `index.lock`. Asking again
//!   is not an error, though: it is somebody who could not tell whether the
//!   first press landed.
//!
//! - **It has to be told apart from having never run**, or a page that says
//!   nothing looks like one that ignored the button — so the outcome outlives
//!   the errand.
//!
//! A plain `std::thread` and not `spawn_blocking`, that pool being for work a
//! request waits on. It opens its own `Notebook` because `git2::Repository` is
//! `!Send`, and takes the write lock because a merge landing mid-Save is the
//! collision the lock is for.
//!
//! Outliving its request makes it the one thing a shutdown has to wait for:
//! closing the listener finishes everything else by definition, and finishes
//! this halfway through a commit. `settle` is that wait.

use std::collections::BTreeMap;
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

use crate::notebook::Notebook;
use crate::{Result, cmd};

/// Which of the three was asked for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Errand {
    Sync,
    Pull,
    Push,
}

impl Errand {
    /// One spelling for the route, the button and the terminal.
    pub fn of(word: &str) -> Option<Errand> {
        match word {
            "sync" => Some(Errand::Sync),
            "pull" => Some(Errand::Pull),
            "push" => Some(Errand::Push),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Errand::Sync => "sync",
            Errand::Pull => "pull",
            Errand::Push => "push",
        }
    }

    /// What to call it while it is happening.
    pub fn doing(self) -> &'static str {
        match self {
            Errand::Sync => "Syncing",
            Errand::Pull => "Pulling",
            Errand::Push => "Pushing",
        }
    }

    /// What to call it once it has.
    pub fn done(self) -> &'static str {
        match self {
            Errand::Sync => "Synced",
            Errand::Pull => "Pulled",
            Errand::Push => "Pushed",
        }
    }

    /// Named after the errand rather than apologising: "Push failed" is a fact
    /// about what was attempted, and the reason is on the line under it.
    pub fn stuck(self) -> &'static str {
        match self {
            Errand::Sync => "Sync failed",
            Errand::Pull => "Pull failed",
            Errand::Push => "Push failed",
        }
    }

    /// Through `cmd` and not `notebook`, on the module's rule. It matters most
    /// here: `sync` is a commit, a pull and a push *in that order*, and a second
    /// arrangement of the three would be a second `sync`.
    fn run(self, notebook: &Notebook) -> Result<String> {
        match self {
            Errand::Sync => cmd::sync_in(notebook),
            Errand::Pull => cmd::pull_in(notebook),
            Errand::Push => cmd::push_in(notebook),
        }
    }
}

/// The failure is a `String` and not an `Error`: by the time anybody reads it
/// the thread that produced it is gone.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Outcome {
    /// What the command printed. Several lines, for `sync`.
    Went(String),
    /// What went wrong, in the words the command used.
    Failed(String),
}

/// One notebook's errand: the one running, or the last one that ran.
struct Doing {
    errand: Errand,
    outcome: Option<Outcome>,
    started: Instant,
    /// Filled in when it ends, so that a finished errand stops ageing.
    took: Option<Duration>,
}

/// An errand as a page needs it, out from under the lock.
pub struct Report {
    pub errand: Errand,
    /// `None` while it is still going.
    pub outcome: Option<Outcome>,
    pub took: Duration,
}

impl Report {
    pub fn running(&self) -> bool {
        self.outcome.is_none()
    }
}

/// Keyed by notebook, two notebooks being two repositories with nothing to
/// collide over.
///
/// **The condvar is what a shutdown waits on**, and it is here because this is
/// the only state in `noda web` outliving the request that made it: everything
/// else finishes inside a request, so closing the listener is the whole of
/// stopping. See `settle`.
#[derive(Default)]
pub struct Errands {
    state: Mutex<State>,
    ended: Condvar,
}

/// The map, and the one thing about it that is not about a notebook.
#[derive(Default)]
struct State {
    each: BTreeMap<String, Doing>,
    /// Under the same lock as the map and not an `AtomicBool` beside it, because
    /// `settle` tests it and then sleeps on the condvar: a flag set between
    /// those two steps is a wake-up sent to a waiter that has not started.
    abandoned: bool,
}

impl State {
    /// The errands still going, as `(notebook, errand)`, in notebook order.
    fn running(&self) -> Vec<(String, Errand)> {
        self.each
            .iter()
            .filter(|(_, doing)| doing.outcome.is_none())
            .map(|(book, doing)| (book.clone(), doing.errand))
            .collect()
    }
}

impl Errands {
    /// The caller starts the thread only if this says yes, and the check and the
    /// mark are under one lock: two requests arriving together must not both be
    /// told they are the first.
    pub fn begin(&self, book: &str, errand: Errand) -> bool {
        let mut state = self.held();
        if state
            .each
            .get(book)
            .is_some_and(|doing| doing.outcome.is_none())
        {
            return false;
        }
        state.each.insert(
            book.to_string(),
            Doing {
                errand,
                outcome: None,
                started: Instant::now(),
                took: None,
            },
        );
        true
    }

    /// Records how it ended. Called by the thread that ran it, always.
    pub fn finish(&self, book: &str, outcome: Outcome) {
        if let Some(doing) = self.held().each.get_mut(book) {
            doing.took = Some(doing.started.elapsed());
            doing.outcome = Some(outcome);
        }
        // Outside the `if` and after the guard: `settle` has to be woken however
        // this ended.
        self.ended.notify_all();
    }

    /// For the one caller that says out loud what it is about to wait for. A
    /// page asks `report`; nothing but a shutdown asks this.
    pub fn running(&self) -> Vec<(String, Errand)> {
        self.held().running()
    }

    /// Blocks until no errand is running, answering with what it gave up on —
    /// nothing at all in an ordinary shutdown.
    ///
    /// **The last thing `serve` does, and why a signal is worth handling here.**
    /// A process killed mid-`sync` leaves `index.lock` behind, and the next write
    /// from anywhere fails with libgit2 saying a file exists.
    ///
    /// It waits rather than timing out: there is no length of time after which
    /// abandoning a half-finished push is right. `abandon` ends it — a second,
    /// deliberate signal, not a guess about somebody's network.
    pub fn settle(&self) -> Vec<(String, Errand)> {
        let mut state = self.held();
        while !state.abandoned && !state.running().is_empty() {
            state = self
                .ended
                .wait(state)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
        state.running()
    }

    /// The second signal. It does not stop the errand — nothing can, short of
    /// the process ending — it stops the waiting.
    pub fn abandon(&self) {
        self.held().abandoned = true;
        self.ended.notify_all();
    }

    /// What that notebook's errand is doing, or did.
    pub fn report(&self, book: &str) -> Option<Report> {
        self.held().each.get(book).map(|doing| Report {
            errand: doing.errand,
            outcome: doing.outcome.clone(),
            took: doing.took.unwrap_or_else(|| doing.started.elapsed()),
        })
    }

    /// A panic in one errand must not take the button away for the session: a
    /// record refusing to be read because a reader once panicked is worse.
    fn held(&self) -> std::sync::MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// Runs the errand and records how it went.
///
/// Given the notebook opened here rather than one passed in, because this runs
/// on a thread of its own and a `Repository` cannot cross one.
pub fn work(errand: Errand, notebook: Result<Notebook>) -> Outcome {
    match notebook.and_then(|notebook| errand.run(&notebook)) {
        Ok(said) => Outcome::Went(said),
        Err(e) => Outcome::Failed(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_word_in_the_url_names_a_command() {
        assert_eq!(Errand::of("sync"), Some(Errand::Sync));
        assert_eq!(Errand::of("pull"), Some(Errand::Pull));
        assert_eq!(Errand::of("push"), Some(Errand::Push));
        assert_eq!(Errand::of("fetch"), None);
        assert_eq!(Errand::of(""), None);
    }

    /// The reason `begin` answers at all: the second press must not start a
    /// second push, and it must not be an error either.
    #[test]
    fn one_notebook_runs_one_errand() {
        let errands = Errands::default();
        assert!(errands.begin("work", Errand::Sync));
        assert!(!errands.begin("work", Errand::Push));
        // A different notebook is a different repository.
        assert!(errands.begin("home", Errand::Pull));

        errands.finish("work", Outcome::Went("pull: already up to date".into()));
        assert!(errands.begin("work", Errand::Push));
    }

    /// An outcome outlives the errand: a page that said nothing after a sync
    /// would look like a page that had ignored the button.
    #[test]
    fn what_it_did_stays_until_the_next_one() {
        let errands = Errands::default();
        assert!(errands.report("work").is_none());

        errands.begin("work", Errand::Sync);
        let report = errands.report("work").expect("it was begun");
        assert_eq!(report.errand, Errand::Sync);
        assert!(report.running());

        errands.finish("work", Outcome::Failed("no remote".into()));
        let report = errands.report("work").expect("it ran");
        assert!(!report.running());
        assert_eq!(report.outcome, Some(Outcome::Failed("no remote".into())));

        // And the next one replaces it rather than joining it.
        errands.begin("work", Errand::Push);
        let report = errands.report("work").expect("it was begun");
        assert_eq!(report.errand, Errand::Push);
        assert!(report.running());
    }

    /// What a shutdown waits for, and what it says it is waiting for.
    ///
    /// The wake-up is the part worth a test: if `finish` stopped notifying,
    /// every other test here would still pass and `noda web` would hang on
    /// Ctrl-C until somebody pressed it again.
    #[test]
    fn stopping_waits_for_an_errand_to_end() {
        let errands = std::sync::Arc::new(Errands::default());
        assert!(errands.begin("work", Errand::Sync));
        assert_eq!(errands.running(), vec![("work".to_string(), Errand::Sync)]);

        let ending = std::sync::Arc::clone(&errands);
        let thread = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            ending.finish("work", Outcome::Went("push: sent 1 commit".into()));
        });

        let waited = Instant::now();
        assert!(errands.settle().is_empty(), "it gave up on something");
        assert!(
            waited.elapsed() >= Duration::from_millis(50),
            "it came back before the errand had ended"
        );
        assert!(errands.running().is_empty());
        thread.join().expect("the errand's thread");
    }

    /// And a second signal ends the wait rather than the errand: `settle` comes
    /// back at once, naming what it walked away from, and the errand is still
    /// running because nothing here can stop one.
    #[test]
    fn a_second_signal_stops_the_waiting() {
        let errands = Errands::default();
        assert!(errands.begin("work", Errand::Push));

        errands.abandon();
        let waited = Instant::now();
        assert_eq!(errands.settle(), vec![("work".to_string(), Errand::Push)]);
        assert!(
            waited.elapsed() < Duration::from_millis(500),
            "it kept waiting after being told not to"
        );
    }

    /// And nothing running is nothing to wait for — both before anything has
    /// happened and after everything has. A shutdown of an idle server has to be
    /// immediate, or the common case is the slow one.
    #[test]
    fn nothing_running_is_nothing_to_wait_for() {
        let errands = Errands::default();
        assert!(errands.running().is_empty());
        assert!(errands.settle().is_empty());

        errands.begin("work", Errand::Pull);
        errands.finish("work", Outcome::Went("pull: already up to date".into()));
        assert!(errands.running().is_empty());
        assert!(errands.settle().is_empty());
    }

    /// A finished errand stops ageing: what the page shows is how long it took,
    /// not how long ago it was.
    #[test]
    fn a_finished_errand_stops_the_clock() {
        let errands = Errands::default();
        errands.begin("work", Errand::Pull);
        errands.finish("work", Outcome::Went("pull: already up to date".into()));
        let took = errands.report("work").expect("it ran").took;
        std::thread::sleep(Duration::from_millis(20));
        assert_eq!(errands.report("work").expect("it ran").took, took);
    }
}
