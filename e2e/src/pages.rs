//! The pages, as the steps talk about them. One object, since noda's pages share
//! a shape (a bar, maybe a search field, a column of rows).
//!
//! Everything is found by the classes the stylesheet already uses; nothing is
//! added to the markup for tests, since a test-only hook breaks unnoticed.

use anyhow::{Context, Result};
use thirtyfour::prelude::*;

use crate::browser::{Browser, WAIT_INTERVAL, WAIT_TIMEOUT};
use crate::server::BASE_URL;

/// A string literal for `XPath` 1.0, which has no escape character: a value with
/// an apostrophe (a title like "don't") is assembled with `concat`.
fn xpath_string(value: &str) -> String {
    if !value.contains('\'') {
        return format!("'{value}'");
    }
    let pieces: Vec<String> = value
        .split('\'')
        .map(|piece| format!("'{piece}'"))
        .collect();
    format!("concat({})", pieces.join(", \"'\", "))
}

pub struct Page<'a>(pub &'a Browser);

impl Page<'_> {
    fn driver(&self) -> &WebDriver {
        self.0.driver()
    }

    pub async fn go(&self, path: &str) -> Result<()> {
        self.driver().goto(format!("{BASE_URL}{path}")).await?;
        Ok(())
    }

    /// The address bar's path, without the query string.
    pub async fn path(&self) -> Result<String> {
        Ok(self.driver().current_url().await?.path().to_string())
    }

    /// Every visible row's text.
    ///
    /// **One round trip**, so a navigation between finding and reading cannot
    /// cause a `stale element reference`. **The visibility filter is required**:
    /// the listing carries every note and hides the excluded ones, and `innerText`
    /// falls back to `textContent` on an unrendered element. `offsetParent` is
    /// enough because nothing here is `position: fixed`.
    pub async fn rows(&self) -> Result<Vec<String>> {
        let rows = self
            .0
            .measure(
                "return Array.from(document.querySelectorAll('.row'))
                 .filter(e => e.offsetParent !== null).map(e => e.innerText);",
            )
            .await?;
        Ok(rows
            .as_array()
            .context("the page did not answer with a list of rows")?
            .iter()
            .map(|value| value.as_str().unwrap_or_default().to_string())
            .collect())
    }

    /// Rows the query excluded: on the page, not on the screen. `textContent`,
    /// since `innerText` is defined by rendering.
    pub async fn hidden_rows(&self) -> Result<Vec<String>> {
        let rows = self
            .0
            .measure(
                "return Array.from(document.querySelectorAll('.row'))
                 .filter(e => e.offsetParent === null).map(e => e.textContent);",
            )
            .await?;
        Ok(rows
            .as_array()
            .context("the page did not answer with a list of rows")?
            .iter()
            .map(|value| value.as_str().unwrap_or_default().to_string())
            .collect())
    }

    /// Presses any link naming `what`, rows included. Found by `XPath` in one
    /// round trip, and clicked for real so reachability is tested and the
    /// script-less pass is not bypassed through `Execute Script`.
    pub async fn press(&self, what: &str) -> Result<()> {
        let target = format!("//a[contains(., {})]", xpath_string(what));
        self.click(By::XPath(&target), &format!("a link naming {what:?}"))
            .await
    }

    /// [`Self::press`] within the margin note, whose titles usually also appear
    /// in the listing, where `press` would find them first.
    pub async fn press_in_margin(&self, what: &str) -> Result<()> {
        let target = format!(
            "//aside[contains(@class, 'beside')]//a[contains(., {})]",
            xpath_string(what)
        );
        self.click(
            By::XPath(&target),
            &format!("a link naming {what:?} in the margin note"),
        )
        .await
    }

    /// Presses the first *displayed* match, retrying until [`WAIT_TIMEOUT`].
    ///
    /// A stale element counts as "not yet": the network screen reloads itself
    /// during an errand. Hidden matches are skipped because panes can hold two of
    /// a control — below 1024px the index pane's way back is `display: none`, and
    /// clicking it fails as "element not interactable".
    async fn click(&self, target: By, what: &str) -> Result<()> {
        let deadline = std::time::Instant::now() + WAIT_TIMEOUT;
        let mut last;
        loop {
            match self.driver().find_all(target.clone()).await {
                Ok(found) => {
                    last = format!("nothing matching {target:?} is on the screen");
                    for element in found {
                        // Fails on a replaced document: also "not yet".
                        if element.is_displayed().await.unwrap_or(false) {
                            match element.click().await {
                                Ok(()) => return Ok(()),
                                Err(e) => last = e.to_string(),
                            }
                            break;
                        }
                    }
                }
                Err(e) => last = e.to_string(),
            }
            if std::time::Instant::now() >= deadline {
                anyhow::bail!("could not press {what} within {WAIT_TIMEOUT:?}: {last}");
            }
            tokio::time::sleep(WAIT_INTERVAL).await;
        }
    }

    /// Types into the field with this `name`: what the form sends, unlike a
    /// label, which is prose and gets rewritten.
    pub async fn fill(&self, name: &str, value: &str) -> Result<()> {
        let field = self
            .driver()
            .find(By::Css(format!("[name='{name}']")))
            .await
            .with_context(|| format!("this page has no field called {name}"))?;
        field.clear().await?;
        field.send_keys(value).await?;
        Ok(())
    }

    /// Presses the button containing `what`.
    pub async fn submit(&self, what: &str) -> Result<()> {
        let target = format!("//button[contains(., {})]", xpath_string(what));
        self.click(By::XPath(&target), &format!("a button saying {what:?}"))
            .await
    }

    /// Unticks the box for a tag.
    pub async fn untick(&self, tag: &str) -> Result<()> {
        let box_for = format!("input[name='keep'][value='{tag}']");
        self.driver()
            .find(By::Css(&box_for))
            .await
            .with_context(|| format!("no box for the tag {tag:?}"))?
            .click()
            .await?;
        Ok(())
    }

    /// Presses the new-note button, an icon, by the `aria-label` a screen reader
    /// reads.
    pub async fn tap_write(&self) -> Result<()> {
        self.driver()
            .find(By::Css("[aria-label='New note']"))
            .await
            .context("this screen offers no way to write a note")?
            .click()
            .await?;
        Ok(())
    }

    /// The bar item marked `aria-current`, the attribute the colour also hangs
    /// off. `None` rather than an error, since the listing marks nothing and the
    /// retry loop must see "not yet".
    pub async fn marked_place(&self) -> Result<Option<String>> {
        let found = self
            .0
            .measure(
                "const at = document.querySelector('.actionbar a[aria-current]');\
                 return at ? at.innerText.trim() : null;",
            )
            .await?;
        Ok(found.as_str().map(std::string::ToString::to_string))
    }

    pub async fn first_row(&self) -> Result<Option<String>> {
        Ok(self.rows().await?.into_iter().next())
    }

    /// Whether this text is drawn as overdue — by class, not colour, which is
    /// `web/theme.rs`'s business.
    pub async fn is_overdue(&self, text: &str) -> Result<bool> {
        let found = self
            .0
            .measure(&format!(
                "return Array.from(document.querySelectorAll('.overdue'))\
                 .some(e => e.innerText.includes({}));",
                serde_json::to_string(text).unwrap_or_default()
            ))
            .await?;
        Ok(found.as_bool().unwrap_or(false))
    }

    pub async fn tap_back(&self) -> Result<()> {
        self.click(By::Css(".back"), "the way back").await
    }

    /// Types a query and presses `Enter`, as a phone keyboard would.
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

    /// Types a query without submitting, for filter-as-you-type.
    ///
    /// Real keystrokes, and **real backspaces rather than `clear()`**, which
    /// fires no `input` event and so let "delete what you typed" pass against a
    /// script that never ran.
    pub async fn type_search(&self, query: &str) -> Result<()> {
        let field = self
            .driver()
            .find(By::Css("input[name='q']"))
            .await
            .context("this page has no search field")?;
        let held = field.value().await?.unwrap_or_default();
        for _ in 0..held.chars().count() {
            field.send_keys(Key::Backspace).await?;
        }
        if !query.is_empty() {
            field.send_keys(query).await?;
        }
        Ok(())
    }

    /// The address's `q` parameter, if any.
    pub async fn searched(&self) -> Result<Option<String>> {
        Ok(self
            .driver()
            .current_url()
            .await?
            .query_pairs()
            .find(|(name, _)| name == "q")
            .map(|(_, value)| value.to_string()))
    }

    /// The search bar's hint; absent and hidden are both `None`.
    pub async fn hint(&self) -> Result<Option<String>> {
        let said = self
            .0
            .measure(
                "const hint = document.querySelector('.searchbar .hint');
                 return hint && hint.offsetParent !== null ? hint.innerText : null;",
            )
            .await?;
        Ok(said.as_str().map(str::to_string))
    }

    /// The first row's id if drawn. Every row carries it and the stylesheet
    /// hides it on a phone, so this asks `offsetParent`, not the markup.
    pub async fn shown_id(&self) -> Result<Option<String>> {
        let said = self
            .0
            .measure(
                "const id = document.querySelector('.rows .row .ident .id');
                 return id && id.offsetParent !== null ? id.innerText : null;",
            )
            .await?;
        Ok(said.as_str().map(str::to_string))
    }

    /// The search field's parsed grouping, flattened as
    /// `(tag:work or tag:q3) and (budget)`: the brackets stand for the on-screen
    /// pills, which a feature file cannot quote.
    pub async fn grouping(&self) -> Result<Option<String>> {
        let said = self
            .0
            .measure(
                "const parse = document.querySelector('.searchbar .parse');
                 if (!parse || parse.offsetParent === null) return null;
                 return [...parse.children].map((piece) => piece.className === 'g'
                   ? '(' + [...piece.children].map((t) => t.textContent).join(' ') + ')'
                   : piece.textContent).join(' ');",
            )
            .await?;
        Ok(said.as_str().map(str::to_string))
    }

    /// Whether a `<meta refresh>` is present: the script-less network screen
    /// reloads by it, and the script removes it to poll instead.
    pub async fn reloads_itself(&self) -> Result<bool> {
        let meta = self
            .0
            .measure("return !!document.querySelector('meta[http-equiv=\"refresh\"]');")
            .await?;
        Ok(meta.as_bool().unwrap_or(false))
    }

    /// The rendered text (`innerText`) of `selector`, or `""` when absent — not
    /// an error, because after a submit the page may still be the previous one
    /// and the retry loop must see "not yet". Rendered text is what shows
    /// whether `<b>bold</b>` was escaped.
    async fn reads(&self, selector: &str) -> Result<String> {
        let text = self
            .0
            .measure(&format!(
                "const el = document.querySelector('{selector}');
                 return el ? el.innerText : '';"
            ))
            .await?;
        Ok(text.as_str().unwrap_or_default().to_string())
    }

    pub async fn heading(&self) -> Result<String> {
        self.reads("h1").await
    }

    /// The browser's own back button, not the page's chevron (a plain link):
    /// it pops the history entry a press pushed.
    pub async fn go_back(&self) -> Result<()> {
        self.driver().back().await?;
        Ok(())
    }

    /// Marks the window, to tell a pane swap from a navigation: the mark
    /// survives exactly as long as the document.
    pub async fn remember(&self) -> Result<()> {
        self.0.measure("window.__noda_here = 1; return 1;").await?;
        Ok(())
    }

    /// Whether the document marked by [`Self::remember`] is still loaded.
    pub async fn remembered(&self) -> Result<bool> {
        let held = self.0.measure("return !!window.__noda_here;").await?;
        Ok(held.as_bool().unwrap_or(false))
    }

    /// The tab's title. A pane swap delivers it as a `<title>` in the fragment,
    /// so this checks the server's string reached the tab.
    pub async fn tab(&self) -> Result<String> {
        let text = self.0.measure("return document.title;").await?;
        Ok(text.as_str().unwrap_or_default().to_string())
    }

    pub async fn filename(&self) -> Result<String> {
        self.reads(".filename").await
    }

    pub async fn body(&self) -> Result<String> {
        self.reads(".body").await
    }

    /// A note's stamps and tags as one line of text.
    pub async fn stamps(&self) -> Result<String> {
        self.reads(".note-meta").await
    }

    /// The page's error message, if any.
    pub async fn problem(&self) -> Result<Option<String>> {
        let found = self.driver().find_all(By::Css(".problem")).await?;
        match found.first() {
            Some(element) => Ok(Some(element.text().await?)),
            None => Ok(None),
        }
    }

    /// Every control smaller than a thumb, with its measurements — only a
    /// laid-out page knows whether `min-height: var(--tap)` held. Measures the
    /// hit area, not the ink (a 24px chevron in a 48px target).
    pub async fn controls_smaller_than(&self, wide: u32, tall: u32) -> Result<Vec<String>> {
        let measured = self
            .0
            .measure(&format!(
                r"
                const wide = {wide}, tall = {tall};
                const short = [];
                for (const el of document.querySelectorAll('a, input, button')) {{
                    // A note's rendered checkboxes are disabled (ticking one
                    // would be a commit), so they are text, not controls.
                    if (el.disabled) {{ continue; }}
                    // A checkbox's target is its label.
                    const target = el.closest('label') || el;
                    const r = target.getBoundingClientRect();
                    // Laid out to nothing: no target at all.
                    if (r.width === 0 && r.height === 0) {{ continue; }}
                    if (r.width < wide || r.height < tall) {{
                        const what = (target.textContent || el.getAttribute('aria-label')
                            || el.name || el.tagName).trim().slice(0, 40);
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

    /// The whole page's rendered text.
    pub async fn text(&self) -> Result<String> {
        let text = self.0.measure("return document.body.innerText;").await?;
        Ok(text.as_str().unwrap_or_default().to_string())
    }

    /// Every field below `least` px, which iOS Safari zooms into on focus.
    pub async fn fields_under(&self, least: f64) -> Result<Vec<String>> {
        let measured = self
            .0
            .measure(&format!(
                r"
                const least = {least};
                const small = [];
                for (const el of document.querySelectorAll('input, textarea, select')) {{
                    if (el.type === 'hidden' || el.type === 'checkbox') {{ continue; }}
                    const size = parseFloat(getComputedStyle(el).fontSize);
                    if (size < least) {{
                        small.push(`${{el.name || el.type}} — ${{size}}px`);
                    }}
                }}
                return small;
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

    /// The search field's computed font size in CSS pixels; below 16, iOS
    /// Safari zooms the page on focus.
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

    /// The body's computed background colour.
    pub async fn background(&self) -> Result<String> {
        let colour = self
            .0
            .measure("return getComputedStyle(document.body).backgroundColor;")
            .await?;
        Ok(colour.as_str().unwrap_or_default().to_string())
    }

    /// `(left, width, viewport width)` of `selector`: layout questions are about
    /// relationships, which a width alone cannot answer.
    pub async fn box_of(&self, selector: &str) -> Result<(f64, f64, f64)> {
        let measured = self
            .0
            .measure(&format!(
                "const el = document.querySelector('{selector}');
                 if (!el) {{ return null; }}
                 const r = el.getBoundingClientRect();
                 return [r.left, r.width, window.innerWidth];"
            ))
            .await?;
        let numbers = measured
            .as_array()
            .with_context(|| format!("nothing matches {selector}"))?;
        let at = |i: usize| {
            numbers
                .get(i)
                .and_then(serde_json::Value::as_f64)
                .unwrap_or(0.0)
        };
        Ok((at(0), at(1), at(2)))
    }

    /// [`Self::box_of`] relative to a container: the reading column is centred
    /// in its pane, not the window.
    pub async fn box_in(&self, child: &str, parent: &str) -> Result<(f64, f64, f64)> {
        let measured = self
            .0
            .measure(&format!(
                "const p = document.querySelector('{parent}');
                 const c = document.querySelector('{child}');
                 if (!p || !c) {{ return null; }}
                 const pr = p.getBoundingClientRect();
                 const cr = c.getBoundingClientRect();
                 return [cr.left - pr.left, cr.width, pr.width];"
            ))
            .await?;
        let numbers = measured
            .as_array()
            .with_context(|| format!("nothing matches {child} inside {parent}"))?;
        let at = |i: usize| {
            numbers
                .get(i)
                .and_then(serde_json::Value::as_f64)
                .unwrap_or(0.0)
        };
        Ok((at(0), at(1), at(2)))
    }

    /// How many lines a bar's children wrap onto, which only the boxes reveal.
    /// Counts children starting below the previous ones' bottom, not distinct
    /// tops: chips sharing a baseline have several tops but are one line.
    pub async fn lines_of(&self, selector: &str) -> Result<u64> {
        let measured = self
            .0
            .measure(&format!(
                "const el = document.querySelector('{selector}');
                 if (!el) {{ return null; }}
                 const kids = [...el.children]
                     .filter(k => k.getClientRects().length)
                     .map(k => k.getBoundingClientRect())
                     .sort((a, b) => a.top - b.top);
                 let lines = 0;
                 let floor = -Infinity;
                 for (const r of kids) {{
                     if (r.top >= floor - 0.5) {{ lines += 1; floor = r.bottom; }}
                     else {{ floor = Math.min(floor, r.bottom); }}
                 }}
                 return lines;"
            ))
            .await?;
        measured
            .as_u64()
            .with_context(|| format!("nothing matches {selector}"))
    }

    /// Whether the listing pane has width and holds rows; either alone can be
    /// true by accident.
    pub async fn listing_on_screen(&self) -> Result<bool> {
        let seen = self
            .0
            .measure(
                "const pane = document.querySelector('.pane.index');
                 if (!pane) { return false; }
                 return pane.getBoundingClientRect().width > 0
                     && !!pane.querySelector('main.rows a.row');",
            )
            .await?;
        Ok(seen.as_bool().unwrap_or(false))
    }

    /// The title of the listing row marked as being read.
    pub async fn marked_row(&self) -> Result<Option<String>> {
        let title = self
            .0
            .measure(
                "const row = document.querySelector('.pane.index main.rows a.row.here');
                 return row ? row.querySelector('.title').textContent.trim() : null;",
            )
            .await?;
        Ok(title.as_str().map(str::to_string))
    }

    /// The margin note's lines, one per link, or its single "nothing points
    /// here" line. `None` when not drawn (under 1440px, or without scripts) —
    /// asked via `offsetParent`, since the box is always in the markup. The
    /// loading line is not filtered out, so "absent" cannot pass while loading.
    pub async fn margin_note(&self) -> Result<Option<Vec<String>>> {
        let said = self
            .0
            .measure(
                "const aside = document.querySelector('.pane.read .beside');
                 if (!aside || aside.offsetParent === null) return null;
                 const minis = [...aside.querySelectorAll('.mini')];
                 if (minis.length) {
                   return minis.map((m) => m.childNodes[0].textContent.trim());
                 }
                 const one = aside.querySelector('.none,.said');
                 return one ? [one.innerText.trim()] : [];",
            )
            .await?;
        let Some(lines) = said.as_array() else {
            return Ok(None);
        };
        Ok(Some(
            lines
                .iter()
                .filter_map(|line| line.as_str().map(str::to_string))
                .collect(),
        ))
    }

    /// Whether the page scrolls sideways, i.e. a layout that did not fit.
    pub async fn scrolls_sideways(&self) -> Result<bool> {
        let over = self
            .0
            .measure("return document.documentElement.scrollWidth > window.innerWidth;")
            .await?;
        Ok(over.as_bool().unwrap_or(false))
    }
}
