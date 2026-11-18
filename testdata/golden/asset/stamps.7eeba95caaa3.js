
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
