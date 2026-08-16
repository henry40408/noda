//! The Cucumber runner.
//!
//! `harness = false`: cucumber drives the scenarios itself, so there is no
//! libtest harness collecting `#[test]` functions. Run it with
//! `cargo test --test e2e` from `e2e/`.
//!
//! **Every scenario runs twice — once with the page's own scripts enabled and
//! once with them disabled — and has to pass both ways.** "It works with
//! JavaScript off" is the contract for the whole interface, and the moment it
//! becomes a property of particular scenarios it becomes a property of
//! whichever ones somebody remembered.
//!
//! One exception, and it arrived with the enhancement layer that this file
//! spent six pull requests keeping honest: a scenario tagged `@scripted` runs
//! only in the scripted pass. It has to, because what it describes is the
//! shortcut — filtering as you type, polling instead of reloading — and with
//! scripts off there is nothing to observe, not a different outcome.
//!
//! The tag is deliberately narrow, and the narrowing is what keeps it from
//! being the escape hatch the paragraph above was written to prevent: **it buys
//! a scenario the right to be about the shortcut, never the right to be the
//! only account of the result.** Every claim about what an answer *is* — which
//! rows a query allows, what the page says about them — is also made by an
//! untagged scenario that goes through the form. If a `@scripted` scenario is
//! ever the only place a result is asserted, the scriptless path has stopped
//! being tested and the tag is how it happened.

mod steps;

use cucumber::World as _;
use cucumber::writer::Stats as _;
use noda_e2e::Server;
use noda_e2e::browser::Scripting;
use noda_e2e::world::NodaWorld;

const FEATURES: &str = "features";

/// The tag that means "this scenario is about the shortcut".
const ONLY_SCRIPTED: &str = "scripted";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Killed, and its notebook removed, when this binding drops.
    let _server = Server::start()?;

    eprintln!("\n── with the page's scripts enabled");
    let scripted = run(Scripting::Enabled).await;

    eprintln!("\n── with the page's scripts disabled");
    let plain = run(Scripting::Disabled).await;

    // Both passes run before either can fail the process: knowing that the
    // script-less run broke *and* that the scripted one did not is the whole
    // diagnosis, and stopping after the first would hide half of it.
    let failures = scripted + plain;
    anyhow::ensure!(failures == 0, "{failures} cucumber failure(s)");
    Ok(())
}

/// One pass over every feature, reporting how many ways it failed.
async fn run(scripting: Scripting) -> usize {
    let writer = NodaWorld::cucumber()
        // One browser at a time. The scenarios share a server and a notebook,
        // and a second Chromium buys less than it costs.
        .max_concurrent_scenarios(1)
        // A skipped step is a step whose definition somebody deleted. Silence
        // there reads as a pass.
        .fail_on_skipped()
        .before(move |_feature, _rule, _scenario, world| {
            Box::pin(async move {
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
                // The notebook goes back to what the fixture built, because some
                // of these scenarios write to it. Without this the second pass
                // opens a notebook the first one edited, and fails on notes that
                // are no longer called what they were called.
                noda_e2e::server::reset().expect("could not put the notebook back");
            })
        })
        // The scriptless pass skips what only exists when scripts run. Written
        // as "unless it is tagged" rather than as a list of features, so that a
        // scenario opts itself in where it is read.
        .filter_run(FEATURES, move |_feature, _rule, scenario| {
            scripting == Scripting::Enabled || !scenario.tags.iter().any(|tag| tag == ONLY_SCRIPTED)
        })
        .await;

    writer.failed_steps() + writer.parsing_errors() + writer.hook_errors()
}
