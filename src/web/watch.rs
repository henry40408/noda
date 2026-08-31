//! Telling an open editor that the note under it moved.
//!
//! The optimistic lock already means an edit onto a note that changed
//! underneath is merged rather than lost. What it cannot do is say so *while*
//! somebody is typing: the first they hear of it is the answer to a Save they
//! have already pressed. This is the same fact, arriving earlier.
//!
//! **It watches the file, not the writes.** A notebook is an ordinary git
//! repository and a terminal in another window writes to it — `noda edit`, an
//! editor opened by hand, a `sync` bringing somebody else's afternoon down from
//! a remote. Broadcasting only what the server itself wrote would be silent in
//! precisely the case noda exists for, so the file is looked at instead. That
//! costs a hash of one small file every couple of seconds per open editor, and
//! buys a mechanism with no blind side.
//!
//! **One thread, however many editors are open.** A thread per connection is a
//! thread that outlives the tab it was opened for: a browser closing a
//! connection is not something a sleeping thread can notice. So there is one,
//! and what it walks is a registry that connections put themselves into and
//! take themselves out of.
//!
//! **Nothing here can hold a connection open past a stop.** An SSE response is
//! by design a request that never finishes, and `axum`'s graceful shutdown
//! waits for what is in flight — so a stop that did not end these would wait
//! for them forever. `stop` drops every sender, each stream sees its channel
//! close, and the wait has nothing left to wait for.
//!
//! **A file watcher was the other way, and it was costed.** `notify` 8.2.0 —
//! the stable one; 9.0.0 is a release candidate — brings five crates and, with
//! a watcher actually constructed so the linker keeps it:
//!
//!     main                7,707,504 bytes
//!     with `notify`      +    52,112   (+0.68%)
//!     with this          +    16,576   (+0.21%)
//!
//! A third of the size, no new crate at all, and it catches the same writes.
//! What it gives up is latency — up to `EVERY` — on an event whose whole
//! content is "somebody else has saved". The watcher's own difficulty argued
//! the same way: an editor that writes by renaming a temporary file over the
//! original produces a create where a reader expects a modify, and some write
//! twice. A hash does not have opinions about how the bytes arrived.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::Duration;

use tokio::sync::mpsc;

/// How often the file under an open editor is looked at.
///
/// The event is "somebody else has saved", which nobody is waiting on to the
/// second — and every tick is a hash of a file that is usually a few kilobytes.
/// Two seconds is under the time it takes to notice a line has appeared.
const EVERY: Duration = Duration::from_secs(2);

/// How many changes a connection may fall behind before it is dropped.
///
/// One would do: every message says the same thing — the file is at this
/// fingerprint now — so a reader that missed three has missed nothing the
/// fourth does not carry. The room is here so a saturated channel cannot make
/// the thread that holds the registry lock wait.
const BEHIND: usize = 8;

/// One note being watched, and everybody watching it.
struct Watched {
    path: PathBuf,
    /// The last fingerprint the thread saw. A change is a change since the last
    /// look, not since any one page was drawn — the readers each compare what
    /// arrives against what their own form holds.
    seen: String,
    tell: Vec<mpsc::Sender<String>>,
}

#[derive(Default)]
struct State {
    /// By notebook and note id. The slug is not in the key: it follows the
    /// title, and two readers on one note must land on one entry.
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
    /// Starts the thread that does the looking.
    ///
    /// A plain `std::thread`, as `work.rs` uses for the same reason: the
    /// blocking pool is for work a request is waiting on, and no request waits
    /// on this. It also keeps the tick out of the runtime, which is built with
    /// I/O only — a timer here would mean a timer driver for the whole server.
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
    /// `now` seeds what the thread compares against, so the reader is told about
    /// the next change rather than about the state of the file when they
    /// arrived. An entry that already exists keeps the fingerprint it had:
    /// a second reader must not reset what the first is waiting on.
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

    /// Ends every stream, and the thread with them.
    ///
    /// Called as the server begins to stop rather than after it has: dropping
    /// the senders is what lets the wait for in-flight requests finish at all.
    pub fn stop(&self) {
        let mut state = self.shared.0.lock().unwrap_or_else(PoisonError::into_inner);
        state.stopping = true;
        // Every sender goes with it, so every reader's channel closes.
        state.notes.clear();
        drop(state);
        self.shared.1.notify_all();
    }
}

/// The loop: wait a tick, look at what has readers, tell them what moved.
fn look(shared: &Arc<(Mutex<State>, Condvar)>) {
    let (lock, wake) = &**shared;
    loop {
        {
            let state = lock.lock().unwrap_or_else(PoisonError::into_inner);
            if state.stopping {
                return;
            }
            // On the condvar and not `sleep`, so a stop is not made to wait out
            // a tick it arrived at the start of.
            let (state, _) = wake
                .wait_timeout(state, EVERY)
                .unwrap_or_else(PoisonError::into_inner);
            if state.stopping {
                return;
            }
        }

        // Hashing happens with the lock down. It is file I/O, and the lock is
        // what every connection opening and closing goes through.
        let wanted: Vec<((String, String), PathBuf)> = {
            let mut state = lock.lock().unwrap_or_else(PoisonError::into_inner);
            // **The liveness check, and it is here rather than at the send.** A
            // sender only learns its reader is gone when it tries to use it, and
            // a note nobody edits again is never sent to — so a tab closed on a
            // quiet note would be watched until the server stopped.
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
                // A note renamed since is a note this path no longer names, and
                // one deleted has no hash at all. Neither is what this exists to
                // catch, and a reader told the file changed because it went
                // missing has been told something misleading.
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
            // `try_send` because the thread holding this lock must not wait on
            // a reader. A full channel is a reader already holding more of these
            // than it can use.
            watched.tell.retain(|tell| {
                !matches!(
                    tell.try_send(hash.clone()),
                    Err(mpsc::error::TrySendError::Closed(_))
                )
            });
        }
    }
}
