// /download (F-WB-01, D-132): names the visitor's OS above the builds. It
// reads only the browser's own platform string; nothing is stored or sent.
(() => {
  "use strict";
  const note = document.querySelector("[data-os-note]");
  if (!note) return;
  const ua = navigator.userAgentData;
  const platform = ((ua && ua.platform) || navigator.platform || navigator.userAgent || "").toLowerCase();
  const os = /mac|iphone|ipad/.test(platform)
    ? "macOS"
    : /win/.test(platform)
      ? "Windows"
      : /linux|x11|cros/.test(platform)
        ? "Linux"
        : null;
  if (os === null) return;
  note.textContent =
    os === "Linux"
      ? "You're on Linux: the build below is for you."
      : `You're on ${os}. There's no ${os} build yet; Vibezeo runs on Linux for now.`;
  note.hidden = false;
})();
