//! The enhancement layer: the only part of this interface allowed to be absent.
//!
//! Every page works with scripts off. **Nothing here adds a capability;
//! everything here removes a wait** — a round trip to narrow the listing, a
//! whole-page reload on the network screen, a press to see backlinks on a wide
//! screen, a rebuilt page on search and on back — with one exception.
//!
//! ## The exception: stamps
//!
//! Nothing in a request says where the reader is, so the server cannot render
//! `2026-08-15T23:30:00Z` as the day it was for them. `STAMPS` converts it to
//! the reader's zone. The scriptless page is unconverted rather than wrong: it
//! shows the stamp as the file holds it, `Z` and all, as `noda show` does. A
//! `due:` date is left alone — it is a calendar day somebody typed, and
//! converting it would move an item due today into tomorrow.
//!
//! ## The rule
//!
//! *The server is the only authority; the script may answer sooner, or not at
//! all, never differently.*
//!
//! The network screen gets that free: it fetches the server's own page and
//! reads "still running?" off its `<meta refresh>`. The listing filter is a
//! second implementation of `query.rs`, and stays honest by being *narrower*
//! than the server, never wider:
//!
//! | the query holds | the script can say | why |
//! | --- | --- | --- |
//! | only `tag:` / `title:` / `id:` / `pinned:` | the whole answer | the page carries every field those terms read — a pinned row is the one wearing the mark |
//! | a bare word, or `text:` | part of the answer | `Field::Text` reads the body too, which is not on the page; title-and-tag hits are a **subset** of text hits, so what is shown is right and possibly short |
//! | a *negated* bare word or `text:` | nothing | `-budget` wants notes without the word, and the script cannot see bodies, so it would *keep* a row the server drops. The filter stands aside |
//!
//! The third row is not visible from the design: it follows from
//! `Term::matches` returning `found != negated`, which inverts the subset.
//! A query that does not parse also makes the listing stand aside, without
//! repeating the server's complaint — a half-typed query is not a mistake.
//!
//! The script touches only what the server put there: excluded rows arrive
//! `hidden`, so filtering here and there are the same operation on one DOM,
//! and nothing changes before the first keystroke.
//!
//! Each fetch names the region it will use in `x-noda-fragment` (`web::Part`),
//! since the rest of a note page is 48 of its 52 KB. Every fetch still looks up
//! the element it wants in what arrives, so a server ignoring the header answers
//! correctly.

/// The listing's filter, and its grouping pills.
///
/// Reads the rows out of the DOM rather than a JSON copy, which would double the
/// page and go stale; `textContent` also unwraps the server's `<mark>`s.
///
/// The grouping (`page::grouping`, redrawn per keystroke from the filter's own
/// `parse`) never stands aside: it is a fact about the words, not the notes, so
/// it is drawn whenever the query parses — negated text included — and emptied
/// when it does not, rather than left showing a stale line.
pub const LISTING: &str = r#"
(() => {
  const app = document.querySelector(".app");
  const form = document.querySelector("form.searchbar");
  const list = document.querySelector("main.rows");
  if (!form || !list) return;
  const field = form.querySelector("input[name=q]");
  if (!field) return;

  // Re-read when `PANES` replaces the rows, or this would go on filtering
  // detached elements. The form and field are never replaced, so read once.
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
        pinned: !!row.querySelector(".pin"),
        id: row.getAttribute("href").split("/").pop(),
      };
    });
    total = notes.length;
  };
  look();
  if (!total) return;

  const FIELDS = ["tag", "title", "id", "pinned", "text"];

  // `query::split`: an unclosed quote runs to the end, its closer usually being
  // the next character typed.
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

  // `said` keeps the token as typed: the grouping draws the reader's own words.
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
    // `Term::parse` refuses any other value.
    if (field === "pinned" && value !== "true" && value !== "false") return null;
    return value ? { field, value, negated, said: token } : null;
  };

  // `null` for half a query and for one the server would refuse alike.
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

  // `note::normalize_id`.
  const fold = (id) => id.toLowerCase().replace(/[il]/g, "1").replace(/o/g, "0");

  const hits = (parsed, note) => {
    const value = parsed.value.toLowerCase();
    const inWords = note.words.toLowerCase().includes(value);
    let found;
    if (parsed.field === "tag") found = note.tags.includes(parsed.value);
    else if (parsed.field === "id") found = fold(note.id).startsWith(fold(parsed.value));
    else if (parsed.field === "title") found = inWords;
    else if (parsed.field === "pinned") found = note.pinned === (parsed.value === "true");
    // A bare word, minus the body, which is not on the page.
    else found = inWords || note.tags.some((tag) => tag.toLowerCase().includes(value));
    return found !== parsed.negated;
  };

  // `page::highlight`: earliest match wins, then longest. Built as nodes, never
  // markup, so a title can never be read as HTML.
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

  // `page::grouping`: a pill per group, `or` inside and `and` between. Nodes,
  // for `paint`'s reason.
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

  // `full`: the screen holds the whole answer. When it does not, the hint says
  // the count is the script's, not the server's.
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
    // The complaint was about the URL's query, which the field no longer holds.
    if (problem) problem.hidden = true;

    const tokens = split(field.value);
    // Before either early return below, which leave the rows but not the
    // grouping alone.
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
    // Negated text inverts the subset: see the module table.
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
  // New rows are the server's answer, left as sent until the next keystroke.
  if (app) app.addEventListener("noda:rows", look);
})();
"#;

/// The network screen's reload, as a fetch of its own URL that swaps `<main>`.
/// It polls while the answer still carries the `<meta refresh>` the scriptless
/// page steers by.
pub const STANDING: &str = r#"
(() => {
  const meta = document.querySelector('meta[http-equiv="refresh"]');
  if (!meta) return;
  let main = document.querySelector("main");
  if (!main) return;

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
      // A phone that lost the network: the errand runs regardless, so ask again.
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

/// The two panes: bringing the index pane, and keeping it.
///
/// A note page is sent without the listing — about 290 bytes a note, half a
/// megabyte at two thousand, none of it drawn below 1024px — so on a wide screen
/// this fetches `/nb/<book>` and lifts its `main.rows`: the listing keeps one
/// renderer. `indexed` goes on synchronously, before the first paint, or the
/// reading pane is laid out twice.
///
/// Picking a note then swaps only the reading pane, so the rows are fetched once
/// and stay while you read — which is also why the pane has no loading state.
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
      // A column that never arrives is the scriptless layout, which works.
      return;
    } finally {
      asking = false;
    }
    const sent = new DOMParser().parseFromString(text, "text/html");
    if (box.firstElementChild) return;
    column(sent, false);
  };

  // The index column, as the server now has it. The input is never replaced,
  // only what follows it: it may hold a cursor, and `LISTING` listens to it.
  //
  // `retype`: the server may set the field — true when the address changed
  // under the reader, false when they just typed, so keystrokes made in flight
  // survive.
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
    // New elements; the filter holds the old ones.
    app.dispatchEvent(new CustomEvent("noda:rows"));
    return true;
  };

  // Off the address, so it is right after a swap and after a hard load.
  const mark = () => {
    const at = location.pathname;
    for (const row of app.querySelectorAll(".index main.rows a.row")) {
      row.classList.toggle("here", row.getAttribute("href") === at);
    }
  };

  // The address is pushed on the press, before the fetch, as a navigation
  // would: otherwise back pressed mid-flight skips past this page and out of
  // the notebook. Failures therefore `replace`, the entry being there already.
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
    // The pane is a new element, so `BESIDE` and `STAMPS` must look again.
    app.dispatchEvent(new CustomEvent("noda:read"));
  };

  // Both panes in one round trip, so the screen never half-arrives.
  const screen = async (href) => {
    let text;
    try {
      const answer = await fetch(href, {
        headers: { accept: "text/html", "x-noda-fragment": "screen" },
      });
      if (!answer.ok) return location.reload();
      text = await answer.text();
    } catch {
      // A reload asks the same question the scriptless way.
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

  // Sending the search and choosing an order: both change only the rows. Pushes
  // first for `swap`'s reason; failures go where a scriptless press would.
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
    // `false`: keep keystrokes typed in flight; an order press leaves the field.
    if (!column(sent, false)) return location.replace(where);
  };

  // Only on the listing screen: on a note page ⏎ in this form is the way *to*
  // the listing, and answering in place would keep the note on screen.
  app.addEventListener("submit", (event) => {
    if (event.defaultPrevented) return;
    const form = event.target.closest(".index form.searchbar");
    if (!form || !app.classList.contains("at-list")) return;
    const field = form.querySelector("input[name=q]");
    const action = form.getAttribute("action");
    if (!field || !action) return;
    event.preventDefault();
    // Every field, as the form would submit: it also holds the order, which a
    // URL built from `q` alone would silently drop.
    const where = action + "?" + new URLSearchParams(new FormData(form));
    relist(where);
  });

  // The order chips, on the listing screen only for the same reason.
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
    // The margin note's links are rows too, so they swap as well.
    const row = event.target.closest(".index main.rows a.row,.read .beside .mini");
    if (!row || !wide.matches) return;
    const href = row.getAttribute("href");
    if (!href) return;
    event.preventDefault();
    swap(href);
  });

  // Back puts on screen what the address names, without a reload.
  addEventListener("popstate", () => {
    // Below the breakpoint nothing was pushed, so this history is not ours.
    if (!wide.matches) return location.reload();
    const at = location.pathname;
    // As the router tells them apart: only `/nb/<book>/n/<id>` names a note.
    if (/\/n\/[^/]+$/.test(at)) return swap(at, false);
    screen(at + location.search);
  });

  wide.addEventListener("change", bring);
  bring();
  mark();
})();
"#;

/// Backlinks in the note's margin, above 1440px; the Links button stays for
/// phones.
///
/// Fetched rather than sent: `backlinks_to_note` walks every note (about 8% on
/// top of `ls` at two thousand), which would turn a note page's one file read
/// into two thousand on every phone. `margined` goes on before the answer, as
/// `indexed` does, so the layout reserves the 236px column and lays the prose
/// out once.
///
/// Unlike the index pane this shows a loading state: it is empty on every note,
/// and an unexplained gap reads as failure. No backlinks is drawn as an answer;
/// a failed fetch closes the box.
pub const BESIDE: &str = r#"
(() => {
  const app = document.querySelector(".app.split");
  if (!app) return;
  const wide = matchMedia("(min-width:1440px)");

  // The pathname the fetch was built from, so the two cannot disagree.
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
    // Going back can put the notebook's page in the reading pane. Forgetting
    // lets the same note, opened again, ask again.
    if (!app.classList.contains("at-note")) {
      asked = null;
      return;
    }
    if (!wide.matches) return;
    const aside = app.querySelector(".pane.read .beside");
    const answer = aside && aside.querySelector(".answer");
    if (!answer) return;
    // Before the first await, or the note is laid out twice.
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
      // Close and forget, unless the reader has moved to another note whose
      // answer is in flight.
      if (asked === at) {
        asked = null;
        aside.hidden = true;
        answer.replaceChildren();
      }
      return;
    }

    if (location.pathname !== at) return;
    const box = app.querySelector(".pane.read .beside");
    const into = box && box.querySelector(".answer");
    if (!into) return;

    // Rebuilt as title-and-id minis rather than lifted whole like `bring`'s
    // rows: in 236px a row's tags would wrap into a paragraph.
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

  wide.addEventListener("change", ask);
  app.addEventListener("noda:read", ask);
  ask();
})();
"#;

/// Every stamp on the page, in the reader's time zone.
///
/// Selected by `<time datetime>`, never a class: `.when` is also worn by a
/// tag's note count and a `due:` date, neither an instant. Repainted on
/// `PANES`'s events; reading `datetime` makes a second pass idempotent.
pub const STAMPS: &str = r#"
(() => {
  // English: only the zone is the reader's, not the locale.
  const clock = new Intl.DateTimeFormat("en", { dateStyle: "medium", timeStyle: "short" });

  // `noda ls -l`'s spelling, by hand because `Intl` has no fixed ISO format.
  const iso = (at) => {
    const pad = (n) => String(n).padStart(2, "0");
    return at.getFullYear() + "-" + pad(at.getMonth() + 1) + "-" + pad(at.getDate());
  };

  const paint = (root) => {
    for (const said of root.querySelectorAll("time[datetime]")) {
      const raw = said.getAttribute("datetime");
      const at = new Date(raw);
      // An imported date nothing can parse is left as the file has it.
      if (Number.isNaN(at.getTime())) continue;
      said.textContent = said.hasAttribute("data-clock") ? clock.format(at) : iso(at);
      said.title = raw;
    }

    // A row prints its day twice, drawn one at a time, but only one carries
    // the stamp (saving thirty bytes a note), so copy the result across.
    for (const row of root.querySelectorAll("a.row")) {
      const said = row.querySelector("time.when");
      const beside = row.querySelector(".ident .day");
      if (!said || !beside) continue;
      beside.textContent = said.textContent;
      beside.title = said.title;
    }
  };

  paint(document);

  const app = document.querySelector(".app");
  if (!app) return;
  for (const done of ["noda:rows", "noda:read"]) {
    app.addEventListener(done, () => paint(app));
  }
})();
"#;

/// Warns, while somebody is editing, that the note has changed on disk.
///
/// Save merges either way; this only says so sooner. The watch address is
/// derived from `…/edit` because scripts are static files and cannot carry the
/// id. Each message is compared with the form's fingerprint, so the reader's own
/// save says nothing.
pub const WATCHING: &str = r#"
(() => {
  const form = document.querySelector("form.write");
  if (!form) return;
  const held = form.querySelector('input[name="fingerprint"]');
  if (!held) return;
  const main = form.parentNode;
  if (!main) return;

  const at = location.pathname.replace(/\/edit$/, "/watch");
  if (at === location.pathname) return;

  let said = null;

  const tell = () => {
    if (said) return;
    said = document.createElement("p");
    said.className = "said bad";
    // Not "reload": their work is in the box and the server merges.
    said.innerHTML =
      "<b>This note has changed since you opened it.</b> " +
      "Nothing of yours is lost — saving merges what you wrote with what was saved.";
    main.insertBefore(said, form);
  };

  const source = new EventSource(at);
  source.onmessage = (e) => {
    if (e.data && e.data !== held.value) tell();
  };
  // `EventSource` reconnects by itself after a server stop or proxy timeout.
})();
"#;

#[cfg(test)]
mod tests {
    use super::*;

    /// The scripts are inlined, so this is the whole injection defence.
    #[test]
    fn no_script_can_end_its_own_element() {
        for source in [LISTING, STANDING, PANES, BESIDE, STAMPS, WATCHING] {
            assert!(!source.to_lowercase().contains("</script"), "{source}");
        }
    }

    /// The hook tests below pin selectors shared with `web::page`: a rename on
    /// either side compiles and passes every other Rust test.
    #[test]
    fn the_watch_looks_for_what_the_edit_form_writes() {
        for hook in ["form.write", "input[name=\"fingerprint\"]", "/watch"] {
            assert!(WATCHING.contains(hook), "{hook} is gone:\n{WATCHING}");
        }
    }

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
        // `undefined` would follow the browser's locale.
        assert!(STAMPS.contains("Intl.DateTimeFormat(\"en\""), "{STAMPS}");
    }

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

    /// Drawn by `page::grouping` and by `LISTING`, which cannot share code; this
    /// checks both write the classes the stylesheet draws.
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
        // Built from `q` alone, the URL would drop the order.
        assert!(PANES.contains("new FormData(form)"), "{PANES}");
        let sheet = crate::web::page::stylesheet();
        for rule in [".sortbar", ".sortbar a[aria-current] .pill"] {
            assert!(
                sheet.contains(rule),
                "the stylesheet stopped drawing {rule}"
            );
        }
    }

    #[test]
    fn the_script_and_the_stylesheet_split_at_the_same_width() {
        assert!(PANES.contains("(min-width:1024px)"), "{PANES}");
        assert!(
            crate::web::page::stylesheet().contains("(min-width:1024px)"),
            "the stylesheet no longer splits at 1024px"
        );
    }

    #[test]
    fn the_poll_steers_by_the_meta_the_scriptless_page_steers_by() {
        assert!(
            STANDING.contains(r#"meta[http-equiv="refresh"]"#),
            "{STANDING}"
        );
        assert!(STANDING.contains("querySelector(\"main\")"), "{STANDING}");
    }

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

    #[test]
    fn the_margin_note_and_the_stylesheet_widen_at_the_same_number() {
        assert!(BESIDE.contains("(min-width:1440px)"), "{BESIDE}");
        assert!(
            crate::web::page::stylesheet().contains("(min-width:1440px)"),
            "the stylesheet no longer puts the margin note beside the note"
        );
    }

    /// Without it the margin note is right on a hard load and stale after.
    #[test]
    fn a_pane_swap_tells_the_margin_note_the_note_changed() {
        assert!(PANES.contains("\"noda:read\""), "{PANES}");
        assert!(BESIDE.contains("\"noda:read\""), "{BESIDE}");
    }

    /// A mismatched name still gets a correct answer — the whole page — so it
    /// would fail silently.
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

    /// Selecting what it wants, not assuming a shape, keeps the header an
    /// optimisation rather than a protocol.
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

    /// Without it the filter goes on filtering detached rows.
    #[test]
    fn replacing_the_rows_tells_the_filter_to_read_them_again() {
        assert!(PANES.contains("\"noda:rows\""), "{PANES}");
        assert!(LISTING.contains("\"noda:rows\""), "{LISTING}");
    }

    /// Only above the width where anything was pushed.
    #[test]
    fn going_back_asks_the_server_rather_than_the_page() {
        assert!(PANES.contains("popstate"), "{PANES}");
        assert!(
            PANES.contains("if (!wide.matches) return location.reload();"),
            "the narrow screen stopped falling back to a reload"
        );
    }

    /// On a note page the same form is the way *to* the listing.
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

    /// Back pressed mid-flight must have an entry to return to; see `swap`.
    #[test]
    fn a_press_pushes_its_address_before_it_asks_for_the_answer() {
        // From each function's start: `bring` fetches a `where` of its own.
        for (what, from, push, ask) in [
            (
                "the swap",
                "const swap = async",
                "history.pushState",
                "fetch(href",
            ),
            (
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
        // Search and order share one `relist`, so they fail alike.
        assert_eq!(PANES.matches("relist(").count(), 2, "{PANES}");
    }
}
