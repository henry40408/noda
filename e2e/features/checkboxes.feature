Feature: A note's checkboxes

  A done item is struck through and dimmed, and nothing else is. A text
  decoration is drawn through everything inside the element that sets it, so
  the items nested in a done one are the case that needs care: each is its own
  item and says its own state, in a list of either shape (a blank line between
  items makes every item a paragraph).

  These are the page's own style, so they run with scripts on and off.

  Scenario: A done item is struck through and an open one is not
    Given the notebook holds a note "Packing" saying:
      """
      - [x] passport
      - [ ] charger
      """
    And I open the notebook
    When I press "Packing"
    Then "passport" is struck through
    And "charger" is not struck through
    And "passport" is not drawn in the colour of "charger"

  # The reason the strike is not simply put on the item: the open child would
  # inherit it.
  Scenario: An open item inside a done one is not struck through
    Given the notebook holds a note "Packing" saying:
      """
      - [x] alpha
        - [ ] bravo
        - [x] charlie
      - [ ] delta
        - [x] echo
      """
    And I open the notebook
    When I press "Packing"
    Then "alpha" is struck through
    And "bravo" is not struck through
    And "charlie" is struck through
    And "delta" is not struck through
    And "echo" is struck through
    And "bravo" is drawn in the colour of "delta"

  Scenario: An open item inside a done one in a loose list is not struck through
    Given the notebook holds a note "Packing" saying:
      """
      - [x] alpha

        - [ ] bravo

      - [ ] delta

        - [x] echo
      """
    And I open the notebook
    When I press "Packing"
    Then "alpha" is struck through
    And "bravo" is not struck through
    And "delta" is not struck through
    And "echo" is struck through
    And "bravo" is drawn in the colour of "delta"

  Scenario: What follows a done list is not struck through
    Given the notebook holds a note "Packing" saying:
      """
      - [x] alpha
        - [ ] bravo

      and a paragraph after
      """
    And I open the notebook
    When I press "Packing"
    Then "alpha" is struck through
    And "a paragraph after" is not struck through

  # The line through a done item is enough to say it is done; a link in it is
  # still a link someone may want to read.
  Scenario: A link in a done item keeps its colour
    Given the notebook holds a note "Packing" saying:
      """
      - [x] renew [the visa](https://example.com/visa) today
      - [ ] charger
      """
    And I open the notebook
    When I press "Packing"
    Then "renew" is struck through
    And "the visa" is struck through
    And "the visa" is drawn in the colour of "charger"
    And "renew" is not drawn in the colour of "charger"
