//! The enhancement layer: the only part of this interface allowed to be absent.
//!
//! Every page works with scripts off, and six pull requests were spent making
//! sure of it before a line of this file existed. That order was deliberate —
//! write the script first and the scriptless path quietly loses a corner nobody
//! notices. **Nothing here adds a capability. Everything here removes a wait**,
//! with one exception set out below.
//!
//! The waits:
//!
//! * **The listing waits for a round trip to narrow itself**, though every fact
//!   a title-or-tag query needs is already on the page.
//! * **The network screen waits by reloading itself whole.** A fetch of the same
//!   URL is the same news without the flash, the scroll jump and the lost focus.
//! * **What points at a note waits behind a press.** On a screen with room
//!   beside the prose, the Links page buys nothing that could not be there
//!   already. The button stays, and is still the only way there on a phone.
//! * **Sending the search, and going back, wait for the page to be built
//!   again** — the same wait from either end of one press, and back was paying
//!   for it in full as a reload.
//!
//! ## And one thing here does add something
//!
//! A stamp is an instant, and an instant is not a day until somebody says where
//! they are standing. Nothing in a request says so, so the server cannot render
//! `2026-08-15T23:30:00Z` as the day it was: for half the world it was the
//! sixteenth. `script::STAMPS` says the same instant in the reader's own zone —
//! the one fact this interface cannot state from the server at all.
//!
//! What keeps it honest is that the scriptless page is not *wrong* but
//! unconverted: it shows the stamp as the file holds it, `Z` and all, which is
//! what `noda show` prints and the one rendering that cannot be misread. It
//! follows that a listing's day may differ from the scriptless one, and that is
//! the point rather than a defect — but it is the first time anything here has
//! drawn what the server would have drawn differently.
//!
//! Two stamps are deliberately untouched. A `due:` date is a calendar day
//! somebody typed and `noda todo` already decides against git's own offset, so
//! converting it would move an item due today into tomorrow. And the count
//! beside a tag wears a stamp's class without being one — which is why this
//! looks for `<time datetime>` and never a class.
//!
//! ## The rule both halves are written against
//!
//! *The server is the only authority, and the script must never be able to
//! answer differently — only sooner, or not at all.*
//!
//! For the network screen that is free: the script fetches the page the server
//! would have sent, and even "is it still running?" is read off the server's own
//! `<meta refresh>`.
//!
//! For the listing it costs an argument, a filter being a second implementation
//! of `query.rs` — the only one this project permits itself. What keeps it
//! honest is that it may be *narrower* than the server and never wider:
//!
//! | the query holds | the script can say | why |
//! | --- | --- | --- |
//! | only `tag:` / `title:` / `id:` | the whole answer | the page carries every field those terms read |
//! | a bare word, or `text:` | part of the answer | `Field::Text` reads the body too, and a row's body is not here. Title-and-tag hits are a **subset** of text hits, so what is shown is right and possibly short — never wrong |
//! | a *negated* bare word or `text:` | nothing | this is the case that inverts. `-budget` asks for notes without the word; the script cannot see the body, so it would *keep* a row the server would drop. Widening is the one thing the rule forbids, so the filter stands aside |
//!
//! The third row is why this is a table and not a sentence: the subset property
//! that makes the second row safe is destroyed by a leading `-`, which is not
//! visible from the design — it comes from `Term::matches` returning
//! `found != negated`, and was found by writing the filter rather than planning
//! it.
//!
//! A query that does not parse is the same case: the listing stands aside. It
//! does not repeat the server's complaint, a half-typed query not being a
//! mistake.
//!
//! ## What the script is allowed to touch
//!
//! Only what the server put there. Every row is on the page whatever is typed —
//! the excluded ones arrive `hidden` — so filtering here and there are the same
//! operation on the same DOM. Nothing is touched until the first keystroke:
//! until then the screen holds the server's answer to the URL.
//!
//! ## And what it is allowed to ask for
//!
//! Every fetch takes one region out of the page and drops the rest, which on a
//! note is 48 of 52 KB thrown away on a round trip a reader waits through.
//!
//! So each says which region it will use — `x-noda-fragment`, the vocabulary in
//! `web::Part` — and the server sends it out of the same function the whole page
//! is built from. **This is the enhancement rule rather than an exception**:
//! every fetch parses what arrives and asks it for the element it wants, so a
//! server ignoring the header would still be answering correctly.

/// The listing's filter, and the grouping it drew on the way.
///
/// Reads the rows out of the DOM rather than a copy: a JSON block beside the
/// list would put every title and tag on the page twice, and the second copy
/// goes stale. `textContent` also unwraps the server's `<mark>`s for free.
///
/// ## The grouping is the one thing here that never stands aside
///
/// This redraws `page::grouping` on every keystroke, from the same `parse` the
/// filter runs on — not a third implementation but the one the table above
/// already requires, used for a second thing.
///
/// It answers in the two cases the *filter* refuses to: a grouping is a fact
/// about the words and not about the notes, so where there is a parse there is
/// a grouping. Where there is not, the box empties — drawing the last complete
/// grouping under a line that no longer says it is worse than saying nothing.
pub const LISTING: &str = r#"
(() => {
  const app = document.querySelector(".app");
  const form = document.querySelector("form.searchbar");
  const list = document.querySelector("main.rows");
  if (!form || !list) return;
  const field = form.querySelector("input[name=q]");
  if (!field) return;

  // Read again when `script::PANES` replaces the rows without a reload, which
  // is why these are `let`: an element taken out of the document is one this
  // would go on filtering, invisibly, for the session.
  //
  // The field and the form are never replaced — the reader may have a cursor
  // in one — which is why they are read once above.
  let count, hint, parsed, problem, empty, asked, notes, total;
  const look = () => {
    count = document.querySelector(".topbar .count");
    hint = form.querySelector(".hint");
    parsed = form.querySelector(".parse");
    problem = form.querySelector(".problem");
    empty = list.querySelector(".empty");
    asked = empty && empty.querySelector(".asked");
    // `note.rs` refuses a comma in a tag, so the joined line takes apart.
    notes = [...list.querySelectorAll("a.row")].map((row) => {
      const title = row.querySelector(".title");
      const tags = row.querySelector(".tags");
      return {
        row,
        title,
        words: title.textContent,
        tags: tags ? tags.textContent.split(", ") : [],
        id: row.getAttribute("href").split("/").pop(),
      };
    });
    total = notes.length;
  };
  look();
  if (!total) return;

  const FIELDS = ["tag", "title", "id", "text"];

  // `query::split`, said again: quotes hold a piece together, and an unclosed
  // one runs to the end because its closer is usually the next character.
  const split = (text) => {
    const pieces = [];
    let piece = "";
    let quote = null;
    for (const c of text) {
      if (quote) {
        if (c === quote) quote = null;
        else piece += c;
      } else if (c === '"' || c === "'") {
        quote = c;
      } else if (/\s/u.test(c)) {
        if (piece) pieces.push(piece);
        piece = "";
      } else piece += c;
    }
    if (piece) pieces.push(piece);
    return pieces;
  };

  // The token as typed, kept because the grouping is drawn from it: what goes
  // on the screen has to be the reader's own line.
  const term = (token) => {
    const negated = token.startsWith("-");
    const rest = negated ? token.slice(1) : token;
    if (!rest) return null;
    let field = "text";
    let value = rest;
    const colon = rest.indexOf(":");
    if (colon > 0 && FIELDS.includes(rest.slice(0, colon))) {
      field = rest.slice(0, colon);
      value = rest.slice(colon + 1);
    }
    return value ? { field, value, negated, said: token } : null;
  };

  // `null` for anything that does not parse — one answer for the two cases the
  // script treats alike: half a query, and one it may not run.
  const parse = (tokens) => {
    const groups = [];
    let expecting = false;
    for (const token of tokens) {
      if (!token.trim()) continue;
      if (token === "OR") {
        if (!groups.length || expecting) return null;
        expecting = true;
        continue;
      }
      const parsed = term(token);
      if (!parsed) return null;
      if (expecting) groups[groups.length - 1].push(parsed);
      else groups.push([parsed]);
      expecting = false;
    }
    return expecting || !groups.length ? null : groups;
  };

  // `note::normalize_id`: an id is read off a screen and typed back.
  const fold = (id) => id.toLowerCase().replace(/[il]/g, "1").replace(/o/g, "0");

  const hits = (parsed, note) => {
    const value = parsed.value.toLowerCase();
    const inWords = note.words.toLowerCase().includes(value);
    let found;
    if (parsed.field === "tag") found = note.tags.includes(parsed.value);
    else if (parsed.field === "id") found = fold(note.id).startsWith(fold(parsed.value));
    else if (parsed.field === "title") found = inWords;
    // Everything a bare word reaches *here*; the body is what is missing.
    else found = inWords || note.tags.some((tag) => tag.toLowerCase().includes(value));
    return found !== parsed.negated;
  };

  // `page::highlight`: the earliest match wins and the longest of those, so two
  // overlapping terms mark one run. Built as nodes rather than markup — the way
  // to be sure a title is never read as HTML is never to make it a string.
  const paint = (element, text, terms) => {
    element.textContent = "";
    const hay = text.toLowerCase();
    let at = 0;
    while (at < text.length && terms.length) {
      let best = null;
      for (const term of terms) {
        const found = hay.indexOf(term, at);
        if (found < 0) continue;
        if (!best || found < best.at || (found === best.at && term.length > best.length))
          best = { at: found, length: term.length };
      }
      if (!best) break;
      element.append(text.slice(at, best.at));
      const mark = document.createElement("mark");
      mark.textContent = text.slice(best.at, best.at + best.length);
      element.append(mark);
      at = best.at + best.length;
    }
    element.append(text.slice(at));
  };

  // `page::grouping`, again: a pill per group, `or` inside and `and` between.
  // Nodes rather than markup, for `paint`'s reason.
  const chips = (groups) => {
    if (!parsed) return;
    parsed.textContent = "";
    parsed.hidden = !groups;
    if (!groups) return;
    for (const group of groups) {
      if (parsed.firstChild) {
        const and = document.createElement("span");
        and.className = "and";
        and.textContent = "and";
        parsed.append(and);
      }
      const pill = document.createElement("span");
      pill.className = "g";
      for (const one of group) {
        if (pill.firstChild) {
          const or = document.createElement("i");
          or.textContent = "or";
          pill.append(or);
        }
        const said = document.createElement("b");
        if (one.said.startsWith("tag:") || one.said.startsWith("-tag:")) said.className = "t";
        said.textContent = one.said;
        pill.append(said);
      }
      parsed.append(pill);
    }
  };

  // `full` is whether the screen holds the whole answer: when it does not, the
  // count would be a lie in the server's voice, so the hint says whose it is.
  const show = (shown, full) => {
    if (count) count.textContent = shown === total && full ? `${total}` : `${shown} of ${total}`;
    if (empty) {
      empty.hidden = shown > 0;
      if (asked) asked.textContent = field.value;
    }
    if (hint) hint.hidden = full;
  };

  const stand = () => {
    for (const note of notes) {
      note.row.hidden = false;
      paint(note.title, note.words, []);
    }
    show(total, false);
  };

  const apply = () => {
    // The complaint is about the query in the URL and the field no longer holds
    // it. Nothing here writes a new one — a query being typed is half-written by
    // definition.
    if (problem) problem.hidden = true;

    const tokens = split(field.value);
    // Before anything is decided about the rows, from the same parse: both ways
    // out below leave the listing alone, and neither is a reason to leave the
    // grouping wrong.
    const groups = tokens.length ? parse(tokens) : null;
    chips(groups);

    if (!tokens.length) {
      for (const note of notes) {
        note.row.hidden = false;
        paint(note.title, note.words, []);
      }
      show(total, true);
      return;
    }

    if (!groups) return stand();
    const terms = groups.flat();
    // The row that inverts. See the table at the top of `script.rs`.
    if (terms.some((parsed) => parsed.field === "text" && parsed.negated)) return stand();

    const full = !terms.some((parsed) => parsed.field === "text");
    const marks = terms
      .filter((parsed) => !parsed.negated && (parsed.field === "text" || parsed.field === "title"))
      .map((parsed) => parsed.value.toLowerCase());

    let shown = 0;
    for (const note of notes) {
      const matched = groups.every((group) => group.some((parsed) => hits(parsed, note)));
      note.row.hidden = !matched;
      if (matched) shown += 1;
      paint(note.title, note.words, matched ? marks : []);
    }
    show(shown, full);
  };

  field.addEventListener("input", apply);
  // The rows just sent are the answer to the query in the address: what is on
  // the screen is the server's until the next keystroke.
  if (app) app.addEventListener("noda:rows", look);
})();
"#;

/// The page the server would have sent, without the reload: it asks for its own
/// URL and swaps `<main>`, so every word is still the server's — including
/// whether an errand is running, read off the same `<meta refresh>` the
/// scriptless page steers by. When that stops arriving, so does the polling.
pub const STANDING: &str = r#"
(() => {
  const meta = document.querySelector('meta[http-equiv="refresh"]');
  if (!meta) return;
  let main = document.querySelector("main");
  if (!main) return;

  // From the meta rather than repeated, so neither can be changed alone.
  const every = (Number(meta.getAttribute("content")) || 2) * 1000;
  meta.remove();

  const again = () => setTimeout(tick, every);

  const tick = async () => {
    let text;
    try {
      const answer = await fetch(location.href, {
        headers: { accept: "text/html", "x-noda-fragment": "news" },
      });
      if (!answer.ok) return location.reload();
      text = await answer.text();
    } catch {
      // A phone that lost the tailnet mid-sync. The errand is running either
      // way, so ask again rather than invent an error over the server's page.
      return again();
    }
    const fresh = new DOMParser().parseFromString(text, "text/html");
    const next = fresh.querySelector("main");
    if (!next) return location.reload();
    main.replaceWith(next);
    main = next;
    if (fresh.querySelector('meta[http-equiv="refresh"]')) again();
  };

  again();
})();
"#;

/// The two panes: bringing the index one, and keeping it.
///
/// A note page is sent without the listing beside it — about 290 bytes a note,
/// half a megabyte at two thousand, and below 1024px none of it drawn — so the
/// page carries the pane's frame and this asks for the rest where the column is
/// on screen.
///
/// The rows inserted are the ones `/nb/<book>` sent, lifted out of that page's
/// own `main.rows` rather than built here: the listing has exactly one renderer,
/// and this can only be later or absent.
///
/// **Bring it.** On a note route with an empty pane and room for three columns,
/// fetch the listing. `indexed` goes on synchronously — the column has to exist
/// before the first paint or the reading pane is laid out twice.
///
/// **Keep it.** Picking a note replaces the reading pane and leaves the rows
/// alone; without this every press would throw the listing away and ask for it
/// again, when the reason to keep it was that it *stays* while you read. It is
/// also what makes the fetch happen once rather than once per note.
///
/// No loading state on the pane, which follows from keeping it: after the first
/// arrival there is nothing to load, and a notice on every note would *be* the
/// flicker rather than report it.
///
/// Every row is still a link to a page that renders on its own.
pub const PANES: &str = r#"
(() => {
  const app = document.querySelector(".app.split");
  if (!app) return;
  const wide = matchMedia("(min-width:1024px)");

  const book = () => {
    const form = app.querySelector(".index form.searchbar");
    return form ? form.getAttribute("action") : null;
  };

  // Asked when the pane is empty, which — picking a note keeping it — is once
  // on a note page opened cold.
  let asking = false;
  const bring = async () => {
    if (!wide.matches || !app.classList.contains("at-note")) return;
    app.classList.add("indexed");
    const box = app.querySelector(".index main.rows");
    const where = book();
    if (!box || box.firstElementChild || asking || !where) return;
    asking = true;
    let text;
    try {
      const answer = await fetch(where, {
        headers: { accept: "text/html", "x-noda-fragment": "index" },
      });
      if (!answer.ok) return;
      text = await answer.text();
    } catch {
      // The note is on the screen and whole, and a column that never arrives is
      // the scriptless layout — a working one.
      return;
    } finally {
      asking = false;
    }
    const sent = new DOMParser().parseFromString(text, "text/html");
    if (box.firstElementChild) return;
    column(sent, false);
  };

  // The index column, as the server now has it.
  //
  // **The form is never replaced, only what hangs off it.** There may be a
  // cursor in that field, and `script::LISTING` listens to the element the page
  // loaded with — replacing it drops both, and a listing that no longer filters
  // as you type. So the input stays and everything after it goes.
  //
  // `retype` is whether the field is the server's to set: it is when the address
  // changed under the reader, and it is not when they just typed it, where the
  // answer catching up must not take the keystrokes made in flight.
  const column = (sent, retype) => {
    const box = app.querySelector(".index main.rows");
    const rows = sent.querySelector("main.rows");
    if (!box || !rows) return false;
    box.replaceChildren(...rows.childNodes);

    const here = app.querySelector(".index .topbar .count");
    const count = sent.querySelector(".index .topbar .count");
    if (here && count) here.textContent = count.textContent;

    const form = app.querySelector(".index form.searchbar");
    const said = sent.querySelector("form.searchbar");
    const field = form && form.querySelector("input[name=q]");
    const asked = said && said.querySelector("input[name=q]");
    if (field && asked) {
      if (retype) field.value = asked.value;
      const rest = [...said.childNodes];
      const at = rest.indexOf(asked);
      while (field.nextSibling) field.nextSibling.remove();
      form.append(...rest.slice(at + 1));
    }

    mark();
    // Different elements now, and the filter holds the ones it was built with.
    // Said rather than observed: one script's doing is another script's fact.
    app.dispatchEvent(new CustomEvent("noda:rows"));
    return true;
  };

  // Off the address rather than remembered, so it is right after a swap and
  // after a hard load.
  const mark = () => {
    const at = location.pathname;
    for (const row of app.querySelectorAll(".index main.rows a.row")) {
      row.classList.toggle("here", row.getAttribute("href") === at);
    }
  };

  // **The address moves first, and the answer catches up.**
  //
  // A navigation changes the address the moment it starts, and so must this: a
  // reader who presses a row then presses back before the note lands would
  // otherwise go back past the page they are on and leave the notebook. So the
  // entry is pushed on the press, and every way this can fail ends somewhere
  // that address is correct. `replace` and not `assign`, the entry being there
  // already.
  const swap = async (href, push = true) => {
    if (push) history.pushState(null, "", href);
    let text;
    try {
      const answer = await fetch(href, {
        headers: { accept: "text/html", "x-noda-fragment": "read" },
      });
      if (!answer.ok) return location.replace(href);
      text = await answer.text();
    } catch {
      return location.replace(href);
    }
    const sent = new DOMParser().parseFromString(text, "text/html");
    const next = sent.querySelector(".pane.read");
    if (!next) return location.replace(href);
    app.querySelector(".pane.read").replaceWith(next);
    document.title = sent.title;
    app.classList.remove("at-list");
    app.classList.add("at-note");
    mark();
    // Replaced whole, so anything hanging off the note read is now a different,
    // empty element. Said rather than observed: a pane swap is a fact about this
    // script, not something another should infer from the DOM.
    app.dispatchEvent(new CustomEvent("noda:read"));
  };

  // Two panes of one answer in one round trip: a screen half arrived
  // flickers.
  const screen = async (href) => {
    let text;
    try {
      const answer = await fetch(href, {
        headers: { accept: "text/html", "x-noda-fragment": "screen" },
      });
      if (!answer.ok) return location.reload();
      text = await answer.text();
    } catch {
      // Nothing arrived, so nothing is claimed. A reload asks the same question
      // by the same route, and is what going back did before any of this.
      return location.reload();
    }
    const sent = new DOMParser().parseFromString(text, "text/html");
    const read = sent.querySelector(".pane.read");
    if (!read || !column(sent, true)) return location.reload();
    app.querySelector(".pane.read").replaceWith(read);
    document.title = sent.title;
    app.classList.remove("at-note");
    app.classList.add("at-list");
    mark();
    app.dispatchEvent(new CustomEvent("noda:read"));
  };

  // Two presses arrive here and are the same press: sending the search and
  // choosing an order both change which rows there are and nothing else. The
  // address moves first for `swap`'s reason, and every way this can fail ends
  // where a scriptless press would have gone.
  const relist = async (where) => {
    history.pushState(null, "", where);
    let text;
    try {
      const answer = await fetch(where, {
        headers: { accept: "text/html", "x-noda-fragment": "index" },
      });
      if (!answer.ok) return location.replace(where);
      text = await answer.text();
    } catch {
      return location.replace(where);
    }
    const sent = new DOMParser().parseFromString(text, "text/html");
    // `false`: the reader just typed in that field, and the answer catching up
    // must not take the keystrokes made in flight. An order press does not touch
    // the field, so the same answer suits both.
    if (!column(sent, false)) return location.replace(where);
  };

  // Sending the search: the rows change and the page does not.
  //
  // **Only on the listing screen.** The same form is in a note page's index
  // column, where ⏎ is the way *to* the listing — answering it here would keep
  // the note on screen, which is not the server's answer.
  //
  // It asks for the address the form would have submitted, so what lands in the
  // history is what a scriptless press leaves there.
  app.addEventListener("submit", (event) => {
    if (event.defaultPrevented) return;
    const form = event.target.closest(".index form.searchbar");
    if (!form || !app.classList.contains("at-list")) return;
    const field = form.querySelector("input[name=q]");
    const action = form.getAttribute("action");
    if (!field || !action) return;
    event.preventDefault();
    // **Every field, not just the one typed in.** The form also holds the
    // order, and rebuilding the address out of `q` alone put the notes back in
    // the default order without saying so — the script answering differently
    // from the server, which this file may not do.
    const where = action + "?" + new URLSearchParams(new FormData(form));
    relist(where);
  });

  // The same press, from the other control on that form. **Only on the listing
  // screen**, for the reason the search gives above.
  app.addEventListener("click", (event) => {
    if (event.defaultPrevented || event.button || event.metaKey || event.ctrlKey ||
        event.shiftKey || event.altKey) return;
    const chip = event.target.closest(".index form.searchbar .sortbar a");
    if (!chip || !app.classList.contains("at-list")) return;
    const href = chip.getAttribute("href");
    if (!href) return;
    event.preventDefault();
    relist(href);
  });

  app.addEventListener("click", (event) => {
    if (event.defaultPrevented || event.button || event.metaKey || event.ctrlKey ||
        event.shiftKey || event.altKey) return;
    // The margin note's links are the index's rows by another name — a note one
    // press away — so they travel the same way, rather than being the full
    // navigation `swap` exists to avoid.
    const row = event.target.closest(".index main.rows a.row,.read .beside .mini");
    if (!row || !wide.matches) return;
    const href = row.getAttribute("href");
    if (!href) return;
    event.preventDefault();
    swap(href);
  });

  // A press pushed an address, so back has to put what it names on the screen.
  // Asking the server is the reload's answer without the reload.
  addEventListener("popstate", () => {
    // Below the breakpoint nothing was pushed, so anything arriving here is
    // history this script does not own and a reload is correct.
    if (!wide.matches) return location.reload();
    const at = location.pathname;
    // A note's address is the only kind this script pushes besides the
    // listing's own, and the two are told apart the way the router tells them
    // apart: `/nb/<book>/n/<id>` names a note and nothing else does.
    if (/\/n\/[^/]+$/.test(at)) return swap(at, false);
    screen(at + location.search);
  });

  wide.addEventListener("change", bring);
  bring();
  mark();
})();
"#;

/// What points at the note, in the margin of the note.
///
/// Backlinks are a page behind the Links button, and on a screen wide enough to
/// hold them beside the prose that press buys nothing. The button stays, and is
/// still the only way on a phone.
///
/// **Why the server does not send it.** `backlinks_to_note` walks every note —
/// about 8% on top of `ls` at two thousand — which is cheap for a page that was
/// asked for and waste for a column no screen under 1440px draws. And a note
/// page reads exactly one file today, so putting the aside in the markup would
/// turn one read into two thousand on every phone.
///
/// **`margined` goes on before the answer**, the bargain `indexed` makes: the
/// class is what lets the layout keep 236px for a column still in flight, so
/// the prose is laid out once. No script, no class, no reserved column.
///
/// **The one loading state on the page.** The index pane says nothing because
/// it keeps its rows after the first arrival; this column is empty on every note
/// and the walk behind it is the whole notebook, so an unexplained gap under
/// "Backlinks" reads as a column that failed.
///
/// An answer of none is an answer and is drawn as one. A fetch that never comes
/// back is not: the box closes and the reader keeps the whole note.
pub const BESIDE: &str = r#"
(() => {
  const app = document.querySelector(".app.split");
  if (!app) return;
  const wide = matchMedia("(min-width:1440px)");

  // Off the address rather than remembered, and the string the fetch is built
  // from — so the two can never be about different notes.
  let asked = null;

  const working = (text) => {
    const said = document.createElement("p");
    said.className = "said working";
    const bold = document.createElement("b");
    bold.textContent = text;
    said.append(bold);
    return said;
  };

  const nothing = (text) => {
    const none = document.createElement("p");
    none.className = "none";
    none.textContent = text;
    return none;
  };

  const ask = async () => {
    // The reading pane may not hold a note at all — going back puts the
    // notebook's own page there. Forgetting what was asked is what lets the
    // same note, opened again, ask again.
    if (!app.classList.contains("at-note")) {
      asked = null;
      return;
    }
    if (!wide.matches) return;
    const aside = app.querySelector(".pane.read .beside");
    const answer = aside && aside.querySelector(".answer");
    if (!answer) return;
    // Before the first paint and the first await, or the note is laid out at
    // one measure and then another.
    app.classList.add("margined");
    const at = location.pathname;
    if (asked === at) return;
    asked = at;

    answer.replaceChildren(working("Reading the notebook…"));
    aside.hidden = false;

    let text;
    try {
      const got = await fetch(at + "/backlinks", {
        headers: { accept: "text/html", "x-noda-fragment": "rows" },
      });
      if (!got.ok) throw new Error(got.status);
      text = await got.text();
    } catch {
      // Nothing arrived, so nothing is claimed: the box closes, what was asked
      // is forgotten so the next note may try again, and the scriptless page is
      // what is left. Conditional on this still being the note being read — the
      // reader may have moved on to one whose answer is in flight.
      if (asked === at) {
        asked = null;
        aside.hidden = true;
        answer.replaceChildren();
      }
      return;
    }

    // The reader may have moved on while the notebook was walked.
    if (location.pathname !== at) return;
    const box = app.querySelector(".pane.read .beside");
    const into = box && box.querySelector(".answer");
    if (!into) return;

    // Said in the margin's own shape. `bring` lifts the listing's rows because
    // that column *is* the listing; this is 236px beside prose, where a row's
    // tags would wrap into a paragraph. What is taken is which notes and what
    // they are called.
    const sent = new DOMParser().parseFromString(text, "text/html");
    const minis = [];
    for (const row of sent.querySelectorAll("main.rows a.row")) {
      const href = row.getAttribute("href");
      const title = row.querySelector(".title");
      if (!href || !title) continue;
      const mini = document.createElement("a");
      mini.className = "mini";
      mini.href = href;
      mini.textContent = title.textContent;
      const id = document.createElement("span");
      id.textContent = href.split("/").pop();
      mini.append(id);
      minis.push(mini);
    }
    if (minis.length) into.replaceChildren(...minis);
    else into.replaceChildren(nothing("Nothing points here."));
    box.hidden = false;
  };

  // Widening past the breakpoint is the reader asking for the column.
  wide.addEventListener("change", ask);
  app.addEventListener("noda:read", ask);
  ask();
})();
"#;

/// Every stamp on the page, said again where the reader is standing.
///
/// The marker is `<time datetime>` and never a class: `.when` is typography and
/// gets worn by a tag's note count and a `due:` date, neither of which is an
/// instant.
///
/// Repainted rather than watched — `script::PANES` already says when it has
/// replaced the rows or the note — and re-reading `datetime` makes a second pass
/// produce exactly what the first did.
pub const STAMPS: &str = r#"
(() => {
  // English, and only the zone is the reader's: what a reader in Taipei needs
  // is the hour they were at their desk, not a Chinese month name.
  const clock = new Intl.DateTimeFormat("en", { dateStyle: "medium", timeStyle: "short" });

  // `noda ls -l`'s shape, which is what a column of them reads as. By hand
  // rather than `Intl`, because what is wanted is that one spelling.
  const iso = (at) => {
    const pad = (n) => String(n).padStart(2, "0");
    return at.getFullYear() + "-" + pad(at.getMonth() + 1) + "-" + pad(at.getDate());
  };

  const paint = (root) => {
    for (const said of root.querySelectorAll("time[datetime]")) {
      const raw = said.getAttribute("datetime");
      const at = new Date(raw);
      // An import leaves what it found, and a date nothing can parse is left
      // as it reads: the file's own words beat a guess.
      if (Number.isNaN(at.getTime())) continue;
      said.textContent = said.hasAttribute("data-clock") ? clock.format(at) : iso(at);
      // What the file holds: the page no longer shows it.
      said.title = raw;
    }

    // A row prints its day twice and the stylesheet draws one at a time. Only
    // one carries the stamp — the second copy is thirty bytes a note on the one
    // page where bytes are counted — so the other is told what it came to.
    for (const row of root.querySelectorAll("a.row")) {
      const said = row.querySelector("time.when");
      const beside = row.querySelector(".ident .day");
      if (!said || !beside) continue;
      beside.textContent = said.textContent;
      beside.title = said.title;
    }
  };

  paint(document);

  // Said rather than observed: one script's doing is another's fact.
  const app = document.querySelector(".app");
  if (!app) return;
  for (const done of ["noda:rows", "noda:read"]) {
    app.addEventListener(done, () => paint(app));
  }
})();
"#;

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole of the injection defence, and it is one string.
    #[test]
    fn no_script_can_end_its_own_element() {
        for source in [LISTING, STANDING, PANES, BESIDE, STAMPS] {
            assert!(!source.to_lowercase().contains("</script"), "{source}");
        }
    }

    /// The same pact for the stamps. Anything carrying a `datetime`, which is a
    /// decision as much as a selector: converting a tag's note count or a `due:`
    /// date would be wrong in a way nobody notices until another country.
    #[test]
    fn the_stamps_look_for_what_the_pages_write() {
        for hook in [
            "time[datetime]",
            "data-clock",
            "a.row",
            "time.when",
            ".ident .day",
            "noda:rows",
            "noda:read",
        ] {
            assert!(
                STAMPS.contains(hook),
                "the stamps stopped looking for {hook}"
            );
        }
        // The decision, not the default: `undefined` follows the browser's
        // locale and prints a month name in a language nothing else here uses.
        assert!(STAMPS.contains("Intl.DateTimeFormat(\"en\""), "{STAMPS}");
        // The other half is asserted from the page's side, where it can fail:
        // grepping this string for a selector it does not contain passes for the
        // wrong reason the moment somebody writes a different one.
    }

    /// Not a test of the JavaScript — `e2e/` runs that — but that the two halves
    /// of one decision stayed together: renaming a class on one side is a silent
    /// no-op every Rust test still passes.
    #[test]
    fn the_filter_looks_for_what_the_listing_writes() {
        for hook in [
            "form.searchbar",
            "main.rows",
            "input[name=q]",
            "a.row",
            ".title",
            ".tags",
            ".topbar .count",
            ".hint",
            ".parse",
            ".problem",
            ".empty",
            ".asked",
        ] {
            assert!(
                LISTING.contains(hook),
                "the filter stopped looking for {hook}"
            );
        }
    }

    /// Drawn twice, by `page::grouping` and by this on every keystroke, and they
    /// cannot share a function across the language boundary — so a pill changing
    /// shape at the first key would be a flicker nothing catches. What can be
    /// checked is that both write the classes the stylesheet draws.
    #[test]
    fn both_halves_of_the_grouping_draw_the_same_pill() {
        for hook in ["\".parse\"", "\"and\"", "\"g\"", "\"t\"", "\"i\""] {
            assert!(
                LISTING.contains(hook),
                "the grouping stopped writing {hook}"
            );
        }
        let sheet = crate::web::page::stylesheet();
        for rule in [".parse .g", ".parse .g b.t", ".parse .g i", ".parse .and"] {
            assert!(
                sheet.contains(rule),
                "the stylesheet stopped drawing {rule}"
            );
        }
    }

    /// The same for the panes, and it matters most here: this script reads the
    /// *other* page's markup, fetched at runtime, so a class renamed in
    /// `page.rs` breaks it with no compile error and no failing test.
    #[test]
    fn the_panes_look_for_what_the_pages_write() {
        for hook in [
            ".app.split",
            ".index form.searchbar",
            ".index main.rows",
            ".index .topbar .count",
            ".index form.searchbar .sortbar a",
            ".pane.read",
            "a.row",
            "indexed",
            "at-note",
            "at-list",
            "here",
        ] {
            assert!(PANES.contains(hook), "the panes stopped looking for {hook}");
        }
        // The order rides in the form's fields, so the press has to send the
        // form: rebuilt out of `q` it drops the order silently.
        assert!(PANES.contains("new FormData(form)"), "{PANES}");
        let sheet = crate::web::page::stylesheet();
        for rule in [".sortbar", ".sortbar a[aria-current] .pill"] {
            assert!(
                sheet.contains(rule),
                "the stylesheet stopped drawing {rule}"
            );
        }
    }

    /// Written twice, in the stylesheet and here, and the two have to agree or
    /// the script asks for a column that is not there.
    #[test]
    fn the_script_and_the_stylesheet_split_at_the_same_width() {
        assert!(PANES.contains("(min-width:1024px)"), "{PANES}");
        assert!(
            crate::web::page::stylesheet().contains("(min-width:1024px)"),
            "the stylesheet no longer splits at 1024px"
        );
    }

    /// The same for the screen that polls: the server's own facts read back,
    /// silent when they go missing.
    #[test]
    fn the_poll_steers_by_the_meta_the_scriptless_page_steers_by() {
        assert!(
            STANDING.contains(r#"meta[http-equiv="refresh"]"#),
            "{STANDING}"
        );
        assert!(STANDING.contains("querySelector(\"main\")"), "{STANDING}");
    }

    /// It reads two pages it did not write, and both break it silently: the
    /// column arrives empty, at a width no unit test has.
    #[test]
    fn the_margin_note_looks_for_what_the_pages_write() {
        for hook in [
            ".app.split",
            ".pane.read .beside",
            ".answer",
            "main.rows a.row",
            ".title",
            "at-note",
            "margined",
            "\"mini\"",
            "\"/backlinks\"",
        ] {
            assert!(
                BESIDE.contains(hook),
                "the margin note stopped looking for {hook}"
            );
        }
    }

    /// The grouping's pairing again: markup written in JavaScript against rules
    /// written in Rust, with nothing between them but a name.
    #[test]
    fn the_stylesheet_draws_what_the_margin_note_builds() {
        let sheet = crate::web::page::stylesheet();
        for rule in [
            ".beside .mini",
            ".beside .mini span",
            ".beside .none",
            ".beside .said",
        ] {
            assert!(
                sheet.contains(rule),
                "the stylesheet stopped drawing {rule}"
            );
        }
        for made in ["\"mini\"", "\"none\"", "\"said working\""] {
            assert!(
                BESIDE.contains(made),
                "the margin note stopped building {made}"
            );
        }
    }

    /// Written twice for the panes' reason: ask at a width the stylesheet does
    /// not draw and the answer lands in a column nobody can see.
    #[test]
    fn the_margin_note_and_the_stylesheet_widen_at_the_same_number() {
        assert!(BESIDE.contains("(min-width:1440px)"), "{BESIDE}");
        assert!(
            crate::web::page::stylesheet().contains("(min-width:1440px)"),
            "the stylesheet no longer puts the margin note beside the note"
        );
    }

    /// A swap replaces the pane whole, so its aside becomes a different, empty
    /// element. Drop either half of the say-and-listen and the margin note is
    /// right on a hard load and stale on every press after.
    #[test]
    fn a_pane_swap_tells_the_margin_note_the_note_changed() {
        assert!(PANES.contains("\"noda:read\""), "{PANES}");
        assert!(BESIDE.contains("\"noda:read\""), "{BESIDE}");
    }

    /// A string in JavaScript against a `match` arm in Rust, so a name changed
    /// on one side compiles, passes, and quietly costs a reader the whole
    /// stylesheet on every press — the answer still being correct is what makes
    /// it silent.
    #[test]
    fn every_fetch_asks_for_a_part_the_server_can_send() {
        use crate::web::{PART, Part};
        for (script, what, part) in [
            (PANES, "the pane swap", Part::Read),
            (PANES, "the index column", Part::Index),
            (PANES, "going back to the listing", Part::Screen),
            (BESIDE, "the margin note", Part::Rows),
            (STANDING, "the poll", Part::News),
        ] {
            let asked = format!("\"{PART}\": \"{}\"", part.name());
            assert!(script.contains(&asked), "{what} stopped asking for {asked}");
        }
    }

    /// And each still reads the answer by looking for what it wants rather than
    /// assuming a shape, which is what makes the header an optimisation instead
    /// of a protocol.
    #[test]
    fn a_whole_page_would_still_answer_every_one_of_them() {
        for (script, looked_for) in [
            (PANES, "sent.querySelector(\".pane.read\")"),
            (PANES, "sent.querySelector(\"main.rows\")"),
            (BESIDE, "sent.querySelectorAll(\"main.rows a.row\")"),
            (STANDING, "fresh.querySelector(\"main\")"),
        ] {
            assert!(
                script.contains(looked_for),
                "an answer is being taken apart by position rather than by {looked_for}"
            );
        }
    }

    /// The rows a search replaces are the rows the filter holds a list of, and
    /// the two scripts are the two halves of that. Drop either and the listing
    /// goes on filtering elements that are no longer in the document — a page
    /// that looks right until the next keystroke, which is the kind of failure
    /// no Rust test can see.
    #[test]
    fn replacing_the_rows_tells_the_filter_to_read_them_again() {
        assert!(PANES.contains("\"noda:rows\""), "{PANES}");
        assert!(LISTING.contains("\"noda:rows\""), "{LISTING}");
    }

    /// Going back is answered rather than reloaded, and only above the width
    /// where anything was ever pushed. Below it every press was a navigation,
    /// so the history is not this script's to answer for.
    #[test]
    fn going_back_asks_the_server_rather_than_the_page() {
        assert!(PANES.contains("popstate"), "{PANES}");
        assert!(
            PANES.contains("if (!wide.matches) return location.reload();"),
            "the narrow screen stopped falling back to a reload"
        );
    }

    /// And sending a search is answered the same way — but only where a
    /// scriptless press would have stayed on the listing. In the index column
    /// of a note page the same form is the way *to* the listing, and answering
    /// it in place would leave the note on the screen.
    #[test]
    fn a_search_is_answered_in_place_only_on_the_listing() {
        assert!(PANES.contains("submit"), "{PANES}");
        assert!(
            PANES.contains("app.classList.contains(\"at-list\")"),
            "the search stopped checking which screen it is on"
        );
        assert!(
            PANES.contains("location.replace(where)"),
            "nothing falls back to the address the form would have gone to"
        );
    }

    /// **The address moves on the press, not on the answer.** Both of these
    /// push before they fetch, and a reader who presses back while one is in
    /// flight has an entry to go back to; pushing afterwards left them going
    /// back past the page they were standing on and out of the notebook. It is
    /// also why the failures use `replace` — the entry is already there.
    #[test]
    fn a_press_pushes_its_address_before_it_asks_for_the_answer() {
        // Read from where each of them starts, because `bring` fetches a
        // `where` of its own — finding the first one in the file would be
        // asking the question about the wrong function.
        for (what, from, push, ask) in [
            (
                "the swap",
                "const swap = async",
                "history.pushState",
                "fetch(href",
            ),
            (
                // The search and the order are one function now — they are the
                // same press, and this is the property they share.
                "the relisting",
                "const relist = async",
                "history.pushState",
                "fetch(where",
            ),
        ] {
            let at = PANES
                .find(from)
                .unwrap_or_else(|| panic!("{what} is not in this script any more"));
            let rest = &PANES[at..];
            let pushed = rest
                .find(push)
                .unwrap_or_else(|| panic!("{what} stopped pushing its address"));
            let asked = rest
                .find(ask)
                .unwrap_or_else(|| panic!("{what} stopped asking for its answer"));
            assert!(pushed < asked, "{what} asks before it pushes");
        }
        assert!(!PANES.contains("location.assign("), "{PANES}");
        // And both presses go through it rather than one of them growing its
        // own copy, which is how the two would come to disagree about what
        // happens when the fetch fails.
        assert_eq!(PANES.matches("relist(").count(), 2, "{PANES}");
    }
}
