# In a browser

*One of noda's three interfaces — see the [README](../README.md) for the rest.*


`noda web` serves the notebooks over HTTP, so a phone can read them. It renders on the
server and works with JavaScript turned off: the search box is a form, every row is a link.
A script is laid over that on two screens and nothing depends on it — see [the last
section](#and-a-script-on-top-that-nothing-depends-on).

```
$ noda web
noda is at http://127.0.0.1:8080
```

**It says what it does while it runs, and it is quiet about it.** Every other command
answers and exits, so its output is its answer; a server outlives the question, and the only
way to find out afterwards what it refused, what it failed at or what was slow is for it to
have said so at the time. The log goes to **stderr**, so that address stays on stdout where
the rest of noda puts a command's answer.

`RUST_LOG` decides what is on it. The default is `error,noda=info`, which logs nothing per
request: what you get is a refusal, a failure, and any request that took longer than a second.
Turn the stream on with `RUST_LOG=noda=debug`, or narrow it to the requests alone with
`RUST_LOG=noda::web::log=debug`. `--log-format json` (or `NODA_LOG_FORMAT=json`) writes one
JSON object per line instead, for something that collects them.

What you ask for is applied *on top of* that default rather than replacing it, so a typo in
`RUST_LOG` costs you the setting and not the log — a server that went quiet because of a
stray character in a unit file is the sort of failure nobody thinks to look for. A bare level
is the exception and means all of it: `RUST_LOG=off` is off.

```
$ RUST_LOG=noda=debug noda web
noda is at http://127.0.0.1:8080
… DEBUG noda::web::log: request finished event="http.request" method=GET
  route="/nb/{book}/n/{key}" status=200 elapsed=1.78ms elapsed_ms=1.785
```

**A request is logged as the route it matched, never as the address it asked for.** That row
above is a note being read, and which note is not on it — an id is the name of somebody's
note, a file's address carries its filename, and a search is the reader's own words in a query
string. None of that belongs in a file that outlives the request and gets shipped to a log
collector, and none of it is what anybody wants to count either: `/nb/{book}/n/{key}` is one
line in a report and a per-note path is two thousand. A request that matched no route is
labelled `<unmatched>` rather than having its path repeated, because that path is whatever a
scanner put in the URL.

The one event worth an alert is `http.refused`, at WARN: the guard turned a request away, and
that is either a reverse proxy whose name has not been allowed yet or somebody attempting the
rebinding attack the guard exists for. It carries the `Host` and `Origin` it decided on, since
a refusal nobody can read the reason for is a refusal nobody can fix.

Every notebook is named in the URL — `/nb/work` for the listing, `/nb/work/n/k3f9m2p1` for a
note — so the active-notebook pointer never decides what a page shows. That pointer belongs to
a shell session, and a browser tab that quietly changed which notebook it was showing because
something happened in a terminal would be worse than a longer address. A note's slug and any
prefix of its id both redirect to the id, so a bookmark survives a retitle.

**The front page is the list of notebooks, and the one screen that is not inside one.** It
carries neither the rail nor the bar — both hold places inside a notebook, and there is nowhere
further up than this — so the whole width goes to the rows. A row is `noda status` said in a
line: the name, what it holds, the day it was last committed to, and where it stands against
its remote. Two of those are somewhere to go, so the row has two links in it, the shape the
files page already uses: the name opens that notebook, the standing opens its network screen.
A notebook with no remote keeps the words and loses the link, because a press whose answer is
what you have already read is not worth having. The notebook your terminal is pointed at gets
the mark `noda notebook ls` gives it — a dot in the margin, uncoloured, since which notebook a
shell is on is not a fact about what that notebook is. It is the one thing on any of these
pages that the pointer is read for.

The row gives up its columns as the screen narrows, the way a listing's row does: below 1024px
the day goes, and on a phone the file count goes with it. Three counts and the chip do not fit
in 390px, and of the three the file count is the one that keeps least.

**There is no password on it, and there is not going to be one.** It is meant to be reached
over a tailnet or from behind something that already does authentication, and both of those
do that job better than a notebook could. What follows from that is the whole of its security
model:

- It listens on **this machine only** until told otherwise. `--listen 0.0.0.0:8080` opens it
  to the network, and says so on the way up.
- It refuses a request whose `Origin` is another site. There is no session to be missing, so
  without that check a page on any other site could make your browser commit to your
  notebook.
- It answers to **addresses and `localhost` freely, and to a hostname only when told**.
  That is the DNS-rebinding half: an attacker who points `evil.example` at `127.0.0.1` sends
  requests your browser considers same-origin — `Origin` and `Host` agree, because both say
  `evil.example` — and the only thing that gives it away is that a rebinding attack needs a
  *name*. So a name has to be asked for:

```sh
noda web --listen 0.0.0.0:8080 --allow-host noda.tail1234.ts.net
```

  Behind a reverse proxy, name whatever the browser puts in the address bar. The refusal says
  exactly what to add.

**`/health` is the one address outside all of that**, because a probe is a program and not a
browser. It answers `200` and the word `ok` while the server can still answer, and nothing
else: no notebook is opened, nothing is echoed back, and the only thing it discloses is that
something is listening — which whatever asked established by connecting.

```
$ curl -i http://127.0.0.1:8080/health
HTTP/1.1 200 OK
content-type: text/plain; charset=utf-8
cache-control: no-store
x-content-type-options: nosniff

ok
```

It sits outside the guard because the `Host` a probe sends is whatever the thing running it
decided on — a pod's own address, a service name, the name on a proxy's certificate — and a
check that answered `403` because `--allow-host` had not been given would report a healthy
server as dead, then restart it, and the restart would not help.

**What it checks is what a restart can mend.** Every page here does its work on the blocking
pool, because libgit2 offers no other kind, so the check goes through that pool too: a server
whose pool has wedged stops answering this instead of answering `200` while every reader
hangs. It deliberately does *not* open a notebook — a repository that will not open is a thing
to repair, and a check that failed on one would turn a single broken notebook into a container
restarting every thirty seconds and still failing.

```yaml
livenessProbe:
  httpGet: { path: /health, port: 8080 }
```

One caveat about the image: it is distroless and holds nothing but the binary, so a Dockerfile
`HEALTHCHECK` — or a Compose `test:`, which is the same thing — has nothing inside the
container to make the request with. Everything that probes from *outside* it works as written:
Kubernetes' `httpGet` above, a load balancer's pool check, an uptime monitor.

It is logged like any other request, which is to say not at all until `RUST_LOG=noda=debug`
asks — and then as `route="/health"`, one line in a report rather than one per probe.

It writes as well as reads: a note can be started, its body rewritten, its title changed, its
tags ticked on and off, and it can be deleted. Every one of those runs the command that does
it — the same `add`, `mv`, `tag` and `rm` the terminal calls — so what a change *means* has
one implementation, and each lands as its own commit with the same message it would have had.

**A note's own bar is five things to do to it** — Edit, Tags, Rename, Links and Delete — and
Delete is the only item on any bar here that carries a colour. It was a line past the end of
the prose until it was not, on the argument that the one action that cannot be undone by doing
it again should cost a scroll of the whole note to reach. What that argument missed is that
the friction was already built and is somewhere else: `/delete` is a confirmation page, so a
thumb that lands on the wrong item spends a page and never a note. Hiding the way in bought no
safety the confirmation was not already providing, and what it did cost was a reader learning
that a note can be deleted at all.

The tags form ticks off what should go and takes the new ones in one field, separated by
spaces — `ops docs "24.04 Dark patterns"` adds three, quoted the way the `:` prompt and the
search box quote, and the whole change is one commit.

**An edit carries the note's fingerprint, and a stale one is refused.** The form remembers
what the file hashed to when the page was drawn; if it hashes to something else by the time
you save, nothing is written and the page comes back with both versions on it — what is saved
now, above what you typed, still in a box you can edit. That is the whole reason for the check:
an edit begun on a phone at breakfast must not flatten one made at a terminal at lunch, and a
refusal that threw away what you had just written would only be a politer way of losing it.

The fingerprint is the file's git blob id and not its `updated` stamp, because `--no-touch`
exists — a note's content can change without its stamp moving, which is exactly the case that
version marker would be wrong in.

**A note is rendered, and its links go where they went on disk.** A relative link to another
note — `[the plan](k3f9m2p1-the-plan.md)`, the spelling that works on a git host and in any
editor — becomes a link to that note's page, coloured the way an id is coloured everywhere
else in noda, so a link that stays inside the notebook looks different from one that leaves.
A link to anything else the notebook holds becomes a download, and an image is shown where it
stands.

**An address a note only mentions is a link too.** CommonMark says otherwise — a bare
`https://example.com` is a word to it, and only `<https://example.com>` in angle brackets is a
link — but every other Markdown anybody reads disagrees, so a note written anywhere else
arrives with its references as prose nobody can press. noda finds them on GFM's rules, narrowed
to `http://` and `https://`: the punctuation a sentence ends with is not part of the address,
and a `)` is part of it only when the address opened one. `www.example.com` is deliberately
left alone, because making it a link means picking a scheme for the writer and picking the
wrong one sends the reader somewhere else. An address inside code, inside a link's own words,
or inside markup `noda import` could not convert is left exactly as it is: it is being shown,
not offered.

**Nothing a note points at is told where it was pointed from.** An address here holds
somebody's note id — it is why `web/log.rs` will not write one into a log — and the `Referer`
on a followed link would hand `/nb/<book>/n/<id>` to whoever is on the other end. So a link
that leaves opens in a tab of its own carrying `rel="noopener noreferrer"`, and the page it
left says `Referrer-Policy: same-origin` twice more: once as a response header on every HTML
answer, and once in its own `<head>`.

Three statements of one rule, because each covers what the others cannot. The header is free
and is also what a reverse proxy may strip. The meta survives that, and it is the only one that
reaches an image — a picture embedded from somebody else's site is fetched without the reader
choosing anything, which makes it the larger leak and one no attribute on a link would have
touched. And `noopener` is the half neither of them says: a page opened in a new tab can reach
back through `window.opener` at the page that opened it.

`same-origin` and not `no-referrer`, which sounds like the stricter choice and is the one that
does not work. Both send nothing whatsoever to another site. But a document whose policy is
`no-referrer` also posts its forms with `Origin: null` — the same rule nulls both — and the
`Origin` check below is what stands between this server and a cross-site write, so it refuses
an opaque origin. With `no-referrer` set, every write from a browser is turned away by noda's
own defence, and the refusal in the log is indistinguishable from the attack it exists for. It
took a real browser to find: nothing below that layer sends an `Origin` it did not write
itself.

`/nb/<book>/files` lists everything that is not a note: how big it is, what it will arrive as,
and how many notes point at it — a file nothing points at is exactly what `doctor --links`
calls an orphan.

**Three screens beyond the listing, on a bar along the bottom.** `/nb/<book>/tags` is every
tag with how many notes carry it, commonest first, and pressing one narrows the listing to it
— quoted where the tag has a space in it, because the search field splits the way a shell
does. `/nb/<book>/todo` is `noda todo` on a phone: every unticked box in the notebook, soonest
due first, a passed date in red, and the row goes to the note the box is written in. There is
no way to tick one, for the reason there is no `noda done` — an item inside a note has no
address, and giving each one an id would turn the file into a noda-only format.

**A note says when it was made and when it last changed, and the browser says both again where
you are.** The page carries the stamps exactly as the frontmatter holds them — `Z` or `+08:00`
and all, which is the one spelling that cannot be misread and what `noda show` prints — wrapped
in a `<time>` that carries the same value for the script to read. What the script does with it
is the one thing in this whole layer that is not a shortcut: it is a fact the server has no way
of stating. An instant is not a day until somebody says where they are standing, and nothing in
a request says so, so `2026-08-15T23:30:00Z` is the fifteenth here and the sixteenth in Taipei
and the server cannot choose between them.

So it is stated from the only place that knows. A note's page reads `Aug 15, 2026, 4:59 PM`; a
listing's row keeps `YYYY-MM-DD`, which is what `noda ls -l` prints and what a column of them
reads as, moved into your zone. English either way, and only the zone is yours — every other
string noda prints is English, and what a reader in another country needs from this is the hour
they were at their desk, not a translated month. With scripts off the file's own spelling stays
on the screen: not wrong, just unconverted. Hover either and the exact stamp is in the tooltip.

It follows that a listing's day can differ by one from the day the same page shows with scripts
off, and that is the point of it rather than a defect. It is also the first thing here that the
script draws differently from the server, which is why `web/script.rs` names it as the exception
its opening rule now has.

Two stamps are left alone on purpose. A `due:` date is a calendar day somebody typed rather than
an instant, and `noda todo` already decides whether it has passed against git's own offset, so
converting it would move an item due today into tomorrow for a reader one zone east. And the
count beside a tag wears the same class as a stamp without being one. That is why what the
script converts is `<time datetime>` and never a class.

`/nb/<book>/n/<id>/backlinks` is what points *at* a note, reached from `Links` on the note's
own bar; the count beside a file on the files page is the same question asked of a file, which
is the only way to ask it since a file has no page of its own. Both match on the id in the
destination rather than on the filename, so a retitle does not silence them — which is exactly
when the answer is wanted, because every Markdown renderer now shows those links broken.

A screen of its own rather than a strip on the note's page, and the reason is the one `ls` and
`todo` already follow: reading a note opens one file, and answering "what links here" parses
every note in the notebook. One press must not quietly cost the other.

**On a wider screen it is the same interface at a higher density, not a second design.**
Under 640px it is a phone: one screen at a time, that bar along the bottom. Above it the bar
stands up into a rail down the left and a row extends — the day and the tags leave the second
line and go to the right of the title, which is the row `noda ls -l` already prints. Above
1024px the notes screen splits in two: the listing on the left, the note being read on the
right, and picking a note replaces the reading half rather than the page. With no note picked
the notebook's own `README.md` stands there, because that is the page a notebook already has
about the whole of itself. Above 1440px there is room for a third thing, and it is what points
at the note: the answer the Links button opens as a page of its own, in the margin of the note
it is about.

Two things arrive with the width, and both are the browser saying what a terminal already
says:

- **A row grows the id column.** `ls -l` prints id, title, updated, tags; given the room, so
  does this. The id is what `noda show` takes and the first half of the filename in the
  repository, so a listing you can read is one you can act on somewhere else. Tags stay last
  for the reason `-l` gives: they are the one column a note may not have, and anywhere else
  their absence would shift every column behind them.
- **The search field says how it grouped what you typed.** `tag:work OR tag:q3 budget` draws
  as `(tag:work or tag:q3) and (budget)` under the field. `OR` binding tighter than a space is
  the one thing about this grammar that gets read backwards, and both readings look like an
  answer once the notes are on the screen.

Neither is on a phone: there is one column there, and the title has it.

The listing beside a note is fetched by the script rather than sent with the page — a row is
about 290 bytes, and below 1024px not one of them is ever drawn. The margin note is fetched the
same way and for a sharper version of the same reason: what points at a note is a walk of every
note in the notebook, while a note page otherwise reads one file. With no script a note on a
wide screen is what it has always been: the note, whole, and the way back to the listing —
and the Links button, which is the way to the same answer at every width and the only way to
it on a phone.

Two things a note's body cannot do, both on purpose. **Raw HTML is shown as a code block**
rather than rendered: `noda import tiddlywiki` deliberately leaves markup it could not convert
in the body, so that markup is the only copy of what the note said, and dropping it would lose
it. **A destination carrying a scheme noda does not serve — `javascript:` first among them —
keeps its words and loses its link.** Files are served with `nosniff` and a content policy that
loads nothing, and only the formats that cannot carry a script are shown in place: SVG is a
document that runs script, so it arrives as a download.

**The notebook syncs from the browser, and the request does not wait for it.** The corner of
the listing's bar carries where the notebook stands against its remote — `2 to push`, `in
sync`, `never synced` — and pressing it opens `/nb/<book>/status`: the same facts `noda
status` prints, none of them fetched, with `Sync`, `Pull` and `Push` under them.

A sync is a fetch over somebody's network, so it takes as long as it takes. Pressing starts it
and answers straight away; the screen you land on says what is happening and brings itself
back for news every couple of seconds until it stops. That is a `<meta http-equiv="refresh">`
where no script is running, and the same interval fetched and swapped in where one is. Two
things follow from the shape:

- **A reload cannot start it again.** Only the `POST` begins anything, and what you are left
  holding is a `GET` — so the refresh a slow network invites is a question rather than a
  second push. Pressing the button again while one is running is not an error either; it is
  somebody who could not tell whether the first press landed, and the screen already answers
  that.
- **What it did stays until the next one.** `sync` prints three lines — the commit, the pull,
  the push — and they are shown as they were printed. A screen that said nothing afterwards
  would look like a screen that had ignored the button.

One errand per notebook at a time, and it takes the same write lock a Save takes: a merge
landing halfway through somebody pressing Save is what that lock is there for. The lock is per
notebook, so a remote that has gone quiet slows nothing down but the notebook whose remote it
is.

## What every page needs, and no page carries

The stylesheet and the scripts are links, not markup: `/a/style.<hash>.css`, and one address per
script. The name is a hash of the bytes, so the address changes when the content does and an old
one is simply an address nobody asks for — which is the whole of the invalidation question. They
are served `immutable` for a year, and the pages that name them are `no-cache`, because a kept
page is a page that could ask for bytes this build does not have.

It was the other way round for most of this project's life, and the argument then was a good one:
one request draws a whole page, which is what a phone at the far end of a tailnet wants. What
changed is the rest of the layer. Since the enhancement layer started asking for parts of pages,
most of what a reader fetches after their first page carries no chrome at all — and what was left
carrying it was the case fragments cannot cover: a note opened from a link, every form page, and
every screen on a phone, where the panes never split. Each was re-sending 46 KB the browser had
already been given.

The first view costs the same bytes it always did, in as many as four requests instead of one.
Every view after it costs the page alone. The first column is also what every view used to cost,
because it is the second plus everything the page links:

| | first view | every view after |
| --- | ---: | ---: |
| the notebooks page | 38,781 | 905 |
| a listing | 71,325 | 6,437 |
| a note | 59,540 | 4,336 |
| the network screen | 41,692 | 2,507 |
| a backlinks page | 38,704 | 828 |
| an edit form | 38,941 | 1,065 |

Measured against a notebook of five notes. The two screens that show a stamp cost more than
they did: a listing 215 bytes, a note 248, and the first view of either about 2.6 KB for the
script that converts them — fetched once, and never again at that address. Of the listing's
215, 32 are per note: a row prints its day twice and only one of the two carries the instant,
because the second copy would have been another 32 bytes a note on the one page where bytes
are counted. The script tells the other copy what it came to.

## And a script on top, that nothing depends on

Every screen works without one, and four of them carry one anyway: the listing filters as
you type, the network screen asks for its own news instead of reloading whole, on a screen
wide enough for two panes picking a note replaces the reading half instead of the page — as
do sending the search and pressing back, which are the same press seen from either end — and
every stamp is said again in the zone the reader is standing in.

The first three are shortcuts: they remove a wait, never add an ability, and with scripts off
every screen does what it always did. The fourth is the exception, and it is worth naming
rather than smuggling in: it states a fact no server here could have stated, because nothing
in a request says what time it is where the reader is. With scripts off that stamp is not
wrong, it is unconverted — the file's own spelling, which is the one that cannot be misread.

What the ones that go to the server ask for is a *part* of a page rather than a page. A
press on a row replaces the reading pane and leaves the rest of the screen where it is, so
what used to arrive with it — the stylesheet, both scripts, the rail, the index pane's
frame — was 48 of its 52 KB already on the screen. So a fetch says which region it will
use, in an `x-noda-fragment` header, and the server sends that region out of the same
function the whole page is built from. Measured against the test notebook: 49,751 bytes
down to 1,579 for a note, and 41,418 down to 876 for a poll of the network screen.

A search sent from the listing is answered the same way: the column comes back, the field
keeps the cursor in it, and the address gets the query a scriptless press would have put
there — so going back arrives where it would have either way, 67,591 bytes down to 4,723.
The one place it stands aside is the same form in the index column of a *note* page, where
⏎ is the way to the listing and answering it in place would leave the note on the screen.

It stays a shortcut rather than becoming a second interface because the part is a
substring of the page — one rendering, and `page.rs` asserts it by containment — and
because the whole page is always a correct answer. A reader typing the address, a
bookmark, a crawler and a browser with no script send no such header and get the page;
so does a name the server does not know. Every fetch parses what arrives and asks it for
the element it wants, so a server that ignored the header would still be answering them.

The listing can do that because it already holds every note it has — the rows a query
excludes are on the page with `hidden` on them, in both directions, which is what lets a
script put one back. So filtering is the same operation the server did, on the same markup.

What it cannot do is read a body. A bare word searches titles, tags **and bodies** on the
server, and the page carries no bodies — so when a query holds one, the listing says under
the field that it filtered by title and tag, and the search key finishes the job. What is on
the screen is right and possibly short, never wrong: a title-or-tag hit is always a text hit
too. A *negated* bare word inverts that — `-budget` would **keep** a row whose body the script
cannot see — so there the filter stands aside and waits for the key, as it does for a query
half typed.

