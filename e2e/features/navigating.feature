Feature: Getting around the notebook

  Notes, Tags, Todo and Files are places and sit on the bar, which marks the
  current one; New is an action, the round button above it.

  The listing is on the bar because a rail missing the place you spend most
  time reads as an omission, and with two panes the listing is never left.

  The network screen is about the notebook, not inside it: the corner chip
  reaches it and the bar does not.

  Scenario: The bar reaches the notes
    Given I open "/nb/default/tags"
    When I press "Notes"
    Then I see a row for "Budget review"

  Scenario: The bar reaches the tags
    Given I open the notebook
    When I press "Tags"
    Then I am at "/nb/default/tags"

  Scenario: The bar reaches the todo
    Given I open the notebook
    When I press "Todo"
    Then I am at "/nb/default/todo"

  Scenario: The bar reaches the files
    Given I open the notebook
    When I press "Files"
    Then I am at "/nb/default/files"

  Scenario: The bar says which screen you are on
    Given I open "/nb/default/tags"
    Then the bar marks "Tags"
    And the bar does not mark "Todo"

  Scenario: The listing is marked like anywhere else
    Given I open the notebook
    Then the bar marks "Notes"
    And the bar does not mark "Tags"

  Scenario: Nothing is marked on the one screen the bar does not hold
    Given I open "/nb/default/status"
    Then the bar marks nothing

  Scenario: Writing is one press from anywhere in the notebook
    Given I open "/nb/default/files"
    When I press the button to write
    Then I am at "/nb/default/new"

  Scenario: A tag is a way into the listing, not a report
    Given I open "/nb/default/tags"
    When I press "work"
    Then I see a row for "Budget review"
    And I do not see a row for "Reading list"

  # A TiddlyWiki import leaves tags with spaces, and the query splits like a
  # shell, so unquoted this would be three terms and find nothing.
  Scenario: A tag with a space in it still finds its notes
    Given I open "/nb/default/tags"
    When I press "24.04 Dark patterns"
    Then I see a row for "Meeting notes"
    And I do not see a row for "Budget review"

  Scenario: An unticked box turns up on the todo screen
    Given I open "/nb/default/todo"
    Then I see a row for "chase the marketing line"
    And I see a row for "Budget review"

  Scenario: A ticked box is finished and is not a todo
    Given I open "/nb/default/todo"
    Then I do not see a row for "pull the ledger export"

  Scenario: The soonest is first and the late one is marked
    Given I open "/nb/default/todo"
    Then the first row is "chase the marketing line"
    And "2000-01-01" is marked overdue

  Scenario: A todo row goes to the note the box is written in
    Given I open "/nb/default/todo"
    When I press "chase the marketing line"
    Then the note is headed "Budget review"

  Scenario: Backlinks name what points at a note
    Given I open the notebook
    When I press "Meeting notes"
    And I press "Links"
    Then I see a row for "Reading list"
    And I do not see a row for "Budget review"

  Scenario: A note that nothing points at says so
    Given I open the notebook
    When I press "Markup import"
    And I press "Links"
    Then the page says "Nothing links here"

  Scenario: A file is asked what points at it from the count beside it
    Given I open "/nb/default/files"
    When I press "in 1 note"
    Then I see a row for "Markup import"
