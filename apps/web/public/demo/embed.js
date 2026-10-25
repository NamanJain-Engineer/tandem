// The home page's interactive demo (D-128), and the product scenes (D-1880)
// on the home page and /changelog/0.9.0/ (D-1885).
// It frames /demo/app/ (same origin) once the section nears the viewport,
// scales the laptop-sized app to the column, and opens it full screen.
// Phones keep the still image until "Tap to explore". It changes only
// what's inside [data-demo-stage], which React's hydration doesn't touch
// (inner HTML). It also gives the product video its poster as the video
// nears (D-1816), and plays the product scenes while they're on screen. No
// other origin, no storage, no network beyond the demo's own files and the
// poster.
(() => {
  "use strict";
  // A `poster` attribute downloads with the page, and on a slow connection
  // that held up the h1, the largest paint.
  for (const video of document.querySelectorAll("video[data-poster]")) {
    const show = () => {
      video.poster = video.getAttribute("data-poster");
    };
    if (!("IntersectionObserver" in window)) {
      show();
      continue;
    }
    const near = new IntersectionObserver(
      (entries) => {
        if (entries.some((x) => x.isIntersecting)) {
          near.disconnect();
          show();
        }
      },
      { rootMargin: "600px" },
    );
    near.observe(video);
  }

  // The product scenes (D-1880): each plays while a third of it (or 120 px)
  // is on screen and the tab is visible, and waits where it is otherwise. A
  // scene in the first screen plays at once, however little of it shows:
  // the hero's window can start low in a laptop's first screen, and a paused
  // first frame there would look blank. A browser without
  // IntersectionObserver just plays them.
  const scenes = document.querySelectorAll("[data-scene]");
  // Tells the theme script the player runs, so it keeps the effects on.
  document.documentElement.setAttribute("data-scenes", "");
  if (scenes.length) {
    const seen = new Set();
    const sync = () => {
      for (const scene of scenes) scene.toggleAttribute("data-live", seen.has(scene) && !document.hidden);
    };
    if ("IntersectionObserver" in window) {
      const first = new Set(scenes);
      // Every 1 % up to a third, so the 120 px test runs as the scene rises.
      const steps = [];
      for (let i = 0; i <= 30; i++) steps.push(i / 100);
      const watch = new IntersectionObserver(
        (entries) => {
          for (const x of entries) {
            // In the first screen: showing when the page opens, or rising
            // into it (its entry slide) before any scrolling.
            const opening = first.delete(x.target) || window.scrollY === 0;
            if (x.isIntersecting && (opening || x.intersectionRatio >= 0.3 || x.intersectionRect.height >= 120)) seen.add(x.target);
            else if (!x.isIntersecting) seen.delete(x.target);
          }
          sync();
        },
        { threshold: steps },
      );
      for (const scene of scenes) watch.observe(scene);
    } else for (const scene of scenes) seen.add(scene);
    document.addEventListener("visibilitychange", sync);
    sync();
  }

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
