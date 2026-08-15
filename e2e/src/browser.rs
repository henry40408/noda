//! The browser session, and the two emulations the suite depends on.
//!
//! `WebDriver::managed` downloads and supervises a matching chromedriver
//! itself, so nothing has to be installed alongside these tests — but it does
//! *not* download the browser. A Chrome or Chromium in one of the well-known
//! locations is a prerequisite; [`Browser::open`] says so in as many words when
//! it is missing, because the raw driver error does not.
//!
//! Both emulations go through CDP. `Emulation.setEmulatedMedia` is the only way
//! to reach `prefers-color-scheme` at all, and it is what makes the two themes
//! testable rather than merely written down.
//! `Emulation.setScriptExecutionDisabled` is how the script-less pass runs; it
//! is also what Playwright's `javaScriptEnabled: false` did underneath.
//!
//! **A phone-shaped window is the default here, and that is the point.** The
//! reason `noda web` exists is reading and writing a notebook from a phone, so
//! the size a scenario runs at unless it says otherwise is a phone's.

use std::time::Duration;

use anyhow::{Context, Result};
use thirtyfour::prelude::*;

/// How long a retrying assertion waits before giving up.
pub const WAIT_TIMEOUT: Duration = Duration::from_secs(10);

/// How often it re-checks while waiting.
pub const WAIT_INTERVAL: Duration = Duration::from_millis(100);

/// A phone, in CSS pixels: the iPhone 14's viewport.
pub const PHONE: (u32, u32) = (390, 844);

/// A desktop, for the one scenario about the wide layout.
pub const DESKTOP: (u32, u32) = (1280, 800);

/// Whether the page's own scripts run.
///
/// Every scenario is run both ways. PR 1 ships no script at all, which is
/// exactly when this is worth locking in: the contract is that the two passes
/// agree, and it is easier to keep than to recover once an enhancement layer
/// exists to hide behind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scripting {
    Enabled,
    Disabled,
}

/// A browser session, scoped to one scenario.
#[derive(Debug)]
pub struct Browser {
    driver: WebDriver,
}

impl Browser {
    /// Starts a headless session with the page's scripts on or off.
    ///
    /// # Errors
    ///
    /// Fails when no local browser is installed, when the driver cannot be
    /// downloaded, or when the session cannot be created.
    pub async fn open(scripting: Scripting) -> Result<Self> {
        let mut caps = DesiredCapabilities::chrome();
        caps.set_headless()?;
        caps.add_arg(&format!("--window-size={},{}", PHONE.0, PHONE.1))?;
        // Containers get a 64 MB /dev/shm by default, which Chrome outgrows.
        caps.add_arg("--disable-dev-shm-usage")?;

        let driver = WebDriver::managed(caps).await.context(
            "could not start a browser session — a local Chrome or Chromium is required \
             (`brew install --cask chromium`, or `google-chrome` on CI); \
             the driver manager downloads only the driver, never the browser",
        )?;

        let browser = Self { driver };
        browser.resize(PHONE).await?;
        if scripting == Scripting::Disabled {
            browser.disable_scripting().await?;
        }
        Ok(browser)
    }

    /// The underlying session.
    #[must_use]
    pub fn driver(&self) -> &WebDriver {
        &self.driver
    }

    /// Sets the viewport, in CSS pixels.
    ///
    /// Through CDP rather than by setting the window size: a window includes
    /// whatever chrome the platform puts around it, and what the layout is
    /// answering to is the viewport.
    ///
    /// # Errors
    ///
    /// Fails when the CDP command is refused.
    pub async fn resize(&self, (width, height): (u32, u32)) -> Result<()> {
        self.driver
            .cdp()
            .send_raw(
                "Emulation.setDeviceMetricsOverride",
                serde_json::json!({
                    "width": width,
                    "height": height,
                    "deviceScaleFactor": 1,
                    "mobile": true,
                }),
            )
            .await?;
        Ok(())
    }

    /// Emulates `prefers-color-scheme`, with no stored preference.
    ///
    /// There is no theme toggle in noda's pages on purpose — the reader has
    /// already told their phone which they want — so this is the only way the
    /// dark palette is ever reached, and the only way it can be tested.
    ///
    /// # Errors
    ///
    /// Fails when the CDP command is refused.
    pub async fn prefer_scheme(&self, scheme: &str) -> Result<()> {
        self.driver
            .cdp()
            .send_raw(
                "Emulation.setEmulatedMedia",
                serde_json::json!({
                    "media": "screen",
                    "features": [{ "name": "prefers-color-scheme", "value": scheme }],
                }),
            )
            .await?;
        Ok(())
    }

    /// Runs a script and hands back what it returned.
    ///
    /// Works in the script-less pass too:
    /// `Emulation.setScriptExecutionDisabled` stops the *document's* scripts,
    /// not `Execute Script`. That is what makes it possible to measure a layout
    /// on a page that is not allowed to run any code of its own.
    ///
    /// # Errors
    ///
    /// Fails when the script does not run.
    pub async fn measure(&self, script: &str) -> Result<serde_json::Value> {
        Ok(self
            .driver
            .execute(script, Vec::new())
            .await?
            .json()
            .clone())
    }

    /// Ends the session.
    ///
    /// # Errors
    ///
    /// Fails when the driver refuses to close.
    pub async fn quit(self) -> Result<()> {
        self.driver.quit().await?;
        Ok(())
    }

    /// Stops the page's own scripts from running.
    ///
    /// Takes effect on the *next* document, so it is issued before the first
    /// navigation — which is why sessions are per-scenario rather than shared.
    async fn disable_scripting(&self) -> Result<()> {
        self.driver
            .cdp()
            .send_raw(
                "Emulation.setScriptExecutionDisabled",
                serde_json::json!({ "value": true }),
            )
            .await?;
        Ok(())
    }
}
