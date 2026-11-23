(() => {
  let main = document.querySelector("main");
  if (!main) return;
  const meta = document.querySelector('meta[http-equiv="refresh"]');

  const every = ((meta && Number(meta.getAttribute("content"))) || 2) * 1000;
  if (meta) meta.remove();

  const again = () => setTimeout(tick, every);

  // Shared by the poll and the press. True when the answer says an
  // errand is still running, so stopping stays the server's decision.
  const show = (text) => {
    const fresh = new DOMParser().parseFromString(text, "text/html");
    const next = fresh.querySelector("main");
    if (!next) return location.reload();
    main.replaceWith(next);
    main = next;
    // The rail says the same thing in fewer words; it must not be left
    // saying what was true before the errand. The answer carries only that
    // link, not the rail around it.
    const stale = document.querySelector(".rail .sync");
    const said = fresh.querySelector(".sync");
    if (stale && said) stale.replaceWith(said);
    return !!fresh.querySelector('meta[http-equiv="refresh"]');
  };

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
    if (show(text)) again();
  };

  // A press is answered on the press. The line the server will send
  // ("Syncing…") is drawn at once from the button's own data-doing, the three
  // buttons are disabled as the server's page would have them, and the POST
  // goes by fetch, so the screen is never reloaded. What comes back is the
  // same page a scriptless press is redirected to.
  document.addEventListener("submit", async (event) => {
    const form = event.target;
    const button = form.querySelector("button[data-doing]");
    if (!button || event.defaultPrevented) return;
    event.preventDefault();
    const buttons = [...main.querySelectorAll(".abreast button")];
    for (const each of buttons) each.disabled = true;
    const said = document.createElement("p");
    said.className = "said working";
    said.setAttribute("role", "status");
    const bold = document.createElement("b");
    bold.textContent = button.dataset.doing + "…";
    said.append(bold);
    const last = main.querySelector(":scope > .said");
    if (last) last.replaceWith(said);
    else main.prepend(said);
    try {
      const answer = await fetch(form.getAttribute("action"), {
        method: "POST",
        headers: { accept: "text/html", "x-noda-fragment": "news" },
      });
      if (!answer.ok) throw new Error(answer.status);
      if (show(await answer.text())) again();
    } catch {
      // The request never arrived, so nothing was started: say so, and give
      // the buttons back.
      said.className = "said bad";
      said.setAttribute("role", "alert");
      bold.textContent = "Not started.";
      said.append(" The server did not answer. Press again to retry.");
      for (const each of buttons) each.disabled = false;
      button.focus();
    }
  });

  if (meta) again();
})();
