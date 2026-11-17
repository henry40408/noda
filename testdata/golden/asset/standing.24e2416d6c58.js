
(() => {
  const meta = document.querySelector('meta[http-equiv="refresh"]');
  if (!meta) return;
  let main = document.querySelector("main");
  if (!main) return;

  const every = (Number(meta.getAttribute("content")) || 2) * 1000;
  meta.remove();

  const again = () => setTimeout(tick, every);

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
    const fresh = new DOMParser().parseFromString(text, "text/html");
    const next = fresh.querySelector("main");
    if (!next) return location.reload();
    main.replaceWith(next);
    main = next;
    if (fresh.querySelector('meta[http-equiv="refresh"]')) again();
  };

  again();
})();
