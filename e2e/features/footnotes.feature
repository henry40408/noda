Feature: Footnotes in a note

  A footnote is a number in the text and a note under it; each leads to the
  other, and the one arrived at is lit.

  Scenario: A footnote's number leads to its note, and the arrow leads back
    Given the notebook holds a note "Cited" saying:
      """
      The claim.[^1]

      [^1]: The source.
      """
    And I open the notebook
    When I press "Cited"
    Then the body says "The source."
    When I press the footnote "1"
    Then the footnote "1" is the one lit
    When I press the way back from the footnote "1"
    Then the citation of the footnote "1" is the one lit

  # The script swaps a pane on popstate, which a jump to a fragment also fires:
  # swapping would replace the very element the browser has just scrolled to.
  @scripted
  Scenario: Jumping to a footnote keeps the note on screen
    Given the notebook holds a note "Cited" saying:
      """
      The claim.[^1]

      [^1]: The source.
      """
    And I open the notebook on a desktop
    When I press "Cited"
    Then the note is headed "Cited"
    When I remember the note on screen
    And I press the footnote "1"
    Then the footnote "1" is the one lit
    And it is still the note I remembered
    When I go back
    Then it is still the note I remembered
