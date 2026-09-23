//! Telling an open editor that the note under it changed, before Save rather
//! than in the merge that answers it.
//!
//! **It watches the file, not the server's writes**, because `noda edit`, a
//! hand-opened editor and `sync` also write to the repository. The cost is
//! hashing one small file every `EVERY` per open note.
//!
//! **One thread for all editors**, walking a registry connections join and
//! leave; a thread per connection could not notice its tab closing.
//!
//! **`stop` ends every stream.** An SSE response never finishes on its own, and
//! axum's graceful shutdown waits for in-flight requests, so `stop` drops every
//! sender to close them.
//!
//! **Polling, not `notify`.** `notify` 8.2.0 adds five crates, and measured
//! against a 7,707,504-byte binary:
//!
//! ```text
//! with `notify`      +    52,112   (+0.68%)
//! with this          +    16,576   (+0.21%)
//! ```
//!
//! It costs up to `EVERY` of latency, and a hash does not care whether an
//! editor saved by rename or wrote twice, which a watcher has to.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::Duration;

use tokio::sync::mpsc;

/// How often the file under an open editor is hashed.
const EVERY: Duration = Duration::from_secs(2);

/// Each connection's channel capacity. When it is full a fingerprint is
/// skipped, not the connection: each message supersedes the last.
const BEHIND: usize = 8;

/// One note being watched, and everybody watching it.
struct Watched {
    path: PathBuf,
    /// The last fingerprint seen; each reader compares against its own form.
    seen: String,
    tell: Vec<mpsc::Sender<String>>,
}

#[derive(Default)]
struct State {
    /// By notebook and note id; not the slug, which follows the title.
    notes: HashMap<(String, String), Watched>,
    stopping: bool,
}

/// The registry every open editor is in, and the one thread that walks it.
pub struct Watch {
    shared: Arc<(Mutex<State>, Condvar)>,
}

impl Default for Watch {
    fn default() -> Self {
        Self::new()
    }
}

impl Watch {
    /// Starts the watching thread: a plain `std::thread`, since no request waits
    /// on it and the runtime is built without a timer driver.
    #[must_use]
    pub fn new() -> Watch {
        let shared = Arc::new((Mutex::new(State::default()), Condvar::new()));
        std::thread::spawn({
            let shared = Arc::clone(&shared);
            move || look(&shared)
        });
        Watch { shared }
    }

    /// Puts one open editor into the registry.
    ///
    /// `now` seeds the comparison for a new entry; an existing entry keeps its
    /// fingerprint, so a second reader does not reset the first's.
    pub fn subscribe(
        &self,
        book: &str,
        id: &str,
        path: PathBuf,
        now: &str,
    ) -> mpsc::Receiver<String> {
        let (tell, hear) = mpsc::channel(BEHIND);
        let mut state = self.shared.0.lock().unwrap_or_else(PoisonError::into_inner);
        state
            .notes
            .entry((book.to_string(), id.to_string()))
            .or_insert_with(|| Watched {
                path,
                seen: now.to_string(),
                tell: Vec::new(),
            })
            .tell
            .push(tell);
        hear
    }

    /// Ends every stream, and the thread. Called as the server begins to stop,
    /// since graceful shutdown cannot finish until the streams close.
    pub fn stop(&self) {
        let mut state = self.shared.0.lock().unwrap_or_else(PoisonError::into_inner);
        state.stopping = true;
        state.notes.clear();
        drop(state);
        self.shared.1.notify_all();
    }
}

fn look(shared: &Arc<(Mutex<State>, Condvar)>) {
    let (lock, wake) = &**shared;
    loop {
        {
            let state = lock.lock().unwrap_or_else(PoisonError::into_inner);
            if state.stopping {
                return;
            }
            // A condvar, not `sleep`, so `stop` need not wait out a tick.
            let (state, _) = wake
                .wait_timeout(state, EVERY)
                .unwrap_or_else(PoisonError::into_inner);
            if state.stopping {
                return;
            }
        }

        // Hashing is file I/O, so it runs without the lock.
        let wanted: Vec<((String, String), PathBuf)> = {
            let mut state = lock.lock().unwrap_or_else(PoisonError::into_inner);
            // Prune closed readers here, not only at a send: a quiet note is
            // never sent to, so its closed tab would be watched forever.
            state.notes.retain(|_, watched| {
                watched.tell.retain(|tell| !tell.is_closed());
                !watched.tell.is_empty()
            });
            state
                .notes
                .iter()
                .map(|(key, watched)| (key.clone(), watched.path.clone()))
                .collect()
        };

        let looked: Vec<((String, String), String)> = wanted
            .into_iter()
            .filter_map(|(key, path)| {
                // A renamed or deleted note is skipped, not reported as changed.
                git2::Oid::hash_file(git2::ObjectType::Blob, &path)
                    .ok()
                    .map(|oid| (key, oid.to_string()))
            })
            .collect();

        let mut state = lock.lock().unwrap_or_else(PoisonError::into_inner);
        if state.stopping {
            return;
        }
        for (key, hash) in looked {
            let Some(watched) = state.notes.get_mut(&key) else {
                continue;
            };
            if watched.seen == hash {
                continue;
            }
            watched.seen.clone_from(&hash);
            // `try_send`: never wait on a reader while holding the lock.
            watched.tell.retain(|tell| {
                !matches!(
                    tell.try_send(hash.clone()),
                    Err(mpsc::error::TrySendError::Closed(_))
                )
            });
        }
    }
}
