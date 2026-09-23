Feature: Answering without asking

  The listing carries every note and hides the ones the query excludes, even
  with scripts off, so the script can widen a query as well as narrow it.

  The script may answer sooner or not at all, never differently. A bare word
  matches bodies on the server and the page has none, so the script's answer
  may be short, which the remark under the field says. For a *negated* bare
  word the filter stands aside.

  `@scripted` cases describe the shortcut and run only with scripts on; every
  claim about what an answer *is* also runs both ways.

  Scenario: The rows a query excludes are still on the page
    Given I open the notebook
    When I search for "tag:work"
    Then I see a row for "Budget review"
    And I do not see a row for "Reading list"
    And the page holds a hidden row for "Reading list"

  Scenario: The search key gives the same answer either way
    Given I open the notebook
    When I search for "tag:work"
    Then the address carries the search "tag:work"
    And I see a row for "Budget review"
    And I do not see a row for "Reading list"
    And the listing says nothing about whose answer it is

  @scripted
  Scenario: Typing narrows the listing before anything is sent
    Given I open the notebook
    When I type "tag:work" into the search field
    Then I see a row for "Budget review"
    And I do not see a row for "Reading list"
    And the address carries no search

  @scripted
  Scenario: Deleting what was typed puts the rows back
    Given I open the notebook
    When I type "tag:work" into the search field
    Then I do not see a row for "Reading list"
    When I type "" into the search field
    Then I see a row for "Reading list"
    And I see a row for "Budget review"

  @scripted
  Scenario: A whole answer says nothing about itself
    Given I open the notebook
    When I type "tag:work" into the search field
    Then the listing says nothing about whose answer it is

  @scripted
  Scenario: A word that could be in a body says the answer is partial
    Given I open the notebook
    When I type "budget" into the search field
    Then I see a row for "Budget review"
    And the listing says it filtered by title and tag

  @scripted
  Scenario: A negated word the script cannot judge leaves every row alone
    Given I open the notebook
    When I type "-budget" into the search field
    Then I see a row for "Budget review"
    And I see a row for "Reading list"
    And the listing says it filtered by title and tag

  @scripted
  Scenario: Half a query is not a complaint and not a filter
    Given I open the notebook
    When I type "OR" into the search field
    Then I see a row for "Budget review"
    And I see a row for "Reading list"
    And the page says nothing is wrong

  # Redrawn per keystroke from the parse the filter already uses.
  @scripted
  # Redrawn per keystroke from the parse the filter already uses.
  Scenario: The grouping follows what is being typed
    Given I open the notebook on a tablet
    When I type "tag:work OR tag:ops budget" into the search field
    Then the field groups it as "(tag:work or tag:ops) and (budget)"

  # The filter stands aside (it would have to widen the answer), but grouping
  # is about the words, not the notes, so it is still drawn.
  @scripted
  # The filter stands aside (it would have to widen the answer), but grouping
  # is about the words, not the notes, so it is still drawn.
  Scenario: A query the filter stands aside for is still grouped
    Given I open the notebook on a tablet
    When I type "-budget tag:work" into the search field
    Then I see a row for "Reading list"
    And the field groups it as "(-budget) and (tag:work)"

  # The last complete grouping would describe a line no longer typed.
  @scripted
  # The last complete grouping would describe a line no longer typed.
  Scenario: Half a query has no grouping to show
    Given I open the notebook on a tablet
    When I type "tag:work OR" into the search field
    Then the field groups nothing

  @scripted
  Scenario: The network screen asks for news instead of reloading
    Given I open "/nb/default/status"
    When I submit "Sync"
    Then the page is not reloading itself
    And the page says "Synced"
