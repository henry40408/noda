package tui

import (
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"
	"unicode/utf8"

	"github.com/henry40408/noda/internal/cmd"
	"github.com/henry40408/noda/internal/tui/ui"
)

// casesNotebook is four notes written as files, so each carries the dates, tag
// and pin a screen shows: an overdue and a future box, a pin, a wide-character
// title, and a tag with a space in it.
func casesNotebook(t *testing.T) cmd.Env {
	t.Helper()
	env := emptyNotebook(t)
	active := notebookOf(t, env)
	files := map[string]string{
		"01abc234-budget-review.md": "---\ntitle: Budget review\ntags: [work]\ncreated: 2024-01-10T09:00:00Z\nupdated: 2024-03-01T10:00:00Z\n---\n\nthe q3 budget is late\n\n- [ ] pay the bill due:2024-01-02\n- [ ] plan ahead due:2099-01-01\n- [x] done already\n",
		"02def567-meeting-notes.md": "---\ntitle: Meeting notes\ntags: [work, q3]\ncreated: 2024-02-01T10:00:00+08:00\nupdated: 2024-02-02T10:00:00+08:00\npinned: true\n---\n\n# Agenda\n\nsee [the budget](01abc234-budget-review.md)\n",
		"03ghj890-reading-list.md":  "---\ntitle: Reading list\ncreated: 2024-01-05T00:00:00Z\nupdated: 2024-01-06T00:00:00Z\n---\n\na book\n",
		"04kmn123-日本語メモ.md":         "---\ntitle: 日本語メモ\ntags: [日本語, has space]\ncreated: 2024-01-01T00:00:00Z\nupdated: 2024-01-02T00:00:00Z\n---\n\n本文はここにあります。\n\n- [ ] 日本語の用事 due:2024-05-06\n",
		"diagram.png":               "not really a png",
	}
	for name, text := range files {
		if err := os.WriteFile(filepath.Join(active.Path, name), []byte(text), 0o666); err != nil {
			t.Fatal(err)
		}
	}
	if _, err := active.CommitAll("add notes"); err != nil {
		t.Fatal(err)
	}
	return env
}

type screenCase struct {
	name          string
	width, height int // 100x30 when zero
	keys          []string
	screen        []string // the whole frame, for the few cases where layout is the behaviour
	styles        []styleWant
	rows          map[int]string // a row's exact text, trailing blanks cut
	has           []string       // each on some row
	hasSqueezed   []string       // each on some row once spaces are dropped: a wide character is followed by a blank cell
	lacks         []string
	cursor        *[2]int // nil: the frame draws no cursor
}

// styleWant is the colour and weight of every cell of a text on one row.
type styleWant struct {
	row  int
	text string
	fg   ui.Color
	mod  ui.Modifier
}

func at(x, y int) *[2]int { return &[2]int{x, y} }

var screenCases = []screenCase{
	{
		name: "the listing names its columns, marks the cursor row and says pinned",
		keys: nil,
		rows: map[int]string{
			0: "Notebook: default          <enter>  read       <e>  edit         <space>  mark                  noda",
			1: "Branch:   main             </>  filter         <a>  new          <*>  mark shown",
			2: "Remote:   none             <:>  command        <m>  retitle      <Q>  queue",
			3: "Notes:    4 notes  1 file  <ctrl-a>  commands  <#>  tags         <T>  keep updated",
			4: "Changes:  none             <?>  keys           <ctrl-d>  delete  <q>  quit",
		},
		has:         []string{"Notes  all  4", "ID        TITLE", "TAGS              PINNED", "▌   01abc234  Budget review", "[work, q3]        pinned", "Branch:   main"},
		hasSqueezed: []string{"04kmn123日本語メモ[日本語,hasspa"},
		cursor:      nil,
	},
	{
		name:  "a 40-column terminal folds the header to a line and cuts titles and tags where they stop fitting",
		width: 40, height: 12,
		keys: nil,
		screen: []string{
			"default  (main)  4 notes",
			"Notes  all  4 ──────────────────────────",
			"    ID        TITLE       TAGS   PINNED",
			"▌   01abc234  Budget rev  [work",
			"    02def567  Meeting no  [work  pinned",
			"    03ghj890  Reading li",
			"    04kmn123  日 本 語 メ モ   [日 本",
			"",
			"",
			"",
			" notes",
			"",
		},
	},
	{
		name:  "a terminal too short for the header folds it to one line and gives the rows a scrollbar",
		width: 90, height: 7,
		keys: nil,
		screen: []string{
			"default  (main)  4 notes",
			"Notes  all  4 ────────────────────────────────────────────────────────────────────────────",
			"    ID        TITLE                                              TAGS              PINNED",
			"▌   01abc234  Budget review                                      [work]                  █",
			"    02def567  Meeting notes                                      [work, q3]        pinned│",
			" notes",
			"",
		},
	},
	{
		name:  "a 60-column terminal drops the hints that do not fit and the columns that cannot",
		width: 60, height: 20,
		keys: nil,
		screen: []string{
			"Notebook: default          <enter>  read                noda",
			"Branch:   main             </>  filter",
			"Remote:   none             <:>  command",
			"Notes:    4 notes  1 file  <ctrl-a>  commands",
			"Changes:  none             <?>  keys",
			"Notes  all  4 ──────────────────────────────────────────────",
			"    ID        TITLE                TAGS              PINNED",
			"▌   01abc234  Budget review        [work]",
			"    02def567  Meeting notes        [work, q3]        pinned",
			"    03ghj890  Reading list",
			"    04kmn123  日 本 語 メ モ            [日 本 語 , has spa",
			"",
			"",
			"",
			"",
			"",
			"",
			"",
			" notes",
			"",
		},
	},
	{
		name:  "an open note on a 60x20 terminal keeps all its lines",
		width: 60, height: 20,
		keys: []string{"Enter"},
		screen: []string{
			"Notebook: default          <esc>  back                  noda",
			"Branch:   main             <j/k>  scroll",
			"Remote:   none             <:>  command",
			"Notes:    4 notes  1 file  <ctrl-a>  commands",
			"Changes:  none             <?>  keys",
			"Note  01abc234  Budget review ──────────────────────────────",
			"  ---",
			"  title: Budget review",
			"  tags: [work]",
			"  created: 2024-01-10T09:00:00Z",
			"  updated: 2024-03-01T10:00:00Z",
			"  ---",
			"",
			"  the q3 budget is late",
			"",
			"  - [ ] pay the bill due:2024-01-02",
			"  - [ ] plan ahead due:2099-01-01",
			"  - [x] done already",
			" notes   01abc234",
			"",
		},
	},
	{
		name:  "a wide-character note on a 40x12 terminal scrolls with a bar and a cut right edge",
		width: 40, height: 12,
		keys: []string{"G", "Enter"},
		screen: []string{
			"default  (main)  4 notes",
			"Note  04kmn123  日 本 語 メ モ  ─────────────",
			"  ---                                  █",
			"  title: 日 本 語 メ モ                     █",
			"  tags: [日 本 語 , has space]            █",
			"  created: 2024-01-01T00:00:00Z        █",
			"  updated: 2024-01-02T00:00:00Z        █",
			"  ---                                  █",
			"                                       █",
			"  本 文 は こ こ に あ り ま す 。                │",
			" notes   04kmn123",
			"",
		},
	},
	{
		name:  "the commands card holds what the terminal holds and counts the rest",
		width: 60, height: 12,
		keys: []string{"C-a"},
		screen: []string{
			"╭ commands ────────────────────────────────────────────────╮",
			"│open <note>                 open a note by id or by slug  │",
			"│notes [query...]            back to the listing, filtered…│",
			"│todo                        every unticked box, soonest d…│",
			"│tags                        every tag; enter filters the… │",
			"│backlinks [note]            what links to a note — the on…│",
			"│log [note]                  commits, newest first: the no…│",
			"│blame [note]                which commit put each line of…│",
			"│diff                        what is uncommitted, or what… │",
			"│                                                          │",
			"│enter  put it on the prompt       esc  back       22 more │",
			"╰──────────────────────────────────────────────────────────╯",
		},
	},
	{
		name:  "the commands card scrolls to keep the chosen command in view",
		width: 60, height: 12,
		keys: append([]string{"C-a"}, slices.Repeat([]string{"Down"}, 17)...),
		screen: []string{
			"╭ commands ────────────────────────────────────────────────╮",
			"│notebooks                   every notebook; enter moves t…│",
			"│edit [note]                 open in $EDITOR: the one name…│",
			"│add [title...]              make a note; no title takes i…│",
			"│mv <title...>               retitle the note on screen; t…│",
			"│tag [note] +tag -tag...     add and remove tags: +work -q…│",
			"│pin [note]                  float a note to the top of ev…│",
			"│unpin [note]                let a pinned note back down a…│",
			"│rm                          delete the note on screen, af…│",
			"│                                                          │",
			"│enter  put it on the prompt       esc  back       12 more │",
			"╰──────────────────────────────────────────────────────────╯",
		},
	},
	{
		name:  "a typed tag the notebook lacks is offered with the near miss beside it",
		width: 70, height: 14,
		keys: []string{"#", "type:wor"},
		screen: []string{
			"default  (main)  4 notes",
			"Notes  all  4 ────────────────────────────────────────────────────────",
			"    ID        TITLE                          TAGS              PINNED",
			"▌   01abc234  Budget review                  [work]",
			"  ╭ tags: budget-review / wor ─────────────────────────────────────╮d",
			"  │[x] work  2 notes                                               │",
			"  │[ ] wor   new — close to work, 2 notes                          │",
			"  │                                                                │",
			"  │type to narrow      tab  choose      enter  apply      esc  back│",
			"  ╰────────────────────────────────────────────────────────────────╯",
			"",
			"",
			" notes",
			"",
		},
	},
	{
		name:  "keeping updated and a mark share the title band, apart by two blanks",
		width: 60, height: 12,
		keys: []string{"T", "Space"},
		screen: []string{
			"default  (main)  4 notes",
			"Notes  all  4 ────────────────────── keeping updated  1 mark",
			"    ID        TITLE                TAGS              PINNED",
			"▌ • 01abc234  Budget review        [work]",
			"    02def567  Meeting notes        [work, q3]        pinned",
			"    03ghj890  Reading list",
			"    04kmn123  日 本 語 メ モ            [日 本 語 , has spa",
			"",
			"",
			"",
			" notes",
			"",
		},
	},
	{
		name:  "a listing that just fits has no scrollbar",
		width: 60, height: 9,
		keys: nil,
		screen: []string{
			"default  (main)  4 notes",
			"Notes  all  4 ──────────────────────────────────────────────",
			"    ID        TITLE                TAGS              PINNED",
			"▌   01abc234  Budget review        [work]",
			"    02def567  Meeting notes        [work, q3]        pinned",
			"    03ghj890  Reading list",
			"    04kmn123  日 本 語 メ モ            [日 本 語 , has spa",
			" notes",
			"",
		},
	},
	{
		name:   "the slug column is yellow and dim under the cursor bar",
		keys:   []string{"C-w"},
		styles: []styleWant{{7, "budget-review", ui.Yellow, ui.Bold | ui.Dim}},
	},
	{
		name:   "a word of the filter is picked out of the title it matched",
		keys:   []string{"/", "type:budget"},
		styles: []styleWant{{7, "Budget", ui.Yellow, ui.Bold}, {7, " review", ui.Reset, ui.Bold}},
		cursor: at(7, 29),
	},
	{
		name:   "the crumb for the screen on show is reversed and the ones behind it dim",
		keys:   []string{"Enter"},
		styles: []styleWant{{28, " notes", ui.Reset, ui.Dim}, {28, "01abc234", ui.Reset, ui.Reversed}},
	},
	{
		name:   "a mark count is yellow and bold at the right end of the title band",
		keys:   []string{"Space"},
		styles: []styleWant{{5, "1 mark", ui.Yellow, ui.Bold}},
	},
	{
		name:   "a pinned note says so in magenta and the row off the cursor is not bold",
		styles: []styleWant{{8, "pinned", ui.Magenta, 0}, {8, "02def567", ui.Yellow, 0}},
	},
	{
		name:   "the near miss beside a proposed tag is yellow and bold",
		keys:   []string{"#", "type:wor"},
		styles: []styleWant{{14, "close to work, 2 notes", ui.Yellow, ui.Bold}},
	},
	{
		name:  "a command card one row short counts the one it cut",
		width: 70, height: 6,
		keys: []string{"C-a", "type:remote"},
		screen: []string{
			"defaul╭ commands: remote ──────────────────────────────────────╮",
			"Notes │status  notes, changes, and drift from the remote       │──────",
			"    ID│push    send this notebook's commits to the remote      │INNED",
			"▌   01│                                                        │     │",
			" notes│enter  put it on the prompt       esc  back       1 more│",
			"      ╰────────────────────────────────────────────────────────╯",
		},
	},
	{
		name:  "a command card that just fits has no count of the rest",
		width: 70, height: 7,
		keys: []string{"C-a", "type:remote"},
		screen: []string{
			"de╭ commands: remote ──────────────────────────────────────────────╮",
			"No│status  notes, changes, and drift from the remote               │──",
			"  │push    send this notebook's commits to the remote              │D",
			"▌ │pull    bring in the remote's commits                           │ █",
			"  │                                                                │d│",
			" n│type to narrow       enter  put it on the prompt       esc  back│",
			"  ╰────────────────────────────────────────────────────────────────╯",
		},
	},
	{
		name:   "the chosen command is reversed and the others are not",
		width:  60,
		height: 12,
		keys:   []string{"C-a"},
		styles: []styleWant{{1, "open <note>", ui.Reset, ui.Reversed}, {2, "notes [query...]", ui.Yellow, 0}},
	},
	{
		name:   "the tag under the cursor is reversed and a proposed one is plain",
		width:  70,
		height: 14,
		keys:   []string{"#", "type:wor"},
		styles: []styleWant{{5, "work", ui.Reset, ui.Reversed}, {6, "wor ", ui.Cyan, 0}},
	},
	{
		name:  "a proposed tag with no near miss says only that it is new",
		keys:  []string{"#", "type:zzz"},
		has:   []string{"[ ] zzz  new"},
		lacks: []string{"close to"},
	},
	{
		name:  "ctrl-w adds the slug and created columns",
		keys:  []string{"C-w"},
		has:   []string{"wide", "SLUG           CREATED", "2024-02-01T10:00:00+08:00", "budget-review  2024-01-10T09:00:00Z"},
		lacks: []string{"UPDATED"},
	},
	{
		name: "j moves the bar down one row",
		keys: []string{"j"},
		has:  []string{"▌   02def567  Meeting notes"},
	},
	{
		name:  "k after j puts the bar back on the first row",
		keys:  []string{"j", "k"},
		has:   []string{"▌   01abc234"},
		lacks: []string{"▌   02def567"},
	},
	{
		name:        "G goes to the last row and g to the first",
		keys:        []string{"G"},
		has:         []string{"    01abc234  Budget review"},
		hasSqueezed: []string{"▌04kmn123日本語メモ"},
	},
	{
		name: "g after G is back on the first row",
		keys: []string{"G", "g"},
		has:  []string{"▌   01abc234"},
	},
	{
		name:        "ctrl-f on a short list lands on the last row",
		keys:        []string{"C-f"},
		hasSqueezed: []string{"▌04kmn123日本語メモ"},
	},
	{
		name:  "enter opens the note under the cursor and shows it whole",
		keys:  []string{"Enter"},
		has:   []string{"Note  01abc234  Budget review", "  title: Budget review", "  - [x] done already", " notes   01abc234", "<esc>  back"},
		lacks: []string{"PINNED"},
	},
	{
		name: "enter on the second row opens the second note",
		keys: []string{"j", "Enter"},
		has:  []string{"Note  02def567  Meeting notes", "  pinned: true", "  see [the budget](01abc234-budget-review.md)"},
	},
	{
		name:        "a note with wide characters opens with its body intact",
		keys:        []string{"G", "Enter"},
		hasSqueezed: []string{"Note04kmn123日本語メモ", "本文はここにあります。", "-[]日本語の用事due:2024-05-06"},
	},
	{
		name:  "escape on an open note comes back to the listing",
		keys:  []string{"Enter", "Esc"},
		has:   []string{"Notes  all  4", "▌   01abc234"},
		lacks: []string{"Note  01abc234"},
	},
	{
		name:   "typing a filter narrows the listing and leaves the cursor on the prompt",
		keys:   []string{"/", "type:tag:q3"},
		rows:   map[int]string{29: "/tag:q3"},
		has:    []string{"Notes  tag:q3  1", "02def567  Meeting notes"},
		lacks:  []string{"Budget review"},
		cursor: at(7, 29),
	},
	{
		name:   "enter keeps the filter and takes the cursor off the prompt",
		keys:   []string{"/", "type:tag:q3", "Enter"},
		rows:   map[int]string{29: "/tag:q3"},
		has:    []string{"Notes  tag:q3  1"},
		cursor: nil,
	},
	{
		name:   "words in a filter match the text of a note",
		keys:   []string{"/", "type:budget"},
		has:    []string{"Notes  budget  2", "Budget review", "Meeting notes"},
		lacks:  []string{"Reading list"},
		cursor: at(7, 29),
	},
	{
		name: "enter on the prompt then enter again opens the first match",
		keys: []string{"/", "type:budget", "Enter", "Enter"},
		has:  []string{"Note  01abc234  Budget review", "/budget"},
	},
	{
		name:   "an unfinished query says why and shows nothing",
		keys:   []string{"/", "type:tag:x OR"},
		has:    []string{"nothing matches", "`OR` needs a term on both sides"},
		cursor: at(9, 29),
	},
	{
		name:        "a quoted tag with a space finds the note that has it",
		keys:        []string{"/", "type:tag:\"has space\""},
		has:         []string{"Notes  tag:\"has space\"  1"},
		hasSqueezed: []string{"04kmn123日本語メモ"},
		lacks:       []string{"Budget review"},
		cursor:      at(16, 29),
	},
	{
		name: "escape on the prompt drops the filter",
		keys: []string{"/", "type:budget", "Esc"},
		has:  []string{"Notes  all  4", "Reading list"},
	},
	{
		name:   "a query nothing matches says so",
		keys:   []string{"/", "type:zzzzzz"},
		rows:   map[int]string{6: "  nothing matches", 29: "/zzzzzz"},
		cursor: at(7, 29),
	},
	{
		name:   "ctrl-a ctrl-k ctrl-y cut a whole line and paste it back after typing",
		keys:   []string{"/", "type:hello world", "C-a", "C-k", "type:x", "C-y"},
		rows:   map[int]string{29: "/xhello world"},
		cursor: at(13, 29),
	},
	{
		name:   "ctrl-w cuts the word before the cursor",
		keys:   []string{"/", "type:one two three", "C-w", "type:X"},
		rows:   map[int]string{29: "/one two X"},
		cursor: at(10, 29),
	},
	{
		name:   "alt-b moves the cursor back a word at a time",
		keys:   []string{"/", "type:one two three", "A-b", "A-b", "type:Y"},
		rows:   map[int]string{29: "/one Ytwo three"},
		cursor: at(6, 29),
	},
	{
		name:   "the colon opens an empty command prompt",
		keys:   []string{":"},
		rows:   map[int]string{29: ":"},
		cursor: at(1, 29),
	},
	{
		name: "a command that names nothing is refused on the status line",
		keys: []string{":", "type:nonsense", "Enter"},
		rows: map[int]string{29: "no command `nonsense` — Ctrl-a lists them"},
	},
	{
		name:   "the pin command stays on the prompt until it is sent",
		keys:   []string{":", "type:pin"},
		rows:   map[int]string{29: ":pin"},
		cursor: at(4, 29),
	},
	{
		name:        "the todo command lists boxes by date, overdue first, wide characters included",
		keys:        []string{":", "type:todo", "Enter"},
		has:         []string{"Todo  3", "ID        SLUG           DUE         TASK", "▌ 01abc234  budget-review  2024-01-02  pay the bill", "01abc234  budget-review  2099-01-01  plan ahead", " notes   todo"},
		hasSqueezed: []string{"04kmn123日本語メモ2024-05-06日本語の用事"},
	},
	{
		name: "t is the todo command on one key, and enter opens the note of the box",
		keys: []string{"t", "Enter"},
		has:  []string{"Note  01abc234  Budget review", " notes   todo   01abc234"},
	},
	{
		name:        "the tags command numbers every tag with how many notes carry it",
		keys:        []string{":", "type:tags", "Enter"},
		has:         []string{"Tags  4", "▌ 1  work       2 notes", "2  has space  1 note", "3  q3         1 note"},
		hasSqueezed: []string{"4日本語1note"},
	},
	{
		name: "enter on a tag narrows the listing to it",
		keys: []string{":", "type:tags", "Enter", "Enter"},
		has:  []string{"Notes  tag:work  2", "/tag:work"},
	},
	{
		name: "the files command lists what is not a note, and enter asks what links to it",
		keys: []string{":", "type:files", "Enter", "Enter"},
		has:  []string{"Backlinks  diagram.png  0", "nothing links here", " notes   files   backlinks"},
	},
	{
		name: "the files command lists the attachment",
		keys: []string{":", "type:files", "Enter"},
		has:  []string{"Files  1", "▌ diagram.png"},
	},
	{
		name: "the notebooks command marks the one in use",
		keys: []string{":", "type:notebooks", "Enter"},
		has:  []string{"Notebooks  default  1", "▌ • default", "<enter>  switch to it"},
	},
	{
		name: "the deleted command on a notebook with no deletions says so",
		keys: []string{":", "type:deleted", "Enter"},
		has:  []string{"Deleted  0", "nothing has been deleted"},
	},
	{
		name: "the keys command draws a card over the listing",
		keys: []string{":", "type:keys", "Enter"},
		has:  []string{"╭ keys ", "j / k, ↓ / ↑  move · scroll", "readline: ctrl-a/e/w/u/k/y, alt-b/f"},
	},
	{
		name: "the question mark draws the same card",
		keys: []string{"?"},
		has:  []string{"╭ keys ", "S, R, ctrl-w, 1-9  sort · reverse · wide row · a tag (0 = all)"},
	},
	{
		name: "ctrl-a lists every command",
		keys: []string{"C-a"},
		has:  []string{"backlinks [note]", "deleted", "notebooks"},
	},
	{
		name: "typing narrows the command card",
		keys: []string{"C-a", "type:remote"},
		has:  []string{"commands: remote"},
	},
	{
		name: "a command card nothing matches says so",
		keys: []string{"C-a", "type:zzzz"},
		has:  []string{"commands: zzzz", "nothing goes by that", "enter  put it on the prompt"},
	},
	{
		name: "the tags card boxes each tag, ticks the ones the note has, and tab chooses",
		keys: []string{"#", "Tab", "Tab"},
		has:  []string{"tags: budget-review", "[x] work       2 notes", "[ ] has space  1 note", "[ ] q3         1 note", "tab  choose"},
	},
	{
		name: "typing narrows the tags card",
		keys: []string{"#", "type:wor"},
		has:  []string{"tags: budget-review", "work"},
	},
	{
		name:  "a digit narrows to that tag and 0 clears it",
		keys:  []string{"1"},
		has:   []string{"Notes  tag:work  2", "/tag:work"},
		lacks: []string{"Reading list"},
	},
	{
		name: "zero after a digit clears the tag again",
		keys: []string{"1", "0"},
		has:  []string{"Notes  all  4", "Reading list"},
	},
	{
		name: "b on a note lists what links to it, and on the listing the note under the cursor",
		keys: []string{"b"},
		has:  []string{"Backlinks  01abc234  1", "▌ 02def567  Meeting notes"},
	},
	{
		name: "b on a note nothing links to says so",
		keys: []string{"j", "Enter", "b"},
		has:  []string{"Backlinks  02def567  0", "nothing links here", " notes   02def567   backlinks"},
	},
	{
		name: "space marks the row under the cursor and counts it",
		keys: []string{"Space"},
		has:  []string{"1 mark", "▌ • 01abc234", "    02def567"},
	},
	{
		name:  "a second space on the same row unmarks it",
		keys:  []string{"Space", "Space"},
		lacks: []string{"•"},
	},
	{
		name: "star marks every row shown",
		keys: []string{"*"},
		has:  []string{"4 marks", "▌ • 01abc234", "  • 02def567", "  • 03ghj890"},
	},
	{
		name:  "star twice unmarks them all",
		keys:  []string{"*", "*"},
		lacks: []string{"•"},
	},
	{
		name:  "escape drops the marks",
		keys:  []string{"Space", "Esc"},
		lacks: []string{"•"},
	},
	{
		name: "Q with nothing queued says so on a card",
		keys: []string{"Q"},
		has:  []string{"╭ queued ", "nothing queued — mark some notes, then # or ctrl-d"},
	},
	{
		name: "S sorts by created, putting the pinned note first",
		keys: []string{"S"},
		has:  []string{"by created", "    02def567  Meeting notes", "▌   01abc234  Budget review"},
	},
	{
		name: "a second S sorts by updated",
		keys: []string{"S", "S"},
		has:  []string{"by updated"},
	},
	{
		name: "R reverses the order and the cursor stays on its note",
		keys: []string{"R"},
		has:  []string{"by slug reversed", "▌   01abc234  Budget review"},
	},
	{
		name: "T says the note is kept as it is",
		keys: []string{"T"},
		has:  []string{"keeping updated"},
	},
	{
		name: "ctrl-g hides the crumb row",
		keys: []string{"C-g"},
		rows: map[int]string{28: "", 29: ""},
	},
	{
		name:  "ctrl-g leaves the crumbs off on a note too",
		keys:  []string{"C-g", "Enter"},
		has:   []string{"Note  01abc234  Budget review"},
		lacks: []string{" notes   01abc234"},
	},
	{
		name:   "m asks for a new title with the old one in the field",
		keys:   []string{"m"},
		rows:   map[int]string{29: "retitle  Budget review"},
		cursor: at(22, 29),
	},
	{
		name:   "a asks for a title and says what an empty one does",
		keys:   []string{"a"},
		has:    []string{"new note", "Enter alone takes the title from the body"},
		cursor: at(10, 29),
	},
	{
		name: "ctrl-d asks before a delete and says it can be reverted",
		keys: []string{"C-d"},
		has:  []string{"╭ delete this note? ", "01abc234  Budget review", "the commit that removes it stays, so git revert brings it back", "y  delete       any other key  keep it"},
	},
	{
		name: "r reloads and leaves the listing as it was",
		keys: []string{"r"},
		has:  []string{"Notes  all  4", "▌   01abc234"},
	},
}

func TestKeySequencesDrawTheScreen(t *testing.T) {
	env := casesNotebook(t)
	for _, c := range screenCases {
		t.Run(c.name, func(t *testing.T) { checkScreenCase(t, env, c) })
	}
}

func checkScreenCase(t *testing.T, env cmd.Env, c screenCase) {
	t.Helper()
	width, height := c.width, c.height
	if width == 0 {
		width, height = 100, 30
	}
	app := load(t, env)
	for _, token := range c.keys {
		pressToken(t, app, token)
	}
	frame := drawAt(env, app, width, height)
	rows := screenAt(env, app, width, height)
	screenText := strings.Join(rows, "\n")
	squeezed := strings.ReplaceAll(screenText, " ", "")
	if c.screen != nil && !slices.Equal(rows, c.screen) {
		t.Errorf("screen\n got %q\nwant %q", rows, c.screen)
	}
	for y, want := range c.rows {
		if rows[y] != want {
			t.Errorf("row %d\n got %q\nwant %q", y, rows[y], want)
		}
	}
	for _, want := range c.has {
		if !strings.Contains(screenText, want) {
			t.Errorf("no %q on\n%s", want, screenText)
		}
	}
	for _, want := range c.hasSqueezed {
		if !strings.Contains(squeezed, strings.ReplaceAll(want, " ", "")) {
			t.Errorf("no %q (spaces dropped) on\n%s", want, screenText)
		}
	}
	for _, unwanted := range c.lacks {
		if strings.Contains(screenText, unwanted) {
			t.Errorf("%q should not be on\n%s", unwanted, screenText)
		}
	}
	for _, want := range c.styles {
		checkStyle(t, frame.Buffer, rows[want.row], want)
	}
	checkCursor(t, frame.Cursor, c.cursor)
}

func checkStyle(t *testing.T, buffer *ui.Buffer, row string, want styleWant) {
	t.Helper()
	index := strings.Index(row, want.text)
	if index < 0 {
		t.Errorf("no %q on row %d: %q", want.text, want.row, row)
		return
	}
	start := utf8.RuneCountInString(row[:index])
	for x := start; x < start+utf8.RuneCountInString(want.text); x++ {
		cell := buffer.At(x, want.row)
		if cell.Fg != want.fg || cell.Modifier != want.mod {
			t.Errorf("%q on row %d, column %d is fg %v mod %v, want fg %v mod %v", want.text, want.row, x, cell.Fg, cell.Modifier, want.fg, want.mod)
			return
		}
	}
}

func checkCursor(t *testing.T, got *ui.Point, want *[2]int) {
	t.Helper()
	if want == nil {
		if got != nil {
			t.Errorf("cursor at %d,%d, want none", got.X, got.Y)
		}
		return
	}
	if got == nil || got.X != want[0] || got.Y != want[1] {
		t.Errorf("cursor %v, want %v", got, *want)
	}
}

func TestReversingKeepsTheCursorOnTheNoteNotTheRow(t *testing.T) {
	env := threeNotes(t)
	app := load(t, env)
	before := app.Selected().ID

	app.OnKey(Rune('R'))

	if got := app.Selected().ID; got != before {
		t.Errorf("cursor moved to %s, want it kept on %s", got, before)
	}
}

func TestAnAskNamesItsPromptAndHint(t *testing.T) {
	for _, c := range []struct {
		name         string
		ask          Ask
		prompt, hint string
	}{
		{"new note", AskTitle, "new note", "Enter alone takes the title from the body"},
		{"retitle", AskRetitle, "retitle", ""},
	} {
		t.Run(c.name, func(t *testing.T) {
			if got := c.ask.Prompt(); got != c.prompt {
				t.Errorf("prompt %q, want %q", got, c.prompt)
			}
			if got := c.ask.Hint(); got != c.hint {
				t.Errorf("hint %q, want %q", got, c.hint)
			}
		})
	}
}
