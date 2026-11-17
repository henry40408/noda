
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
