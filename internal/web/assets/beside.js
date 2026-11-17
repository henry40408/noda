
(() => {
  const app = document.querySelector(".app.split");
  if (!app) return;
  const wide = matchMedia("(min-width:1440px)");

  // The pathname the fetch was built from, so the two cannot disagree.
  let asked = null;

  const working = (text) => {
    const said = document.createElement("p");
    said.className = "said working";
    const bold = document.createElement("b");
    bold.textContent = text;
    said.append(bold);
    return said;
  };

  const nothing = (text) => {
    const none = document.createElement("p");
    none.className = "none";
    none.textContent = text;
    return none;
  };

  const ask = async () => {
    // Going back can put the notebook's page in the reading pane. Forgetting
    // lets the same note, opened again, ask again.
    if (!app.classList.contains("at-note")) {
      asked = null;
      return;
    }
    if (!wide.matches) return;
    const aside = app.querySelector(".pane.read .beside");
    const answer = aside && aside.querySelector(".answer");
    if (!answer) return;
    // Before the first await, or the note is laid out twice.
    app.classList.add("margined");
    const at = location.pathname;
    if (asked === at) return;
    asked = at;

    answer.replaceChildren(working("Reading the notebook…"));
    aside.hidden = false;

    let text;
    try {
      const got = await fetch(at + "/backlinks", {
        headers: { accept: "text/html", "x-noda-fragment": "rows" },
      });
      if (!got.ok) throw new Error(got.status);
      text = await got.text();
    } catch {
      // Close and forget, unless the reader has moved to another note whose
      // answer is in flight.
      if (asked === at) {
        asked = null;
        aside.hidden = true;
        answer.replaceChildren();
      }
      return;
    }

    if (location.pathname !== at) return;
    const box = app.querySelector(".pane.read .beside");
    const into = box && box.querySelector(".answer");
    if (!into) return;

    // Rebuilt as title-and-id minis rather than lifted whole like `bring`'s
    // rows: in 236px a row's tags would wrap into a paragraph.
    const sent = new DOMParser().parseFromString(text, "text/html");
    const minis = [];
    for (const row of sent.querySelectorAll("main.rows a.row")) {
      const href = row.getAttribute("href");
      const title = row.querySelector(".title");
      if (!href || !title) continue;
      const mini = document.createElement("a");
      mini.className = "mini";
      mini.href = href;
      mini.textContent = title.textContent;
      const id = document.createElement("span");
      id.textContent = href.split("/").pop();
      mini.append(id);
      minis.push(mini);
    }
    if (minis.length) into.replaceChildren(...minis);
    else into.replaceChildren(nothing("Nothing points here."));
    box.hidden = false;
  };

  wide.addEventListener("change", ask);
  app.addEventListener("noda:read", ask);
  ask();
})();
