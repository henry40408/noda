//! The Cucumber runner (`harness = false`: cucumber drives the scenarios). Run
//! with `cargo test --test e2e` from `e2e/`.
//!
//! **Every scenario runs twice, with the page's scripts enabled and disabled,
//! and must pass both ways**, so working without JavaScript is a property of the
//! whole interface rather than of the scenarios somebody remembered.
//!
//! The one exception is `@scripted`: a scenario about the shortcut itself
//! (filter-as-you-type, polling) runs only in the scripted pass, since with
//! scripts off there is nothing to observe. **It may be about the shortcut, never
//! the only account of a result** — every claim about what an answer is must also
//! be made by an untagged scenario that goes through the form.

mod steps;

use cucumber::World as _;
use cucumber::writer::Stats as _;
use noda_e2e::Server;
use noda_e2e::browser::Scripting;
use noda_e2e::world::NodaWorld;

const FEATURES: &str = "features";

const ONLY_SCRIPTED: &str = "scripted";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Killed, and its notebook removed, on drop.
    let _server = Server::start()?;

    eprintln!("\n── with the page's scripts enabled");
    let scripted = run(Scripting::Enabled).await;

    eprintln!("\n── with the page's scripts disabled");
    let plain = run(Scripting::Disabled).await;

    // Both passes run before failing: which of the two broke is the diagnosis.
    let failures = scripted + plain;
    anyhow::ensure!(failures == 0, "{failures} cucumber failure(s)");
    Ok(())
}

/// One pass over every feature, returning its failure count.
async fn run(scripting: Scripting) -> usize {
    let writer = NodaWorld::cucumber()
        // The scenarios share one server and notebook.
        .max_concurrent_scenarios(1)
        // A skipped step is one whose definition was deleted; silence reads as a pass.
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
                // Some scenarios write; restore the fixture so later ones (and
                // the second pass) see the notes they expect.
                noda_e2e::server::reset().expect("could not put the notebook back");
            })
        })
        // A tag rather than a list of features, so a scenario opts in where it is read.
        .filter_run(FEATURES, move |_feature, _rule, scenario| {
            scripting == Scripting::Enabled || !scenario.tags.iter().any(|tag| tag == ONLY_SCRIPTED)
        })
        .await;

    writer.failed_steps() + writer.parsing_errors() + writer.hook_errors()
}
