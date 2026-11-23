(() => {
  const app = document.querySelector(".app");
  // Looked up on every use rather than once. sending.js can put a
  // new form (a conflict to settle) in this <main> without a page load, and
  // the fingerprint to compare against is then the new form's.
  const main = document.querySelector("main");
  const form = () => main && main.querySelector("form.write");
  const held = () => form() && form().querySelector('input[name="fingerprint"]');
  if (!held()) return;

  const at = location.pathname.replace(/\/edit$/, "/watch");
  if (at === location.pathname) return;

  let said = null;

  const tell = () => {
    if (said && said.isConnected) return;
    said = document.createElement("p");
    said.className = "said bad";
    // Announced, since it arrives while the reader is typing.
    said.setAttribute("role", "status");
    // Not "reload": their work is in the box and the server merges.
    said.innerHTML =
      "<b>This note has changed since you opened it.</b> " +
      "Nothing of yours is lost — saving merges what you wrote with what was saved.";
    main.insertBefore(said, form());
  };

  // Git's conflict markers get a way to walk them. A button, added
  // only when there is a marker to find; the scriptless page has the same
  // text and the browser's own find.
  const settle = () => {
    const box = form() && form().querySelector("textarea");
    const buttons = form() && form().querySelector(".buttons");
    if (!box || !buttons || !box.value.includes("<<<<<<<") || buttons.querySelector(".settle")) return;
    const next = document.createElement("button");
    next.type = "button";
    next.className = "settle";
    next.textContent = "Next conflict";
    next.addEventListener("click", () => {
      let from = box.value.indexOf("<<<<<<<", box.selectionEnd);
      if (from < 0) from = box.value.indexOf("<<<<<<<");
      if (from < 0) {
        next.textContent = "No conflicts left";
        return;
      }
      const closer = box.value.indexOf(">>>>>>>", from);
      const line = closer < 0 ? -1 : box.value.indexOf("\n", closer);
      box.focus();
      box.setSelectionRange(from, line < 0 ? box.value.length : line);
    });
    buttons.append(next);
  };
  settle();
  if (app) app.addEventListener("noda:form", settle);

  const source = new EventSource(at);
  source.onmessage = (e) => {
    const now = held();
    if (now && e.data && e.data !== now.value) tell();
  };
  // `EventSource` reconnects by itself after a server stop or proxy timeout.
})();
