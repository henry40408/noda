//! The enhancement layer: the only part of this interface allowed to be absent.
//!
//! Every page here works with scripts turned off, and six pull requests were
//! spent making sure of it before a line of this file existed. That order was
//! deliberate — write the script first and the scriptless path quietly loses a
//! corner nobody notices — and it decides what this file may do. **Nothing here
//! adds a capability. Everything here removes a wait** — with one exception,
//! set out below, which is a fact no server could have stated.
//!
//! Three waits, specifically. The first two the design named up front:
//!
//! * **The listing waits for a round trip to narrow itself.** Every fact a
//!   title-or-tag query needs is already on the page, so the round trip buys
//!   nothing but latency. It is spent anyway when the query needs the body,
//!   which the page does not carry.
//! * **The network screen waits by reloading itself whole.** `<meta refresh>`
//!   is how a page with no script comes back for news; a fetch of the same URL
//!   is the same news without the flash, the scroll jump, and the field losing
//!   focus.
//!
//! And the third, which the wide layout added: **what points at a note waits
//! behind a press.** On a screen with room beside the prose, the Links page and
//! the round trip to it buy nothing that could not be on the screen already.
//! The button stays, it is still the only way there on a phone, and the answer
//! shown is the one that route sends.
//!
//! Two more came with the split screen, and they are the same wait seen from
//! either end of one press. **Sending the search waits for the page to be
//! built again** — the answer is a column of rows, and everything around it is
//! already on the screen it would be drawn on. **Going back waits for the same
//! thing**, and had been paying for it in full: a press of back was a reload,
//! which is the one navigation nobody chose to make. Both are answered by the
//! route the address names, in the shape that route sends.
//!
//! ## And one thing here does add something
//!
//! **The rule above is not quite the whole of it, and the exception is worth
//! stating rather than smuggling in.** A stamp in a note's frontmatter is an
//! instant, and an instant is not a day until somebody says where they are
//! standing. Nothing in a request says so — there is no header for it, and
//! asking would be a question the reader never agreed to answer — so the server
//! genuinely cannot render `2026-08-15T23:30:00Z` as the day it was, because
//! for half the world it was the sixteenth.
//!
//! So `script::STAMPS` says the same instant again in the reader's own zone.
//! That is not a wait removed. It is the one fact this interface cannot state
//! from the server at all, and the enhancement layer is the only place it can
//! be stated from.
//!
//! What keeps it honest is the half that did not change. The scriptless page is
//! not *wrong*, it is unconverted: it shows the stamp exactly as the file holds
//! it, `Z` or `+08:00` and all, which is the one rendering that cannot be
//! misread and is what `noda show` and `noda ls -l` print. A reader with no
//! script gets the notebook's own answer; a reader with one gets the same
//! instant in their own words. Neither is told something the other is not.
//!
//! It follows that a listing's day may differ from the scriptless one by a day,
//! and that is the point of it rather than a defect in it — but it is the first
//! time anything here has drawn something the server would have drawn
//! differently, and pretending otherwise would make the rule above worthless
//! for the next thing that wants an exception.
//!
//! Two stamps are deliberately not touched. A `due:` date on the todo screen is
//! a calendar day somebody typed, not an instant, and `noda todo` already
//! decides "has it passed" against git's own offset — converting it would move
//! an item due today into tomorrow for a reader in another zone. And the count
//! beside a tag wears the same class as a stamp and is not one, which is why
//! what this script looks for is `<time datetime>` and never a class.
//!
//! ## The rule both halves are written against
//!
//! *The server is the only authority, and the script must never be able to
//! answer differently — only sooner, or not at all.*
//!
//! For the network screen that is free: the script fetches the page the server
//! would have sent and puts it on the screen. There is no second opinion in it,
//! and even "is it still running?" is read off the server's own `<meta
//! refresh>` rather than worked out again here.
//!
//! For the listing it costs an argument, because a filter genuinely is a second
//! implementation of `query.rs` — a small one, and the only one this project
//! permits itself. What keeps it honest is that it is allowed to be *narrower*
//! than the server and never wider:
//!
//! | the query holds | the script can say | why |
//! | --- | --- | --- |
//! | only `tag:` / `title:` / `id:` | the whole answer | the page carries every field those terms read |
//! | a bare word, or `text:` | part of the answer | `Field::Text` reads the body too, and a row's body is not here. Title-and-tag hits are a **subset** of text hits, so what is shown is right and possibly short — never wrong |
//! | a *negated* bare word or `text:` | nothing | this is the case that inverts. `-budget` asks for notes without the word; the script cannot see the body, so it would *keep* a row the server would drop. Widening is the one thing the rule forbids, so the filter stands aside |
//!
//! The third row is why this is a table and not a sentence. The subset property
//! that makes the second row safe is destroyed by a leading `-`, and it is not
//! visible from the design — it comes from `Term::matches` returning `found !=
//! negated`. It was found by writing the filter, not by planning it.
//!
//! A query that does not parse is the same case as the third row: the listing
//! stands aside and says so. It does not repeat the server's complaint about
//! it, because a half-typed query is not a mistake and the server already has
//! the only wording of what a real one is.
//!
//! ## What the script is allowed to touch
//!
//! Only what the server put there. The rows are all on the page whatever is
//! typed — the ones the query excludes arrive with `hidden` on them — so
//! filtering here and filtering there are the same operation on the same DOM,
//! and the scriptless page is not a different page with fewer rows in it. The
//! script does not touch anything until the first keystroke: until then what is
//! on the screen is the server's answer to the URL, and it is already correct.
//!
//! ## And what it is allowed to ask for
//!
//! Every fetch below takes one region out of the page it gets and drops the
//! rest: the stylesheet, both scripts and the whole rail are already on the
//! screen that region is going into. On a note that is 48 of 52 KB thrown away,
//! on the round trip a reader is waiting through.
//!
//! So each fetch says which region it will use — `x-noda-fragment`, one name,
//! the vocabulary in `web::Part` — and the server sends that region out of the
//! same function the whole page is built from. **This is the enhancement rule
//! rather than an exception to it.** What comes back is a piece of the server's
//! answer, never a different one; every fetch here parses what arrives and asks
//! it for the element it wants, so a server that ignored the header would still
//! be answering correctly, and only a reader without a script — who sends no
//! such header — is ever sent a page to look at.

/// The listing's filter, and the grouping it drew on the way.
///
/// Reads the rows out of the DOM rather than being handed a copy of them. The
/// alternative — a `<script type="application/json">` beside the list — would
/// put every title and tag on the page twice, and the second copy is the one
/// that goes stale. `textContent` also unwraps the server's `<mark>`s for free,
/// which is exactly the string the query has to be matched against.
///
/// ## The grouping is the one thing here that never stands aside
///
/// `page::grouping` draws what the server parsed; this redraws it on every
/// keystroke, from the same `parse` the filter runs on. That is not a third
/// implementation — it is the one already required by the table above, used for
/// a second thing.
///
/// It also answers in the two cases the *filter* refuses to. A negated bare
/// word makes the filter stand aside, and a query still being typed does not
/// parse at all — but a grouping is a fact about the words, not about the
/// notes, so where there is a parse there is a grouping, whatever the rows are
/// doing. Where there is not, the box empties: half a query has no grouping
/// yet, and drawing the last complete one under a line that no longer says it
/// would be the one thing worse than saying nothing.
pub const LISTING: &str = r#"
(() => {
  const app = document.querySelector(".app");
  const form = document.querySelector("form.searchbar");
  const list = document.querySelector("main.rows");
  if (!form || !list) return;
  const field = form.querySelector("input[name=q]");
  if (!field) return;

  // Read at startup and read again when the server answers a search without the
  // page being reloaded: `script::PANES` replaces the rows and everything under
  // the field, and says so. Held in `let` rather than `const` for exactly that
  // reason — an element taken out of the document is an element this would go
  // on filtering, invisibly, for the rest of the session.
  //
  // The field and the form itself are never replaced, which is why they are
  // read once above: the reader may have a cursor in one of them.
  let count, hint, parsed, problem, empty, asked, notes, total;
  const look = () => {
    count = document.querySelector(".topbar .count");
    hint = form.querySelector(".hint");
    parsed = form.querySelector(".parse");
    problem = form.querySelector(".problem");
    empty = list.querySelector(".empty");
    asked = empty && empty.querySelector(".asked");
    // Each row, as the query sees it. A tag cannot contain a comma — `note.rs`
    // refuses one — so the line the server joined is safe to take apart again.
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

  // `query::split`, said again. Quotes hold a piece together so that a tag with
  // a space in it survives as one term; an unclosed quote runs to the end,
  // because the character that would close it is usually the next one typed.
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

  // `said` is the token as it was typed, kept because the grouping is drawn
  // from it: what goes on the screen has to be the reader's own line, and
  // everything else here is what the line was read to mean.
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

  // Groups that must all match, each satisfied by any one of its terms. `null`
  // for anything that does not parse, which is one answer for two cases the
  // script treats alike: half a query, and a query it is not entitled to run.
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

  // `note::normalize_id`. An id is read off a screen and typed back, and the
  // characters that get confused doing it are folded together.
  const fold = (id) => id.toLowerCase().replace(/[il]/g, "1").replace(/o/g, "0");

  const hits = (parsed, note) => {
    const value = parsed.value.toLowerCase();
    const inWords = note.words.toLowerCase().includes(value);
    let found;
    if (parsed.field === "tag") found = note.tags.includes(parsed.value);
    else if (parsed.field === "id") found = fold(note.id).startsWith(fold(parsed.value));
    else if (parsed.field === "title") found = inWords;
    // Everything a bare word can reach *here*. The body is the part that is
    // missing, and the table in `script.rs` is where that is accounted for.
    else found = inWords || note.tags.some((tag) => tag.toLowerCase().includes(value));
    return found !== parsed.negated;
  };

  // `page::highlight`: the earliest match wins, and the longest of the ones
  // starting there, so two terms overlapping mark one run rather than nesting.
  // Built as nodes rather than as markup — a title is the reader's own text,
  // and the way to be sure it is never read as HTML is to never make it into a
  // string that could be.
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

  // `page::grouping`, again: a pill per group, `or` inside one and `and`
  // between them. Built as nodes for the same reason `paint` is — the text in
  // it is the reader's, and the way to be sure it is never read as markup is
  // to never make it into a string that could be.
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

  // `full` is whether what is on the screen is the whole answer. When it is
  // not, the count would be a lie told in the server's own voice, so the hint
  // says whose answer it is and which key finishes it.
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
    // The server's complaint is about the query in the URL, and the field no
    // longer holds it. Nothing here writes a new one: a query being typed is
    // half-written by definition, and the wording of what a whole one looks
    // like belongs to the parser that has all of it.
    if (problem) problem.hidden = true;

    const tokens = split(field.value);
    // Drawn before anything is decided about the rows, and from the same parse
    // the deciding uses. Both of the ways out below leave the listing alone;
    // neither is a reason to leave the grouping wrong.
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
  // The rows the server just sent are the answer to the query in the address,
  // and nothing here re-filters them: what is on the screen is the server's,
  // until the next keystroke asks this to narrow it.
  if (app) app.addEventListener("noda:rows", look);
})();
"#;

/// The network screen's poll.
///
/// The page the server would have sent, put on the screen without the reload.
/// It asks for its own URL and swaps `<main>`, so every word on it is still the
/// server's — including whether an errand is still running, which is read off
/// the `<meta refresh>` the scriptless page steers by rather than decided here.
/// When that meta stops arriving, the polling stops with it, exactly as the
/// reloading does.
pub const STANDING: &str = r#"
(() => {
  const meta = document.querySelector('meta[http-equiv="refresh"]');
  if (!meta) return;
  let main = document.querySelector("main");
  if (!main) return;

  // The server's own interval, in the server's own units. Taking it from the
  // meta rather than repeating the number is what keeps one of them from being
  // changed alone.
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
      // A phone that lost the tailnet mid-sync. The errand is running on the
      // server either way, so the right move is to ask again rather than to
      // put an error of the script's own invention over the server's page.
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
/// **This is the third wait, and it is the one the layout created.** A note
/// page is sent without the listing beside it, because the listing is about 290
/// bytes a note — 57KB at two hundred, half a megabyte at two thousand — and
/// below 1024px not one of those bytes is ever drawn. So the page carries the
/// pane's frame and this asks for the rest, and only where the column is on
/// screen.
///
/// It obeys the same rule as the rest of this file: *the server is the only
/// authority.* The rows it inserts are the rows `/nb/<book>` sent, lifted out
/// of that page's own `main.rows` rather than built here, so the listing has
/// exactly one renderer and this cannot disagree with it — only be later, or
/// absent.
///
/// ## Two things, and the second is why the first is worth doing
///
/// **Bring it.** On a note route, when the pane is empty and the width holds
/// three columns, fetch the listing and put its rows in. `indexed` goes on
/// first, synchronously: the column has to exist before the first paint or the
/// reading pane is laid out at one width and then again at another, which is
/// the flicker this exists to avoid.
///
/// **Keep it.** Picking a note replaces the reading pane and leaves the rows
/// alone. Without this every press would be a full navigation that threw the
/// listing away and asked for it again — a listing blinking out and back on
/// every note, when the reason to keep it on screen was that it *stays* while
/// you read. It is also what makes the fetch above happen once rather than once
/// per note.
///
/// There is no loading state on the pane, and that is a consequence of keeping
/// it rather than an omission: after the first arrival there is nothing to
/// load, and a notice that appeared on every note would be the flicker rather
/// than a report of it.
///
/// Every row is still a link to a page that renders on its own. With no script,
/// or on a screen too narrow to hold both panes, a press is an ordinary
/// navigation — which is what a note page has always been.
pub const PANES: &str = r#"
(() => {
  const app = document.querySelector(".app.split");
  if (!app) return;
  const wide = matchMedia("(min-width:1024px)");

  const book = () => {
    const form = app.querySelector(".index form.searchbar");
    return form ? form.getAttribute("action") : null;
  };

  // Asked when the pane is empty, which — because picking a note keeps it — is
  // once on a note page opened cold and never again while the reader stays in
  // the notebook.
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
      // The tailnet went away. The note is on the screen and whole, which is
      // the page the reader asked for; a column that never arrives is the
      // scriptless layout, and that is a working one.
      return;
    } finally {
      asking = false;
    }
    const sent = new DOMParser().parseFromString(text, "text/html");
    if (box.firstElementChild) return;
    column(sent, false);
  };

  // The index column, as the server now has it, put on the screen.
  //
  // **The form is never replaced, only what hangs off it.** There may be a
  // cursor in that field, and `script::LISTING` is listening to the element the
  // page was loaded with — replacing it would drop both, and dropping the
  // listener would leave a listing that no longer filters as you type. So the
  // input stays and everything after it goes: the hint, the grouping, and the
  // complaint about a query that does not parse are the server's answer to the
  // query, taken whole.
  //
  // `retype` is whether the field itself is the server's to set. It is when the
  // address changed under the reader — going back is arriving at a query
  // somebody typed a while ago — and it is not when they are the one who just
  // typed it, where the answer catching up must not take the keystrokes made
  // while it was in flight.
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
    // The rows are different elements now, and the filter holds a list of the
    // ones it was built with. Said rather than observed, for the reason the
    // read pane says it: one script's doing is another script's fact.
    app.dispatchEvent(new CustomEvent("noda:rows"));
    return true;
  };

  // The row the reading pane is showing. Read off the address rather than
  // remembered, so it is right after a swap and right after a hard load.
  const mark = () => {
    const at = location.pathname;
    for (const row of app.querySelectorAll(".index main.rows a.row")) {
      row.classList.toggle("here", row.getAttribute("href") === at);
    }
  };

  // **The address moves first, and the answer catches up.**
  //
  // A navigation changes the address the moment it starts, and this has to do
  // the same: a reader who presses a row and then presses back before the note
  // lands would otherwise go back past the page they are standing on — there
  // was no entry for it yet — and leave the notebook entirely. So the entry is
  // pushed on the press, and the ways this can fail all end somewhere that
  // address is correct. `location.replace` and not `assign`, because the entry
  // is already there and a navigation would be a second one.
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
    // The reading pane is replaced whole, so anything that hangs off the note
    // being read is now a different, empty element. Said rather than observed:
    // `script::BESIDE` is the one listener today, and a pane swap is a fact
    // about this script, not something another one should have to infer from
    // the DOM changing under it.
    app.dispatchEvent(new CustomEvent("noda:read"));
  };

  // Back to the listing: the rows it had, and the pane the note was standing
  // in. Two panes of one answer and one round trip, because a screen half
  // arrived is a screen that flickers.
  const screen = async (href) => {
    let text;
    try {
      const answer = await fetch(href, {
        headers: { accept: "text/html", "x-noda-fragment": "screen" },
      });
      if (!answer.ok) return location.reload();
      text = await answer.text();
    } catch {
      // Nothing arrived, so nothing is claimed about where the reader is. A
      // reload asks the same question by the same route, and it is what going
      // back did before any of this.
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

  // The listing's own column, asked for and put on the screen.
  //
  // Two presses arrive here and they are the same press: sending the search and
  // choosing an order both change which rows there are and nothing else, and
  // the page they would be drawn on is the one already up. The address moves
  // first for the reason `swap` gives, and every way this can fail ends at the
  // address a scriptless press would have gone to.
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
    // `false`: the reader is the one who just typed in that field, and the
    // answer catching up must not take the keystrokes made while it was in
    // flight. An order press does not touch the field at all, so the same
    // answer is right for both.
    if (!column(sent, false)) return location.replace(where);
  };

  // Sending the search: the rows change and the page does not.
  //
  // **Only on the listing screen.** The same form is in the index column of a
  // note page, and there a press of ⏎ is the way *to* the listing — a
  // navigation that leaves the note behind. Answering it here would keep the
  // note on the screen, which is not the answer the server would have given.
  //
  // The address it asks for is the address the form would have submitted,
  // `q=` and all, so what lands in the history is what a scriptless press
  // leaves there and back arrives at the same place either way.
  app.addEventListener("submit", (event) => {
    if (event.defaultPrevented) return;
    const form = event.target.closest(".index form.searchbar");
    if (!form || !app.classList.contains("at-list")) return;
    const field = form.querySelector("input[name=q]");
    const action = form.getAttribute("action");
    if (!field || !action) return;
    event.preventDefault();
    // **Every field, not just the one that was typed in.** The form also holds
    // what order the listing is in, and a press that rebuilt the address out of
    // `q` alone put the notes back in the default order without saying so —
    // which is the script answering differently from the server, the one thing
    // this file may not do. `FormData` is what the form would have submitted.
    const where = action + "?" + new URLSearchParams(new FormData(form));
    relist(where);
  });

  // Choosing an order: the same press, from the other control on that form.
  //
  // **Only on the listing screen**, for the reason the search gives one comment
  // up: the same form is in the index column of a note page, and there a press
  // is the way *to* the listing. Answering it here would keep the note on the
  // screen, which is not the answer the server would have given.
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
    // The margin note's links are the same kind of thing as the index's rows —
    // a note in this notebook, one press away — so they travel the same way.
    // Left out, pressing one would be a full navigation that threw away the
    // listing beside it and asked for it again, which is the flicker `swap`
    // exists to avoid.
    const row = event.target.closest(".index main.rows a.row,.read .beside .mini");
    if (!row || !wide.matches) return;
    const href = row.getAttribute("href");
    if (!href) return;
    event.preventDefault();
    swap(href);
  });

  // A press pushed an address, so going back has to put what that address names
  // on the screen. Asking the server for it is the same answer by the same
  // route — the one the reader would have got by pressing reload, minus the
  // reload.
  addEventListener("popstate", () => {
    // Below the breakpoint nothing was ever pushed: every press at this width
    // is a navigation, so anything arriving here is history this script does
    // not own, and a reload is both correct and what has always happened.
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
/// The third wait, and the one the module's opening list did not have room for:
/// backlinks are a page of their own behind the Links button, and on a screen
/// wide enough to hold them beside the prose, that press and its round trip buy
/// nothing. Nothing new is reachable here. The button is still there, it is
/// still the only way on a phone, and this answers with the same route's answer.
///
/// ## Why the server does not send it
///
/// Because of what it costs, and where. `backlinks_to_note` walks every note in
/// the notebook — measured at about 8% on top of `ls` at two thousand notes,
/// which is cheap for a page that was asked for and pure waste for a column no
/// screen under 1440px draws. Worse, a note page today reads exactly one file:
/// `resolve` looks at filenames and nothing opens a second note. Putting the
/// aside in the markup would turn one read into two thousand on every phone.
///
/// So the server writes the box, hidden, and this fills it where it shows.
///
/// ## `margined`, and why the class comes before the answer
///
/// The same bargain `indexed` makes on the index pane. The class goes on
/// synchronously, before the first paint, and it is what lets the layout keep
/// 236px for a column whose contents are still in flight — so the prose is laid
/// out once and the answer lands into space already held for it, rather than
/// pushing a short note's body up as it arrives. No script, no class, no
/// reserved column: the note keeps the centred measure it has at 1024px.
///
/// ## The one loading state on the page
///
/// The index pane opposite says nothing while it fills, because after the first
/// arrival it has rows and keeps them. This column is the other case: it is
/// empty on every note, the walk behind it is the whole notebook, and an
/// unexplained gap in a column that says "Backlinks" reads as a column that
/// failed. So it says so, once, in the status screen's own breathing dot.
///
/// An answer of none is an answer and is drawn as one. A fetch that never comes
/// back is not: the box goes away again and the reader is left with the note,
/// whole, which is the page they asked for.
pub const BESIDE: &str = r#"
(() => {
  const app = document.querySelector(".app.split");
  if (!app) return;
  const wide = matchMedia("(min-width:1440px)");

  // The note the column is about, read off the address rather than remembered
  // — right after a swap, right after a hard load, and the string the fetch is
  // built from, so the two can never be about different notes.
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
    // The reading pane may not hold a note at all any more — going back to the
    // listing puts the notebook's own page there. Forgetting what was asked is
    // what lets the same note, opened again, ask again: the aside the answer
    // was going into went away with the pane it was in.
    if (!app.classList.contains("at-note")) {
      asked = null;
      return;
    }
    if (!wide.matches) return;
    const aside = app.querySelector(".pane.read .beside");
    const answer = aside && aside.querySelector(".answer");
    if (!answer) return;
    // Before the first paint and before the first await: the column has to be
    // reserved while the answer is still coming, or the note is laid out at one
    // measure and then again at another.
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
      // Nothing arrived, so nothing is claimed. The box closes, what was asked
      // for is forgotten so widening or the next note may try again, and what is
      // left on the screen is a whole note and a Links button — the page with no
      // script. Both are conditional on this still being the note being read:
      // the reader may have moved on to one whose answer is on its way, and
      // clearing that would show it arriving twice.
      if (asked === at) {
        asked = null;
        aside.hidden = true;
        answer.replaceChildren();
      }
      return;
    }

    // The reader may have moved on while the notebook was being walked, and the
    // pane they moved to is a different element than the one asked for.
    if (location.pathname !== at) return;
    const box = app.querySelector(".pane.read .beside");
    const into = box && box.querySelector(".answer");
    if (!into) return;

    // The server's answer, said in the margin's own shape. `bring` lifts the
    // listing's rows because that column *is* the listing; this one is not — it
    // is 236px beside prose, where a row's tags would wrap into a paragraph.
    // What is taken is which notes and what they are called; the shape is the
    // column's, and the id under each title is the same identity the index puts
    // on its rows.
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

  // Widening past the breakpoint is the reader asking for the column, so it is
  // when the question gets asked. Narrowing asks nothing and undoes nothing.
  wide.addEventListener("change", ask);
  app.addEventListener("noda:read", ask);
  ask();
})();
"#;

/// Every stamp on the page, said again where the reader is standing.
///
/// The marker is `<time datetime>` and never a class: `.when` is typography on
/// these pages and gets used for a tag's note count and for a `due:` date, and
/// neither is an instant. What carries a `datetime` is what noda wrote out of a
/// note's frontmatter, and that is the only thing converted.
///
/// A page is repainted rather than watched. `script::PANES` already says when
/// it has replaced the rows or the note — `noda:rows` and `noda:read`, the same
/// two `script::BESIDE` listens for — and re-reading `datetime` makes a second
/// pass over an element that was already converted produce exactly what the
/// first one did.
pub const STAMPS: &str = r#"
(() => {
  // English, and only the zone is the reader's. Every other string noda prints
  // is English, and what a reader in Taipei needs from this is not a Chinese
  // month name — it is the hour they were actually at their desk.
  const clock = new Intl.DateTimeFormat("en", { dateStyle: "medium", timeStyle: "short" });

  // The listing keeps `YYYY-MM-DD`, which is the shape `noda ls -l` prints and
  // the shape a column of them reads as. Built by hand rather than asked of
  // `Intl`, because what is wanted is not a locale's idea of a short date — it
  // is that one spelling, in the reader's zone.
  const iso = (at) => {
    const pad = (n) => String(n).padStart(2, "0");
    return at.getFullYear() + "-" + pad(at.getMonth() + 1) + "-" + pad(at.getDate());
  };

  const paint = (root) => {
    for (const said of root.querySelectorAll("time[datetime]")) {
      const raw = said.getAttribute("datetime");
      const at = new Date(raw);
      // A note may carry a stamp noda never wrote — an import leaves what it
      // found — and a date nothing can parse is left exactly as it reads. It
      // is the file's own words, and a guess would be worse than them.
      if (Number.isNaN(at.getTime())) continue;
      said.textContent = said.hasAttribute("data-clock") ? clock.format(at) : iso(at);
      // What the file actually holds, for whoever wants to know which instant
      // this was. The page no longer shows it, and nothing else records it.
      said.title = raw;
    }

    // A row prints its day twice and the stylesheet draws one at a time,
    // depending on how wide the column is. Only one of them carries the stamp
    // — the second copy would be about thirty bytes a note on the one page
    // where bytes are counted — so the other is told what it came to.
    for (const row of root.querySelectorAll("a.row")) {
      const said = row.querySelector("time.when");
      const beside = row.querySelector(".ident .day");
      if (!said || !beside) continue;
      beside.textContent = said.textContent;
      beside.title = said.title;
    }
  };

  paint(document);

  // Said rather than observed, for the reason the pane swap gives: one
  // script's doing is another script's fact.
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

    /// The same pact, for the stamps. What this script converts is anything
    /// carrying a `datetime`, which is a decision as much as a selector: `.when`
    /// is typography on these pages and gets worn by a tag's note count and by
    /// a `due:` date, and converting either would be wrong in a way nobody
    /// would notice until they were in another country.
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
        // English is the decision, not the default. `undefined` here would
        // follow the browser's locale and print a month name in a language
        // nothing else on any of these pages is written in; what the reader
        // needs from this is the hour they were at their desk, not a
        // translation.
        assert!(STAMPS.contains("Intl.DateTimeFormat(\"en\""), "{STAMPS}");
        // The other half of "never a class" is asserted from the page's side,
        // where it can actually fail: `a_due_date_is_not_an_instant_and_is_not
        // _marked_as_one` in `web::page` holds the todo screen to writing no
        // `<time>` at all. Asserting it from in here would mean grepping this
        // string for a selector it does not contain, which passes for the
        // wrong reason the moment somebody writes a different one.
    }

    /// Not a test of the JavaScript — nothing here runs it, and `e2e/` does.
    /// This is a test that the two halves of one decision stayed together: the
    /// filter reads the classes the pages write, and renaming one without the
    /// other is a silent no-op that every Rust test would still pass.
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

    /// The grouping is drawn twice — by `page::grouping` when the page arrives
    /// and by this on every keystroke after — so the two have to build the same
    /// markup. They cannot share a function across the language boundary, and a
    /// pill that changed shape the moment a key was pressed would be a flicker
    /// nothing else here would catch. What can be checked is that both write
    /// the classes the stylesheet draws, and that the stylesheet draws them.
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

    /// The same for the panes, and it matters more here than anywhere else in
    /// this file: what this script reads is the *other* page's markup, fetched
    /// at runtime. A class renamed in `page.rs` breaks it with no compile error
    /// and no failing Rust test — the note simply arrives beside an empty
    /// column, on a screen size nobody's unit tests have.
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
        // The order rides in the form's own fields, so the press that sends the
        // search has to send the form and not a line built out of `q`. Rebuilt
        // by hand, it would drop the order silently — the notes come back, in
        // the wrong order, with nothing to say why.
        assert!(PANES.contains("new FormData(form)"), "{PANES}");
        let sheet = crate::web::page::stylesheet();
        for rule in [".sortbar", ".sortbar a[aria-current] .pill"] {
            assert!(
                sheet.contains(rule),
                "the stylesheet stopped drawing {rule}"
            );
        }
    }

    /// The breakpoint is written twice — once in the stylesheet, once here —
    /// and the two have to be the same number or the script asks for a column
    /// that is not on the screen, or leaves one empty that is.
    #[test]
    fn the_script_and_the_stylesheet_split_at_the_same_width() {
        assert!(PANES.contains("(min-width:1024px)"), "{PANES}");
        assert!(
            crate::web::page::stylesheet().contains("(min-width:1024px)"),
            "the stylesheet no longer splits at 1024px"
        );
    }

    /// The same, for the screen that polls. Both of these are the server's own
    /// facts being read back, and both are silent when they go missing.
    #[test]
    fn the_poll_steers_by_the_meta_the_scriptless_page_steers_by() {
        assert!(
            STANDING.contains(r#"meta[http-equiv="refresh"]"#),
            "{STANDING}"
        );
        assert!(STANDING.contains("querySelector(\"main\")"), "{STANDING}");
    }

    /// The margin note reads two pages it did not write: the note page it sits
    /// in, and the backlinks page it fetches. Both break it silently — the
    /// column simply arrives empty, at a width no unit test has.
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

    /// The shapes it builds are drawn by the stylesheet and nowhere else. This
    /// is the same pairing the grouping needs: markup written in JavaScript
    /// against rules written in Rust, with nothing between them but a name.
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

    /// The margin note's breakpoint, written twice for the same reason the
    /// panes' is: ask at a width the stylesheet does not draw and the answer
    /// lands in a column nobody can see.
    #[test]
    fn the_margin_note_and_the_stylesheet_widen_at_the_same_number() {
        assert!(BESIDE.contains("(min-width:1440px)"), "{BESIDE}");
        assert!(
            crate::web::page::stylesheet().contains("(min-width:1440px)"),
            "the stylesheet no longer puts the margin note beside the note"
        );
    }

    /// A swap replaces the reading pane whole, so the aside in it becomes a
    /// different, empty element. One script says so and the other listens; drop
    /// either half and the margin note is right on a hard load and stale on
    /// every press after it.
    #[test]
    fn a_pane_swap_tells_the_margin_note_the_note_changed() {
        assert!(PANES.contains("\"noda:read\""), "{PANES}");
        assert!(BESIDE.contains("\"noda:read\""), "{BESIDE}");
    }

    /// Every fetch here names a part of a page, and the server has a vocabulary
    /// of them. The two halves are a string in JavaScript and a `match` arm in
    /// Rust, so a name changed on one side alone compiles, passes and quietly
    /// costs a reader the whole stylesheet on every press — the answer is still
    /// correct, which is exactly what makes it silent.
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

    /// And every one of them still reads the answer by looking for what it
    /// wants, rather than assuming the shape of what arrived. That is what
    /// makes the header an optimisation instead of a protocol: a server that
    /// ignored it would send the whole page, and every one of these would find
    /// its element in it exactly as before.
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
