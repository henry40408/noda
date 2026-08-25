Feature: Everything can be pressed with a thumb

  The assertions no other layer can make. A stylesheet can promise a minimum
  height; whether a control ends up that big depends on the box it is in, what
  is beside it, and how its text wrapped — which only a laid-out page knows.

  Forty-eight and not forty-four: Apple's guideline says one, Material's says
  the other, and a thumb does not know which platform it is on.

  Scenario: Every control on the front page is big enough
    Given I open the front page
    Then no control is smaller than 48 by 48

  # A tablet is touched too, and the front page's row is where that stopped
  # being automatic: the chip saying where a notebook stands is as tall as its
  # row on a phone and was as tall as its own pill here, because at this width
  # the row aligns its contents on a baseline rather than stretching them.
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

  # A note's bar is five items and the listing's is four, so this is the width
  # where the fifth either fits or is the reason a phone scrolls sideways.
  Scenario: The narrowest phone still fits a note's five actions
    Given I open the notebook on a 320 pixel phone
    When I press "Budget review"
    Then the page does not scroll sideways
    And no control is smaller than 48 by 48
