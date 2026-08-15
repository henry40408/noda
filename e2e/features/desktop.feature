Feature: The same pages on a wide screen

  Mobile is what this is for, so the wide layout is a handful of lines rather
  than a second design. What it has to get right is the rule the CLI already
  follows: a row *extends* when there is room, and never rearranges.

  On a phone a row is two lines — the title, then the tags and the day beneath
  it. Given the width, those slide out to the right of the title instead. Same
  information, same order, one line.

  Scenario: A row extends rather than stacking
    Given I open the notebook on a desktop
    Then the row's tags sit beside the title

  Scenario: A row stacks again on a phone
    Given I open the notebook
    Then the row's tags sit under the title

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
