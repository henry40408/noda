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

  };

  paint(document);

  const app = document.querySelector(".app");
  if (!app) return;
  for (const done of ["noda:rows", "noda:read"]) {
    app.addEventListener(done, () => paint(app));
  }
})();
