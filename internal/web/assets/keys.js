// The keyboard, for every page.
//
// One rule carries the whole design: a key only presses a control that is
// already on the page. The server writes `data-key` on a link, a button or a
// field; this script finds the visible one and clicks it (or, for a field,
// focuses it). So a key can never do what the page cannot, what a key does
// without this script is simply what the control does, and which keys exist on
// a screen is decided by the markup of that screen, not by a table in here.
//
// What is in here and not in the markup is movement (j k J K gg G, arrows),
// Esc, the `g` prefix, and the card that lists it all.
(() => {
  const app = document.querySelector(".app");
  if (!app) return;
  const root = document.documentElement;
  const wide = matchMedia("(min-width:1024px)");

  // Storage can be refused (a private window, a file:// page, all cookies
  // blocked), and a refusal must cost a preference, never the keys. The store
  // is named, not passed: with cookies blocked, reading `localStorage` itself
  // throws, so it has to happen inside the try.
  const kept = (store, name, value) => {
    try {
      if (value === undefined) return window[store].getItem(name);
      if (value === null) window[store].removeItem(name);
      else window[store].setItem(name, value);
    } catch {}
    return null;
  };
  let off = kept("localStorage", "noda.keys") === "off";
  if (kept("sessionStorage", "noda.keyed")) root.classList.add("keyed");

  const visible = (el) => el.getClientRects().length > 0 && !el.disabled;
  const typing = (el) =>
    el.isContentEditable || el.tagName === "TEXTAREA" || el.tagName === "SELECT" ||
    (el.tagName === "INPUT" && !/^(checkbox|radio|button|submit|reset)$/.test(el.type));

  // The key that was meant. `key` first: a shortcut is the first letter of a
  // word on the screen, so it follows the letter on the keycap, wherever a
  // Dvorak or AZERTY layout puts it. `code` only when `key` is not ASCII (a
  // Cyrillic, Greek or Zhuyin layout has no "e" to press): then the key in the
  // QWERTY position stands in. Case comes from Shift alone, so Caps Lock never
  // turns `d` into `D`.
  const meant = (event) => {
    let key = event.key;
    if (key.length !== 1) return key;
    if (key.charCodeAt(0) > 126) {
      const at = /^Key([A-Z])$/.exec(event.code);
      if (at) key = at[1];
      else if (event.code === "Slash") return event.shiftKey ? "?" : "/";
      else return "";
    }
    if (/[a-z]/i.test(key)) return event.shiftKey ? key.toUpperCase() : key.toLowerCase();
    return key;
  };

  const controls = () => [...document.querySelectorAll("[data-key]")].filter(visible);
  const control = (key) => controls().find((el) => el.dataset.key === key);
  const named = (el) => el.dataset.does || el.getAttribute("aria-label") || el.textContent.trim();
  const press = (el) => {
    if (typing(el)) {
      el.focus();
      el.select();
    } else el.click();
  };

  // ---- movement ------------------------------------------------------------
  const rows = () => [...document.querySelectorAll("main.rows a.row, main.rows .row > a.most")].filter(visible);
  const reading = () => app.querySelector(".pane.read main.note");
  // Two panes on screen: the rows on one side and a note on the other.
  const beside = () => wide.matches && app.classList.contains("indexed") && app.classList.contains("at-note");
  const inRead = () => {
    const note = reading();
    return !!note && note.contains(document.activeElement);
  };
  // What scrolls: the pane from 640px up, the window below.
  const scroller = (el) => {
    const pane = el && el.closest(".pane");
    if (pane && pane.scrollHeight > pane.clientHeight && getComputedStyle(pane).overflowY !== "visible") return pane;
    return document.scrollingElement;
  };
  const scroll = (by) => {
    const box = scroller(inRead() || !rows().length ? reading() || app.querySelector("main") : rows()[0]);
    if (by === "top") box.scrollTop = 0;
    else if (by === "end") box.scrollTop = box.scrollHeight;
    else box.scrollBy(0, by);
  };
  // The cursor is the browser's own focus, so Enter, the context menu and a
  // screen reader all agree with it about which row is meant.
  const step = (by) => {
    const all = rows();
    if (!all.length) return false;
    const at = all.indexOf(document.activeElement);
    let next;
    if (at < 0) next = all.find((row) => row.classList.contains("here")) || all[by > 0 ? 0 : all.length - 1];
    else next = all[Math.max(0, Math.min(all.length - 1, at + by))];
    next.focus();
    return true;
  };
  // J and K: the note after or before the one that is open.
  const open = (by) => {
    const all = rows();
    const at = all.findIndex((row) => row.classList.contains("here"));
    const next = all[at < 0 ? 0 : at + by];
    if (!next) return;
    follow = inRead();
    // From the list, the cursor goes with the note that opens.
    if (!follow) next.focus();
    next.click();
  };

  // Enter on a row, with the note beside it: the note opens and the focus
  // goes into it, so j and k then scroll what was just opened. Esc comes back.
  let follow = false;
  app.addEventListener("noda:read", () => {
    const note = reading();
    if (!follow || !note) return;
    follow = false;
    note.tabIndex = -1;
    note.focus();
  });

  // Coming back to a list by a page load, the row that was left is focused
  // again — only for someone who has used a key, since a focus ring nobody
  // asked for is noise.
  const place = "noda.row:" + location.pathname + location.search;
  addEventListener("click", (event) => {
    const row = event.target.closest && event.target.closest("main.rows a[href]");
    if (row) kept("sessionStorage", place, row.getAttribute("href"));
  }, true);
  const left = kept("sessionStorage", place);
  if (left && root.classList.contains("keyed") && !document.querySelector("[autofocus]")) {
    const row = rows().find((each) => each.getAttribute("href") === left);
    if (row) row.focus();
  }

  // ---- the `g` prefix ------------------------------------------------------
  let prefix = "";
  let strip = null;
  let timer = 0;
  const forget = () => {
    prefix = "";
    clearTimeout(timer);
    if (strip) strip.remove();
    strip = null;
  };
  const kbd = (text) => {
    const el = document.createElement("kbd");
    el.textContent = text === "Escape" ? "Esc" : text;
    return el;
  };
  const begin = () => {
    prefix = "g";
    strip = document.createElement("div");
    strip.className = "keys-next";
    strip.setAttribute("role", "status");
    strip.append(kbd("g"), "then");
    const places = controls().filter((el) => /^g.$/.test(el.dataset.key));
    for (const el of places) {
      const item = document.createElement("span");
      item.append(kbd(el.dataset.key[1]), named(el));
      strip.append(item);
    }
    const top = document.createElement("span");
    top.append(kbd("g"), "Top");
    strip.append(top);
    document.body.append(strip);
    timer = setTimeout(forget, 2500);
  };

  // ---- the card ------------------------------------------------------------
  let card = null;
  const line = (list, keys, text) => {
    const dt = document.createElement("dt");
    for (const key of keys) dt.append(kbd(key));
    const dd = document.createElement("dd");
    dd.textContent = text;
    list.append(dt, dd);
  };
  const part = (title) => {
    const section = document.createElement("section");
    const heading = document.createElement("h3");
    heading.textContent = title;
    const list = document.createElement("dl");
    section.append(heading, list);
    return { section, list };
  };
  // Built from the page each time it opens, so it lists what works here, now.
  const draw = () => {
    const parts = new Map();
    const into = (title) => {
      if (!parts.has(title)) parts.set(title, part(title));
      return parts.get(title).list;
    };
    for (const el of controls()) {
      const key = el.dataset.key;
      if (key === "Escape") continue;
      const nav = el.closest("nav[aria-label]");
      line(into(nav ? nav.getAttribute("aria-label") : "This screen"), /^g.$/.test(key) ? [...key] : [key], named(el));
    }
    const move = into("Moving");
    if (rows().length) {
      line(move, ["j", "k"], "Next row, previous row");
      line(move, ["⏎"], beside() || app.classList.contains("indexed") ? "Open the row, and step into the note" : "Open the row");
    } else line(move, ["j", "k"], "Scroll");
    if (app.classList.contains("indexed") && wide.matches) line(move, ["J", "K"], "Open the next note, the previous note");
    line(move, ["g", "g"], "Top");
    line(move, ["G"], "Bottom");
    const always = into("Everywhere");
    const out = control("Escape");
    line(always, ["Esc"], out ? "Step back: leave a field, clear the search, then " + named(out).toLowerCase() : "Step back: leave a field, clear the search");
    if (document.querySelector("form.write")) line(always, ["Ctrl", "⏎"], "Send the form (⌘ ⏎ on a Mac)");
    line(always, ["?"], "This card");

    const heading = document.createElement("h2");
    heading.textContent = "Keys on this screen";
    const body = document.createElement("div");
    body.className = "cols";
    for (const { section } of parts.values()) body.append(section);
    // WCAG 2.1.4: keys made of one character can be turned off.
    const foot = document.createElement("form");
    foot.method = "dialog";
    const toggle = document.createElement("button");
    toggle.type = "button";
    toggle.textContent = off ? "Turn single keys on" : "Turn single keys off";
    toggle.addEventListener("click", () => {
      off = !off;
      kept("localStorage", "noda.keys", off ? "off" : null);
      draw();
      card.querySelector("form button").focus();
    });
    const close = document.createElement("button");
    close.textContent = "Close";
    // Enter on arrival closes the card; it never flips the switch.
    close.autofocus = true;
    const note = document.createElement("p");
    note.textContent = off
      ? "Single keys are off. Esc, Ctrl ⏎ and ? still answer."
      : "Keys wait while a field has the focus or an input method is composing.";
    foot.append(toggle, close, note);
    card.replaceChildren(heading, body, foot);
    card.classList.toggle("off", off);
  };
  const show = () => {
    if (!card) {
      card = document.createElement("dialog");
      card.className = "keys";
      card.setAttribute("aria-label", "Keyboard shortcuts");
      document.body.append(card);
    }
    if (card.open) return card.close();
    draw();
    card.showModal();
  };
  // A way to the card that is not a key: for a pointer, and for someone who
  // turned the keys off.
  const foot = app.querySelector(".foot");
  if (foot) {
    const opener = document.createElement("button");
    opener.type = "button";
    opener.className = "keys-open";
    // It is drawn as one more line of the rail, under the sync line: a glyph,
    // a word and its key. The word is the name; the key is said by
    // aria-keyshortcuts, so the cap itself is hidden from a screen reader.
    opener.setAttribute("aria-keyshortcuts", "?");
    opener.setAttribute("aria-haspopup", "dialog");
    opener.innerHTML = '<svg viewBox="0 0 24 24" aria-hidden="true"><rect x="3" y="6" width="18" height="12" rx="2"/><path d="M7 10h.01M11 10h.01M15 10h.01M8 14h8"/></svg><span>Shortcuts</span>';
    const cap = kbd("?");
    cap.setAttribute("aria-hidden", "true");
    opener.append(cap);
    opener.addEventListener("click", show);
    foot.insertBefore(opener, foot.querySelector(".ver"));
  }

  // ---- Esc: one step back, never more --------------------------------------
  const search = () => {
    const field = app.querySelector("form.searchbar input[name=q]");
    return field && visible(field) ? field : null;
  };
  const escape = (event, target) => {
    if (prefix) return forget();
    if (typing(target)) {
      // A search field with text in it: the browser's own Esc empties it.
      if (target.type === "search" && target.value) return;
      return target.blur();
    }
    if (beside() && inRead()) {
      const here = rows().find((row) => row.classList.contains("here"));
      if (here) return here.focus();
    }
    const field = search();
    if (field && field.value) {
      field.value = "";
      field.dispatchEvent(new Event("input", { bubbles: true }));
      // Asked of the server (it is in the address): ask again, for everything.
      if (new URLSearchParams(location.search).get("q")) field.form.requestSubmit();
      return;
    }
    const out = control("Escape");
    if (out) {
      event.preventDefault();
      out.click();
    }
  };

  addEventListener("keydown", (event) => {
    // An input method is composing: every key belongs to it. 229 is what
    // Safari reports for the keydown that ends a composition.
    if (event.isComposing || event.keyCode === 229) return;
    const target = event.target;
    const key = meant(event);
    if (card && card.open) {
      if (key === "?" && !typing(target)) {
        event.preventDefault();
        card.close();
      }
      return;
    }

    // The one chord: Ctrl or ⌘ with Enter sends the form being written.
    if (key === "Enter" && (event.ctrlKey || event.metaKey)) {
      const form = target.closest && target.closest("form.write");
      if (!form) return;
      event.preventDefault();
      return form.requestSubmit();
    }
    // Every other chord is the browser's or the system's.
    if (event.ctrlKey || event.metaKey || event.altKey) return;
    if (key === "Escape") return escape(event, target);

    if (typing(target)) {
      // ↓ leaves the search field for the first row, as in a combobox.
      if (key === "ArrowDown" && target.type === "search" && rows().length) {
        event.preventDefault();
        rows()[0].focus();
      }
      return;
    }
    const onRow = rows().includes(document.activeElement);
    if (onRow && (key === "ArrowDown" || key === "ArrowUp")) {
      event.preventDefault();
      if (key === "ArrowUp" && rows()[0] === document.activeElement && search()) return search().focus();
      return step(key === "ArrowDown" ? 1 : -1);
    }
    if (onRow && key === "Enter") {
      follow = wide.matches && app.classList.contains("indexed");
      return;
    }
    if (key === "?") {
      event.preventDefault();
      forget();
      return show();
    }
    if (off || key.length !== 1) return;
    if (event.repeat && key !== "j" && key !== "k") return;

    const did = () => {
      event.preventDefault();
      if (!root.classList.contains("keyed")) {
        root.classList.add("keyed");
        kept("sessionStorage", "noda.keyed", "1");
      }
    };
    if (prefix) {
      const to = key === "g" ? null : control("g" + key);
      forget();
      did();
      if (key === "g") return rows().length && !inRead() ? rows()[0].focus() : scroll("top");
      if (to) press(to);
      return;
    }
    if (key === "g") return did(), begin();
    if (key === "j" || key === "k") {
      did();
      const by = key === "j" ? 1 : -1;
      if (inRead() || !step(by)) scroll(by * 90);
      return;
    }
    if (key === "J" || key === "K") {
      if (!(wide.matches && app.classList.contains("indexed"))) return;
      return did(), open(key === "J" ? 1 : -1);
    }
    if (key === "G") {
      did();
      const all = rows();
      return all.length && !inRead() ? all[all.length - 1].focus() : scroll("end");
    }
    const el = control(key);
    if (!el) return;
    did();
    press(el);
  });
})();
