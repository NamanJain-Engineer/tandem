// The site's pointer light (F-WB-01, D-2576), the one script every marketing
// page loads (scripts/script-allowlist.mjs). A surface marked `data-lit` gets
// a light that follows a fine pointer (components/site/light.css draws it
// from --mx/--my). At most one update a frame; nothing with reduced motion
// or touch; the page is the same without it.
(function () {
  var fine = window.matchMedia("(hover: hover) and (pointer: fine)");
  var still = window.matchMedia("(prefers-reduced-motion: reduce)");
  var frame = 0;
  var last = null;
  function paint() {
    frame = 0;
    if (!last) return;
    var r = last.el.getBoundingClientRect();
    last.el.style.setProperty("--mx", last.x - r.left + "px");
    last.el.style.setProperty("--my", last.y - r.top + "px");
  }
  document.addEventListener(
    "pointermove",
    function (e) {
      if (!fine.matches || still.matches) return;
      var t = e.target;
      var el = t && t.closest ? t.closest("[data-lit]") : null;
      if (!el) return;
      last = { el: el, x: e.clientX, y: e.clientY };
      if (!frame) frame = requestAnimationFrame(paint);
    },
    { passive: true },
  );
})();
