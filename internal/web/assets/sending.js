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
    try {
      const answer = await fetch(form.getAttribute("action"), {
        method: "POST",
        headers: { accept: "text/html", "x-noda-fragment": "read" },
      });
      if (!answer.ok) throw new Error(answer.status);
      const next = parse(await answer.text()).querySelector(".pane.read");
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
    } catch {
      label.textContent = was;
      button.removeAttribute("aria-busy");
      const note = pane.querySelector("main");
      note.prepend(complain("Not " + was.toLowerCase() + "ned.", "The server did not answer. Press " + was + " again to retry."));
    }
  };

  const write = async (form, button) => {
    const main = form.closest("main");
    if (!main || form.dataset.sending) return;
    form.dataset.sending = "1";
    const was = button ? button.textContent : "";
    if (button && button.dataset.busy) button.textContent = button.dataset.busy;
    if (button) button.setAttribute("aria-busy", "true");
    retract(main);
    try {
      const answer = await fetch(form.getAttribute("action"), {
        method: "POST",
        headers: { accept: "text/html" },
        // Urlencoded, as a form sends it: the server reads no multipart.
        body: new URLSearchParams(new FormData(form)),
      });
      // Accepted: the server redirected, and the browser goes where it said.
      if (answer.redirected) return location.replace(answer.url);
      // Refused, or a conflict to settle: a page with a form on it, whatever
      // its status. Its <main> takes the place of this one.
      const sent = parse(await answer.text());
      const next = sent.querySelector("main");
      if (!next || !next.querySelector("form")) throw new Error(answer.status);
      main.replaceChildren(...next.childNodes);
      document.title = sent.title;
      const first = main.querySelector("[autofocus]");
      if (first) first.focus();
      main.scrollIntoView({ block: "start" });
      app.dispatchEvent(new CustomEvent("noda:form"));
    } catch {
      delete form.dataset.sending;
      if (button) {
        button.textContent = was;
        button.removeAttribute("aria-busy");
      }
      main.insertBefore(
        complain("Not sent.", "The server did not answer. What you typed is still here — press " + was + " again."),
        form,
      );
    }
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
