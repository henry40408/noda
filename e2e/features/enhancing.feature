Feature: Answering without asking

  The listing carries every note it has and hides the ones the query excludes.
  That is true of the page the server sends, so it is true with the scripts off
  — and it is what lets the script widen a query as well as narrow one, because
  a row it needs to put back is already there.

  What the script may do with that is bounded: it may answer sooner, or not at
  all, but never differently. A bare word reads the body on the server and this
  page has no bodies, so the script's answer is a subset — right, and possibly
  short, which is what the remark under the field is for. A *negated* bare word
  inverts that, and there the filter stands aside altogether.

  A tagged case below describes the shortcut itself and runs only in the pass
  where scripts run. Everything else runs both ways, including every claim
  about what an answer *is* — the tag buys the right to be about the shortcut,
  never the right to be the only account of the result.

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

  @scripted
  Scenario: The network screen asks for news instead of reloading
    Given I open "/nb/default/status"
    When I submit "Sync"
    Then the page is not reloading itself
    And the page says "Synced"
