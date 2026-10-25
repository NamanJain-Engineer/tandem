import { expect, test, type Page } from "@playwright/test";

// Accent discipline (F-WB-01 AC10, DESIGN §4.2, D-1875): at most three teal
// things in any first viewport, in both themes: the primary action, the one
// highlighted phrase and the mark. Eyebrows, inline links, ticks and list
// icons are neutral. Product media (captures, scenes, the demo) are the app,
// drawn as it is, so they don't count.

const PAGES = ["/", "/download/", "/changelog/", "/docs/", "/blog/", "/about/", "/demo/"];

/** Elements in the viewport that paint teal themselves (text, fill, border,
 * gradient or SVG stroke), counting a teal element once with whatever teal
 * is inside it. Returns a short description of each. */
async function tealInView(page: Page): Promise<string[]> {
  return page.evaluate(() => {
    // The palette's teal stops, as tokens.css defines them.
    const TEAL = ["#effcf9", "#ccfbf1", "#99f6e4", "#5eead4", "#2dd4bf", "#14b8a6", "#0d9488", "#0f766e", "#115e59", "#134e4a"].map((h) => {
      const n = parseInt(h.slice(1), 16);
      return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
    });
    const isTeal = (value: string) =>
      [...value.matchAll(/rgba?\((\d+),\s*(\d+),\s*(\d+)(?:,\s*([\d.]+))?\)/g)].some((m) => {
        if (m[4] !== undefined && Number(m[4]) < 0.35) return false; // a faint wash isn't an accent
        const c = [Number(m[1]), Number(m[2]), Number(m[3])];
        return TEAL.some((t) => t.every((v, i) => Math.abs(v - (c[i] as number)) <= 2));
      });
    const ownText = (el: Element) => [...el.childNodes].some((n) => n.nodeType === 3 && (n.textContent ?? "").trim() !== "");
    const counted: Element[] = [];
    for (const el of document.querySelectorAll("body *")) {
      if (counted.some((c) => c.contains(el))) continue;
      const r = el.getBoundingClientRect();
      if (r.width === 0 || r.height === 0 || r.bottom <= 0 || r.top >= innerHeight || r.right <= 0 || r.left >= innerWidth) continue;
      const s = getComputedStyle(el);
      if (s.visibility === "hidden" || Number(s.opacity) === 0) continue;
      if (el.closest("[data-orbit]")) continue; // the rings' light behind the page (DESIGN §4.5)
      if (el.closest("[data-stage]")) continue; // product media show the app as it is (DESIGN §4.6)
      const paints =
        (ownText(el) && isTeal(s.color)) ||
        isTeal(s.backgroundColor) ||
        isTeal(s.backgroundImage) ||
        (parseFloat(s.borderTopWidth) > 0 && isTeal(s.borderTopColor)) ||
        (el instanceof SVGElement && (isTeal(s.stroke) || isTeal(s.fill)));
      if (!paints) continue;
      const svg = el.closest("svg");
      const target = svg ?? el;
      if (counted.some((c) => c.contains(target))) continue;
      counted.push(target);
    }
    return counted.map((c) => `${c.tagName.toLowerCase()}.${String(c.getAttribute("class") ?? "").split(" ").slice(0, 3).join(".")} "${(c.textContent ?? "").trim().slice(0, 30)}"`);
  });
}

for (const scheme of ["dark", "light"] as const) {
  test(`F-WB-01 AC10 no first viewport shows more than three teal things (${scheme})`, async ({ browser }) => {
    const ctx = await browser.newContext({ colorScheme: scheme, reducedMotion: "reduce", viewport: { width: 1440, height: 900 } });
    await ctx.route(/\.(webm|mp4)$/, (route) => route.fulfill({ status: 204, body: "" }));
    await ctx.route(/\/demo\/app\//, (route) => route.fulfill({ status: 204, body: "" }));
    const page = await ctx.newPage();
    const over: Record<string, string[]> = {};
    for (const path of PAGES) {
      await page.goto(path);
      const teal = await tealInView(page);
      if (teal.length > 3) over[path] = teal;
    }
    expect(over).toEqual({});
    await ctx.close();
  });
}
