//! The enhancement layer: the only part of this interface allowed to be absent.
//!
//! Every page here works with scripts turned off, and six pull requests were
//! spent making sure of it before a line of this file existed. That order was
//! deliberate — write the script first and the scriptless path quietly loses a
//! corner nobody notices — and it decides what this file may do. **Nothing here
//! adds a capability. Everything here removes a wait.**
//!
//! Two waits, specifically, and they are the two the design named up front:
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

use std::fmt::Write;

/// The listing's filter.
///
/// Reads the rows out of the DOM rather than being handed a copy of them. The
/// alternative — a `<script type="application/json">` beside the list — would
/// put every title and tag on the page twice, and the second copy is the one
/// that goes stale. `textContent` also unwraps the server's `<mark>`s for free,
/// which is exactly the string the query has to be matched against.
pub const LISTING: &str = r#"
(() => {
  const form = document.querySelector("form.searchbar");
  const list = document.querySelector("main.rows");
  if (!form || !list) return;
  const field = form.querySelector("input[name=q]");
  const rows = [...list.querySelectorAll("a.row")];
  if (!field || !rows.length) return;

  const count = document.querySelector(".topbar .count");
  const hint = form.querySelector(".hint");
  const problem = form.querySelector(".problem");
  const empty = list.querySelector(".empty");
  const asked = empty && empty.querySelector(".asked");
  const total = rows.length;

  // Each row, as the query sees it. A tag cannot contain a comma — `note.rs`
  // refuses one — so the line the server joined is safe to take apart again.
  const notes = rows.map((row) => {
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
    return value ? { field, value, negated } : null;
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
    if (!tokens.length) {
      for (const note of notes) {
        note.row.hidden = false;
        paint(note.title, note.words, []);
      }
      show(total, true);
      return;
    }

    const groups = parse(tokens);
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
      const answer = await fetch(location.href, { headers: { accept: "text/html" } });
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
      const answer = await fetch(where, { headers: { accept: "text/html" } });
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
    const rows = sent.querySelector("main.rows");
    const count = sent.querySelector(".index .topbar .count");
    if (!rows || box.firstElementChild) return;
    box.replaceChildren(...rows.childNodes);
    const here = app.querySelector(".index .topbar .count");
    if (here && count) here.textContent = count.textContent;
    mark();
  };

  // The row the reading pane is showing. Read off the address rather than
  // remembered, so it is right after a swap and right after a hard load.
  const mark = () => {
    const at = location.pathname;
    for (const row of app.querySelectorAll(".index main.rows a.row")) {
      row.classList.toggle("here", row.getAttribute("href") === at);
    }
  };

  const swap = async (href) => {
    let text;
    try {
      const answer = await fetch(href, { headers: { accept: "text/html" } });
      if (!answer.ok) return location.assign(href);
      text = await answer.text();
    } catch {
      return location.assign(href);
    }
    const sent = new DOMParser().parseFromString(text, "text/html");
    const next = sent.querySelector(".pane.read");
    if (!next) return location.assign(href);
    app.querySelector(".pane.read").replaceWith(next);
    document.title = sent.title;
    app.classList.remove("at-list");
    app.classList.add("at-note");
    history.pushState(null, "", href);
    mark();
  };

  app.addEventListener("click", (event) => {
    if (event.defaultPrevented || event.button || event.metaKey || event.ctrlKey ||
        event.shiftKey || event.altKey) return;
    const row = event.target.closest(".index main.rows a.row");
    if (!row || !wide.matches) return;
    const href = row.getAttribute("href");
    if (!href) return;
    event.preventDefault();
    swap(href);
  });

  // A swap pushed an address, so going back has to put the note that address
  // named on the screen. Asking the server for it is the same answer by the
  // same route, and it is the one the reader would have got by pressing reload.
  addEventListener("popstate", () => location.reload());

  wide.addEventListener("change", bring);
  bring();
  mark();
})();
"#;

/// A script, as it goes into a page.
///
/// The one thing worth checking is in here rather than at each call site: a
/// `</script` anywhere in the text ends the element, wherever it appears, and
/// the rest of the file becomes markup. Neither script contains one today and
/// the assertion is what keeps that true of the next line somebody adds.
pub fn tag(source: &str) -> String {
    debug_assert!(
        !source.to_lowercase().contains("</script"),
        "a script cannot contain the string that ends it"
    );
    let mut out = String::with_capacity(source.len() + 32);
    let _ = writeln!(out, "<script>{source}</script>");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole of the injection defence, and it is one string.
    #[test]
    fn neither_script_can_end_its_own_element() {
        for source in [LISTING, STANDING] {
            assert!(!source.to_lowercase().contains("</script"), "{source}");
        }
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
            ".pane.read",
            "a.row",
            "indexed",
            "at-note",
            "at-list",
            "here",
        ] {
            assert!(PANES.contains(hook), "the panes stopped looking for {hook}");
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

    #[test]
    fn a_script_arrives_wrapped_in_its_element() {
        let out = tag("let a = 1;");
        assert!(out.starts_with("<script>"), "{out}");
        assert!(out.trim_end().ends_with("</script>"), "{out}");
        assert!(out.contains("let a = 1;"), "{out}");
    }
}
