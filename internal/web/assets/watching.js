
(() => {
  const form = document.querySelector("form.write");
  if (!form) return;
  const held = form.querySelector('input[name="fingerprint"]');
  if (!held) return;
  const main = form.parentNode;
  if (!main) return;

  const at = location.pathname.replace(/\/edit$/, "/watch");
  if (at === location.pathname) return;

  let said = null;

  const tell = () => {
    if (said) return;
    said = document.createElement("p");
    said.className = "said bad";
    // Not "reload": their work is in the box and the server merges.
    said.innerHTML =
      "<b>This note has changed since you opened it.</b> " +
      "Nothing of yours is lost — saving merges what you wrote with what was saved.";
    main.insertBefore(said, form);
  };

  const source = new EventSource(at);
  source.onmessage = (e) => {
    if (e.data && e.data !== held.value) tell();
  };
  // `EventSource` reconnects by itself after a server stop or proxy timeout.
})();
