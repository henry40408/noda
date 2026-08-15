//! The Cucumber world: one browser session per scenario.
//!
//! The session cannot be opened in `new`, because whether the page's scripts run
//! is decided by which pass is under way and `World::new` never sees it. A
//! `before` hook opens it instead, which is also the only order that works:
//! `Emulation.setScriptExecutionDisabled` applies to the next document, so it
//! has to be issued before the first navigation.

use anyhow::{Context, Result};
use cucumber::World;

use crate::browser::{Browser, Scripting};
use crate::pages::Page;

/// State shared by the steps of one scenario.
#[derive(Debug, World)]
#[world(init = Self::new)]
pub struct NodaWorld {
    browser: Option<Browser>,
}

impl NodaWorld {
    fn new() -> Self {
        Self { browser: None }
    }

    /// Opens the session for a scenario.
    ///
    /// # Errors
    ///
    /// Fails when no browser session can be started.
    pub async fn open(&mut self, scripting: Scripting) -> Result<()> {
        self.browser = Some(Browser::open(scripting).await?);
        Ok(())
    }

    /// Ends the session, if one was opened.
    ///
    /// # Errors
    ///
    /// Fails when the driver refuses to close.
    pub async fn close(&mut self) -> Result<()> {
        if let Some(browser) = self.browser.take() {
            browser.quit().await?;
        }
        Ok(())
    }

    /// The scenario's browser.
    ///
    /// # Errors
    ///
    /// Fails when no session was opened — a `before` hook that did not run.
    pub fn browser(&self) -> Result<&Browser> {
        self.browser
            .as_ref()
            .context("no browser session: the `before` hook did not open one")
    }

    /// The page in front of us.
    ///
    /// # Errors
    ///
    /// Fails when no session was opened.
    pub fn page(&self) -> Result<Page<'_>> {
        Ok(Page(self.browser()?))
    }
}
