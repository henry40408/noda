Feature: The same pages on a wider screen

  One interface at three densities, not three designs. What changes with the
  width is how much of it can be on screen at once, and each step is an
  extension of the one below it rather than a rearrangement.

  Under 640px it is a phone: one screen at a time, a bar along the bottom.
  Above that the bar stands up into a rail down the left and the row extends —
  the tags and the day leave the second line and go to the right of the title,
  which is the rule `noda ls -l` already follows. Above 1024px the notes screen
  splits: the listing on the left, the note being read on the right.

  Scenario: A row extends rather than stacking
    Given I open the notebook on a tablet
    Then the row's tags sit beside the title

  Scenario: A row stacks again on a phone
    Given I open the notebook
    Then the row's tags sit under the title

  # And a third time in the index column, which is neither. The column is
  # narrow on purpose — it is a list you scan while reading something else —
  # so the row goes back to being two lines. Same rule as the phone, same
  # reason: there is no room for a second column of anything.
  Scenario: A row stacks again in the index column
    Given I open the notebook on a desktop
    Then the row's tags sit under the title

  Scenario: The content does not run the whole width of a tablet
    Given I open the notebook on a tablet
    Then the content is narrower than the window

  Scenario: A note reads at a comfortable measure on a tablet
    Given I open the notebook on a tablet
    When I press "Budget review"
    Then the content is narrower than the window

  # On a screen holding two panes the question moves: the prose is not centred
  # in the window, because the window has a rail and a listing in it as well.
  # It is centred in the pane it lives in.
  Scenario: A note reads at a comfortable measure beside the listing
    Given I open the notebook on a desktop
    When I press "Budget review"
    Then the reading column is narrower than its pane
    And the reading column is centred in its pane

  Scenario: A wide page does not scroll sideways either
    Given I open the notebook on a desktop
    Then the page does not scroll sideways

  Scenario: A tablet does not scroll sideways
    Given I open the notebook on a tablet
    Then the page does not scroll sideways

  # The result, asserted without reference to how it was reached, so that the
  # scriptless pass makes it too. With no script a note on a desktop is the
  # tablet's single pane — the note, whole — which is what a note page has
  # always been.
  Scenario: A note opens whole on a desktop
    Given I open the notebook on a desktop
    When I press "Budget review"
    Then the note is headed "Budget review"

  # The point of the width. The listing is sent to the note page by the script,
  # because below this width it would be downloaded and never drawn — so this
  # is the shortcut, and it is tagged as one. The scenario above is the
  # untagged account of the result.
  @scripted
  Scenario: The listing stays on screen while a note is read
    Given I open the notebook on a desktop
    When I press "Budget review"
    Then the note is headed "Budget review"
    And the listing is still on screen

  # Which row you are on is a question only two panes can ask, so it is only
  # here that there is an answer to mark.
  @scripted
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
