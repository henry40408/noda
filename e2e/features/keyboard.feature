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

  # Chrome set to block all cookies throws from the very reading of
  # `localStorage`, before any call that could be guarded. What is lost is the
  # preference to switch the keys off, never the keys.
  @scripted
  Scenario: The keys work in a browser that refuses to store anything
    Given the browser refuses to store anything
    And I open the notebook on a desktop
    When I hit the keys "n"
    Then I am at "/nb/default/new"

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
    Given the browser notes what the keys press
    And I open the notebook on a desktop
    When I type "gntdf?" into the search field
    Then the field named "q" holds "gntdf?"
    And no key pressed anything
    And I am at "/nb/default"
    And the shortcut card is not open

  @scripted
  Scenario: Letters typed into a form are text
    Given the browser notes what the keys press
    And I open the notebook on a desktop
    When I press the button to write
    And I write "gtdfs" as the title
    And I write "n?pDej" as the body
    Then the field named "title" holds "gtdfs"
    And the field named "body" holds "n?pDej"
    And no key pressed anything
    And I am at "/nb/default/new"
    And the shortcut card is not open

  # Firefox's search field does not empty itself on Escape, so the page does.
  @scripted
  Scenario: Escape empties a search field the browser leaves alone
    Given the browser does not empty a search field on Escape
    And I open the notebook on a desktop
    When I type "budget" into the search field
    And I hit the Escape key
    Then the field named "q" holds ""

  @scripted
  Scenario: Ctrl and Enter send the form being written
    Given I open the notebook
    When I press the button to write
    And I write "Sent by chord" as the title
    And I write "from the keyboard" as the body
    And I hit Ctrl and Enter
    Then the note is headed "Sent by chord"

  # A tag's box is a field of the form like any other, and the chord sends from
  # it, with the focus where the click left it.
  @scripted
  Scenario: Ctrl and Enter send the tags from one of their boxes
    Given I open the notebook
    When I press "Meeting notes"
    And I press "Tags"
    And I untick "work"
    And the box for "work" has the focus
    And I hit Ctrl and Enter
    Then the note is headed "Meeting notes"
    And the page does not say "work"

  # The page asking whether to delete rests its focus on "Keep it", so that a
  # plain Enter keeps the note. The chord that sends a form must not turn that
  # round: it sends from a field, not from a link.
  @scripted
  Scenario: Ctrl and Enter do not delete a note from the button that keeps it
    Given the browser notes what the keys press
    And I open the notebook on a desktop
    When I press "Budget review"
    And I press "Delete"
    And I hit Ctrl and Enter
    Then no key pressed anything
    And the notebook still holds "Budget review"

  # Safari sends key code 229 for the keydown that ends a composition, with the
  # letter that was pressed.
  @scripted
  Scenario: A key an input method is composing with is not a shortcut
    Given I open the notebook on a desktop
    When I hit the key "g" while an input method is composing
    Then the page is not waiting for a second key
    When I hit the key "n" while an input method is composing
    Then I am at "/nb/default"
