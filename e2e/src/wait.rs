//! Retrying assertions.
//!
//! A `find` that runs before the browser has finished loading the next document
//! reports the old page. `WebDriver` has no layer that waits for a condition to
//! settle, so these two are it.
//!
//! On timeout they name the last value seen, not merely that a wait expired.
//! That is the difference between "the listing never came down to one row" and a
//! message you have to reproduce by hand to understand.

use std::fmt::Debug;
use std::future::Future;
use std::time::Instant;

use anyhow::{Result, bail};

use crate::browser::{WAIT_INTERVAL, WAIT_TIMEOUT};

/// Polls `probe` until it reports the expected value.
///
/// # Errors
///
/// Fails when `probe` errors, or when the value has still not matched by
/// [`WAIT_TIMEOUT`].
pub async fn eventually_eq<T, E, F, Fut>(what: &str, expected: E, mut probe: F) -> Result<()>
where
    T: Debug,
    E: Debug + PartialEq<T>,
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T>>,
{
    let deadline = Instant::now() + WAIT_TIMEOUT;
    let mut last = probe().await?;
    loop {
        if expected == last {
            return Ok(());
        }
        if Instant::now() >= deadline {
            bail!("{what}: expected {expected:?}, last saw {last:?} after {WAIT_TIMEOUT:?}");
        }
        tokio::time::sleep(WAIT_INTERVAL).await;
        last = probe().await?;
    }
}

/// Polls `probe` until it reports `true`.
///
/// # Errors
///
/// Fails when `probe` errors, or when it has still not held by
/// [`WAIT_TIMEOUT`].
pub async fn eventually<F, Fut>(what: &str, mut probe: F) -> Result<()>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<bool>>,
{
    let deadline = Instant::now() + WAIT_TIMEOUT;
    loop {
        if probe().await? {
            return Ok(());
        }
        if Instant::now() >= deadline {
            bail!("{what}: still not true after {WAIT_TIMEOUT:?}");
        }
        tokio::time::sleep(WAIT_INTERVAL).await;
    }
}
