Feature: The same pages on a wider screen

  One interface at several densities, each extending the one below.

  Under 640px it is a phone: one screen at a time, a bar along the bottom.
  Above that the bar becomes a rail down the left. Above 1024px the notes
  screen splits into listing and note. A note's margin, what points at it and
  what it says about itself, sits under the note and moves to its side above
  1440px.

  A row is the same two lines at every width: the title with the id `ls -l`
  prints, then the day and the tags. The search field shows how it grouped the
  query, also at every width.

  Scenario Outline: A row keeps its tags under the title
    Given I open the notebook on a <screen>
    Then the row's tags sit under the title

    Examples:
      | screen  |
      | phone   |
      | tablet  |
      | desktop |

  # The id: what `noda show` takes and the first half of the filename. A phone
  # has room for it on the title's line.
  Scenario Outline: A row names the note the way the notebook does
    Given I open the notebook on a <screen>
    Then the row shows the note's id

    Examples:
      | screen  |
      | phone   |
      | tablet  |
      | desktop |

  # Wrapped, the fourth order sits alone and reads as something different. It
  # broke on a tablet held sideways, where the index column was 44px short.
  Scenario Outline: The order stays on one line
    Given I open the notebook on a <screen>
    Then the order chips are on one line

    Examples:
      | screen  |
      | phone   |
      | tablet  |
      | desktop |

  # `OR` binds tighter than the space, backwards from most search boxes, so
  # the field shows the grouping. It comes from the server: a fact about the
  # query, not a shortcut.
  Scenario Outline: The field says how it grouped what was searched for
    Given I open the notebook on a <screen>
    When I search for "tag:work OR tag:ops budget"
    Then the field groups it as "(tag:work or tag:ops) and (budget)"

    Examples:
      | screen  |
      | phone   |
      | tablet  |
      | desktop |

  Scenario: A note reads at a comfortable measure on a tablet
    Given I open the notebook on a tablet
    When I press "Budget review"
    Then the note is headed "Budget review"
    And the reading column is narrower than its pane
    And the reading column is centred in its pane

  # Centred in its pane, not the window. The heading step comes first because
  # the pane swap is a round trip: until it lands, the pane holds something else.
  Scenario: A note reads at a comfortable measure beside the listing
    Given I open the notebook on a desktop
    When I press "Budget review"
    Then the note is headed "Budget review"
    And the reading column is narrower than its pane
    And the reading column is centred in its pane

  Scenario: A wide page does not scroll sideways
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
  Scenario: The listing stays on screen while a note is read
    Given I open the notebook on a desktop
    When I press "Budget review"
    Then the note is headed "Budget review"
    And the listing is still on screen

  # The name arrives as a `<title>` at the head of the fragment, so the tab gets
  # the server's string, not one the script composed.
  @scripted
  Scenario: The tab takes the name of the note being read
    Given I open the notebook on a desktop
    When I press "Budget review"
    Then the note is headed "Budget review"
    And the tab is named "Budget review — noda"

  # The rows are right either way; only the remembered page tells a swap from a
  # navigation. `q3` is only in a body, so the row must come from the server.
  @scripted
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

  # The margin goes with the pane it was in, and the page that comes back has
  # to bring its own.
  @scripted
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
  Scenario: The listing marks the note being read
    Given I open the notebook on a desktop
    When I press "Budget review"
    Then the listing marks "Budget review"
    When I press "Reading list"
    Then the listing marks "Reading list"

  # A row's address carries the notebook's name as it was typed and the browser
  # reports the same address percent-encoded, so the two are compared as addresses.
  @scripted
  Scenario: The listing marks the note being read in a notebook named in Unicode
    Given the notebook is also kept under the name "日記 2026"
    And I open "/nb/日記 2026" on a desktop
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

  # What points at a note comes with the note, so none of the margin scenarios
  # below needs a script, and none of them is about a width.
  Scenario: What points at a note sits beside it on a monitor
    Given I open the notebook on a monitor
    When I press "Meeting notes"
    Then the margin note lists "Reading list"

  Scenario: What points at a note sits under it on a laptop
    Given I open the notebook on a desktop
    When I press "Meeting notes"
    Then the note is headed "Meeting notes"
    And the margin note lists "Reading list"

  Scenario: What points at a note sits under it on a phone
    Given I open the notebook
    When I press "Meeting notes"
    Then the note is headed "Meeting notes"
    And the margin note lists "Reading list"

  # A tag lights up under the pointer, and on a phone stays lit after a tap. The
  # highlight is the pill's own fill, so it has to sit behind the word: a
  # hit test cannot tell, since the pill answers as its link, so the step reads
  # the pixels.
  Scenario Outline: A tag can still be read while the pointer is on it
    Given I open the notebook on a <screen>
    When I press "Budget review"
    And I rest the pointer on the tag "work"
    Then the tag "work" can still be read

    Examples:
      | screen  |
      | phone   |
      | tablet  |
      | desktop |
      | monitor |

  # An empty answer is said; a column gone quiet reads as broken.
  Scenario: A note nothing points at says so
    Given I open the notebook on a monitor
    When I press "Budget review"
    Then the margin note says "No other note links here."

  # The margin then follows the note now being read.
  Scenario: A link in the margin leads to the note it names
    Given I open the notebook on a monitor
    When I press "Meeting notes"
    Then the margin note lists "Reading list"
    When I press "Reading list" in the margin note
    Then the note is headed "Reading list"
    And the margin note says "No other note links here."

  # The front page has no rail, but the rail's grid column once left 76px of
  # nothing down the left, invisible in the markup.
  Scenario: The front page is laid out with no rail
    Given I open the front page on a tablet
    Then the notebooks fill the window
    And the page does not scroll sideways
