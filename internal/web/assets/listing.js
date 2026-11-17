
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
