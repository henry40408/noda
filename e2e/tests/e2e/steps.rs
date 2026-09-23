//! Step definitions. Steps speak of pressing and reading, never of selectors,
//! which live in the page object so a stylesheet change does not reach the
//! features.

use anyhow::Result;
use cucumber::{given, then, when};
use noda_e2e::browser::{DESKTOP, MONITOR, PHONE, TABLET};
use noda_e2e::server::NOTEBOOK;
use noda_e2e::wait::eventually;
use noda_e2e::world::NodaWorld;

/// The narrowest phone still in use.
const NARROW: (u32, u32) = (320, 568);

/// Mean-channel brightness thresholds, rather than exact colours that would copy
/// `web/theme.rs`.
const LIGHT_ENOUGH: f64 = 200.0;
const DARK_ENOUGH: f64 = 60.0;

#[given("I open the front page")]
async fn open_front(world: &mut NodaWorld) -> Result<()> {
    world.page()?.go("/").await
}

#[given(expr = "I open the front page on a tablet")]
async fn open_front_tablet(world: &mut NodaWorld) -> Result<()> {
    world.browser()?.resize(TABLET).await?;
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

#[given(expr = "I open the notebook on a tablet")]
async fn open_notebook_tablet(world: &mut NodaWorld) -> Result<()> {
    world.browser()?.resize(TABLET).await?;
    world.page()?.go(&format!("/nb/{NOTEBOOK}")).await
}

#[given(expr = "I open the notebook on a desktop")]
async fn open_notebook_wide(world: &mut NodaWorld) -> Result<()> {
    world.browser()?.resize(DESKTOP).await?;
    world.page()?.go(&format!("/nb/{NOTEBOOK}")).await
}

#[given(expr = "I open the notebook on a monitor")]
async fn open_notebook_monitor(world: &mut NodaWorld) -> Result<()> {
    world.browser()?.resize(MONITOR).await?;
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

#[when(expr = "I press {string} in the margin note")]
async fn press_in_margin(world: &mut NodaWorld, what: String) -> Result<()> {
    world.page()?.press_in_margin(&what).await
}

#[when("I press back")]
async fn press_back(world: &mut NodaWorld) -> Result<()> {
    world.page()?.tap_back().await
}

#[when("I go back")]
async fn go_back(world: &mut NodaWorld) -> Result<()> {
    world.page()?.go_back().await
}

#[given("I remember this page")]
#[when("I remember this page")]
async fn remember(world: &mut NodaWorld) -> Result<()> {
    world.page()?.remember().await
}

#[then("it is still the same page")]
async fn still_here(world: &mut NodaWorld) -> Result<()> {
    eventually("the page to still be the one that was marked", || async {
        world.page()?.remembered().await
    })
    .await
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

/// On the page but not on the screen: `I do not see a row for` alone would also
/// pass if the listing had left the note out.
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

#[then(expr = "the tab is named {string}")]
async fn tab_named(world: &mut NodaWorld, name: String) -> Result<()> {
    eventually(&format!("a tab named {name:?}"), || async {
        Ok(world.page()?.tab().await? == name)
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

/// As in the CLI, a row given room extends: tags and day move right of the title.
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

/// Wrapped, the fourth order sits alone and reads as something different.
#[then("the order chips are on one line")]
async fn order_on_one_line(world: &mut NodaWorld) -> Result<()> {
    let lines = world.page()?.lines_of(".sortbar").await?;
    anyhow::ensure!(lines == 1, "the order is on {lines} lines at this width");
    Ok(())
}

#[then("the row shows the note's id")]
async fn row_shows_an_id(world: &mut NodaWorld) -> Result<()> {
    let shown = world.page()?.shown_id().await?;
    let Some(id) = shown else {
        anyhow::bail!("no id is drawn on the row at this width");
    };
    // `note::ID_LEN` characters of Crockford base32; ids are minted, so no
    // particular one.
    anyhow::ensure!(
        id.len() == 8 && id.chars().all(|c| c.is_ascii_alphanumeric()),
        "the id column says {id:?}, which is not an id"
    );
    Ok(())
}

/// A phone's one column belongs to the title.
#[then("the row shows no id")]
async fn row_shows_no_id(world: &mut NodaWorld) -> Result<()> {
    let shown = world.page()?.shown_id().await?;
    anyhow::ensure!(
        shown.is_none(),
        "the id column is drawn on a screen with no room for it: {shown:?}"
    );
    Ok(())
}

/// `a OR b c` is `(a OR b) AND c`, which people read backwards, so the field
/// shows its grouping.
#[then(expr = "the field groups it as {string}")]
async fn field_groups_it(world: &mut NodaWorld, expected: String) -> Result<()> {
    eventually(
        &format!("the field to group it as {expected:?}"),
        || async { Ok(world.page()?.grouping().await?.as_deref() == Some(expected.as_str())) },
    )
    .await
}

#[then("the field groups nothing")]
async fn field_groups_nothing(world: &mut NodaWorld) -> Result<()> {
    eventually("the field to group nothing", || async {
        Ok(world.page()?.grouping().await?.is_none())
    })
    .await
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

/// The front page has no rail, but the rail's grid column once left 76px of
/// nothing down the left — invisible in the markup.
#[then("the notebooks fill the window")]
async fn notebooks_fill_the_window(world: &mut NodaWorld) -> Result<()> {
    let (left, width, window) = world.page()?.box_of("main.books").await?;
    anyhow::ensure!(
        left <= 1.0 && (window - width).abs() <= 1.0,
        "the notebooks start {left} in and are {width} wide in a {window} window"
    );
    Ok(())
}

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

/// Against the pane, not the window, which also holds a rail and an index.
#[then("the reading column is narrower than its pane")]
async fn reading_is_narrow(world: &mut NodaWorld) -> Result<()> {
    let (_, width, pane) = world
        .page()?
        .box_in(".pane.read main.note", ".pane.read")
        .await?;
    anyhow::ensure!(
        width < pane,
        "the column is {width} wide in a {pane} pane — a line that long is not read, it is scanned"
    );
    Ok(())
}

#[then("the reading column is centred in its pane")]
async fn reading_is_centred(world: &mut NodaWorld) -> Result<()> {
    let (left, width, pane) = world
        .page()?
        .box_in(".pane.read main.note", ".pane.read")
        .await?;
    let right = pane - left - width;
    anyhow::ensure!(
        (left - right).abs() <= 2.0,
        "there is {left} to the left of the column and {right} to the right of it"
    );
    Ok(())
}

/// A form page's strip spans the pane; placed inside the form, it got the
/// form's padding twice and stood 16px right of its buttons. Measured on the
/// first bold run, since the paragraph itself is full-bleed.
#[then("the words line up with the buttons under them")]
async fn words_line_up(world: &mut NodaWorld) -> Result<()> {
    let (words, _, _) = world.page()?.box_in(".said b", "main").await?;
    let (buttons, _, _) = world.page()?.box_in(".buttons button", "main").await?;
    anyhow::ensure!(
        (words - buttons).abs() <= 1.0,
        "the words start {words} into the pane and the buttons {buttons}"
    );
    Ok(())
}

/// Both stamps are present in either pass; how they read is the next step's.
#[then("the note says when it was made and when it changed")]
async fn stamps_are_labelled(world: &mut NodaWorld) -> Result<()> {
    let said = world.page()?.stamps().await?;
    anyhow::ensure!(
        said.contains("created"),
        "the note does not say when it was made: {said}"
    );
    anyhow::ensure!(
        said.contains("updated"),
        "the note does not say when it changed: {said}"
    );
    Ok(())
}

/// The server cannot know the reader's zone, so it sends the file's
/// `2026-08-15T09:54:23Z` and the script restates it locally. Asserted by shape,
/// since the answer depends on the machine's zone: a converted stamp has a comma
/// and no `Z`.
#[then("the stamps are said in the reader's own words")]
async fn stamps_are_local(world: &mut NodaWorld) -> Result<()> {
    let said = world.page()?.stamps().await?;
    anyhow::ensure!(
        !said.contains('Z'),
        "the stamps are still the file's own: {said}"
    );
    anyhow::ensure!(
        said.contains(", "),
        "the stamps were not said again in words: {said}"
    );
    Ok(())
}

#[then("the listing is still on screen")]
async fn listing_still_there(world: &mut NodaWorld) -> Result<()> {
    eventually("the listing to stay on screen", || async {
        world.page()?.listing_on_screen().await
    })
    .await
}

#[then(expr = "the listing marks {string}")]
async fn listing_marks(world: &mut NodaWorld, what: String) -> Result<()> {
    eventually(&format!("the listing to mark {what:?}"), || async {
        Ok(world.page()?.marked_row().await?.as_deref() == Some(what.as_str()))
    })
    .await
}

#[then(expr = "the margin note lists {string}")]
async fn margin_lists(world: &mut NodaWorld, what: String) -> Result<()> {
    eventually(&format!("the margin note to list {what:?}"), || async {
        Ok(world
            .page()?
            .margin_note()
            .await?
            .is_some_and(|lines| lines.iter().any(|line| line == &what)))
    })
    .await
}

/// "Nothing points here" is said, not shown as a closed column.
#[then(expr = "the margin note says {string}")]
async fn margin_says(world: &mut NodaWorld, what: String) -> Result<()> {
    eventually(&format!("the margin note to say {what:?}"), || async {
        Ok(world.page()?.margin_note().await? == Some(vec![what.clone()]))
    })
    .await
}

/// Not drawn at all, not merely empty.
#[then("the margin note is not on screen")]
async fn margin_not_there(world: &mut NodaWorld) -> Result<()> {
    anyhow::ensure!(
        world.page()?.margin_note().await?.is_none(),
        "the margin note is on screen beside the note"
    );
    Ok(())
}

#[then("the listing is not on screen")]
async fn listing_not_there(world: &mut NodaWorld) -> Result<()> {
    anyhow::ensure!(
        !world.page()?.listing_on_screen().await?,
        "the listing is on screen beside the note"
    );
    Ok(())
}

#[when(expr = "I write {string} as the title")]
async fn write_title(world: &mut NodaWorld, text: String) -> Result<()> {
    world.page()?.fill("title", &text).await
}

/// `\n` in the feature is a newline: Gherkin cannot write one inside a string.
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

/// The network screen is reached by the corner chip, not the bar.
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

/// Below 16px, iOS Safari zooms the page when a field takes focus.
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

/// The background and its mean-channel brightness; `getComputedStyle` always
/// answers in `rgb(…)`.
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
