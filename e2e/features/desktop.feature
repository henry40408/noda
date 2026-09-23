Feature: The same pages on a wider screen

  One interface at three densities, each extending the one below.

  Under 640px it is a phone: one screen at a time, a bar along the bottom.
  Above that the bar becomes a rail down the left and the row extends: tags and
  day move right of the title, as `noda ls -l` does. Above 1024px the notes
  screen splits into listing and note. Above 1440px the note's backlinks, what
  the Links button opens, sit in its margin.

  With the width a row also gains the id column `ls -l` prints, and the search
  field shows how it grouped the query. Neither appears on a phone, whose one
  column belongs to the title.

  Scenario: A row extends rather than stacking
    Given I open the notebook on a tablet
    Then the row's tags sit beside the title

  # The id: what `noda show` takes and the first half of the filename.
  Scenario: A row names the note the way the notebook does
    Given I open the notebook on a tablet
    Then the row shows the note's id

  Scenario: A phone spends its one column on the title
    Given I open the notebook
    Then the row shows no id

  # The index column has no right-hand side, so the day sits beside the id.
  Scenario: The index column keeps the id
    Given I open the notebook on a desktop
    Then the row shows the note's id

  Scenario: A row stacks again on a phone
    Given I open the notebook
    Then the row's tags sit under the title

  # The index column is narrow on purpose, so the row is two lines again.
  Scenario: A row stacks again in the index column
    Given I open the notebook on a desktop
    Then the row's tags sit under the title

  # Wrapped, the fourth order sits alone and reads as something different. It
  # broke on a tablet held sideways, where the index column was 44px short.
  Scenario: The order stays on one line on a phone
    Given I open the notebook
    Then the order chips are on one line

  Scenario: The order stays on one line on a tablet
    Given I open the notebook on a tablet
    Then the order chips are on one line

  Scenario: The order stays on one line in the index column
    Given I open the notebook on a desktop
    Then the order chips are on one line

  # `OR` binds tighter than the space, backwards from most search boxes, so
  # the field shows the grouping. It comes from the server: a fact about the
  # query, not a shortcut.
  Scenario: The field says how it grouped what was searched for
    Given I open the notebook on a tablet
    When I search for "tag:work OR tag:ops budget"
    Then the field groups it as "(tag:work or tag:ops) and (budget)"

  # A phone's field already has one remark under it and no room for another.
  Scenario: A phone is not told how it was grouped
    Given I open the notebook
    When I search for "tag:work OR tag:ops budget"
    Then the field groups nothing

  Scenario: The content does not run the whole width of a tablet
    Given I open the notebook on a tablet
    Then the content is narrower than the window

  Scenario: A note reads at a comfortable measure on a tablet
    Given I open the notebook on a tablet
    When I press "Budget review"
    Then the content is narrower than the window

  # Centred in its pane, not the window. The heading step comes first because
  # the pane swap is a round trip: until it lands, the pane holds something else.
  Scenario: A note reads at a comfortable measure beside the listing
    Given I open the notebook on a desktop
    When I press "Budget review"
    Then the note is headed "Budget review"
    And the reading column is narrower than its pane
    And the reading column is centred in its pane

  Scenario: A wide page does not scroll sideways either
    Given I open the notebook on a desktop
    Then the page does not scroll sideways

  Scenario: A tablet does not scroll sideways
    Given I open the notebook on a tablet
    Then the page does not scroll sideways

  # The untagged account of the result: without a script, a desktop note is the
  # tablet's single pane.
  Scenario: A note opens whole on a desktop
    Given I open the notebook on a desktop
    When I press "Budget review"
    Then the note is headed "Budget review"

  # The script sends the listing with the note (below this width it would never
  # be drawn), so this is the shortcut; the scenario above is the untagged one.
  @scripted
  # The script sends the listing with the note (below this width it would never
  # be drawn), so this is the shortcut; the scenario above is the untagged one.
  Scenario: The listing stays on screen while a note is read
    Given I open the notebook on a desktop
    When I press "Budget review"
    Then the note is headed "Budget review"
    And the listing is still on screen

  # The name arrives as a `<title>` at the head of the fragment, so the tab gets
  # the server's string, not one the script composed.
  @scripted
  # The name arrives as a `<title>` at the head of the fragment, so the tab gets
  # the server's string, not one the script composed.
  Scenario: The tab takes the name of the note being read
    Given I open the notebook on a desktop
    When I press "Budget review"
    Then the note is headed "Budget review"
    And the tab is named "Budget review — noda"

  # The rows are right either way; only the remembered page tells a swap from a
  # navigation. `q3` is only in a body, so the row must come from the server.
  @scripted
  # The rows are right either way; only the remembered page tells a swap from a
  # navigation. `q3` is only in a body, so the row must come from the server.
  Scenario: Sending a search replaces the rows and not the page
    Given I open the notebook on a desktop
    And I remember this page
    When I search for "q3"
    Then I see a row for "Budget review"
    And I do not see a row for "Reading list"
    And the address carries the search "q3"
    And it is still the same page

  # The rows come back from the server: the address's query is what defines them.
  @scripted
  # The rows come back from the server: the address's query is what defines them.
  Scenario: Going back undoes a search without reloading
    Given I open the notebook on a desktop
    When I search for "q3"
    Then I do not see a row for "Reading list"
    When I remember this page
    And I go back
    Then I see a row for "Reading list"
    And the address carries no search
    And it is still the same page

  # Both panes are restored: the rows, and the reading pane, which with no note
  # picked holds the notebook's README.
  @scripted
  # Both panes are restored: the rows, and the reading pane, which with no note
  # picked holds the notebook's README.
  Scenario: Going back from a note returns to the listing beside it
    Given I open the notebook on a desktop
    And I remember this page
    When I press "Budget review"
    Then the note is headed "Budget review"
    When I go back
    Then I am at "/nb/default"
    And the tab is named "default — noda"
    And it is still the same page

  @scripted
  Scenario: Going back from a note returns to the note before it
    Given I open the notebook on a desktop
    When I press "Budget review"
    And I press "Reading list"
    Then the note is headed "Reading list"
    When I remember this page
    And I go back
    Then the note is headed "Budget review"
    And the tab is named "Budget review — noda"
    And it is still the same page

  # The margin's answer went away with the pane, so it must be asked for again.
  @scripted
  # The margin's answer went away with the pane, so it must be asked for again.
  Scenario: What points at a note is still there after going back
    Given I open the notebook on a monitor
    When I press "Meeting notes"
    Then the margin note lists "Reading list"
    When I press "Reading list" in the margin note
    Then the note is headed "Reading list"
    When I go back
    Then the note is headed "Meeting notes"
    And the margin note lists "Reading list"

  # Only two panes can say which row is being read.
  @scripted
  # Only two panes can say which row is being read.
  Scenario: The listing marks the note being read
    Given I open the notebook on a desktop
    When I press "Budget review"
    Then the listing marks "Budget review"
    When I press "Reading list"
    Then the listing marks "Reading list"

  Scenario: A phone shows one thing at a time
    Given I open the notebook
    When I press "Budget review"
    Then the note is headed "Budget review"
    And the listing is not on screen

  # The untagged account of the widest screen, which is all a monitor gets
  # without a script.
  Scenario: A note opens whole on a monitor
    Given I open the notebook on a monitor
    When I press "Meeting notes"
    Then the note is headed "Meeting notes"
    And the page does not scroll sideways

  @scripted
  Scenario: What points at a note sits beside it on a monitor
    Given I open the notebook on a monitor
    When I press "Meeting notes"
    Then the margin note lists "Reading list"

  # An empty answer is said; a column gone quiet reads as broken.
  @scripted
  # An empty answer is said; a column gone quiet reads as broken.
  Scenario: A note nothing points at says so
    Given I open the notebook on a monitor
    When I press "Budget review"
    Then the margin note says "Nothing points here."

  # Backlinks walk every note, so below 1440px they stay behind the Links press.
  @scripted
  # Backlinks walk every note, so below 1440px they stay behind the Links press.
  Scenario: A laptop leaves the backlinks behind the press
    Given I open the notebook on a desktop
    When I press "Meeting notes"
    Then the note is headed "Meeting notes"
    And the margin note is not on screen

  # The margin then follows the note now being read.
  @scripted
  # The margin then follows the note now being read.
  Scenario: A link in the margin leads to the note it names
    Given I open the notebook on a monitor
    When I press "Meeting notes"
    Then the margin note lists "Reading list"
    When I press "Reading list" in the margin note
    Then the note is headed "Reading list"
    And the margin note says "Nothing points here."

  # The rail's grid column once left 76px of nothing down the left of the front
  # page, with no sign of it in the markup.
  Scenario: The front page is laid out with no rail
    Given I open the front page on a tablet
    Then the notebooks fill the window
    And the page does not scroll sideways
