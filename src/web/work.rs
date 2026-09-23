//! `sync`, `pull` and `push` from the browser: the commands that take time.
//!
//! The request does not wait: a `POST` starts the errand and answers `303`, and
//! the page it lands on reports progress. Hence:
//!
//! - **A reload does not start it again**, since what the reader holds after
//!   the redirect is a `GET`.
//! - **One errand per notebook**, since two pushes meet in `index.lock`. Asking
//!   again is not an error: the reader may not know the first press landed.
//! - **The outcome outlives the errand**, or a finished run would look like a
//!   button that did nothing.
//!
//! It runs on a plain `std::thread` (the blocking pool is for work a request
//! waits on), with its own `Notebook` because `git2::Repository` is `!Send`,
//! under the notebook's write lock so a merge cannot land mid-save. Being the
//! only work that outlives its request, it is what a shutdown waits for
//! (`settle`).

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

    pub fn stuck(self) -> &'static str {
        match self {
            Errand::Sync => "Sync failed",
            Errand::Pull => "Pull failed",
            Errand::Push => "Push failed",
        }
    }

    /// Through `cmd`, so `sync` here is the same commit-pull-push as the CLI's.
    fn run(self, notebook: &Notebook) -> Result<String> {
        match self {
            Errand::Sync => cmd::sync_in(notebook),
            Errand::Pull => cmd::pull_in(notebook),
            Errand::Push => cmd::push_in(notebook),
        }
    }
}

/// The failure is a `String`: it is read after its thread is gone.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Outcome {
    /// The command's answer (several lines, for `sync`).
    Went(String),
    Failed(String),
}

/// One notebook's errand: the one running, or the last one that ran.
struct Doing {
    errand: Errand,
    outcome: Option<Outcome>,
    started: Instant,
    /// Set when it ends, so a finished errand stops ageing.
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

/// Errands by notebook; the condvar is what a shutdown waits on (`settle`).
#[derive(Default)]
pub struct Errands {
    state: Mutex<State>,
    ended: Condvar,
}

#[derive(Default)]
struct State {
    each: BTreeMap<String, Doing>,
    /// Under the map's lock, not an `AtomicBool`: `settle` tests it then waits,
    /// and a flag set between the two would be a lost wake-up.
    abandoned: bool,
}

impl State {
    /// The errands still going, in notebook order.
    fn running(&self) -> Vec<(String, Errand)> {
        self.each
            .iter()
            .filter(|(_, doing)| doing.outcome.is_none())
            .map(|(book, doing)| (book.clone(), doing.errand))
            .collect()
    }
}

impl Errands {
    /// Whether the caller should start the thread. Check and mark share a lock,
    /// so two simultaneous requests cannot both be first.
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
        // Unconditionally: `settle` must wake however this ended.
        self.ended.notify_all();
    }

    /// For a shutdown to announce what it waits for.
    pub fn running(&self) -> Vec<(String, Errand)> {
        self.held().running()
    }

    /// Blocks until no errand is running, returning what it gave up on (empty
    /// in an ordinary shutdown). The last thing `serve` does: a process killed
    /// mid-`sync` leaves `index.lock` behind and breaks the next write.
    ///
    /// No timeout, since no duration makes abandoning a push right; a second
    /// signal (`abandon`) ends the wait.
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

    /// Stops `settle` waiting; the errand itself cannot be stopped.
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

    /// Ignores poisoning, so one panic does not disable the button for the
    /// session.
    fn held(&self) -> std::sync::MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// Runs the errand. The notebook is opened on the errand's thread, since a
/// `Repository` cannot cross one.
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

    #[test]
    fn one_notebook_runs_one_errand() {
        let errands = Errands::default();
        assert!(errands.begin("work", Errand::Sync));
        assert!(!errands.begin("work", Errand::Push));
        assert!(errands.begin("home", Errand::Pull));

        errands.finish("work", Outcome::Went("pull: already up to date".into()));
        assert!(errands.begin("work", Errand::Push));
    }

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

        errands.begin("work", Errand::Push);
        let report = errands.report("work").expect("it was begun");
        assert_eq!(report.errand, Errand::Push);
        assert!(report.running());
    }

    /// The only test that fails if `finish` stops notifying.
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
