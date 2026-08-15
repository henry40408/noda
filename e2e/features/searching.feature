Feature: Finding a note

  The search field is a form. It is submitted by the key a phone's keyboard
  offers, and it works with the page's scripts turned off — which is the whole
  contract the enhancement layer will later have to keep.

  Scenario: A query narrows the listing
    Given I open the notebook
    When I search for "budget"
    Then I see a row for "Budget review"
    And I do not see a row for "Reading list"

  Scenario: A tag with a space in it can still be filtered by
    Given I open the notebook
    When I search for "tag:'24.04 Dark patterns'"
    Then I see a row for "Meeting notes"
    And I do not see a row for "Budget review"

  Scenario: Nothing typed is not a complaint
    Given I open the notebook
    Then the page says nothing is wrong

  Scenario: Half a query says why and keeps the notes
    Given I open the notebook
    When I search for "OR"
    Then the page complains
    And I see a row for "Budget review"
    And I see a row for "Reading list"

  Scenario: A query matching nothing offers the way back
    Given I open the notebook
    When I search for "tag:ghost"
    Then I do not see a row for "Budget review"
    When I press "Clear the search"
    Then I see a row for "Budget review"
