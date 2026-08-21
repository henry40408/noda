Feature: The same pages on a wider screen

  One interface at three densities, not three designs. What changes with the
  width is how much of it can be on screen at once, and each step is an
  extension of the one below it rather than a rearrangement.

  Under 640px it is a phone: one screen at a time, a bar along the bottom.
  Above that the bar stands up into a rail down the left and the row extends —
  the tags and the day leave the second line and go to the right of the title,
  which is the rule `noda ls -l` already follows. Above 1024px the notes screen
  splits: the listing on the left, the note being read on the right. Above
  1440px there is room for a third thing, and it is what points at the note:
  the answer the Links button opens as a page of its own, in the margin of the
  note it is about.

  Two things arrive with the width rather than being rearranged by it, and both
  are the same page saying more of what it already knew. A row grows the id
  column `ls -l` prints, so the listing is the CLI's own row; and the search
  field says how it grouped what was typed, because `a OR b c` is
  `(a OR b) AND c` and that is the one thing about this grammar people read
  backwards. Neither is on a phone, for one reason said twice: there is one
  column there and the note's title has it.

  Scenario: A row extends rather than stacking
    Given I open the notebook on a tablet
    Then the row's tags sit beside the title

  # And what it extends into is the CLI's own column. `noda ls -l` prints id,
  # title, updated, tags; given the room, so does this. The id is the name the
  # notebook knows a note by — what `noda show` takes, and the first half of
  # the filename in the repository.
  Scenario: A row names the note the way the notebook does
    Given I open the notebook on a tablet
    Then the row shows the note's id

  # The same decision, seen from the side it was decided on. A phone has one
  # column and it belongs to the title: the id is an argument about space, and
  # here there is none to spare.
  Scenario: A phone spends its one column on the title
    Given I open the notebook
    Then the row shows no id

  # The index column is narrow enough to stack and wide enough to name. It has
  # no right-hand side to print a day at, so the day rides beside the id
  # instead — the two things that are not the title, at the two ends of the
  # line above it.
  Scenario: The index column keeps the id
    Given I open the notebook on a desktop
    Then the row shows the note's id

  Scenario: A row stacks again on a phone
    Given I open the notebook
    Then the row's tags sit under the title

  # And a third time in the index column, which is neither. The column is
  # narrow on purpose — it is a list you scan while reading something else —
  # so the row goes back to being two lines. Same rule as the phone, same
  # reason: there is no room for a second column of anything.
  Scenario: A row stacks again in the index column
    Given I open the notebook on a desktop
    Then the row's tags sit under the title

  # `a OR b c` is `(a OR b) AND c` — `OR` binds tighter than the space between
  # terms, which is backwards from every search box that has an `OR` at all.
  # It is the one thing about this grammar people read wrong, and the two
  # readings both look like answers, so the field says which one it took. From
  # the server, because it is a fact about the query and not a shortcut.
  Scenario: The field says how it grouped what was searched for
    Given I open the notebook on a tablet
    When I search for "tag:work OR tag:ops budget"
    Then the field groups it as "(tag:work or tag:ops) and (budget)"

  # And the same reasoning as the id, one element over: a phone's field is one
  # line with one remark under it already, and a second remark about the same
  # line is the taller half of a screen that is short of room.
  Scenario: A phone is not told how it was grouped
    Given I open the notebook
    When I search for "tag:work OR tag:ops budget"
    Then the field groups nothing

  Scenario: The content does not run the whole width of a tablet
    Given I open the notebook on a tablet
    Then the content is narrower than the window

  Scenario: A note reads at a comfortable measure on a tablet
    Given I open the notebook on a tablet
    When I press "Budget review"
    Then the content is narrower than the window

  # On a screen holding two panes the question moves: the prose is not centred
  # in the window, because the window has a rail and a listing in it as well.
  # It is centred in the pane it lives in.
  #
  # The heading first, and it is load-bearing rather than decoration. Pressing a
  # row on a screen this wide replaces the reading pane instead of the page, and
  # that is a round trip: until it lands, the pane still holds whatever stood
  # there with no note picked. Measuring the reading column before the note is
  # in it measures the wrong element, or none.
  Scenario: A note reads at a comfortable measure beside the listing
    Given I open the notebook on a desktop
    When I press "Budget review"
    Then the note is headed "Budget review"
    And the reading column is narrower than its pane
    And the reading column is centred in its pane

  Scenario: A wide page does not scroll sideways either
    Given I open the notebook on a desktop
    Then the page does not scroll sideways

  Scenario: A tablet does not scroll sideways
    Given I open the notebook on a tablet
    Then the page does not scroll sideways

  # The result, asserted without reference to how it was reached, so that the
  # scriptless pass makes it too. With no script a note on a desktop is the
  # tablet's single pane — the note, whole — which is what a note page has
  # always been.
  Scenario: A note opens whole on a desktop
    Given I open the notebook on a desktop
    When I press "Budget review"
    Then the note is headed "Budget review"

  # The point of the width. The listing is sent to the note page by the script,
  # because below this width it would be downloaded and never drawn — so this
  # is the shortcut, and it is tagged as one. The scenario above is the
  # untagged account of the result.
  @scripted
  Scenario: The listing stays on screen while a note is read
    Given I open the notebook on a desktop
    When I press "Budget review"
    Then the note is headed "Budget review"
    And the listing is still on screen

  # A swap replaces the pane, the address and the name of the tab — and the
  # name is the one thing of the three that is not in the pane. It rides at the
  # head of the answer as a `<title>`, where a browser's own parser puts it in
  # the head of what it parsed, so what reaches the tab is the server's string
  # rather than one this script put together. Nothing below the browser can
  # check that.
  @scripted
  Scenario: The tab takes the name of the note being read
    Given I open the notebook on a desktop
    When I press "Budget review"
    Then the note is headed "Budget review"
    And the tab is named "Budget review — noda"

  # Which row you are on is a question only two panes can ask, so it is only
  # here that there is an answer to mark.
  @scripted
  Scenario: The listing marks the note being read
    Given I open the notebook on a desktop
    When I press "Budget review"
    Then the listing marks "Budget review"
    When I press "Reading list"
    Then the listing marks "Reading list"

  Scenario: A phone shows one thing at a time
    Given I open the notebook
    When I press "Budget review"
    Then the note is headed "Budget review"
    And the listing is not on screen

  # The untagged account of the widest screen, and it says the same thing the
  # narrower ones do. Nothing about a monitor changes what the page is: the
  # note, whole, and no sideways scroll. Without a script that is all a monitor
  # gets, which is why this is the scenario both passes run.
  Scenario: A note opens whole on a monitor
    Given I open the notebook on a monitor
    When I press "Meeting notes"
    Then the note is headed "Meeting notes"
    And the page does not scroll sideways

  # What a note points at is in the note, and every Markdown reader shows it.
  # What points *at* the note is the half nothing else could tell you, and it
  # has been a press away since the day there was a Links button. Here there is
  # room for it, so it is simply there.
  @scripted
  Scenario: What points at a note sits beside it on a monitor
    Given I open the notebook on a monitor
    When I press "Meeting notes"
    Then the margin note lists "Reading list"

  # An answer of none is an answer, and worth the column it arrived in. The
  # column that goes quiet instead is the one that reads as broken.
  @scripted
  Scenario: A note nothing points at says so
    Given I open the notebook on a monitor
    When I press "Budget review"
    Then the margin note says "Nothing points here."

  # The breakpoint, asserted from the side below it. Backlinks are a walk of
  # every note in the notebook — worth it for a column somebody is reading,
  # waste for one nothing will draw — so a laptop keeps them behind the press
  # they have always been behind.
  @scripted
  Scenario: A laptop leaves the backlinks behind the press
    Given I open the notebook on a desktop
    When I press "Meeting notes"
    Then the note is headed "Meeting notes"
    And the margin note is not on screen

  # A title in the margin is a note in this notebook, so it goes where the
  # listing's rows go and arrives the same way — and the margin is about
  # whichever note is now being read, not the one that was.
  @scripted
  Scenario: A link in the margin leads to the note it names
    Given I open the notebook on a monitor
    When I press "Meeting notes"
    Then the margin note lists "Reading list"
    When I press "Reading list" in the margin note
    Then the note is headed "Reading list"
    And the margin note says "Nothing points here."

  # The one screen that is not inside a notebook, and so the one with no rail.
  # It is a grid column, and a page that never draws a rail was still being laid
  # out around one: 76 pixels of nothing down the left of every notebook, which
  # the markup gives no sign of either way.
  Scenario: The front page is laid out with no rail
    Given I open the front page on a tablet
    Then the notebooks fill the window
    And the page does not scroll sideways
