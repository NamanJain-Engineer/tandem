// Checks the browser demo's terminals on a real 2x screen (D-2607). Chromium
// starts with --force-device-scale-factor=2 and no deviceScaleFactor
// emulation: under emulation, devicePixelContentBoxSize reports CSS pixels
// and xterm's WebGL renderer draws at the wrong size, which no visitor's
// browser does (D-2606). Each run is a fresh browser, a first visit: the
// opening frame, the frame after a real window resize and back, and the frame
// after Reset. Every terminal pane must draw its text where and as large as a
// 1x reference run draws it, and none may be blank. Serve the built site first:
//   pnpm --filter desktop build:demo && (cd apps/web/public && python3 -m http.server 4321 &)
//   node scripts/agent/demo-retina-check.mjs http://127.0.0.1:4321 10 /tmp/retina
// Exits 1 when any check fails, and keeps the failing frames in the out dir.
// DEMO_RETINA_EMULATE=1 runs the 2x pass under emulation instead, the
// negative control: it must fail, which shows the check catches the artefact.
import { createRequire } from "node:module";
import { mkdirSync, writeFileSync } from "node:fs";
const require = createRequire(new URL("../../apps/web/package.json", import.meta.url));
const { chromium } = require("@playwright/test");

const origin = process.argv[2] ?? "http://127.0.0.1:4321";
const runs = Number(process.argv[3] ?? 10);
const out = process.argv[4] ?? "/tmp/demo-retina";
mkdirSync(out, { recursive: true });
const executablePath = process.env.CHROMIUM ?? "/opt/pw-browsers/chromium";
const PAGES = ["/demo/app/?embed=1", "/demo/app/"];
const WIDE = { width: 1280, height: 800 };
const NARROW = { width: 1100, height: 800 };
const emulate = process.env.DEMO_RETINA_EMULATE === "1";

/** A browser whose screen really is `scale`x, with or without WebGL. */
function launch(scale, size, gl) {
  return chromium.launch({
    executablePath,
    args: [
      `--force-device-scale-factor=${scale}`,
      `--window-size=${size.width},${size.height}`,
      gl ? "--enable-unsafe-swiftshader" : "--disable-webgl",
    ],
  });
}

/** Waits for the terminals and the fonts, then lets the frame settle. */
async function settle(page) {
  await page.waitForSelector('[data-testid="terminal"] .xterm-screen', { timeout: 20_000 });
  await page.waitForFunction(() => document.fonts.status === "loaded");
  await page.waitForTimeout(2_000);
}

/** The visible terminals' screens (CSS px) and their canvases. The hint is
 * hidden first, so only the terminals' own pixels are measured. */
function terminals(page) {
  return page.evaluate(() => {
    const hint = document.querySelector('[data-testid="coachmark"]');
    if (hint) hint.style.visibility = "hidden";
    return Array.from(document.querySelectorAll('[data-testid="terminal"]'))
      .filter((t) => t.offsetParent !== null)
      .map((t) => {
        const screen = t.querySelector(".xterm-screen");
        const r = screen.getBoundingClientRect();
        const canvases = Array.from(screen.querySelectorAll("canvas")).map((c) => {
          const b = c.getBoundingClientRect();
          return { layer: c.className || "webgl", w: c.width, h: c.height, cssW: b.width, cssH: b.height };
        });
        return { pane: t.getAttribute("data-pane-id"), x: r.x, y: r.y, w: r.width, h: r.height, canvases };
      });
  });
}

/** Each pane's ink (pixels unlike the pane's background, in CSS px²), its
 * glyph height and the box around it (CSS px), read from a screenshot. */
async function measure(analyzer, png, panes, scale) {
  return analyzer.evaluate(
    async ({ b64, panes, scale }) => {
      const img = new Image();
      img.src = `data:image/png;base64,${b64}`;
      await img.decode();
      const c = document.createElement("canvas");
      c.width = img.width;
      c.height = img.height;
      const g = c.getContext("2d", { willReadFrequently: true });
      g.drawImage(img, 0, 0);
      return panes.map((p) => {
        const x0 = Math.round(p.x * scale);
        const y0 = Math.round(p.y * scale);
        const w = Math.max(1, Math.round(p.w * scale));
        const h = Math.max(1, Math.round(p.h * scale));
        const d = g.getImageData(x0, y0, w, h).data;
        // The background: the commonest colour on the screen's bottom row.
        const tally = new Map();
        for (let i = (h - 1) * w * 4; i < h * w * 4; i += 4) {
          const k = (d[i] << 16) | (d[i + 1] << 8) | d[i + 2];
          tally.set(k, (tally.get(k) ?? 0) + 1);
        }
        const bg = [...tally].sort((a, b) => b[1] - a[1])[0][0];
        const [br, bgr, bb] = [bg >> 16, (bg >> 8) & 255, bg & 255];
        let n = 0;
        let [minX, minY, maxX, maxY] = [w, h, -1, -1];
        const rows = new Array(h).fill(0);
        for (let y = 0; y < h; y++)
          for (let x = 0; x < w; x++) {
            const i = (y * w + x) * 4;
            if (Math.max(Math.abs(d[i] - br), Math.abs(d[i + 1] - bgr), Math.abs(d[i + 2] - bb)) > 48) {
              n++;
              rows[y]++;
              if (x < minX) minX = x;
              if (x > maxX) maxX = x;
              if (y < minY) minY = y;
              if (y > maxY) maxY = y;
            }
          }
        // Text lines are bands of rows with ink between empty rows; their
        // median height (CSS px) is the glyphs' size, whatever wraps.
        const bands = [];
        for (let y = 0, start = -1; y <= h; y++) {
          if (y < h && rows[y] > 0) {
            if (start < 0) start = y;
          } else if (start >= 0) {
            if (y - start >= 2 * scale) bands.push((y - start) / scale);
            start = -1;
          }
        }
        bands.sort((a, b) => a - b);
        const glyph = bands.length ? bands[Math.floor(bands.length / 2)] : 0;
        const box = n ? [minX / scale, minY / scale, (maxX + 1) / scale, (maxY + 1) / scale].map(Math.round) : null;
        return { pane: p.pane, ink: Math.round(n / (scale * scale)), glyph, box };
      });
    },
    { b64: png.toString("base64"), panes, scale },
  );
}

/** The widest gaps seen between clean frames and the 1x reference. */
const seen = { ink: [Infinity, 0], glyph: [Infinity, 0] };

/** What's wrong with `got` next to the 1x reference, if anything. A blank
 * pane has no ink; text at double size has glyphs twice as tall. Lines may
 * end elsewhere: xterm rounds a cell's width to whole device pixels, so a
 * 2x cell is wider than a 1x one by up to a seventh, and a narrow pane wraps
 * one more line. So the text must start where the reference's does, with
 * glyphs of its size and about its ink. */
function problems(ref, got, panes, scale) {
  const bad = [];
  for (const r of ref) {
    const g = got.find((x) => x.pane === r.pane);
    if (!g) {
      bad.push(`${r.pane}: missing`);
      continue;
    }
    const ratio = r.ink ? g.ink / r.ink : 1;
    const size = r.glyph ? g.glyph / r.glyph : 1;
    if (!g.box || ratio < 0.5) bad.push(`${r.pane}: blank (ink ${g.ink} vs ${r.ink})`);
    else if (ratio > 1.8 || size > 1.35) bad.push(`${r.pane}: text too large (ink ${g.ink} vs ${r.ink}, glyphs ${g.glyph} vs ${r.glyph} px)`);
    else if (size < 0.75) bad.push(`${r.pane}: text too small (glyphs ${g.glyph} vs ${r.glyph} px)`);
    else if (r.box && (Math.abs(g.box[0] - r.box[0]) > 6 || Math.abs(g.box[1] - r.box[1]) > 6))
      bad.push(`${r.pane}: text starts at ${g.box.slice(0, 2)}, not ${r.box.slice(0, 2)}`);
    else {
      seen.ink = [Math.min(seen.ink[0], ratio), Math.max(seen.ink[1], ratio)];
      seen.glyph = [Math.min(seen.glyph[0], size), Math.max(seen.glyph[1], size)];
    }
  }
  // WebGL's own canvas; the DOM renderer leaves xterm's link layer unsized
  // at any scale, and its text is checked by its pixels above.
  for (const p of panes)
    for (const c of p.canvases.filter((x) => x.layer === "webgl")) {
      const [ew, eh] = [Math.round(c.cssW * scale), Math.round(c.cssH * scale)];
      if (Math.abs(c.w - ew) > 1 || Math.abs(c.h - eh) > 1)
        bad.push(`${p.pane}: ${c.layer} canvas ${c.w}x${c.h}, expected ${ew}x${eh}`);
    }
  return bad;
}

/** Resizes the real window (not the emulated viewport) so the page gets
 * exactly `size`: the window's bounds include its frame. */
async function resize(page, size) {
  const cdp = await page.context().newCDPSession(page);
  const { windowId, bounds } = await cdp.send("Browser.getWindowForTarget");
  const [w, h] = await page.evaluate(() => [window.innerWidth, window.innerHeight]);
  await cdp.send("Browser.setWindowBounds", {
    windowId,
    bounds: { width: size.width + bounds.width - w, height: size.height + bounds.height - h },
  });
  await page.waitForFunction(([sw, sh]) => window.innerWidth === sw && window.innerHeight === sh, [size.width, size.height]);
  await page.waitForTimeout(1_500);
}

async function frame(page, analyzer, scale) {
  const panes = await terminals(page);
  const png = await page.screenshot();
  return { panes, png, metrics: await measure(analyzer, png, panes, scale) };
}

const analyzerBrowser = await chromium.launch({ executablePath });
const analyzer = await analyzerBrowser.newPage();
let failures = 0;
const summary = [];

for (const gl of [true, false]) {
  for (const path of PAGES) {
    // 1x references: the opening frame at both widths.
    const refs = {};
    for (const size of [WIDE, NARROW]) {
      const b = await launch(1, size, gl);
      const p = await b.newPage({ viewport: null });
      await resize(p, size);
      await p.goto(origin + path);
      await settle(p);
      refs[size.width] = (await frame(p, analyzer, 1)).metrics;
      await b.close();
    }
    const n = gl ? runs : Math.min(3, runs);
    let passed = 0;
    for (let run = 1; run <= n; run++) {
      const b = await launch(emulate ? 1 : 2, WIDE, gl);
      const p = await b.newPage(emulate ? { viewport: WIDE, deviceScaleFactor: 2 } : { viewport: null });
      const errors = [];
      p.on("pageerror", (e) => errors.push(String(e)));
      if (!emulate) await resize(p, WIDE);
      const steps = [];
      const check = async (step, width) => {
        const f = await frame(p, analyzer, 2);
        const [dpr, iw, ih] = await p.evaluate(() => [window.devicePixelRatio, window.innerWidth, window.innerHeight]);
        const bad =
          dpr !== 2
            ? [`devicePixelRatio ${dpr}`]
            : iw !== width || ih !== WIDE.height
              ? [`page ${iw}x${ih}, expected ${width}x${WIDE.height}`]
              : problems(refs[width], f.metrics, f.panes, 2);
        if (bad.length) writeFileSync(`${out}/${gl ? "gl" : "dom"}-${path.includes("embed") ? "embed" : "page"}-${run}-${step}.png`, f.png);
        steps.push({ step, bad });
      };
      await p.goto(origin + path);
      await settle(p);
      await check("open", WIDE.width);
      if (emulate) {
        await b.close();
        const bad = steps.filter((s) => s.bad.length);
        if (bad.length) failures++;
        else passed++;
        console.log(`emulated ${gl ? "webgl" : "dom"} ${path} run ${run}:`, bad.length ? JSON.stringify(bad) : "clean");
        continue;
      }
      await resize(p, NARROW);
      await check("narrow", NARROW.width);
      await resize(p, WIDE);
      await check("wide again", WIDE.width);
      // Reset reloads the page: wait for the new document, not the old one.
      await p.evaluate(() => (window.__beforeReset = true));
      const reloaded = p.waitForEvent("domcontentloaded");
      await p.getByRole("button", { name: "Reset the demo" }).click();
      await reloaded;
      await settle(p);
      if (await p.evaluate(() => window.__beforeReset === true)) steps.push({ step: "after Reset", bad: ["the page didn't reload"] });
      else await check("after Reset", WIDE.width);
      await b.close();
      const bad = steps.filter((s) => s.bad.length);
      if (errors.length) bad.push({ step: "page errors", bad: errors });
      if (bad.length) {
        failures++;
        console.log(`FAIL ${gl ? "webgl" : "dom"} ${path} run ${run}:`, JSON.stringify(bad));
      } else {
        passed++;
        console.log(`ok ${gl ? "webgl" : "dom"} ${path} run ${run}`);
      }
    }
    const line = `${gl ? "WebGL" : "no WebGL"} ${path}: ${passed}/${n} runs clean (open, narrow, wide again, after Reset)`;
    summary.push(line);
    console.log(line, JSON.stringify(refs[WIDE.width].map((m) => [m.pane, m.ink, m.box])));
  }
}
await analyzerBrowser.close();
console.log(summary.join("\n"));
const range = ([lo, hi]) => `${lo.toFixed(2)}-${hi.toFixed(2)}`;
console.log(`clean frames against 1x: ink ${range(seen.ink)}x, glyph height ${range(seen.glyph)}x`);
process.exit(failures ? 1 : 0);
