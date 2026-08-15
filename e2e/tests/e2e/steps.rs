//! What the Gherkin means.
//!
//! The steps talk about pressing things and reading things, never about
//! selectors — a feature that names a CSS class is a feature that has to be
//! rewritten when the stylesheet is. Where a selector is needed it lives in the
//! page object.

use anyhow::Result;
use cucumber::{given, then, when};
use noda_e2e::browser::{DESKTOP, PHONE};
use noda_e2e::server::NOTEBOOK;
use noda_e2e::wait::eventually;
use noda_e2e::world::NodaWorld;

/// A phone narrow enough to be the worst case anybody still carries.
const NARROW: (u32, u32) = (320, 568);

/// How bright a background has to be to be the light one, averaged over its
/// channels.
///
/// A threshold rather than the exact colours: what is being asked is whether the
/// media query was reached and the palette flipped, and writing the hex values
/// down here would be a second copy of `web/theme.rs` that has to be kept in
/// step with the first.
const LIGHT_ENOUGH: f64 = 200.0;
const DARK_ENOUGH: f64 = 60.0;

#[given("I open the front page")]
async fn open_front(world: &mut NodaWorld) -> Result<()> {
    world.page()?.go("/").await
}

#[given("I open the notebook")]
#[when("I open the notebook")]
async fn open_notebook(world: &mut NodaWorld) -> Result<()> {
    world.page()?.go(&format!("/nb/{NOTEBOOK}")).await
}

#[given(expr = "I open the notebook on a {int} pixel phone")]
async fn open_notebook_narrow(world: &mut NodaWorld, width: u32) -> Result<()> {
    let size = if width <= NARROW.0 { NARROW } else { PHONE };
    world.browser()?.resize(size).await?;
    world.page()?.go(&format!("/nb/{NOTEBOOK}")).await
}

#[given(expr = "I open the notebook on a desktop")]
async fn open_notebook_wide(world: &mut NodaWorld) -> Result<()> {
    world.browser()?.resize(DESKTOP).await?;
    world.page()?.go(&format!("/nb/{NOTEBOOK}")).await
}

#[given(expr = "I open {string}")]
async fn open_path(world: &mut NodaWorld, path: String) -> Result<()> {
    world.page()?.go(&path).await
}

#[given("my phone prefers a dark theme")]
async fn prefers_dark(world: &mut NodaWorld) -> Result<()> {
    world.browser()?.prefer_scheme("dark").await
}

#[given("my phone prefers a light theme")]
async fn prefers_light(world: &mut NodaWorld) -> Result<()> {
    world.browser()?.prefer_scheme("light").await
}

#[when(expr = "I press {string}")]
async fn press(world: &mut NodaWorld, what: String) -> Result<()> {
    world.page()?.press(&what).await
}

#[when("I press back")]
async fn press_back(world: &mut NodaWorld) -> Result<()> {
    world.page()?.tap_back().await
}

#[when(expr = "I search for {string}")]
async fn search(world: &mut NodaWorld, query: String) -> Result<()> {
    world.page()?.search(&query).await
}

#[then(expr = "I see a row for {string}")]
async fn see_row(world: &mut NodaWorld, what: String) -> Result<()> {
    eventually(&format!("a row for {what:?}"), || async {
        Ok(world
            .page()?
            .rows()
            .await?
            .iter()
            .any(|row| row.contains(&what)))
    })
    .await
}

#[then(expr = "I do not see a row for {string}")]
async fn no_row(world: &mut NodaWorld, what: String) -> Result<()> {
    eventually(&format!("no row for {what:?}"), || async {
        Ok(!world
            .page()?
            .rows()
            .await?
            .iter()
            .any(|row| row.contains(&what)))
    })
    .await
}

#[then(expr = "I am at {string}")]
async fn at(world: &mut NodaWorld, path: String) -> Result<()> {
    eventually(&format!("the address to become {path:?}"), || async {
        Ok(world.page()?.path().await? == path)
    })
    .await
}

#[then(expr = "I am not at {string}")]
async fn not_at(world: &mut NodaWorld, path: String) -> Result<()> {
    eventually(&format!("the address to leave {path:?}"), || async {
        Ok(world.page()?.path().await? != path)
    })
    .await
}

#[then(expr = "the note is headed {string}")]
async fn headed(world: &mut NodaWorld, title: String) -> Result<()> {
    eventually(&format!("a heading of {title:?}"), || async {
        Ok(world.page()?.heading().await? == title)
    })
    .await
}

#[then(expr = "the filename ends with {string}")]
async fn filename_ends(world: &mut NodaWorld, ending: String) -> Result<()> {
    eventually(&format!("a filename ending {ending:?}"), || async {
        Ok(world.page()?.filename().await?.ends_with(&ending))
    })
    .await
}

#[then(expr = "the body says {string}")]
async fn body_says(world: &mut NodaWorld, text: String) -> Result<()> {
    eventually(&format!("a body saying {text:?}"), || async {
        Ok(world.page()?.body().await?.contains(&text))
    })
    .await
}

#[then("the page complains")]
async fn complains(world: &mut NodaWorld) -> Result<()> {
    eventually("a complaint", || async {
        Ok(world.page()?.problem().await?.is_some())
    })
    .await
}

#[then("the page says nothing is wrong")]
async fn no_complaint(world: &mut NodaWorld) -> Result<()> {
    let problem = world.page()?.problem().await?;
    anyhow::ensure!(
        problem.is_none(),
        "the page complained without being asked anything: {problem:?}"
    );
    Ok(())
}

#[then(expr = "no control is smaller than {int} by {int}")]
async fn every_control_is_reachable(world: &mut NodaWorld, wide: u32, tall: u32) -> Result<()> {
    let short = world.page()?.controls_smaller_than(wide, tall).await?;
    anyhow::ensure!(
        short.is_empty(),
        "a thumb cannot reach these: {}",
        short.join("; ")
    );
    Ok(())
}

#[then(expr = "the search field's text is at least {int} pixels")]
async fn field_is_big_enough(world: &mut NodaWorld, least: f64) -> Result<()> {
    let size = world.page()?.search_field_font_size().await?;
    anyhow::ensure!(
        size >= least,
        "the search field is set at {size}px — below {least}px, iOS Safari zooms the page on focus"
    );
    Ok(())
}

/// The rule the CLI already follows, one medium over: given room, a row
/// *extends*. The tags and the day leave the second line and go to the right of
/// the title — same information, same order.
#[then("the row's tags sit beside the title")]
async fn tags_beside(world: &mut NodaWorld) -> Result<()> {
    let (title, _, _) = world.page()?.box_of(".row .title").await?;
    let (under, _, _) = world.page()?.box_of(".row .under").await?;
    anyhow::ensure!(
        under > title,
        "the tags start at {under} and the title at {title} — they are still stacked"
    );
    Ok(())
}

#[then("the row's tags sit under the title")]
async fn tags_under(world: &mut NodaWorld) -> Result<()> {
    let (title, _, _) = world.page()?.box_of(".row .title").await?;
    let (under, _, _) = world.page()?.box_of(".row .under").await?;
    anyhow::ensure!(
        (under - title).abs() < 1.0,
        "the tags start at {under} and the title at {title} — they are not stacked"
    );
    Ok(())
}

#[then("the content is narrower than the window")]
async fn content_is_narrow(world: &mut NodaWorld) -> Result<()> {
    let (_, width, window) = world.page()?.box_of("main").await?;
    anyhow::ensure!(
        width < window,
        "the content is {width} wide in a {window} window — a line that long is not read, it is scanned"
    );
    Ok(())
}

/// The margin left over has to fall on both sides.
///
/// A column that stops short of the right edge and hugs the left is not a
/// narrower page, it is a lopsided one.
#[then("the content is centred")]
async fn content_is_centred(world: &mut NodaWorld) -> Result<()> {
    let (left, width, window) = world.page()?.box_of("main").await?;
    let right = window - left - width;
    anyhow::ensure!(
        (left - right).abs() <= 2.0,
        "there is {left} to the left of the content and {right} to the right of it"
    );
    Ok(())
}

#[then("the page does not scroll sideways")]
async fn no_sideways(world: &mut NodaWorld) -> Result<()> {
    anyhow::ensure!(
        !world.page()?.scrolls_sideways().await?,
        "the page is wider than the phone it is on"
    );
    Ok(())
}

#[then("the page is dark")]
async fn is_dark(world: &mut NodaWorld) -> Result<()> {
    let (colour, brightness) = background(world).await?;
    anyhow::ensure!(
        brightness < DARK_ENOUGH,
        "the background is {colour} — that is not the dark palette"
    );
    Ok(())
}

#[then("the page is light")]
async fn is_light(world: &mut NodaWorld) -> Result<()> {
    let (colour, brightness) = background(world).await?;
    anyhow::ensure!(
        brightness > LIGHT_ENOUGH,
        "the background is {colour} — that is not the light palette"
    );
    Ok(())
}

/// The page's background, and how bright it is.
///
/// `getComputedStyle` answers in `rgb(r, g, b)` whatever the stylesheet was
/// written in, which is what makes this comparable at all.
async fn background(world: &mut NodaWorld) -> Result<(String, f64)> {
    let colour = world.page()?.background().await?;
    let channels: Vec<f64> = colour
        .trim_start_matches("rgba")
        .trim_start_matches("rgb")
        .trim_matches(|c: char| !c.is_ascii_digit() && c != ',' && c != '.')
        .split(',')
        .filter_map(|piece| piece.trim().parse::<f64>().ok())
        .take(3)
        .collect();
    anyhow::ensure!(
        channels.len() == 3,
        "could not read a colour out of {colour:?}"
    );
    let brightness = channels.iter().sum::<f64>() / 3.0;
    Ok((colour, brightness))
}
