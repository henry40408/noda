Feature: Everything can be pressed with a thumb

  Only a laid-out page knows whether a control reached its promised minimum
  height. 48 rather than Apple's 44, following Material: a thumb does not know
  the platform.

  Scenario: Every control on the front page is big enough
    Given I open the front page
    Then no control is smaller than 48 by 48

  # At this width the row aligns on a baseline instead of stretching, which
  # once left the notebook's status chip only as tall as its pill.
  Scenario: Every control on the front page is big enough on a tablet
    Given I open the front page on a tablet
    Then no control is smaller than 48 by 48

  Scenario: Every control on a listing is big enough
    Given I open the notebook
    Then no control is smaller than 48 by 48

  Scenario: Every control on a note is big enough
    Given I open the notebook
    When I press "Budget review"
    Then no control is smaller than 48 by 48

  Scenario: The way out of an empty search is big enough
    Given I open the notebook
    When I search for "tag:ghost"
    Then no control is smaller than 48 by 48

  Scenario: The search field does not make a phone zoom
    Given I open the notebook
    Then the search field's text is at least 16 pixels

  Scenario: A phone-width page does not scroll sideways
    Given I open the notebook
    Then the page does not scroll sideways

  Scenario: A note with a long title does not scroll sideways
    Given I open the notebook
    When I press "Markup import"
    Then the page does not scroll sideways

  Scenario: The narrowest phone still fits
    Given I open the notebook on a 320 pixel phone
    Then the page does not scroll sideways
    And no control is smaller than 48 by 48

  # A note's bar has five items to the listing's four.
  Scenario: The narrowest phone still fits a note's five actions
    Given I open the notebook on a 320 pixel phone
    When I press "Budget review"
    Then the page does not scroll sideways
    And no control is smaller than 48 by 48

  # A tablet is as wide as a desktop and has no mouse. The rail, the tags and the
  # shortcut card are drawn tighter for a pointer, so each must give a finger its
  # 48 again when the pointer is one. 1024 and 1366 are an iPad Pro's two widths.
  Scenario Outline: A touch screen as wide as a desktop still has controls big enough for a thumb
    Given my screen is a touch screen
    And I open the notebook on a touch screen <width> pixels wide
    When I press "Budget review"
    Then no control is smaller than 48 by 48

    Examples:
      | width |
      | 1024  |
      | 1366  |
      | 1440  |
      | 1800  |

  @scripted
  Scenario: The shortcut card's buttons are big enough for a thumb
    Given my screen is a touch screen
    And I open the notebook on a touch screen 1366 pixels wide
    When I hit the keys "?"
    Then no control is smaller than 48 by 48
