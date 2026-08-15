Feature: Getting around the notebook

  Three places and one action, told apart by not being in the same row. Tags,
  Todo and Files are somewhere to go and they sit on the bar; New is something
  to do, and it is the round button above it. The bar also answers "where am
  I" — the screen you are on is the one marked on it.

  The listing is not on the bar. It is where the bar leads *from*, and the way
  back to it is the chevron every screen already carries in the same corner.

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

  Scenario: Nothing is marked on the listing the bar leads from
    Given I open the notebook
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

  # A tag may hold a space — a TiddlyWiki import leaves such things behind —
  # and the field it lands in splits the way a shell does. Unquoted it would
  # arrive as three terms and-ed together and find nothing at all.
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
