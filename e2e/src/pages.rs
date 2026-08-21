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

use crate::browser::{Browser, WAIT_INTERVAL, WAIT_TIMEOUT};
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

    /// Every row a reader can see, as the text they see.
    ///
    /// **One round trip, not one per row.** Finding the rows and then asking
    /// each for its text is two visits to a page that may navigate between them,
    /// and the second visit then fails with `stale element reference` — which is
    /// not a fact about the notebook, it is a fact about having looked twice.
    /// Reading them in a single script removes the window rather than retrying
    /// around it.
    ///
    /// **The visibility filter is not tidiness, it is correctness.** A listing
    /// now carries every note whatever the query says and hides the excluded
    /// ones, and `innerText` falls back to `textContent` on an element that is
    /// not being rendered — so without the filter every step asking whether a
    /// row is on the screen would answer yes for all of them, forever, in both
    /// passes. `offsetParent` is the cheap form of the question and it is the
    /// right one here: nothing in this interface is `position: fixed`.
    ///
    /// # Errors
    ///
    /// Fails when the page cannot be queried.
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

    /// Every row the query excluded: on the page, not on the screen.
    ///
    /// `textContent` and not `innerText` — the whole question is about elements
    /// that are not being rendered, and `innerText` is defined in terms of what
    /// rendering produced.
    ///
    /// # Errors
    ///
    /// Fails when the page cannot be queried.
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
        self.click(By::XPath(&target), &format!("a link naming {what:?}"))
            .await
    }

    /// The same press, aimed at the margin note.
    ///
    /// A title in the margin is usually a title in the listing as well — the
    /// column is a list of notes in this notebook — so `press` would find the
    /// index row first and prove nothing about the margin. This is the one
    /// place a region has to be named to say which of two identical links is
    /// meant.
    ///
    /// # Errors
    ///
    /// Fails when the margin note holds no such link.
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

    /// Finds a thing and presses it, treating a page that moved underneath as
    /// "not yet".
    ///
    /// **A stale element is the same kind of answer as an element that is not
    /// there.** The network screen brings itself back for news while an errand
    /// is running, so a handle taken a moment ago can belong to a document that
    /// has since been replaced — and a press that failed for that reason has not
    /// failed, it has arrived between two versions of a page. The rule the rest
    /// of this harness follows applies here too: one round trip, an answer that
    /// can say "not yet", and a loop that can see it.
    ///
    /// **And a match that is on the page but not on the screen is a third
    /// answer of the same kind.** A layout of panes can hold two of something —
    /// a note page carries the index pane's way back as well as its own, and
    /// below 1024px the first of those is `display: none`. Taking the first
    /// match got the hidden one and a `WebDriver` "element not interactable",
    /// which is the browser being right. A reader presses the one they can see,
    /// so the first *displayed* match is the one this presses.
    async fn click(&self, target: By, what: &str) -> Result<()> {
        let deadline = std::time::Instant::now() + WAIT_TIMEOUT;
        let mut last;
        loop {
            match self.driver().find_all(target.clone()).await {
                Ok(found) => {
                    last = format!("nothing matching {target:?} is on the screen");
                    for element in found {
                        // `is_displayed` can fail on an element whose document
                        // has just been replaced. That is "not yet" as well.
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

    /// Types into the field with this `name`.
    ///
    /// By `name` and not by label: the name is what the form sends, so it is the
    /// thing the server and the test are actually agreeing about. A label is
    /// prose and gets rewritten.
    ///
    /// # Errors
    ///
    /// Fails when the page has no such field.
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

    /// Presses the button whose words are `what`.
    ///
    /// # Errors
    ///
    /// Fails when no button says it.
    pub async fn submit(&self, what: &str) -> Result<()> {
        let target = format!("//button[contains(., {})]", xpath_string(what));
        self.click(By::XPath(&target), &format!("a button saying {what:?}"))
            .await
    }

    /// Unticks the box for a tag.
    ///
    /// # Errors
    ///
    /// Fails when the note does not carry that tag.
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

    /// Presses the round button that writes a new note.
    ///
    /// By its label and not its words, because it has none: it is one icon, and
    /// what says what it is for is the `aria-label` a screen reader reads. A
    /// test that reached for it by class would be agreeing with the stylesheet
    /// instead of with the reader.
    ///
    /// # Errors
    ///
    /// Fails when this screen offers no way to write.
    pub async fn tap_write(&self) -> Result<()> {
        self.driver()
            .find(By::Css("[aria-label='New note']"))
            .await
            .context("this screen offers no way to write a note")?
            .click()
            .await?;
        Ok(())
    }

    /// What the bar says you are standing on, if anything.
    ///
    /// `aria-current` is the whole answer — the marked item is marked with the
    /// attribute a screen reader reads for the same fact, and the colour hangs
    /// off it. Asking for the attribute is asking the same question the reader's
    /// software asks.
    ///
    /// **One round trip, absent as `None`.** A screen with nothing marked is the
    /// ordinary case on the listing, not an error, and "not there yet" must
    /// never be thrown at the retry loop as a failure.
    ///
    /// # Errors
    ///
    /// Fails when the page cannot be queried at all.
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

    /// The words of the first row, or `None` when there are none yet.
    ///
    /// # Errors
    ///
    /// Fails when the page cannot be queried.
    pub async fn first_row(&self) -> Result<Option<String>> {
        Ok(self.rows().await?.into_iter().next())
    }

    /// Whether the page draws this text as a date that has gone by.
    ///
    /// The class and not the colour: what the palette resolves to is
    /// `web/theme.rs`'s answer and reading a computed colour here would be a
    /// second copy of it. What this screen has to get right is which of the two
    /// a date is, and that is the class it is given.
    ///
    /// # Errors
    ///
    /// Fails when the page cannot be queried.
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

    /// Presses the way back.
    ///
    /// # Errors
    ///
    /// Fails when the page has no back control.
    pub async fn tap_back(&self) -> Result<()> {
        self.click(By::Css(".back"), "the way back").await
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

    /// Types a query into the search field and stops there.
    ///
    /// The whole of what the enhancement layer is for: with the page's scripts
    /// on, the listing has already answered by the time this returns, and the
    /// server has not been asked anything. With them off nothing happens at
    /// all, which is the other half of the same contract.
    ///
    /// Real keystrokes rather than setting `value` and firing an event: what is
    /// under test includes that the field is reachable and that the listener is
    /// on the thing a person actually types into.
    ///
    /// **And real backspaces rather than `clear()`, which is the whole reason
    /// this is not two lines.** `clear()` empties the field without delivering
    /// an `input` event, so a listener hears nothing — which made "delete what
    /// you typed and watch the rows come back" pass against a script that never
    /// ran. Erasing the way a person erases is the only version that tests
    /// what the scenario says it tests.
    ///
    /// # Errors
    ///
    /// Fails when the page has no search field.
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

    /// What the address carries as the search, if anything.
    ///
    /// Separate from `path`, which deliberately drops the query string: most
    /// scenarios are about where they landed, and only these are about whether
    /// anything was sent at all.
    ///
    /// # Errors
    ///
    /// Fails when the browser cannot be asked where it is.
    pub async fn searched(&self) -> Result<Option<String>> {
        Ok(self
            .driver()
            .current_url()
            .await?
            .query_pairs()
            .find(|(name, _)| name == "q")
            .map(|(_, value)| value.to_string()))
    }

    /// What the listing says about whose answer is on the screen, or nothing
    /// when it is not saying anything.
    ///
    /// Absent and hidden are one answer, because they are one fact: the remark
    /// does not apply. Which of the two it is on any given page is the server's
    /// business and not a feature file's.
    ///
    /// # Errors
    ///
    /// Fails when the page cannot be queried.
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

    /// The id on the first row, or nothing where the width has no column to
    /// print it in.
    ///
    /// Written on every row and shown by the stylesheet, so what is asked here
    /// is whether it was *drawn* — `offsetParent` is null for anything a
    /// `display:none` is hiding, at any depth. A markup assertion would pass on
    /// the phone the id is deliberately absent from.
    ///
    /// # Errors
    ///
    /// Fails when the page cannot be queried.
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

    /// The grouping the search field is showing, flattened into one line:
    /// `(tag:work or tag:q3) and (budget)`.
    ///
    /// The brackets are this function's — on the screen a group is a pill, and
    /// a pill is not a thing a feature file can quote. What is being checked is
    /// that the boundaries fall where noda put them, so they are read off the
    /// elements and written the way the manual writes them.
    ///
    /// # Errors
    ///
    /// Fails when the page cannot be queried.
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

    /// Whether the page is asking the browser to reload it.
    ///
    /// The scriptless network screen steers by `<meta refresh>`; the script's
    /// first act is to take it off and poll instead. So this is the one
    /// observable difference between the two ways of waiting, and it is the
    /// difference the feature file names.
    ///
    /// # Errors
    ///
    /// Fails when the page cannot be queried.
    pub async fn reloads_itself(&self) -> Result<bool> {
        let meta = self
            .0
            .measure("return !!document.querySelector('meta[http-equiv=\"refresh\"]');")
            .await?;
        Ok(meta.as_bool().unwrap_or(false))
    }

    /// What the element matching `selector` reads as, or nothing when the page
    /// has no such element.
    ///
    /// **Absent is a value here, not an error.** A step that has just submitted
    /// a form is asking a page that may still be the previous one, and a `find`
    /// that fails with "no such element" turns "not yet" into a failure the
    /// retry loop cannot see past. One round trip, and `null` for missing.
    ///
    /// `innerText` and not `textContent`, because it is the rendered form —
    /// which is what makes a body holding `<b>bold</b>` read back with its angle
    /// brackets exactly when the page escaped them.
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

    /// The heading of a note.
    ///
    /// # Errors
    ///
    /// Fails when the page cannot be queried.
    pub async fn heading(&self) -> Result<String> {
        self.reads("h1").await
    }

    /// The name of the tab.
    ///
    /// The one thing a pane swap changes that is not in the pane. It arrives as
    /// a `<title>` at the head of the fragment, which the parser puts where a
    /// whole page would have had one — so this asserts the server's own string
    /// reached the tab, rather than the script having composed one.
    ///
    /// # Errors
    ///
    /// Fails when the page cannot be queried.
    pub async fn tab(&self) -> Result<String> {
        let text = self.0.measure("return document.title;").await?;
        Ok(text.as_str().unwrap_or_default().to_string())
    }

    /// The filename line under the heading.
    ///
    /// # Errors
    ///
    /// Fails when the page cannot be queried.
    pub async fn filename(&self) -> Result<String> {
        self.reads(".filename").await
    }

    /// A note's body, as text.
    ///
    /// # Errors
    ///
    /// Fails when the page cannot be queried.
    pub async fn body(&self) -> Result<String> {
        self.reads(".body").await
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
                    // A thing that cannot be pressed is not a control. The
                    // checkboxes a rendered note draws are the case: `noda todo`
                    // reads those boxes across the whole notebook, and ticking
                    // one here would have to be a commit — so they arrive
                    // disabled, exactly as the CLI has no `todo done`. They are
                    // 16 pixels of typography inside a sentence, and a rule
                    // about thumbs has nothing to say about them.
                    if (el.disabled) {{ continue; }}
                    // What a thumb actually presses. A checkbox inside a label
                    // is 22 pixels of ink inside whatever the label is, and the
                    // label is the target: pressing it toggles the box. Measuring
                    // the input would report a control nobody aims at.
                    const target = el.closest('label') || el;
                    const r = target.getBoundingClientRect();
                    // Nothing is measured that nobody can reach: a control laid
                    // out to nothing is not a small target, it is no target.
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

    /// Everything on the page, as a reader would read it.
    ///
    /// # Errors
    ///
    /// Fails when the page cannot be queried.
    pub async fn text(&self) -> Result<String> {
        let text = self.0.measure("return document.body.innerText;").await?;
        Ok(text.as_str().unwrap_or_default().to_string())
    }

    /// Every field a phone would zoom in on, with the size it is set at.
    ///
    /// Generalises the search field's rule to the forms: any field below sixteen
    /// pixels makes iOS Safari scale the page up on focus, and a reader who has
    /// just started typing then has to pinch their way back out.
    ///
    /// # Errors
    ///
    /// Fails when the measuring script does not run.
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

    /// Where something sits in the viewport: its left edge, its width, and how
    /// wide the viewport is.
    ///
    /// All three, because every question worth asking about a wide layout is
    /// about a relationship — is the column narrower than the window, is it
    /// centred in what is left over, does this piece start to the right of that
    /// one. A width on its own answers none of them.
    ///
    /// # Errors
    ///
    /// Fails when the script does not run or nothing matches.
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

    /// The same three numbers, but against a container rather than the window.
    ///
    /// A layout made of panes moves the question: the reading column is not
    /// centred in the *window*, it is centred in the pane it lives in, and the
    /// window has a rail and an index in it as well. Measuring against the
    /// window would ask about a relationship the design never claimed.
    ///
    /// # Errors
    ///
    /// Fails when the script does not run or either selector matches nothing.
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

    /// Whether the listing is on the screen with notes in it.
    ///
    /// Both halves, because either alone is satisfiable by an accident: a pane
    /// with no width is not on screen, and a pane on screen with nothing in it
    /// is the frame waiting for rows that never came.
    ///
    /// # Errors
    ///
    /// Fails when the script does not run.
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

    /// The title of the row the listing has marked as the one being read.
    ///
    /// # Errors
    ///
    /// Fails when the script does not run.
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

    /// What the margin note beside a note is saying, a line per link, or the
    /// one line it says when nothing points here. `None` when the column is not
    /// drawn at all, which is every width under 1440 and every width without a
    /// script.
    ///
    /// `offsetParent` rather than a look at the markup, for the reason the id
    /// column needs it too: the box is in the page from the start and closed,
    /// so what is being asked is whether it was ever opened. The line it shows
    /// while the notebook is being walked is deliberately not filtered out —
    /// a caller waiting for a title will keep waiting, and one asserting the
    /// column is absent must not pass because it happens to still be loading.
    ///
    /// # Errors
    ///
    /// Fails when the page cannot be queried.
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
