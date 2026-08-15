Feature: Writing from a phone

  The forms are the newest thing here and the least like anything the terminal
  does, which makes them the ones a browser has most to say about. Every one is
  checked at a phone's width for the same three things a page has to get right
  before it gets anything else right: reachable controls, no sideways scroll,
  and a field a phone will not zoom in on.

  Scenario: A note can be written, start to finish
    Given I open the notebook
    When I press "New"
    And I write "Written on a train" as the title
    And I write "the meeting is moved" as the body
    And I submit "Add note"
    Then the note is headed "Written on a train"
    And the body says "the meeting is moved"

  Scenario: Every control on the new-note form can be pressed
    Given I open the notebook
    When I press "New"
    Then no control is smaller than 48 by 48
    And the page does not scroll sideways
    And no field is smaller than 16 pixels

  Scenario: Every control on the edit form can be pressed
    Given I open the notebook
    When I press "Budget review"
    And I press "Edit"
    Then no control is smaller than 48 by 48
    And the page does not scroll sideways
    And no field is smaller than 16 pixels

  Scenario: Every control on the tags form can be pressed
    Given I open the notebook
    When I press "Budget review"
    And I press "Tags"
    Then no control is smaller than 48 by 48
    And no field is smaller than 16 pixels

  Scenario: A tag can be taken off by unticking it
    Given I open the notebook
    When I press "Meeting notes"
    And I press "Tags"
    And I untick "work"
    And I submit "Save"
    Then the note is headed "Meeting notes"
    And the page does not say "work"

  Scenario: The body survives a round trip through the browser
    Given I open the notebook
    When I press "Budget review"
    And I press "Edit"
    And I write "one\ntwo\nthree" as the body
    And I submit "Save"
    Then the body says "one"
    And the body says "three"

  Scenario: Deleting asks first, and the way to it is past the note
    Given I open the notebook
    When I press "Budget review"
    And I press "Delete this note"
    Then the page says "Delete Budget review?"
    And no control is smaller than 48 by 48

  Scenario: Renaming keeps the address
    Given I open the notebook
    When I press "Reading list"
    And I press "Rename"
    And I write "Books to read" as the title
    And I submit "Rename"
    Then the note is headed "Books to read"
