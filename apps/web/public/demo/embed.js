// The home page's interactive demo (D-128): the only script on the marketing
// pages. It frames /demo/app/ (same origin) once the section nears the
// viewport, scales the laptop-sized app to the column, and opens it full
// screen. Phones keep the still image until "Tap to explore". It changes only
// what's inside [data-demo-stage], which React's hydration doesn't touch
// (inner HTML). No other origin,
// no storage, no network beyond the demo's own files.
(() => {
  "use strict";
  const slot = document.querySelector("[data-demo-embed]");
  const stage = slot && slot.querySelector("[data-demo-stage]");
  if (!slot || !stage) return;
  const SRC = "/demo/app/?embed=1";
  const W = 1280;
  const H = 800;
  const still = stage.querySelector("[data-demo-still]");
  const bar = stage.querySelector("[data-demo-bar]");
  // "Tap to explore" sits over the still image; never over the open demo.
  const tap = stage.querySelector("[data-demo-open]");
  const small = () => window.matchMedia("(max-width: 1023.98px)").matches;
  let frame = null;
  let full = false;
  let back = null;

  function fit() {
    if (!frame) return;
    if (full) {
      Object.assign(frame.style, { width: "100%", height: "calc(100% - 40px)", top: "40px", transform: "none" });
    } else {
      const scale = stage.clientWidth / W;
      Object.assign(frame.style, { width: W + "px", height: H + "px", top: "0", transform: "scale(" + scale + ")" });
    }
  }

  function load() {
    if (frame) return frame;
    frame = document.createElement("iframe");
    frame.src = SRC;
    frame.title = "Vibezeo interactive demo: simulated agents, sample data";
    frame.setAttribute("referrerpolicy", "same-origin");
    frame.setAttribute("data-demo-frame", "");
    Object.assign(frame.style, { position: "absolute", left: "0", border: "0", transformOrigin: "0 0", background: "transparent" });
    frame.addEventListener("load", () => {
      stage.setAttribute("data-demo-state", "live");
      if (still) still.style.visibility = "hidden";
    });
    stage.appendChild(frame);
    fit();
    return frame;
  }

  function open() {
    load();
    if (full) return;
    full = true;
    back = document.activeElement;
    stage.setAttribute("data-demo-full", "");
    Object.assign(stage.style, { position: "fixed", inset: "0", zIndex: "1000", borderRadius: "0", background: "var(--tn-bg, #0b0f14)" });
    if (bar) bar.hidden = false;
    if (tap) tap.style.display = "none";
    document.documentElement.style.overflow = "hidden";
    fit();
    frame.focus();
  }

  function close() {
    if (!full) return;
    full = false;
    stage.removeAttribute("data-demo-full");
    Object.assign(stage.style, { position: "", inset: "", zIndex: "", borderRadius: "", background: "" });
    if (bar) bar.hidden = true;
    if (tap) tap.style.display = "";
    document.documentElement.style.overflow = "";
    fit();
    if (back && typeof back.focus === "function") back.focus();
  }

  for (const el of document.querySelectorAll("[data-demo-open]")) {
    el.addEventListener("click", (e) => {
      e.preventDefault();
      open();
    });
  }
  for (const el of stage.querySelectorAll("[data-demo-close]")) el.addEventListener("click", close);
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && full) close();
  });
  // Esc inside the demo that the app didn't use.
  window.addEventListener("message", (e) => {
    if (e.origin === window.location.origin && frame && e.source === frame.contentWindow && e.data && e.data.type === "vibezeo-demo:escape") close();
  });
  new ResizeObserver(fit).observe(stage);

  // Large screens: load the live demo when the section comes near.
  if ("IntersectionObserver" in window) {
    const seen = new IntersectionObserver(
      (entries) => {
        if (entries.some((x) => x.isIntersecting) && !small()) {
          seen.disconnect();
          // After the page itself has loaded (or 2 s, whichever is first),
          // so the demo never delays it.
          if (document.readyState === "complete") load();
          else {
            window.addEventListener("load", load, { once: true });
            setTimeout(load, 2000);
          }
        }
      },
      { rootMargin: "200px" },
    );
    seen.observe(slot);
  }
})();
