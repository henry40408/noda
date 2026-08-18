Feature: The same pages on a wider screen

  One interface at three densities, not three designs. What changes with the
  width is how much of it can be on screen at once, and each step is an
  extension of the one below it rather than a rearrangement.

  Under 640px it is a phone: one screen at a time, a bar along the bottom.
  Above that the bar stands up into a rail down the left and the row extends —
  the tags and the day leave the second line and go to the right of the title,
  which is the rule `noda ls -l` already follows. Above 1024px the notes screen
  splits: the listing on the left, the note being read on the right.

  Two things arrive with the width rather than being rearranged by it, and both
  are the same page saying more of what it already knew. A row grows the id
  column `ls -l` prints, so the listing is the CLI's own row; and the search
  field says how it grouped what was typed, because `a OR b c` is
  `(a OR b) AND c` and that is the one thing about this grammar people read
  backwards. Neither is on a phone, for one reason said twice: there is one
  column there and the note's title has it.

  Scenario: A row extends rather than stacking
    Given I open the notebook on a tablet
    Then the row's tags sit beside the title

  # And what it extends into is the CLI's own column. `noda ls -l` prints id,
  # title, updated, tags; given the room, so does this. The id is the name the
  # notebook knows a note by — what `noda show` takes, and the first half of
  # the filename in the repository.
  Scenario: A row names the note the way the notebook does
    Given I open the notebook on a tablet
    Then the row shows the note's id

  # The same decision, seen from the side it was decided on. A phone has one
  # column and it belongs to the title: the id is an argument about space, and
  # here there is none to spare.
  Scenario: A phone spends its one column on the title
    Given I open the notebook
    Then the row shows no id

  # The index column is narrow enough to stack and wide enough to name. It has
  # no right-hand side to print a day at, so the day rides beside the id
  # instead — the two things that are not the title, at the two ends of the
  # line above it.
  Scenario: The index column keeps the id
    Given I open the notebook on a desktop
    Then the row shows the note's id

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

  # `a OR b c` is `(a OR b) AND c` — `OR` binds tighter than the space between
  # terms, which is backwards from every search box that has an `OR` at all.
  # It is the one thing about this grammar people read wrong, and the two
  # readings both look like answers, so the field says which one it took. From
  # the server, because it is a fact about the query and not a shortcut.
  Scenario: The field says how it grouped what was searched for
    Given I open the notebook on a tablet
    When I search for "tag:work OR tag:ops budget"
    Then the field groups it as "(tag:work or tag:ops) and (budget)"

  # And the same reasoning as the id, one element over: a phone's field is one
  # line with one remark under it already, and a second remark about the same
  # line is the taller half of a screen that is short of room.
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
