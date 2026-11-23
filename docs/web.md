# In a browser

*One of noda's three interfaces — see the [README](../README.md) for the rest.*


`noda web` serves the notebooks over HTTP, so a phone can read them. It renders on the
server and works with JavaScript turned off: the search box is a form, every row is a link.
A script is laid over that on a few screens and nothing depends on it — see [the last
section](#and-a-script-on-top-that-nothing-depends-on).

```
$ noda web
noda is at http://127.0.0.1:8080
```

## Logging

The log goes to **stderr**, so the address stays on stdout where noda puts a command's answer.

`NODA_LOG_LEVEL` decides what is on it: `debug`, `info`, `warn` or `error`, default `info`. It is an
environment variable only; there is no flag. At the default nothing is logged per request: only
refusals, failures, and requests slower than a second. `NODA_LOG_LEVEL=debug` turns on a line per
request. `--log-format json` (or `NODA_LOG_FORMAT=json`) writes one JSON object per line. An
unknown level is an error at startup rather than a silent default.

The Rust releases' log variable is retired; noda refuses to start while it is set (see the README).

```
$ NODA_LOG_LEVEL=debug noda web
noda is at http://127.0.0.1:8080
… level=DEBUG msg="request finished" event=http.request method=GET
  route="/nb/{book}/n/{key}" status=200 elapsed=1.78ms elapsed_ms=1.785
```

**A request is logged as the route it matched, never as the address it asked for** — an address
carries a note's id, a filename or a search query, none of which belongs in a log collector. A
request that matched no route is `<unmatched>`.

The one event worth an alert is `http.refused`, at WARN: the guard turned a request away, either
because a reverse proxy's name has not been allowed yet or because somebody tried DNS rebinding.
It carries the `Host` and `Origin` it decided on.

## Addresses

Every notebook is named in the URL — `/nb/work` for the listing, `/nb/work/n/k3f9m2p1` for a
note — so the terminal's active-notebook pointer never changes what a tab shows. A note's slug
and any prefix of its id redirect to the id, so a bookmark survives a retitle.

**The front page lists the notebooks**, without the rail. A row is `noda status` in a few
words: the name (a link to the notebook), what it holds, the day it was last committed to, and
its standing against the remote (a link to its network screen; plain words when there is no
remote). The notebook your terminal is pointed at ends its facts with `active`, as in
`noda notebook ls` — the only place the pointer is read.

**A notebook and a note each have a colour**, a stripe down the row and a dot in a link. It is
`style.Hue` of the notebook's name or the note's id, one of twelve, so it is nothing that is
stored: the same note is the same colour in the listing, in a link from another note and in the
margin, on every machine, for as long as its id lives. `web/theme.go` writes the twelve, for
light and dark both; colour is never the only thing that says which is which.

## Security

**There is no password on it, and there is not going to be one.** It is meant to be reached
over a tailnet or from behind something that already authenticates. So:

- It listens on **this machine only** (`127.0.0.1:8080`) until told otherwise.
  `--listen 0.0.0.0:8080` opens it to the network, and says so on the way up.
- It refuses a request whose `Origin` is another site. With no session to check, that is what
  stops another site's page committing to your notebook through your browser.
- It answers to **addresses and `localhost` freely, and to a hostname only when told**. A DNS
  rebinding attack makes `Origin` and `Host` agree, but it needs a *name*, so a name has to be
  asked for:

```sh
noda web --listen 0.0.0.0:8080 --allow-host noda.tail1234.ts.net
```

  Behind a reverse proxy, name whatever the browser shows in the address bar. The refusal says
  exactly what to add.

## `/health`

**The one address outside the guard**, because a probe sends whatever `Host` its runner picked,
and failing it for a missing `--allow-host` would restart a healthy server. It answers `200`
and `ok`, opens no notebook and echoes nothing.

```
$ curl -i http://127.0.0.1:8080/health
HTTP/1.1 200 OK
content-type: text/plain; charset=utf-8
cache-control: no-store
x-content-type-options: nosniff

ok
```

It checks what a restart can mend: it goes through the blocking pool every page uses, so a
wedged pool fails it. It does not open a notebook, because one broken repository would
otherwise become a container restarting forever.

```yaml
livenessProbe:
  httpGet: { path: /health, port: 8080 }
```

The image holds git and the OpenSSH client but no `curl` or `wget`, so a Dockerfile `HEALTHCHECK`
or Compose `test:` has nothing to make the request with. Probes from outside — Kubernetes' `httpGet`, a
load balancer, an uptime monitor — work as written. It is logged like any request: only under
`NODA_LOG_LEVEL=debug`, as `route="/health"`.

## Writing

A note can be started, rewritten, retitled, tagged, pinned and deleted. Each runs the same
`add`, `mv`, `tag`, `pin` or `rm` the terminal calls, and lands as its own commit with the same
message.

**A note's bar is Edit, Tags, Rename, Pin and Delete.** Delete is the only coloured item on any
bar, and leads to a confirmation page that offers to keep the note first, so a mis-tap costs a
page and never a note.

**Pin is the one write with no page in between**, and the only item that is a button rather
than a link: a write has to be a `POST` for the `Origin` check to mean anything. It says `Pin`
or `Unpin`, and the two are separate addresses, so a retried `POST` lands where you asked. A
pinned row on the listing says `pinned` after its tags. With a script the label flips at
once, to the word the server will send next; if the server does not answer, the old word comes
back with a line saying so.

**The tags form** ticks off what should go and takes new tags in one field, space-separated and
quoted the way the `:` prompt and the search box quote (`ops docs "24.04 Dark patterns"` adds
three); the change is one commit. It removes only what you unticked: the form carries the tags
it showed you, so a tag added while the page was open is left alone.

**An edit onto a note that changed underneath is merged, not overwritten.** The form carries
the file's git blob id from when the page was drawn; if the file has moved since, your text is
merged against that version. Edits to different parts both land silently. Where you both
changed the same lines, the page comes back with both versions in git's `<<<<<<<` markers in one
box; nothing is written until you save it. A blob id rather than `updated`, because `--no-touch`
can change a note without moving its stamp, and because a blob id can fetch the base version
back. A hand-written note that was never committed has no blob, so it comes back as two panes to
reconcile yourself.

**With JavaScript on, an open editor is told while you type.** The form holds a connection open
and puts a line above the box the moment the note moves; saving still merges. It watches the
file, not the server's own writes, so a `noda edit`, a hand edit or a `sync` all count — at the
cost of hashing one file every two seconds, and up to that long before you hear. The connection
closes when you leave the page, and all of them close when the server stops. With JavaScript
off nothing appears and Save behaves the same.

## Reading a note

**What is about the note, not in it, is its margin.** The bar, the tags (each a link to the
listing narrowed to it), when it was updated and created, and **Linked from**: the notes that
point at it, each in its own colour. Below 1440px the margin follows the text; from there it
stands to the side and stays in view as the note scrolls. The backlinks come with the note, so
the margin is never empty on first sight, at the cost of reading every note once for each note
opened. `/nb/<book>/n/<id>/backlinks` is still an address and still answers, but nothing on a
note links to it.

**A ticked item is struck through and dimmed**, and an item nested under it says its own state:
an unticked one is not struck. A line through an item runs through everything inside it, so the
nested list is drawn as a box the line does not enter. A link in a ticked item is struck but not
dimmed, since it may be what you came to read.

**Links go where they went on disk.** A relative link to another note —
`[the plan](k3f9m2p1-the-plan.md)` — opens that note's page and is coloured like an id, so a link
that stays in the notebook looks different from one that leaves. A link to any other file in the
notebook is a download; an image is shown in place.

**A bare `http://` or `https://` address is a link too**, found on GFM's rules: trailing
sentence punctuation is not part of it, and a `)` is only when the address opened one.
`www.example.com` is not linked, since that means guessing a scheme. An address inside code,
inside a link's own text, or inside markup `noda import` could not convert is left as text.

**Nothing a note points at learns where it was pointed from**, because the address holds a note
id. A link that leaves opens in a new tab with `rel="noopener noreferrer"`, and every HTML page
sends `Referrer-Policy: same-origin` both as a header and as a `<meta>` in its `<head>`. The
meta survives a proxy that strips the header and also covers embedded images; `noopener` stops
the new tab reaching back through `window.opener`. The policy is `same-origin` and not
`no-referrer` because `no-referrer` also makes forms post `Origin: null`, which the `Origin`
check refuses — every write from a browser would be turned away.

**Two things a note's body cannot do.** Raw HTML is shown as a code block rather than rendered,
since it may be the only copy of markup `noda import tiddlywiki` could not convert. A link whose
scheme noda does not serve — `javascript:` first among them — keeps its words and loses its link.
Files are served with `nosniff` and a content policy that loads nothing, and only formats that
cannot carry a script are shown in place: SVG arrives as a download.

## The listing

**Under the search field are four order chips** — `slug` (the default), `created`, `updated`,
`title` — with the one in force marked by an arrow. Pressing another chip switches to it;
pressing the one in force reverses it, like `-r`. Switching order drops the reversal, since each
order has a direction it means first. They are plain links, so they work without scripts. The
order rides in the address (`/nb/work?sort=updated`, `&r=1` for reversed), and the default writes
nothing. The search form carries the order in hidden inputs, and the chips carry the query.

**Under the search field, how it grouped your query**, at every width: `tag:work OR tag:q3
budget` draws as two pills, `tag:work or tag:q3` and `budget`, joined by `and`.

**A row's day is the stamp the listing is ordered by**: `created` in `created` order, `updated`
otherwise.

**Rows come in runs under a heading.** Pinned notes are a run of their own, first. When the
order is `created` or `updated`, the rest are cut a month at a time (`November 2026`), and a note
with no usable stamp is under `Undated`. In any other order there is nothing to cut by, and the
heading `Notes` says that on purpose. The month is the one in the **server's** zone (`TZ`), as
`noda todo` judges today, so a heading and the same note in a terminal agree; with scripts on
the day on a row is restated in the reader's zone, so a note near midnight may sit under a
month its row's day does not name. A heading changes how the list is drawn and never what it
holds, so a reversed listing, which puts the pinned notes last, reads right too.

## Other screens

On the rail, which is a bar along the bottom of a phone:

- `/nb/<book>/files` — everything that is not a note: size, type, and how many notes link to it.
  A file nothing links to is what `doctor --links` calls an orphan.
- `/nb/<book>/tags` — every tag with its count, commonest first, and a bar for its weight
  against the commonest. Pressing one narrows the listing to it, quoted if it has a space.
- `/nb/<book>/todo` — `noda todo`: every unticked box, in three runs (overdue, with the date in
  red; dated; and whenever), soonest due first inside each, every row linking to its note. There
  is no way to tick one, for the reason there is no `noda done`.

**Backlinks** are matched on the id in the destination, so a retitle does not silence them. A
note's come with its page, in the margin. A file has no page of its own, so the files page's
counts link to `/nb/<book>/f/<name>/backlinks`, which lists the notes that point at it.

**Times.** The page carries each stamp as the frontmatter spells it (`Z` or `+08:00` and all),
in a `<time>`. With scripts on, it is restated in the reader's zone — the one fact the server
cannot know: `Aug 15, 2026, 4:59 PM` on a note, `YYYY-MM-DD` on a listing row, always English.
Hovering shows the exact stamp. So a listing's day may differ by one from the scriptless page.
`due:` dates are calendar days and are never converted; the script converts `<time datetime>`
only, never anything by class.

## Wider screens

The same interface at a higher density, not a second design:

- **Under 640px** it is a phone: one screen at a time, the rail along the bottom as a dock
  with the new-note button above it. A note's own actions follow its text. A field's text is 16px
  so a phone does not zoom into it.
- **From 640px** the dock becomes a rail on the left, icons over their words, and a note's
  actions move to the head of the note.
- **From 1024px** the notes screen splits: listing beside the note, and picking a note replaces
  only the note. With no note picked, the notebook's `README.md` stands there.
- **From 1280px** the rail has words: the notebook's name with a way to all notebooks, the new
  note button as a button, and each screen named.
- **From 1440px** the margin stands to the side of the note instead of under it.
- **From 1800px** the listing and the margin are given more room.

**A control's hit area is at least 48 by 48px** under 640px, and at any width when the primary
pointer is a finger (`pointer:coarse`, an iPad in landscape for one). With a mouse on a wider
screen the controls are drawn tighter: a rail item is 40px from 1280px, a note's action 36px, an order chip
30px, a tag only as tall as its text, and the shortcut card's buttons 38px.

The listing beside a note is fetched by the script, not sent with the page, since a phone never
draws it. Without scripts a note on a wide screen is the note and the way back to the listing.

## The keyboard

With a script on, the keys below work. **A key only presses a control that is already on the
page**: the server marks a link, a button or a field with the key it answers to, and the script
clicks the visible one. So a key never does what the page cannot, a screen's keys are decided by
its markup, and without the script the control is the link or button it always was. `?` opens a
card of what the keys do *on this screen*, built from the page as it opens; where there is a
rail, its foot has a Shortcuts button for a pointer.

| Key | Does |
| --- | --- |
| `g` then `n` `t` `d` `f` `s` `h` | go to Notes, Tags, Todo, Files, Status, all notebooks |
| `n` · `/` | a new note · the search field |
| `o` · `O` | the next order · reverse this one |
| `e` `t` `r` `p` `D` | on a note: Edit, Tags, Rename, Pin or Unpin, Delete (Shift-d) |
| `j` `k` | the next row, the previous row; on a note, scroll |
| `J` `K` | with the listing beside the note: open the next note, the previous |
| `gg` · `G` | top · bottom |
| `Esc` | one step back: leave a field, clear the search, then go up a level |
| `Ctrl`/`⌘` `⏎` | send the form being written |

After `g`, a strip names what it is waiting for and lets go after two and a half seconds. A
form has no `Esc` for the way back, since it would take what was typed with it.

**Keys wait whenever they could be somebody's text.** Not while a field, a text area or a menu
has the focus, and not while an input method is composing (the keydown with `isComposing`, or
the `229` that Safari reports for the one that ends a composition). A chord with `Ctrl`, `⌘` or
`Alt` is the browser's. A key is the letter on the keycap, so a Dvorak or AZERTY layout follows
what it prints, and only a layout with no Latin letter on the key falls back to its position.

Single keys can be turned off from the card, and then `Esc`, `Ctrl ⏎` and `?` still answer. The
choice is kept in the browser's local storage; where storage is refused it costs the choice,
never the keys.

## Sync

The sync line at the foot of the rail, and the pill above a listing on a phone, show where the
notebook stands — `2 to push`, `in sync`, `never synced` — and open `/nb/<book>/status`: what
`noda status` prints, nothing fetched, with `Sync`, `Pull` and `Push` below. Its last row is the
version answering, as `noda --version` prints it, so a bug report can name the build. The same
version is in small type at the foot of the rail, on screens wide enough to have one.

Pressing starts the errand and answers at once; the screen checks back every two seconds until
it finishes (`<meta http-equiv="refresh">` without scripts, a fetch with them).

- **A reload cannot start it again** — only the `POST` does. Pressing again while one runs is
  not an error.
- **The output stays until the next one**: the commit, pull and push lines `sync` prints.

One errand per notebook at a time, under the same write lock a Save takes. The lock is per
notebook, so a slow remote holds up only its own notebook.

## Stopping

Ctrl-C or `SIGTERM` (what `docker stop` and `systemctl stop` send) closes the listener, answers
the requests in flight, then waits for a running sync:

```
$ noda web
noda is at http://127.0.0.1:8080
^CSIGINT — finishing what is in flight
waiting for sync in work — signal again to leave it unfinished
```

That wait has no deadline, because a sync killed halfway leaves git's `index.lock` behind and
the next write from anywhere fails on it. A second signal stops the *waiting*, not the errand,
and names what was left:

```
^CSIGINT again — not waiting
noda: left sync in work unfinished
```

A clean stop exits `0`; leaving an errand unfinished does not.

## What every page needs, and no page carries

The stylesheet and scripts are separate files at hashed addresses (`/a/style.<hash>.css`, one per
script), served `immutable` for a year; the pages naming them are `no-cache`. A first view costs
the same bytes as a self-contained page would, in up to six requests; every view after costs
the page alone — which matters because fragments cannot cover a note opened from a link, a form
page, or any screen on a phone.

| | first view | first, gzip | every view after | after, gzip |
| --- | ---: | ---: | ---: | ---: |
| the notebooks page | 46,928 | 14,240 | 891 | 480 |
| a listing | 76,532 | 24,319 | 5,632 | 1,827 |
| a note | 67,857 | 21,342 | 5,543 | 1,858 |
| the network screen | 52,545 | 16,377 | 3,235 | 1,272 |
| a backlinks page | 48,604 | 14,808 | 2,567 | 1,048 |
| an edit form | 55,497 | 17,787 | 1,469 | 812 |

Bytes, against a notebook of five notes. The order chips cost 574 bytes on a listing and nothing
on a note, which carries no rows to order.

**Every answer is gzipped.** A listing is rows differing by a few words, so at two hundred notes
an ordered listing falls from 61,781 bytes to 6,076. It was chosen by costing a sort switch:

| a sort switch, 200 notes | bytes | |
| --- | ---: | ---: |
| as it was | 61,781 | |
| a fragment narrower than the column | 59,360 | −3.9% |
| gzip | 6,076 | −90.2% |
| both | 5,108 | −91.7% |

Sending only the new order would have saved more still, at the cost of a new fragment vocabulary
and a client that rewrites rows, so the protocol stayed as it was. The script cannot reorder the
rows itself: a row lacks the slug and the second stamp, and the server (UTF-8) and JavaScript (UTF-16)
order titles differently above the BMP.

Not compressed: bodies under 32 bytes (`/health`'s `ok`), images, PDFs,
`application/octet-stream` (a notebook's zips and videos), and the edit form's event stream. A
`.json` attachment also falls to `octet-stream`, which costs one bigger download.

**A note's backlinks cost a scan of the notebook.** The margin is part of the note, so opening a
note reads every note and parses its Markdown, on a phone too, which never draws the margin. On a
notebook of 2,000 notes (server time, `curl`, the median of 40 runs on one laptop):

| | before the redesign | now |
| --- | ---: | ---: |
| a note, the whole page | 15.2 ms | 60.9 ms |
| a note, the `read` fragment (`j` and `k` beside the listing) | 2.3 ms | 47.1 ms |
| a listing | 41.0 ms | 41.3 ms |
| a save, the POST and the page it lands on | 64 ms | 116 ms |

So opening a note now costs what a listing costs: about 21 ms to read 2,000 files and 18 ms to
parse their Markdown, linear in the notebook (200 notes would be a tenth of it). A save was 176 ms
before the write asked only for the address it went to. Nothing is cached: a table of links would be
a second record of the notebook, kept right against every pull and every edit from a terminal.

## And a script on top, that nothing depends on

Every screen works without it. With it:

- the keys of [the keyboard](#the-keyboard);
- the listing filters as you type;
- the network screen fetches its own news instead of reloading whole;
- on a two-pane screen, picking a note, sending the search and pressing back replace the reading
  half instead of the page;
- a write is sent where it was made: the form goes by `fetch` and the answer lands on the page
  that is already there, so the scroll and the focus stay and a dropped connection leaves what
  was typed in the box;
- every stamp is restated in the reader's zone.

All but the last only remove a wait or a reload, and what arrives is the page the scriptless form
would have shown. The last adds a fact the server cannot know; without it the stamp is
unconverted, not wrong.

**Fetches ask for part of a page.** A fetch names the region it wants in an `x-noda-fragment`
header (`read`, `index`, `screen` or `news`), and the server sends that region from the same
function that builds the whole page: 5,543 bytes down to 2,238 for a note, 3,235 to 1,165 for a
network-screen poll, and 5,771 to 3,350 for a search from the listing (the address still gets the
query, so back works), against five notes. The search box in a note page's index column is left
alone, since ⏎ there means going to the listing. The part is a substring of the page (the page
tests assert it), and the whole page is always a correct answer: no header, or a name the server
does not know, gets the page, and every fetch picks the element it wants out of whatever arrives.
The one name that is not a region is `sent`, sent with a write: the answer is `204` and an
`x-noda-location` header instead of the 303, so the server does not build the page the write
lands on for a fetch to throw away and then build it again when the script goes there. Without
the header the 303 is followed, and the script goes to where it ended.

**The filter may be narrower than the server, never wider.** Every row is on the page, the
excluded ones `hidden`, so filtering is the server's operation on the same markup. It cannot read
bodies: when a query holds a bare word or `text:`, the listing says it filtered by title and tag, and ⏎
finishes the job — a title or tag hit is always a text hit too. A *negated* one would keep
rows the server drops, so there, and for a half-typed query, the filter stands aside until ⏎, and
the remark under the field says so ("Not filtered yet — press ⏎ to search.") rather than go on
claiming a filter that is not happening.
