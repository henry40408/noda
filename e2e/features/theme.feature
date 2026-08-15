Feature: The notebook follows the phone

  There is no theme toggle in these pages and there is not going to be one: the
  reader has already told their phone which they want, and a notebook is not the
  place to ask again. Which makes `prefers-color-scheme` the only way the dark
  palette is ever reached — and the only way it can be checked.

  Scenario: A dark phone gets the dark palette
    Given my phone prefers a dark theme
    When I open the notebook
    Then the page is dark

  Scenario: A light phone gets the light palette
    Given my phone prefers a light theme
    When I open the notebook
    Then the page is light

  Scenario: A phone with no preference gets the light one
    Given I open the notebook
    Then the page is light

  Scenario: A note is dark too, not only the listing
    Given my phone prefers a dark theme
    When I open the notebook
    And I press "Budget review"
    Then the page is dark
