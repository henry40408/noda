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

/// A tablet in portrait, and the case that started the wide layout being looked
/// at again: it is a touch screen *and* room, which is why the rail arrives on
/// width rather than on whether the browser reports a pointer. Narrow enough
/// that the rail and a row of tags have to share 834px and not scroll.
const TABLET: (u32, u32) = (834, 1112);

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

#[given(expr = "I open the notebook on a tablet")]
async fn open_notebook_tablet(world: &mut NodaWorld) -> Result<()> {
    world.browser()?.resize(TABLET).await?;
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

#[when(expr = "I type {string} into the search field")]
async fn type_search(world: &mut NodaWorld, query: String) -> Result<()> {
    world.page()?.type_search(&query).await
}

/// A row the query excluded: on the page, and not on the screen.
///
/// The two halves are the point. `I do not see a row for` above already says
/// the second, and on its own it would pass just as well against a listing that
/// had left the note out — which is the design this replaced.
#[then(expr = "the page holds a hidden row for {string}")]
async fn hidden_row(world: &mut NodaWorld, what: String) -> Result<()> {
    let hidden = world.page()?.hidden_rows().await?;
    anyhow::ensure!(
        hidden.iter().any(|row| row.contains(&what)),
        "no hidden row for {what:?}; the page holds {hidden:?}"
    );
    Ok(())
}

#[then(expr = "the address carries the search {string}")]
async fn address_carries(world: &mut NodaWorld, query: String) -> Result<()> {
    eventually(&format!("the address to carry {query:?}"), || async {
        Ok(world.page()?.searched().await? == Some(query.clone()))
    })
    .await
}

#[then("the address carries no search")]
async fn address_carries_nothing(world: &mut NodaWorld) -> Result<()> {
    let sent = world.page()?.searched().await?;
    anyhow::ensure!(sent.is_none(), "the query was sent after all: {sent:?}");
    Ok(())
}

#[then("the listing says it filtered by title and tag")]
async fn says_partial(world: &mut NodaWorld) -> Result<()> {
    eventually("the remark under the field", || async {
        Ok(world
            .page()?
            .hint()
            .await?
            .is_some_and(|said| said.contains("title and tag")))
    })
    .await
}

#[then("the listing says nothing about whose answer it is")]
async fn says_nothing_partial(world: &mut NodaWorld) -> Result<()> {
    let hint = world.page()?.hint().await?;
    anyhow::ensure!(
        hint.is_none(),
        "the listing called a whole answer partial: {hint:?}"
    );
    Ok(())
}

#[then("the page is not reloading itself")]
async fn not_reloading(world: &mut NodaWorld) -> Result<()> {
    anyhow::ensure!(
        !world.page()?.reloads_itself().await?,
        "the script left the meta refresh in place, so the page is doing both"
    );
    Ok(())
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
///
/// Centred *in the room it has*, which stopped being the window when the bar
/// became a rail: the gutter the rail stands in is chrome and not margin, and
/// counting it as margin would call every wide page lopsided by half a rail.
#[then("the content is centred")]
async fn content_is_centred(world: &mut NodaWorld) -> Result<()> {
    let (left, width, window) = world.page()?.box_of("main").await?;
    let gutter = world.page()?.gutter().await?;
    let right = window - left - width;
    let beside = left - gutter;
    anyhow::ensure!(
        (beside - right).abs() <= 2.0,
        "there is {beside} to the left of the content and {right} to the right of it, \
         past a {gutter} gutter"
    );
    Ok(())
}

/// A rail: taller than it is wide, and out of the content's way.
///
/// Both halves matter. A strip that is taller than it is wide but drawn over the
/// column would be a rail that costs the reading room it was meant to save.
#[then("the bar stands beside the content")]
async fn bar_beside(world: &mut NodaWorld) -> Result<()> {
    let (left, _, width, height) = world.page()?.rect_of(".foot").await?;
    let (content, _, _) = world.page()?.box_of("main").await?;
    anyhow::ensure!(
        height > width,
        "the bar is {width} by {height} — that is a bar along an edge, not a rail"
    );
    anyhow::ensure!(
        left + width <= content + 1.0,
        "the bar ends at {} and the content starts at {content}",
        left + width
    );
    Ok(())
}

/// The same element on a phone, the other way round.
#[then("the bar sits along the bottom")]
async fn bar_along_bottom(world: &mut NodaWorld) -> Result<()> {
    let (_, top, width, height) = world.page()?.rect_of(".foot").await?;
    let (_, content_top, _, content_height) = world.page()?.rect_of("main").await?;
    anyhow::ensure!(
        width > height,
        "the bar is {width} by {height} — that is a rail, not a bar along an edge"
    );
    anyhow::ensure!(
        top >= content_top + content_height - 1.0,
        "the bar starts at {top}, above the end of the content at {}",
        content_top + content_height
    );
    Ok(())
}

/// The one control a thumb gets and a pointer does not need — and the word that
/// only the rail has room for.
#[then(expr = "the button to write reads {string}")]
async fn button_reads(world: &mut NodaWorld, label: String) -> Result<()> {
    let shown = world.page()?.text_of(".fab").await?;
    anyhow::ensure!(
        shown.trim() == label,
        "the button to write reads {shown:?} and not {label:?}"
    );
    Ok(())
}

#[when(expr = "I write {string} as the title")]
async fn write_title(world: &mut NodaWorld, text: String) -> Result<()> {
    world.page()?.fill("title", &text).await
}

/// `\n` in the feature means a new line in the box. Gherkin has no way to write
/// one inside a quoted string, and what is being checked in the scenario that
/// uses it is precisely that several lines survive the trip.
#[when(expr = "I write {string} as the body")]
async fn write_body(world: &mut NodaWorld, text: String) -> Result<()> {
    world.page()?.fill("body", &text.replace("\\n", "\n")).await
}

#[when(expr = "I submit {string}")]
async fn submit(world: &mut NodaWorld, what: String) -> Result<()> {
    world.page()?.submit(&what).await
}

#[when(expr = "I untick {string}")]
async fn untick(world: &mut NodaWorld, tag: String) -> Result<()> {
    world.page()?.untick(&tag).await
}

#[when("I press the button to write")]
async fn press_write(world: &mut NodaWorld) -> Result<()> {
    world.page()?.tap_write().await
}

#[then(expr = "the bar marks {string}")]
async fn bar_marks(world: &mut NodaWorld, place: String) -> Result<()> {
    eventually(&format!("the bar to mark {place:?}"), || async {
        Ok(world.page()?.marked_place().await?.as_deref() == Some(place.as_str()))
    })
    .await
}

#[then(expr = "the bar does not mark {string}")]
async fn bar_does_not_mark(world: &mut NodaWorld, place: String) -> Result<()> {
    eventually(&format!("the bar to leave {place:?} unmarked"), || async {
        Ok(world.page()?.marked_place().await?.as_deref() != Some(place.as_str()))
    })
    .await
}

/// The listing, where nothing is marked because it is what the bar leads from.
#[then("the bar marks nothing")]
async fn bar_marks_nothing(world: &mut NodaWorld) -> Result<()> {
    eventually("the bar to mark nothing", || async {
        Ok(world.page()?.marked_place().await?.is_none())
    })
    .await
}

#[then(expr = "the first row is {string}")]
async fn first_row_is(world: &mut NodaWorld, what: String) -> Result<()> {
    eventually(&format!("{what:?} to be the first row"), || async {
        Ok(world
            .page()?
            .first_row()
            .await?
            .is_some_and(|row| row.contains(&what)))
    })
    .await
}

#[then(expr = "{string} is marked overdue")]
async fn marked_overdue(world: &mut NodaWorld, text: String) -> Result<()> {
    eventually(&format!("{text:?} to be marked overdue"), || async {
        world.page()?.is_overdue(&text).await
    })
    .await
}

#[then(expr = "the page says {string}")]
async fn page_says(world: &mut NodaWorld, text: String) -> Result<()> {
    eventually(&format!("the page to say {text:?}"), || async {
        Ok(world.page()?.text().await?.contains(&text))
    })
    .await
}

#[then(expr = "the page does not say {string}")]
async fn page_does_not_say(world: &mut NodaWorld, text: String) -> Result<()> {
    eventually(&format!("the page to stop saying {text:?}"), || async {
        Ok(!world.page()?.text().await?.contains(&text))
    })
    .await
}

/// Below sixteen pixels, iOS Safari zooms the whole page the moment a field
/// takes focus. It is a rule about a browser nobody here is running, which is
/// exactly why it needs a test that reads the computed value.
#[then(expr = "no field is smaller than {int} pixels")]
async fn fields_are_big_enough(world: &mut NodaWorld, least: f64) -> Result<()> {
    let small = world.page()?.fields_under(least).await?;
    anyhow::ensure!(
        small.is_empty(),
        "a phone will zoom in on these: {}",
        small.join("; ")
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
