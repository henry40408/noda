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

`RUST_LOG` decides what is on it. The default, `error,noda=info`, logs nothing per request:
only refusals, failures, and requests slower than a second. `RUST_LOG=noda=debug` turns on a
line per request; `RUST_LOG=noda::web::log=debug` gives the requests alone. `--log-format json`
(or `NODA_LOG_FORMAT=json`) writes one JSON object per line.

`RUST_LOG` is applied *on top of* the default, so a typo costs you the setting and not the log.
A bare level is the exception and replaces it: `RUST_LOG=off` is off.

```
$ RUST_LOG=noda=debug noda web
noda is at http://127.0.0.1:8080
… DEBUG noda::web::log: request finished event="http.request" method=GET
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

**The front page lists the notebooks**, without the rail or the bar. A row is `noda status` in
one line: the name (a link to the notebook), what it holds, the day it was last committed to,
and its standing against the remote (a link to its network screen; plain words when there is no
remote). The notebook your terminal is pointed at gets an uncoloured dot in the margin, as in
`noda notebook ls` — the only place the pointer is read. Below 1024px the day goes; on a phone
the file count goes too.

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

The image is distroless and holds only the binary, so a Dockerfile `HEALTHCHECK` or Compose
`test:` has nothing to make the request with. Probes from outside — Kubernetes' `httpGet`, a
load balancer, an uptime monitor — work as written. It is logged like any request: only under
`RUST_LOG=noda=debug`, as `route="/health"`.

## Writing

A note can be started, rewritten, retitled, tagged, pinned and deleted. Each runs the same
`add`, `mv`, `tag`, `pin` or `rm` the terminal calls, and lands as its own commit with the same
message.

**A note's bar is Edit, Tags, Rename, Links, Pin and Delete.** Delete is the only coloured item
on any bar, and leads to a confirmation page, so a mis-tap costs a page and never a note.

**Pin is the one write with no page in between**, and the only item that is a button rather
than a link: a write has to be a `POST` for the `Origin` check to mean anything. It says `Pin`
or `Unpin`, and the two are separate addresses, so a retried `POST` lands where you asked. A
pinned row on the listing says `pinned` after its tags.

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

**A row's day is the stamp the listing is ordered by**: `created` in `created` order, `updated`
otherwise.

## Other screens

On a bar along the bottom (a rail on wider screens):

- `/nb/<book>/files` — everything that is not a note: size, type, and how many notes link to it.
  A file nothing links to is what `doctor --links` calls an orphan.
- `/nb/<book>/tags` — every tag with its count, commonest first. Pressing one narrows the listing
  to it, quoted if it has a space.
- `/nb/<book>/todo` — `noda todo`: every unticked box, soonest due first, a passed date in red,
  each row linking to its note. There is no way to tick one, for the reason there is no
  `noda done`.

`/nb/<book>/n/<id>/backlinks`, reached from **Links**, lists what points at a note. Like the
files page's counts, it matches on the id in the destination, so a retitle does not silence it.
It is its own screen because it parses every note, where reading a note opens one file.

**Times.** The page carries each stamp as the frontmatter spells it (`Z` or `+08:00` and all),
in a `<time>`. With scripts on, it is restated in the reader's zone — the one fact the server
cannot know: `Aug 15, 2026, 4:59 PM` on a note, `YYYY-MM-DD` on a listing row, always English.
Hovering shows the exact stamp. So a listing's day may differ by one from the scriptless page.
`due:` dates are calendar days and are never converted; the script converts `<time datetime>`
only, never anything by class.

## Wider screens

The same interface at a higher density, not a second design:

- **Under 640px** it is a phone: one screen at a time, the bar along the bottom.
- **From 640px** the bar becomes a rail on the left, and a row's day and tags move to the right
  of the title. Rows gain the id column, in `noda ls -l` order (id, title, updated, tags), and
  the search field shows how it grouped your query: `tag:work OR tag:q3 budget` draws as
  `(tag:work or tag:q3) and (budget)`.
- **From 1024px** the notes screen splits: listing on the left, the note on the right, and
  picking a note replaces only the right half. With no note picked, the notebook's `README.md`
  stands there.
- **From 1440px** the note's backlinks sit in its margin.

The listing beside a note and the margin backlinks are fetched by the script, not sent with the
page. Without scripts a note on a wide screen is the note and the way back; the Links button
reaches the backlinks at every width.

## Sync

The corner of the listing's bar shows where the notebook stands — `2 to push`, `in sync`,
`never synced` — and opens `/nb/<book>/status`: what `noda status` prints, nothing fetched, with
`Sync`, `Pull` and `Push` below. Its last row is the version answering, as `noda --version`
prints it, so a bug report can name the build.

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
the same bytes as a self-contained page would, in up to four requests; every view after costs
the page alone — which matters because fragments cannot cover a note opened from a link, a form
page, or any screen on a phone.

| | first view | first, gzip | every view after | after, gzip |
| --- | ---: | ---: | ---: | ---: |
| the notebooks page | 40,503 | 12,934 | 905 | 445 |
| a listing | 75,194 | 25,250 | 7,138 | 2,531 |
| a note | 62,708 | 20,684 | 4,336 | 1,432 |
| the network screen | 43,416 | 14,050 | 2,509 | 925 |
| a backlinks page | 40,426 | 12,962 | 828 | 473 |
| an edit form | 40,663 | 13,095 | 1,065 | 606 |

Bytes, against a notebook of five notes. The order chips cost 701 bytes on a listing and nothing
on a note, which carries no rows to order.

**Every answer is gzipped.** A listing is rows differing by a few words, so at two hundred notes
an ordered listing falls from 74,163 bytes to 4,043. It was chosen by costing a sort switch:

| a sort switch, 200 notes | bytes | |
| --- | ---: | ---: |
| as it was | 74,163 | |
| a fragment narrower than the column | 73,164 | −1.3% |
| gzip | 4,043 | −94.6% |
| sending the order rather than the rows | 4,704 | −93.7% |
| both | 1,640 | −97.8% |

Sending only the new order would have saved 2.4 KB more at the cost of a new fragment vocabulary
and a client that rewrites rows, so the protocol stayed as it was. The script cannot reorder the
rows itself: a row lacks the slug and the second stamp, and Rust (UTF-8) and JavaScript (UTF-16)
order titles differently above the BMP.

Not compressed: bodies under 32 bytes (`/health`'s `ok`), images, PDFs,
`application/octet-stream` (a notebook's zips and videos), and the edit form's event stream. A
`.json` attachment also falls to `octet-stream`, which costs one bigger download.

## And a script on top, that nothing depends on

Every screen works without it. With it:

- the listing filters as you type;
- the network screen fetches its own news instead of reloading whole;
- on a two-pane screen, picking a note, sending the search and pressing back replace the reading
  half instead of the page;
- every stamp is restated in the reader's zone.

The first three only remove a wait. The fourth adds a fact the server cannot know; without it
the stamp is unconverted, not wrong.

**Fetches ask for part of a page.** A fetch names the region it wants in an `x-noda-fragment`
header, and the server sends that region from the same function that builds the whole page:
49,751 bytes down to 1,579 for a note, 41,418 to 876 for a network-screen poll, and 67,591 to
4,723 for a search from the listing (the address still gets the query, so back works). The
search box in a note page's index column is left alone, since ⏎ there means going to the
listing. The part is a substring of the page (`page.rs` asserts it), and the whole page is always
a correct answer: no header, or a name the server does not know, gets the page, and every fetch
picks the element it wants out of whatever arrives.

**The filter may be narrower than the server, never wider.** Every row is on the page, the
excluded ones `hidden`, so filtering is the server's operation on the same markup. It cannot read
bodies: when a query holds a bare word or `text:`, the listing says it filtered by title and tag, and ⏎
finishes the job — a title or tag hit is always a text hit too. A *negated* one would keep
rows the server drops, so there, and for a half-typed query, the filter stands aside until ⏎.
