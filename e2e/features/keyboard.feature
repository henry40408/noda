Feature: Pressing keys

  A key only presses a control that is already on the page, so each scenario
  sends real keystrokes and reads where the page ended up. They run with
  scripts on: without the script the controls are the links and buttons they
  always were, which every other feature presses.

  The two ways a key must be left alone, a field being typed in and an input
  method composing, are checked against the key that would have acted.

  @scripted
  Scenario: A letter presses the control that carries it
    Given I open the notebook on a desktop
    When I hit the keys "n"
    Then I am at "/nb/default/new"

  @scripted
  Scenario Outline: g then a letter goes to a place
    Given I open the notebook on a desktop
    When I hit the keys "g<second>"
    Then I am at "<path>"

    Examples:
      | second | path               |
      | t      | /nb/default/tags   |
      | d      | /nb/default/todo   |
      | f      | /nb/default/files  |
      | s      | /nb/default/status |
      | h      | /                  |

  @scripted
  Scenario: g alone waits for the letter that says where
    Given I open the notebook on a desktop
    When I hit the keys "g"
    Then the page is waiting for the second key

  # A note's own actions are on its page, which the swap brings: a key hit
  # before the note has arrived has nothing to press.
  @scripted
  Scenario: A letter presses what the note's bar offers
    Given I open the notebook on a desktop
    When I press "Budget review"
    Then the note is headed "Budget review"
    When I hit the keys "e"
    Then the tab is named "Edit — noda"

  # The label flips before the server answers; the word "pinned" is the server's
  # own, so waiting for it keeps the second press from landing on a busy button.
  @scripted
  Scenario: Pin is undone by pressing it again
    Given I open the notebook on a desktop
    When I press "Budget review"
    Then the note is headed "Budget review"
    When I hit the keys "p"
    Then the page says "pinned"
    When I hit the keys "p"
    Then the page does not say "pinned"

  # The list beside the note has no way back, so Escape has nothing to press
  # there; on a tablet the note stands alone and has.
  @scripted
  Scenario: Escape steps back from a note
    Given I open the notebook on a tablet
    When I press "Budget review"
    Then the note is headed "Budget review"
    When I hit the Escape key
    Then I am at "/nb/default"

  @scripted
  Scenario: A question mark opens the card of what the keys do
    Given I open the notebook on a desktop
    When I hit the keys "?"
    Then the shortcut card is open

  @scripted
  Scenario: The slash key lands in the search field and is not typed into it
    Given I open the notebook on a desktop
    When I hit the keys "/"
    Then the search field has the focus
    When I hit the keys "gt"
    Then the field named "q" holds "gt"
    And I am at "/nb/default"

  # Every one of these letters is a shortcut on this page.
  @scripted
  Scenario: Letters typed into the search field are text
    Given I open the notebook on a desktop
    When I type "gntdf?" into the search field
    Then the field named "q" holds "gntdf?"
    And I am at "/nb/default"
    And the shortcut card is not open

  @scripted
  Scenario: Letters typed into a form are text
    Given I open the notebook on a desktop
    When I press the button to write
    And I write "gtdfs" as the title
    And I write "n?pDej" as the body
    Then the field named "title" holds "gtdfs"
    And the field named "body" holds "n?pDej"
    And I am at "/nb/default/new"
    And the shortcut card is not open

  # Safari sends key code 229 for the keydown that ends a composition, with the
  # letter that was pressed.
  @scripted
  Scenario: A key an input method is composing with is not a shortcut
    Given I open the notebook on a desktop
    When I hit the key "g" while an input method is composing
    Then the page is not waiting for a second key
    When I hit the key "n" while an input method is composing
    Then I am at "/nb/default"
