//! The browser session and its two CDP emulations.
//!
//! `WebDriver::managed` downloads a matching chromedriver but not the browser;
//! [`Browser::open`] says so when Chrome is missing, because the raw driver error
//! does not. `Emulation.setEmulatedMedia` is the only way to reach
//! `prefers-color-scheme`; `Emulation.setScriptExecutionDisabled` runs the
//! script-less pass.
//!
//! **The default window is a phone's**, because `noda web` exists to reach a
//! notebook from a phone.

use std::time::Duration;

use anyhow::{Context, Result};
use thirtyfour::prelude::*;

/// How long a retrying assertion waits before giving up.
pub const WAIT_TIMEOUT: Duration = Duration::from_secs(10);

/// How often it re-checks while waiting.
pub const WAIT_INTERVAL: Duration = Duration::from_millis(100);

/// A phone, in CSS pixels: the iPhone 14's viewport.
pub const PHONE: (u32, u32) = (390, 844);

/// A tablet in portrait: the iPad Air's viewport. Its own layout: at 640px+ the
/// bar becomes a rail and the row gains columns, but below 1024px there is one
/// pane. At desktop width the row stacks again inside the index column.
pub const TABLET: (u32, u32) = (834, 1112);

/// A laptop: over the 1024px pane split and under the 1440px margin, so "two
/// columns and no third" can be asserted.
pub const DESKTOP: (u32, u32) = (1280, 800);

/// A monitor: wide enough for what points at the note to sit beside it.
pub const MONITOR: (u32, u32) = (1680, 1050);

/// Whether the page's own scripts run.
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

    #[must_use]
    pub fn driver(&self) -> &WebDriver {
        &self.driver
    }

    /// Sets the viewport in CSS pixels, through CDP because a window size would
    /// include the platform's window chrome.
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

    /// Runs a script and returns its result. Works in the script-less pass too:
    /// `setScriptExecutionDisabled` stops the document's scripts, not `Execute Script`.
    pub async fn measure(&self, script: &str) -> Result<serde_json::Value> {
        Ok(self
            .driver
            .execute(script, Vec::new())
            .await?
            .json()
            .clone())
    }

    pub async fn quit(self) -> Result<()> {
        self.driver.quit().await?;
        Ok(())
    }

    /// Takes effect on the next document, so it must precede the first
    /// navigation — hence a session per scenario.
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
