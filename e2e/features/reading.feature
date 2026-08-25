Feature: Reading a notebook from a phone

  The whole reason `noda web` exists. Three pages and the way between them,
  driven by pressing what is on the screen rather than by typing addresses.

  Scenario: The front page leads to a notebook, and a notebook to a note
    Given I open the front page
    Then I see a row for "default"
    When I press "default"
    Then I am at "/nb/default"
    And I see a row for "Budget review"
    When I press "Budget review"
    Then the note is headed "Budget review"
    And the body says "the q3 budget is late"

  Scenario: A note names the file it is
    Given I open the notebook
    When I press "Budget review"
    Then the filename ends with "-budget-review.md"

  Scenario: A note says when it was made and when it last changed
    Given I open the notebook
    When I press "Budget review"
    Then the note says when it was made and when it changed

  # The exception to the rule the rest of this layer is built on. Everywhere
  # else the script only removes a wait; here it states a fact the server has
  # no way of knowing, because nothing in a request says what time it is where
  # the reader is. Without a script the page keeps the file's own spelling,
  # which is the one that cannot be misread — that half is asserted in Rust,
  # where the bytes can be looked at directly.
  @scripted
  Scenario: The stamps arrive in the reader's own zone
    Given I open the notebook
    When I press "Budget review"
    Then the stamps are said in the reader's own words

  Scenario: The way back goes back
    Given I open the notebook
    When I press "Reading list"
    And I press back
    Then I am at "/nb/default"
    And I see a row for "Budget review"

  Scenario: A note holding markup shows the markup
    Given I open the notebook
    When I press "Markup import"
    Then the body says "a <b>bold</b> here"

  Scenario: A slug is an address, and it leads to the note's own
    Given I open "/nb/default/n/budget-review"
    Then the note is headed "Budget review"
    And I am not at "/nb/default/n/budget-review"

  Scenario: Nothing on the page needs a script
    Given I open the notebook
    Then the page does not scroll sideways
    And I see a row for "Meeting notes"
