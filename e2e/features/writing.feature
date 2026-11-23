Feature: Writing from a phone

  Each form is checked at phone width for reachable controls, no sideways
  scroll, and fields a phone will not zoom in on.

  Scenario: A note can be written, start to finish
    Given I open the notebook
    When I press the button to write
    And I write "Written on a train" as the title
    And I write "the meeting is moved" as the body
    And I submit "Add note"
    Then the note is headed "Written on a train"
    And the body says "the meeting is moved"

  Scenario: Every control on the new-note form can be pressed
    Given I open the notebook
    When I press the button to write
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

  Scenario: Deleting asks first, and the way to it is on the note's own bar
    Given I open the notebook
    When I press "Budget review"
    And I press "Delete"
    Then the page says "Delete Budget review?"
    And the words line up with the buttons under them
    And no control is smaller than 48 by 48

  # A save the server refuses is not a save the server missed. Whatever the
  # script does, the reader is told what the server said, which is what the page
  # without the script would be showing.
  Scenario: Saving a note that was deleted in the meantime says so
    Given I open the notebook
    When I press "Reading list"
    And I press "Edit"
    And the note "reading-list" is deleted from another machine
    And I write "too late" as the body
    And I submit "Save"
    Then the page says "No such note"
    And the page does not say "did not answer"

  Scenario: Saving more than the server takes says so
    Given I open the notebook
    When I press "Budget review"
    And I press "Edit"
    And I paste a body longer than the server takes
    And I submit "Save"
    Then the page says "length limit exceeded"
    And the page does not say "did not answer"

  # The pin is the one write the script draws before the server has answered,
  # so a refusal also has to take the guess back.
  Scenario: Pinning a note that was deleted in the meantime says so
    Given I open the notebook
    When I press "Reading list"
    And the note "reading-list" is deleted from another machine
    And I submit "Pin"
    Then the page says "No such note"
    And the page does not say "did not answer"

  Scenario: Renaming keeps the address
    Given I open the notebook
    When I press "Reading list"
    And I press "Rename"
    And I write "Books to read" as the title
    And I submit "Rename"
    Then the note is headed "Books to read"
