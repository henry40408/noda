Feature: The notebook follows the phone

  No theme toggle, by design: the phone already says which the reader wants,
  so `prefers-color-scheme` is the only way to the dark palette.

  Scenario: A dark phone gets the dark palette
    Given my phone prefers a dark theme
    When I open the notebook
    Then the page is dark

  Scenario: A light phone gets the light palette
    Given my phone prefers a light theme
    When I open the notebook
    Then the page is light

  Scenario: A note is dark too, not only the listing
    Given my phone prefers a dark theme
    When I open the notebook
    And I press "Budget review"
    Then the page is dark
