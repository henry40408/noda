Feature: The same pages on a wide screen

  Mobile is what this is for, so the wide layout is not a second design. The
  content obeys the rule the CLI already follows — a row *extends* when there is
  room, and never rearranges — and the chrome turns through ninety degrees.

  On a phone a row is two lines: the title, then the tags and the day beneath
  it. Given the width, those slide out to the right of the title instead. Same
  information, same order, one line.

  The bar is the same story told about chrome. A strip stuck to the bottom edge
  and a round button floating over the last row are the two things here that
  exist because of a thumb; on a monitor that bar is four icons 360px apart,
  which is not a bar. So the same three links stand in a column down the left,
  and the button takes the label it was always carrying.

  Scenario: A row extends rather than stacking
    Given I open the notebook on a desktop
    Then the row's tags sit beside the title

  Scenario: A row stacks again on a phone
    Given I open the notebook
    Then the row's tags sit under the title

  Scenario: The bar is a rail down the side
    Given I open the notebook on a desktop
    Then the bar stands beside the content

  # The tablet is the case this was built for, and the one the width rule is
  # answerable to: 834px in portrait is a touch screen *with* room on it, and it
  # was being given the layout meant for a screen with none. So it gets the rail
  # — and because a finger is still what presses it, the rail is made of targets
  # and not of decoration, in 834px that must not scroll sideways to hold both.
  Scenario: A tablet gets the rail, and it is still made of targets
    Given I open the notebook on a tablet
    Then the bar stands beside the content
    And the row's tags sit beside the title
    And no control is smaller than 48 by 48
    And the page does not scroll sideways

  Scenario: The same bar is along the bottom on a phone
    Given I open the notebook
    Then the bar sits along the bottom

  Scenario: The button to write says what it does when there is room to
    Given I open the notebook on a desktop
    Then the button to write reads "New note"

  # The rail is what makes the notebook's three places reachable from a note,
  # which on a phone they are not: there is one bottom edge and the note's own
  # four actions have it.
  Scenario: A note reaches the notebook's places too
    Given I open the notebook on a desktop
    When I press "Budget review"
    And I press "Tags"
    Then I am at "/nb/default/tags"

  # And the note's own tag screen is still one press away — under a name of its
  # own, because two links spelled "Tags" going to different places is not a
  # thing a wide screen can hold.
  Scenario: A note's own tags are a separate press
    Given I open the notebook on a desktop
    When I press "Budget review"
    And I press "Retag"
    Then the page says "On this note"

  Scenario: The reading column does not run the whole width of a monitor
    Given I open the notebook on a desktop
    Then the content is narrower than the window

  Scenario: The reading column is centred in the window
    Given I open the notebook on a desktop
    Then the content is centred

  Scenario: A note reads at a comfortable measure
    Given I open the notebook on a desktop
    When I press "Budget review"
    Then the content is narrower than the window
    And the content is centred

  Scenario: A wide page does not scroll sideways either
    Given I open the notebook on a desktop
    Then the page does not scroll sideways

  # A form page has no bar on a phone, deliberately, and so had no rail either —
  # which made it the one page whose column sat half a rail to the left of every
  # other one. That jump is what a reader notices, and the chevron in the corner
  # already abandons what has been typed, so the rail costs nothing new.
  Scenario: A form page stands its column where the rest of them do
    Given I open the notebook on a desktop
    When I press "Budget review"
    And I press "Edit"
    Then the bar stands beside the content
    And the content is centred
