Feature: Syncing from a phone

  The one thing here that does not finish while the browser waits. A sync is a
  fetch over somebody's tailnet, so the press answers straight away and the
  screen it lands on says what is happening — which means these scenarios are
  the only ones that watch a page change without anybody touching it.

  The notebook has a remote it has never spoken to, so what the chip says at the
  start is a fact about the fixture and not about the scenario before it.

  Scenario: The listing says where the notebook stands
    Given I open the notebook
    Then the page says "never synced"

  Scenario: The chip leads to the screen that says the rest
    Given I open the notebook
    When I press "never synced"
    Then I am at "/nb/default/status"
    And the page says "Branch"
    And the page says "Remote"

  Scenario: Syncing sends the notes and says what it did
    Given I open "/nb/default/status"
    When I submit "Sync"
    Then the page says "Synced"
    And the page says "push:"
    And the page says "in sync"

  Scenario: What a sync did is still there on the way back
    Given I open "/nb/default/status"
    When I submit "Sync"
    Then the page says "Synced"
    When I press back
    And I press "in sync"
    Then the page says "Synced"

  Scenario: The screen is reachable from the bar it carries
    Given I open "/nb/default/status"
    When I press "Tags"
    Then I am at "/nb/default/tags"

  Scenario: The bar marks nothing on the network screen
    Given I open "/nb/default/status"
    Then the bar marks nothing

  Scenario: Every control on the network screen can be pressed
    Given I open "/nb/default/status"
    Then no control is smaller than 48 by 48
    And the page does not scroll sideways

  Scenario: The narrowest phone still fits the network screen
    Given I open the notebook on a 320 pixel phone
    When I press "never synced"
    Then the page does not scroll sideways
    And no control is smaller than 48 by 48
