Feature: Runs in the listing

  The rows are cut into runs under a heading: the pinned notes first, then,
  when the order is a date, a month at a time. A heading changes how the
  listing is drawn and never what it holds, so these run with scripts on and
  off. A month is named by the server's zone, which a feature cannot know, so
  a month is recognised by its shape.

  Scenario: A listing ordered by a date is cut by month
    Given I open "/nb/default?sort=created"
    Then the listing is headed by a month and a year

  # A heading of "Notes" says the list is one run on purpose.
  Scenario: An order that is not a date is one run
    Given I open the notebook
    Then the listing has the heading "Notes"
    And the listing is not cut by month

  Scenario: A pinned note is a run of its own
    Given the note "budget-review" is pinned
    When I open "/nb/default?sort=updated"
    Then the listing has the heading "Pinned"
    And the listing is headed by a month and a year

  Scenario: A pinned note is a run of its own in any order
    Given the note "budget-review" is pinned
    When I open the notebook
    Then the listing has the heading "Pinned"
    And the listing has the heading "Notes"

  # A heading over nothing would say a run is there when the search sent it away.
  Scenario: A run whose notes were all sent away has no heading
    Given the note "budget-review" is pinned
    When I open "/nb/default?sort=updated"
    And I search for "meeting"
    Then I see a row for "Meeting notes"
    And the listing is headed by a month and a year
    And the listing does not have the heading "Pinned"
