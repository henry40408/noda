// A write is answered where it was made.
//
// Without this script a write is a form: POST, a redirect, a new page. With
// it the same form is sent by fetch and the same answer is put on the page
// that is already there — so the press is acknowledged at once, the scroll
// and the focus stay, and a dropped connection leaves what was typed in the
// box instead of a browser error page. The answer itself is never different:
// it is the page the scriptless form would have been shown.
//
// Only the pin is drawn before the server answers. It is the one write with
// no screen of its own, it cannot be refused for what it says, and undoing it
// is the same press again. A save is not optimistic: the server may merge it
// or refuse it, and "saved" shown early would be a lie in exactly those cases.
(() => {
  const app = document.querySelector(".app");
  if (!app) return;

  const parse = (text) => new DOMParser().parseFromString(text, "text/html");
  // "Not pinned." in red, where the eye already is, announced once.
  const complain = (bold, rest) => {
    const said = document.createElement("p");
    said.className = "said bad";
    said.setAttribute("role", "alert");
    const b = document.createElement("b");
    b.textContent = bold;
    said.append(b, " " + rest);
    return said;
  };
  const retract = (within) => {
    for (const old of within.querySelectorAll(".said[role=alert]")) old.remove();
  };

  // "The server did not answer" is for a request that never completed. A
  // refusal is an answer, and what it said is what the reader is told.
  const send = async (action, init) => {
    try {
      return await fetch(action, init);
    } catch {
      return null;
    }
  };
  const read = async (answer) => {
    try {
      return await answer.text();
    } catch {
      return null;
    }
  };
  // The server's own sentence from a reply that is not the page asked for: a
  // failure page says it in its `.empty`, anything else is taken as text.
  const words = (answer, text) => {
    const html = (answer.headers.get("content-type") || "").includes("html");
    const page = html ? parse(text) : null;
    const empty = page && page.querySelector(".empty");
    if (empty) {
      const parts = [...empty.childNodes].map((node) => node.textContent.trim());
      return parts.filter(Boolean).join(" — ");
    }
    const said = (page ? page.body.textContent : text).trim().replace(/\s+/g, " ");
    return said ? said + " (" + answer.status + ")" : "status " + answer.status;
  };

  const pin = async (form, button) => {
    const pane = button.closest(".pane.read");
    const label = button.querySelector("span");
    if (!pane || !label || button.hasAttribute("aria-busy")) return;
    const was = label.textContent;
    // data-then is the label the server will send next, so the flip is the
    // server's own word.
    if (button.dataset.then) label.textContent = button.dataset.then;
    button.setAttribute("aria-busy", "true");
    retract(pane);
    const failed = (why) => {
      label.textContent = was;
      button.removeAttribute("aria-busy");
      pane.querySelector("main").prepend(complain("Not " + was.toLowerCase() + "ned.", why));
    };
    const answer = await send(form.getAttribute("action"), {
      method: "POST",
      headers: { accept: "text/html", "x-noda-fragment": "read" },
    });
    const text = answer && (await read(answer));
    if (text === null) return failed("The server did not answer. Press " + was + " again to retry.");
    if (!answer.ok) return failed("The server said: " + words(answer, text));
    const next = parse(text).querySelector(".pane.read");
    if (!next) return location.reload();
    // The server's page replaces the guess. The reader stays where they
    // were: same scroll, and the focus back on the button if it was there.
    const held = pane.contains(document.activeElement) && document.activeElement.dataset.key;
    const tops = [pane.scrollTop, document.scrollingElement.scrollTop];
    pane.replaceWith(next);
    next.scrollTop = tops[0];
    document.scrollingElement.scrollTop = tops[1];
    const again = held && next.querySelector(`[data-key="${held}"]`);
    if (again) again.focus();
    app.dispatchEvent(new CustomEvent("noda:read"));
    // A pin moves the row: panes.js asks for the rows again.
    app.dispatchEvent(new CustomEvent("noda:stale"));
  };

  const write = async (form, button) => {
    const main = form.closest("main");
    if (!main || form.dataset.sending) return;
    form.dataset.sending = "1";
    const was = button ? button.textContent : "";
    if (button && button.dataset.busy) button.textContent = button.dataset.busy;
    if (button) button.setAttribute("aria-busy", "true");
    retract(main);
    const unsent = (why) => {
      delete form.dataset.sending;
      if (button) {
        button.textContent = was;
        button.removeAttribute("aria-busy");
      }
      main.insertBefore(complain("Not sent.", why + " What you typed is still here."), form);
    };
    const answer = await send(form.getAttribute("action"), {
      method: "POST",
      headers: { accept: "text/html" },
      // Urlencoded, as a form sends it: the server reads no multipart.
      body: new URLSearchParams(new FormData(form)),
    });
    // Accepted: the server redirected, and the browser goes where it said.
    if (answer && answer.redirected) return location.replace(answer.url);
    const text = answer && (await read(answer));
    if (text === null) return unsent("The server did not answer. Press " + was + " again.");
    // Refused, or a conflict to settle: whatever page the server sent, its
    // <main> takes the place of this one, as the page itself would have.
    const sent = parse(text);
    const next = sent.querySelector("main");
    if (!next) return unsent("The server said: " + words(answer, text) + ".");
    main.replaceChildren(...next.childNodes);
    document.title = sent.title;
    const first = main.querySelector("[autofocus]");
    if (first) first.focus();
    main.scrollIntoView({ block: "start" });
    app.dispatchEvent(new CustomEvent("noda:form"));
  };

  document.addEventListener("submit", (event) => {
    const form = event.target;
    if (event.defaultPrevented || form.method !== "post") return;
    const button = event.submitter || form.querySelector("button[type=submit]") ||
      document.querySelector(`button[form="${form.id}"]`);
    if (form.id === "pin" && button) {
      event.preventDefault();
      pin(form, button);
    } else if (form.matches("form.write")) {
      event.preventDefault();
      write(form, button);
    }
  });
})();
