//! The Cucumber runner.
//!
//! `harness = false`: cucumber drives the scenarios itself, so there is no
//! libtest harness collecting `#[test]` functions. Run it with
//! `cargo test --test e2e` from `e2e/`.
//!
//! **Every scenario runs twice — once with the page's own scripts enabled and
//! once with them disabled — and has to pass both ways.** Not a tag on selected
//! scenarios, because there is nothing here that is only true one way: "it works
//! with JavaScript off" is the contract for the whole interface, and the moment
//! it becomes a property of particular scenarios it becomes a property of
//! whichever ones somebody remembered.
//!
//! PR 1 ships no script at all, so today the two passes are trivially the same.
//! That is the argument for fixing it in place now rather than later: the
//! contract is easy to keep while it is free and hard to recover once there is
//! an enhancement layer to hide behind.

mod steps;

use std::sync::atomic::{AtomicBool, Ordering};

use cucumber::World as _;
use cucumber::writer::Stats as _;
use noda_e2e::Server;
use noda_e2e::browser::Scripting;
use noda_e2e::world::NodaWorld;

const FEATURES: &str = "features";

/// Which pass is under way.
///
/// A global rather than something threaded through, because the only thing that
/// reads it is the `before` hook, which cucumber calls with a fixed signature.
static SCRIPTS_RUN: AtomicBool = AtomicBool::new(true);

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Killed, and its notebook removed, when this binding drops.
    let _server = Server::start()?;

    eprintln!("\n── with the page's scripts enabled");
    SCRIPTS_RUN.store(true, Ordering::Relaxed);
    let scripted = run().await;

    eprintln!("\n── with the page's scripts disabled");
    SCRIPTS_RUN.store(false, Ordering::Relaxed);
    let plain = run().await;

    // Both passes run before either can fail the process: knowing that the
    // script-less run broke *and* that the scripted one did not is the whole
    // diagnosis, and stopping after the first would hide half of it.
    let failures = scripted + plain;
    anyhow::ensure!(failures == 0, "{failures} cucumber failure(s)");
    Ok(())
}

/// One pass over every feature, reporting how many ways it failed.
async fn run() -> usize {
    let writer = NodaWorld::cucumber()
        // One browser at a time. The scenarios share a server and a notebook,
        // and a second Chromium buys less than it costs.
        .max_concurrent_scenarios(1)
        // A skipped step is a step whose definition somebody deleted. Silence
        // there reads as a pass.
        .fail_on_skipped()
        .before(|_feature, _rule, _scenario, world| {
            Box::pin(async move {
                let scripting = if SCRIPTS_RUN.load(Ordering::Relaxed) {
                    Scripting::Enabled
                } else {
                    Scripting::Disabled
                };
                world
                    .open(scripting)
                    .await
                    .expect("could not open a browser session");
            })
        })
        .after(|_feature, _rule, _scenario, _finished, world| {
            Box::pin(async move {
                if let Some(world) = world {
                    world.close().await.expect("could not close the session");
                }
            })
        })
        .run(FEATURES)
        .await;

    writer.failed_steps() + writer.parsing_errors() + writer.hook_errors()
}
