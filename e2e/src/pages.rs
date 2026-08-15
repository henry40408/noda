//! What the pages are made of, as the steps talk about them.
//!
//! One object rather than one per page: noda's three pages are the same shape —
//! a bar with a way back, sometimes a search field, and a column of rows — and
//! three objects for that would be three copies of `rows()`.
//!
//! Everything is found by class, and the classes are the ones the markup
//! already carries for the stylesheet. Nothing is added to the pages for the
//! benefit of the tests: a hook that only a test uses is a hook nobody notices
//! breaking.

use anyhow::{Context, Result};
use thirtyfour::prelude::*;

use crate::browser::Browser;
use crate::server::BASE_URL;

/// A string literal `XPath` 1.0 will accept.
///
/// `XPath` has no escape character at all, so a value holding an apostrophe has to be
/// assembled out of pieces — which is exactly the shape a note title takes the
/// first time somebody writes "don't".
fn xpath_string(value: &str) -> String {
    if !value.contains('\'') {
        return format!("'{value}'");
    }
    let pieces: Vec<String> = value
        .split('\'')
        .map(|piece| format!("'{piece}'"))
        .collect();
    // The apostrophes themselves come back as double-quoted literals between
    // the pieces they separated.
    format!("concat({})", pieces.join(", \"'\", "))
}

/// The page in front of us.
pub struct Page<'a>(pub &'a Browser);

impl Page<'_> {
    fn driver(&self) -> &WebDriver {
        self.0.driver()
    }

    /// # Errors
    ///
    /// Fails when the navigation does not complete.
    pub async fn go(&self, path: &str) -> Result<()> {
        self.driver().goto(format!("{BASE_URL}{path}")).await?;
        Ok(())
    }

    /// The path currently in the address bar.
    ///
    /// # Errors
    ///
    /// Fails when the driver cannot report a URL.
    pub async fn path(&self) -> Result<String> {
        Ok(self.driver().current_url().await?.path().to_string())
    }

    /// Every row on the page, as the text a reader sees.
    ///
    /// **One round trip, not one per row.** Finding the rows and then asking
    /// each for its text is two visits to a page that may navigate between them,
    /// and the second visit then fails with `stale element reference` — which is
    /// not a fact about the notebook, it is a fact about having looked twice.
    /// Reading them in a single script removes the window rather than retrying
    /// around it.
    ///
    /// # Errors
    ///
    /// Fails when the page cannot be queried.
    pub async fn rows(&self) -> Result<Vec<String>> {
        let rows = self
            .0
            .measure("return Array.from(document.querySelectorAll('.row')).map(e => e.innerText);")
            .await?;
        Ok(rows
            .as_array()
            .context("the page did not answer with a list of rows")?
            .iter()
            .map(|value| value.as_str().unwrap_or_default().to_string())
            .collect())
    }

    /// Presses whatever on the page names `what`.
    ///
    /// Every link, not only the rows: a row is an `<a>` and so is the way out of
    /// an empty search, and a step that could only press one of them would need
    /// the feature to know which kind of thing it was pressing.
    ///
    /// Found by `XPath` so the search and the answer are one round trip — walking
    /// every anchor and asking each for its text is the same
    /// `stale element reference` waiting to happen as reading the rows one at a
    /// time. A real click and not a scripted one: what is under test includes
    /// that the thing is reachable, and the script-less pass would be proving
    /// nothing if the press went through `Execute Script` anyway.
    ///
    /// # Errors
    ///
    /// Fails when nothing names it.
    pub async fn press(&self, what: &str) -> Result<()> {
        // Quoted for XPath rather than interpolated: a title may hold an
        // apostrophe, and `concat` is the only way XPath 1.0 escapes one.
        let target = format!("//a[contains(., {})]", xpath_string(what));
        self.driver()
            .find(By::XPath(&target))
            .await
            .with_context(|| format!("nothing on this page names {what:?}"))?
            .click()
            .await?;
        Ok(())
    }

    /// Presses the way back.
    ///
    /// # Errors
    ///
    /// Fails when the page has no back control.
    pub async fn tap_back(&self) -> Result<()> {
        self.driver()
            .find(By::Css(".back"))
            .await
            .context("this page has no way back")?
            .click()
            .await?;
        Ok(())
    }

    /// Types a query into the search field and sends it.
    ///
    /// `Enter` rather than a submit button, because that is what a form with one
    /// field does and what a phone's keyboard offers.
    ///
    /// # Errors
    ///
    /// Fails when the page has no search field.
    pub async fn search(&self, query: &str) -> Result<()> {
        let field = self
            .driver()
            .find(By::Css("input[name='q']"))
            .await
            .context("this page has no search field")?;
        field.clear().await?;
        field.send_keys(query).await?;
        field.send_keys(Key::Enter).await?;
        Ok(())
    }

    /// The heading of a note.
    ///
    /// # Errors
    ///
    /// Fails when the page has no heading.
    pub async fn heading(&self) -> Result<String> {
        Ok(self.driver().find(By::Css("h1")).await?.text().await?)
    }

    /// The filename line under the heading.
    ///
    /// # Errors
    ///
    /// Fails when the page has no filename.
    pub async fn filename(&self) -> Result<String> {
        Ok(self
            .driver()
            .find(By::Css(".filename"))
            .await?
            .text()
            .await?)
    }

    /// A note's body, as text.
    ///
    /// `WebDriver`'s "Get Element Text" is already the rendered form, so a body
    /// holding `<b>bold</b>` reads back with the angle brackets in it exactly
    /// when the page escaped them — which is the assertion worth making.
    ///
    /// # Errors
    ///
    /// Fails when the page has no body.
    pub async fn body(&self) -> Result<String> {
        Ok(self.driver().find(By::Css(".body")).await?.text().await?)
    }

    /// Whatever the page is saying went wrong, if anything.
    ///
    /// # Errors
    ///
    /// Fails when the page cannot be queried.
    pub async fn problem(&self) -> Result<Option<String>> {
        let found = self.driver().find_all(By::Css(".problem")).await?;
        match found.first() {
            Some(element) => Ok(Some(element.text().await?)),
            None => Ok(None),
        }
    }

    /// Every control that falls short of a thumb, with its measurements.
    ///
    /// **This is the assertion no other layer can make.** The markup says
    /// `min-height: var(--tap)`; whether a control ends up that big depends on
    /// the box it is in, what is beside it, and how the text wrapped. Only a
    /// laid-out page knows.
    ///
    /// Measured as the hit area rather than the ink: a back control draws a
    /// 24-pixel chevron inside a 48-pixel target, and it is the target that the
    /// thumb has to find.
    ///
    /// # Errors
    ///
    /// Fails when the measuring script does not run.
    pub async fn controls_smaller_than(&self, wide: u32, tall: u32) -> Result<Vec<String>> {
        let measured = self
            .0
            .measure(&format!(
                r"
                const wide = {wide}, tall = {tall};
                const short = [];
                for (const el of document.querySelectorAll('a, input, button')) {{
                    const r = el.getBoundingClientRect();
                    // Nothing is measured that nobody can reach: a control laid
                    // out to nothing is not a small target, it is no target.
                    if (r.width === 0 && r.height === 0) {{ continue; }}
                    if (r.width < wide || r.height < tall) {{
                        const what = (el.textContent || el.getAttribute('aria-label') || el.tagName)
                            .trim().slice(0, 40);
                        short.push(`${{what}} — ${{Math.round(r.width)}}x${{Math.round(r.height)}}`);
                    }}
                }}
                return short;
                "
            ))
            .await?;
        Ok(measured
            .as_array()
            .context("the measuring script did not answer with a list")?
            .iter()
            .map(|value| value.as_str().unwrap_or_default().to_string())
            .collect())
    }

    /// The computed font size of the search field, in CSS pixels.
    ///
    /// Below 16, iOS Safari zooms the whole page when the field takes focus and
    /// leaves the reader pinching their way back out. It is a rule about a
    /// browser nobody here is running, which is exactly why it needs a test that
    /// reads the computed value rather than a promise in a stylesheet.
    ///
    /// # Errors
    ///
    /// Fails when the script does not run or there is no search field.
    pub async fn search_field_font_size(&self) -> Result<f64> {
        let size = self
            .0
            .measure(
                r"
                const field = document.querySelector(`input[name='q']`);
                if (!field) { return null; }
                return parseFloat(getComputedStyle(field).fontSize);
                ",
            )
            .await?;
        size.as_f64().context("no search field on this page")
    }

    /// The page's background, as the browser computed it.
    ///
    /// # Errors
    ///
    /// Fails when the script does not run.
    pub async fn background(&self) -> Result<String> {
        let colour = self
            .0
            .measure("return getComputedStyle(document.body).backgroundColor;")
            .await?;
        Ok(colour.as_str().unwrap_or_default().to_string())
    }

    /// Whether the page scrolls sideways.
    ///
    /// A phone with a horizontal scrollbar is a layout that did not fit, and it
    /// is invisible to every assertion made about the markup.
    ///
    /// # Errors
    ///
    /// Fails when the script does not run.
    pub async fn scrolls_sideways(&self) -> Result<bool> {
        let over = self
            .0
            .measure("return document.documentElement.scrollWidth > window.innerWidth;")
            .await?;
        Ok(over.as_bool().unwrap_or(false))
    }
}
