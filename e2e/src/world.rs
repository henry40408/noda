//! The Cucumber world: one browser session per scenario.
//!
//! A `before` hook opens the session, not `World::new`, because only the hook
//! knows which pass (scripts on or off) is running.

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

    pub async fn open(&mut self, scripting: Scripting) -> Result<()> {
        self.browser = Some(Browser::open(scripting).await?);
        Ok(())
    }

    /// Ends the session, if one was opened.
    pub async fn close(&mut self) -> Result<()> {
        if let Some(browser) = self.browser.take() {
            browser.quit().await?;
        }
        Ok(())
    }

    pub fn browser(&self) -> Result<&Browser> {
        self.browser
            .as_ref()
            .context("no browser session: the `before` hook did not open one")
    }

    pub fn page(&self) -> Result<Page<'_>> {
        Ok(Page(self.browser()?))
    }
}
