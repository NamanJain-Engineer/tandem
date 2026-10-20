// Design self-check (docs/DESIGN.md §6): captures pages of a built site or the
// browser demo at 1440×900 and 390×844, in the OS's dark and light themes and
// with Light or Dark forced through the site's theme key, under reduced motion.
// For each capture it prints the theme the page resolved, whether the page
// scrolls sideways, and how many teal (accent) elements sit in the first
// viewport (DESIGN §4.2: at most three on the site). Serve the build first:
//   pnpm --filter web build:site && (cd apps/web/out && python3 -m http.server 4321 &)
//   node scripts/agent/design-shots.mjs /tmp/design-shots http://127.0.0.1:4321 / /pricing/ /download/
// Exits 1 if a page scrolls sideways or shows more than MAX_ACCENT teal elements.
/* global process, console, URL, document, window, localStorage, getComputedStyle */
import { createRequire } from "node:module";
import { mkdirSync } from "node:fs";
const require = createRequire(new URL("../../apps/web/package.json", import.meta.url));
const { chromium } = require("@playwright/test");

const [out = "/tmp/design-shots", base = "http://127.0.0.1:4321", ...argPaths] = process.argv.slice(2);
const paths = argPaths.length
  ? argPaths
  : ["/", "/pricing/", "/download/", "/changelog/", "/docs/", "/blog/"];
const MAX_ACCENT = Number(process.env.MAX_ACCENT ?? 3);
const SIZES = [
  [1440, 900],
  [390, 844],
];
// [name, OS colour scheme, the site's stored choice]
const SCENARIOS = [
  ["os-dark", "dark", null],
  ["os-light", "light", null],
  ["force-light", "dark", "light"],
  ["force-dark", "light", "dark"],
];
mkdirSync(out, { recursive: true });

const browser = await chromium.launch({
  executablePath: process.env.CHROMIUM ?? "/opt/pw-browsers/chromium",
});
let failed = 0;
for (const [width, height] of SIZES)
  for (const [name, scheme, choice] of SCENARIOS) {
    const context = await browser.newContext({
      viewport: { width, height },
      colorScheme: scheme,
      reducedMotion: "reduce",
    });
    if (choice)
      await context.addInitScript((v) => {
        try {
          localStorage.setItem("vibezeo-theme", v);
        } catch {
          // storage blocked: the page follows the OS
        }
      }, choice);
    for (const path of paths) {
      const page = await context.newPage();
      await page.goto(base + path, { waitUntil: "networkidle" });
      await page.waitForTimeout(500);
      const report = await page.evaluate(() => {
        // The accent ramp in both themes (tokens.css): teal-200 … teal-800.
        const teal = new Set([
          "153, 246, 228",
          "94, 234, 212",
          "45, 212, 191",
          "20, 184, 166",
          "13, 148, 136",
          "15, 118, 110",
          "17, 94, 89",
        ]);
        const isTeal = (v) => [...teal].some((t) => v.includes(`(${t}`));
        const vw = document.documentElement.clientWidth;
        const vh = window.innerHeight;
        const accents = [];
        for (const el of document.body.querySelectorAll("*")) {
          const r = el.getBoundingClientRect();
          if (r.width === 0 || r.height === 0 || r.bottom < 0 || r.top > vh || r.right < 0 || r.left > vw)
            continue;
          const s = getComputedStyle(el);
          if (s.visibility === "hidden" || Number(s.opacity) === 0) continue;
          const ownText = [...el.childNodes].some((n) => n.nodeType === 3 && n.textContent.trim());
          // The rings' blooms are radial gradients: the page's light (§4.5), not an accent.
          const fill =
            isTeal(s.backgroundColor) ||
            (isTeal(s.backgroundImage) && !s.backgroundImage.startsWith("radial-gradient"));
          const ink = ownText && (isTeal(s.color) || isTeal(s.backgroundImage));
          if (fill || ink)
            accents.push(`${el.tagName.toLowerCase()} "${(el.textContent ?? "").trim().slice(0, 24)}"`);
        }
        const root = document.documentElement;
        return {
          theme: `${root.dataset.themeChoice ?? "-"}/${root.dataset.theme ?? "-"}`,
          sideways: root.scrollWidth - vw,
          accents,
        };
      });
      const file = `${path.replace(/\W+/g, "_").replace(/^_|_$/g, "") || "home"}_${name}_${width}.png`;
      await page.screenshot({ path: `${out}/${file}` });
      const bad = report.sideways > 0 || report.accents.length > MAX_ACCENT;
      if (bad) failed++;
      console.log(
        `${bad ? "FAIL" : "ok  "} ${file}  theme ${report.theme}  sideways ${report.sideways}px  teal ${report.accents.length}`,
      );
      if (report.accents.length > MAX_ACCENT) console.log(`       teal: ${report.accents.join(", ")}`);
      await page.close();
    }
    await context.close();
  }
await browser.close();
console.log(`${failed} capture(s) need a look; screenshots in ${out}`);
process.exit(failed ? 1 : 0);
