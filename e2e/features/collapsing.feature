Feature: Collapsible sections in a note

  `<details>` folds a section, and the browser does the folding: no script.

  Scenario: Pressing a section's summary unfolds it
    Given the notebook holds a note "Folded" saying:
      """
      <details>
      <summary>More</summary>

      The hidden part.

      </details>
      """
    And I open the notebook
    When I press "Folded"
    Then the note is headed "Folded"
    When I press the summary of the section
    Then the section is unfolded

  # A pin swaps the pane for the server's copy, which only knows how the note
  # writes its sections.
  @scripted
  Scenario: A pin leaves an unfolded section open
    Given the notebook holds a note "Folded" saying:
      """
      <details>
      <summary>More</summary>

      The hidden part.

      </details>
      """
    And I open the notebook on a desktop
    When I press "Folded"
    Then the note is headed "Folded"
    When I press the summary of the section
    Then the section is unfolded
    When I hit the keys "p"
    Then the page says "pinned"
    And the section is unfolded
