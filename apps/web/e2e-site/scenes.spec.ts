import { expect, test, type Locator, type Page } from "@playwright/test";

// The product scenes in a real browser (F-WB-01 AC8, AC9; D-1880, D-1881).

test.beforeEach(async ({ context }) => {
  await context.route(/\.(webm|mp4)$/, (route) => route.fulfill({ status: 204, body: "" }));
  await context.route(/\/demo\/app\//, (route) => route.fulfill({ status: 204, body: "" }));
});

/** The play states of a scene's own effects (its `sc-*` animations; a
 * working agent's dots pulse for as long as it works). */
const states = (page: Page, id: string) =>
  page.evaluate((id) => {
    const scene = document.querySelector(`[data-scene="${id}"]`);
    if (!scene) return ["missing"];
    const own = scene.getAnimations({ subtree: true }).filter((a) => String((a as CSSAnimation).animationName).startsWith("sc-"));
    return [...new Set(own.map((a) => a.playState))].sort();
  }, id);

const opacity = (page: Page, selector: string) =>
  page.evaluate((s) => Number(getComputedStyle(document.querySelector(s) as Element).opacity), selector);

/** The text of each of `selector`'s elements in `scene` that shows: drawn,
 * with its words opaque once every element above them is counted. A value's
 * step can be cut by an element above the one selected (Changes, D-3002),
 * and a finished fade can rest a hair above 0. */
const shownIn = (scene: Locator, selector: string) =>
  scene.locator(selector).evaluateAll((els) =>
    els
      .filter((e) => {
        const words = [e, ...e.querySelectorAll("*")].filter((n) => [...n.childNodes].some((c) => c.nodeType === Node.TEXT_NODE && (c.textContent ?? "").trim() !== ""));
        const opacity = (n: Element) => {
          let o = 1;
          for (let x: Element | null = n; x; x = x.parentElement) o *= Number(getComputedStyle(x).opacity);
          return o;
        };
        return e.checkVisibility() && words.some((w) => opacity(w) > 0.5);
      })
      .map((e) => e.textContent?.trim()),
  );

/** `shownIn` for the scene `id` on the page. */
const seen = (page: Page, id: string, selector: string) => shownIn(page.locator(`[data-scene="${id}"]`), selector);

/** The release the built site's scenes show, from a scene's status bar ("v0.12.0"), and whether it's `min` or later:
 * the same build is checked at the release the site shows and at the next slice's (D-3006, OPERATING §6). */
const sceneBuild = (page: Page) =>
  page.locator("[data-scene] .sc-statusbar").first().evaluate((e) => (/v(\d+)\.(\d+)\.(\d+)/.exec(e.textContent ?? "") ?? []).slice(1).map(Number));
const atLeast = (version: number[], min: string) => {
  const m = min.split(".").map(Number);
  for (let i = 0; i < 3; i++) if ((version[i] ?? 0) !== (m[i] ?? 0)) return (version[i] ?? 0) > (m[i] ?? 0);
  return true;
};

/** The scene `id`'s own effects, paused at `seconds`. */
const seek = (page: Page, id: string, seconds: number) =>
  page.evaluate(
    ([id, ms]) => {
      const root = document.querySelector(`[data-scene="${id}"]`) as Element;
      for (const a of root.getAnimations({ subtree: true }).filter((x) => String((x as CSSAnimation).animationName).startsWith("sc-") && x.timeline instanceof DocumentTimeline)) {
        a.pause();
        a.currentTime = ms;
      }
    },
    [id, seconds * 1000] as const,
  );

for (const [w, h] of [[1440, 900], [1366, 768], [1280, 800]] as const) {
  test(`F-WB-01 AC9 the product is in the first viewport at ${w}×${h} (DESIGN §5.1)`, async ({ page }) => {
    await page.setViewportSize({ width: w, height: h });
    await page.goto("/");
    const box = await page.locator('[data-scene="workspace"] .frame').boundingBox();
    expect(box).not.toBeNull();
    if (!box) return;
    const visible = h - box.y;
    expect(visible / box.height, `${visible}px of ${box.height}px`).toBeGreaterThanOrEqual(0.15);
  });
}

// Screen sizes, and the viewports a laptop's browser actually gets once its
// tabs, address bar and the taskbar take 100–140 px (D-1883).
for (const [w, h] of [[1366, 768], [1280, 800], [1536, 864], [1100, 768], [1366, 650], [1536, 740], [1280, 700]] as const) {
  test(`F-WB-01 AC9 the hero's scene plays in a ${w}×${h} first screen, without scrolling`, async ({ page }) => {
    await page.setViewportSize({ width: w, height: h });
    await page.goto("/");
    await expect.poll(() => states(page, "workspace"), { timeout: 10_000 }).toContain("running");
  });
}

test("F-WB-01 AC9 with the scene player blocked, every scene shows its final frame, not its first (D-1880)", async ({ browser }) => {
  const context = await browser.newContext({ viewport: { width: 1440, height: 900 } });
  await context.route(/\/demo\/embed\.js$/, (route) => route.abort());
  await context.route(/\/demo\/app\//, (route) => route.fulfill({ status: 204, body: "" }));
  const page = await context.newPage();
  await page.goto("/");
  await page.waitForLoadState("load");
  expect(await page.evaluate(() => document.documentElement.hasAttribute("data-fx"))).toBe(false);
  for (const id of ["workspace", "dashboard", "review"]) {
    await page.locator(`[data-scene="${id}"]`).scrollIntoViewIfNeeded();
    expect(await states(page, id), id).toEqual([]);
  }
  // The finished work is in the review queue, its check passed, and the waiting agent still needs you (D-2702).
  expect(await seen(page, "workspace", ".sc-rq-check > .sc-swap > *")).toEqual(["Check passed"]);
  expect(await seen(page, "workspace", ".sc-wtrow-ask")).toEqual(["Needs you"]);
  expect(await opacity(page, '[data-scene="review"] .sc-review-bar .sc-btn-primary')).toBe(1);
  await context.close();
});

test("F-WB-01 AC9 a scene waits off screen, plays on screen, and stops in a hidden tab", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto("/");
  // Below the fold: waiting at its first frame.
  expect(await states(page, "review")).toEqual(["paused"]);
  await page.locator('[data-scene="review"]').scrollIntoViewIfNeeded();
  await expect.poll(() => states(page, "review")).toContain("running");
  // The tab goes into the background: everything pauses where it is.
  await page.evaluate(() => {
    Object.defineProperty(document, "hidden", { configurable: true, get: () => true });
    document.dispatchEvent(new Event("visibilitychange"));
  });
  await expect.poll(() => states(page, "review")).not.toContain("running");
  await page.evaluate(() => {
    Object.defineProperty(document, "hidden", { configurable: true, get: () => false });
    document.dispatchEvent(new Event("visibilitychange"));
  });
  // It plays once and rests on its final frame.
  await expect.poll(() => states(page, "review"), { timeout: 20_000 }).toEqual(["finished"]);
  expect(await opacity(page, '[data-scene="review"] .sc-review-bar .sc-btn-primary')).toBe(1);
});

test("F-WB-01 AC9 D-3000 the Review scene's comments go in at once: no frame draws the rows under them twice", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto("/");
  const scene = page.locator('[data-scene="review"]');
  await scene.scrollIntoViewIfNeeded();
  await expect.poll(() => states(page, "review"), { timeout: 10_000 }).toContain("running");
  // Just before the comments arrive, then through what would be any fade.
  const frames = await page.evaluate(() => {
    const root = document.querySelector('[data-scene="review"]') as Element;
    const [was, now] = [...root.querySelectorAll(".sc-diff-swap > .sc-diff-rows")];
    const at = Number.parseFloat(getComputedStyle(was as Element).getPropertyValue("--at"));
    return [-0.02, 0.02, 0.09, 0.18, 0.3].map((d) => {
      // The scene's own effects (a scroll reveal around it runs on the scroll, not the clock).
      for (const a of root.getAnimations({ subtree: true }).filter((x) => x.timeline instanceof DocumentTimeline)) {
        a.pause();
        a.currentTime = (at + d) * 1000;
      }
      return [d, Number(getComputedStyle(was as Element).opacity), Number(getComputedStyle(now as Element).opacity)];
    });
  });
  for (const [d, was, now] of frames) expect([was, now], `${d} s after the comments`).toEqual((d ?? 0) < 0 ? [1, 0] : [0, 1]);
});

/** Each page with scenes, and its scenes. */
const SCENE_IDS = [
  ["/", ["workspace", "dashboard", "review"]],
  ["/changelog/0.9.0/", ["land", "checks"]],
  ["/changelog/0.12.0/", ["plan"]],
] as const;

/** The text the browser draws for each scene on the page, by scene: every piece of text its layout holds, what CSS
 * writes included (::before, ::after and ::marker, with counters, attr() and counter styles worked out), read from
 * Chromium's layout tree (DOMSnapshot). Nothing with display: none is drawn, so a width draws its own; a value's
 * steps all lie out at once, shown or not (D-3002), so each is read. */
async function drawnText(page: Page): Promise<Map<string, string[]>> {
  const cdp = await page.context().newCDPSession(page);
  const { documents, strings } = await cdp.send("DOMSnapshot.captureSnapshot", { computedStyles: [] });
  await cdp.detach();
  const doc = documents[0];
  const out = new Map<string, string[]>();
  if (!doc) return out;
  const str = (i: number | undefined) => (i === undefined || i < 0 ? "" : (strings[i] ?? ""));
  const parent = doc.nodes.parentIndex ?? [];
  const attrs = doc.nodes.attributes ?? [];
  // Each node's scene: its own data-scene, or its nearest ancestor's.
  const sceneOf = new Map<number, string | null>();
  const sceneAt = (i: number): string | null => {
    const known = sceneOf.get(i);
    if (known !== undefined) return known;
    const a = attrs[i] ?? [];
    let id: string | null = null;
    for (let k = 0; k + 1 < a.length; k += 2) if (str(a[k]) === "data-scene") id = str(a[k + 1]);
    const up = parent[i] ?? -1;
    const found = id ?? (up >= 0 ? sceneAt(up) : null);
    sceneOf.set(i, found);
    return found;
  };
  doc.layout.nodeIndex.forEach((node, k) => {
    const text = str(doc.layout.text[k]);
    const id = sceneAt(node);
    if (id !== null && text.trim() !== "") out.set(id, [...(out.get(id) ?? []), text]);
  });
  return out;
}

// No scene draws the placeholder name (D-1898), as the browser draws it (D-3006). The vitest reads each scene's markup
// and the scene sheets' `content`, so CSS it doesn't pin could still write "tandem/" (the coordinator's delta review of
// 400f7c55: a `content` rule aimed at a scene in another sheet, a list-style string, a counter style's symbols). Here
// every sheet the page loads is in play, at both widths, with the effects on and under reduced motion.
for (const width of [1440, 390] as const) {
  test(`F-WB-01 AC9 D-1898 D-3006 at ${width} px no scene draws the placeholder name, read as the browser lays it out with ::before, ::after and ::marker`, async ({ browser }) => {
    for (const reducedMotion of ["no-preference", "reduce"] as const) {
      const context = await browser.newContext({ reducedMotion, viewport: { width, height: 900 } });
      await context.route(/\.(webm|mp4)$/, (route) => route.fulfill({ status: 204, body: "" }));
      await context.route(/\/demo\/app\//, (route) => route.fulfill({ status: 204, body: "" }));
      const page = await context.newPage();
      for (const [path, ids] of SCENE_IDS) {
        await page.goto(path);
        await page.waitForLoadState("load");
        if (reducedMotion === "no-preference") await expect.poll(() => page.evaluate(() => document.documentElement.hasAttribute("data-fx"))).toBe(true);
        const drawn = await drawnText(page);
        expect([...drawn.keys()].sort(), path).toEqual([...ids].sort());
        for (const [id, texts] of drawn) {
          // Its caption's label, so the scene was read.
          expect(texts.join(" "), `${path} ${id}`).toMatch(/simplified/i);
          expect(texts.filter((x) => /tandem/i.test(x)), `${path} ${id} (${reducedMotion})`).toEqual([]);
        }
      }
      // The reader sees what CSS writes: each of the three ways in, planted in the last page's first scene, is read.
      await page.addStyleTag({
        content: `[data-scene] .sc-window::after { content: "tandem/"; }
          @counter-style placeholder-probe { system: cyclic; symbols: tandem; }
          .probe-string { display: list-item; list-style: "tandem/ " inside; }
          .probe-counter { display: list-item; list-style: placeholder-probe inside; }`,
      });
      await page.evaluate(() => {
        const win = document.querySelector("[data-scene] .sc-window") as Element;
        for (const c of ["probe-string", "probe-counter"]) {
          const probe = document.createElement("span");
          probe.className = c;
          probe.textContent = "x";
          win.append(probe);
        }
      });
      const planted = [...(await drawnText(page)).values()].flat().filter((x) => /tandem/i.test(x));
      for (const way of [/^tandem\/$/, /^tandem\/ $/, /^tandem\. $/]) expect(planted.some((x) => way.test(x)), `${way} (${reducedMotion})`).toBe(true);
      await context.close();
    }
  });
}

test("F-WB-01 AC9 D-3001 in real playback every effect in a scene starts on one clock, so what must coincide does", async ({ page }) => {
  // Seeking (the test above) puts every effect on one clock, so it can't see this: in Chrome the Review scene's cut and
  // show once started two to five frames before the rest, and the rows moved before the comments came (SF-1).
  await page.setViewportSize({ width: 1440, height: 900 });
  for (const [path, ids] of SCENE_IDS) {
    await page.goto(path);
    for (const id of ids) {
      await page.locator(`[data-scene="${id}"]`).scrollIntoViewIfNeeded();
      await expect.poll(() => states(page, id), { timeout: 10_000 }).toContain("running");
      await page.waitForTimeout(100);
      const starts = await page.evaluate((id) => {
        const own = (document.querySelector(`[data-scene="${id}"]`) as Element)
          .getAnimations({ subtree: true })
          .filter((a) => String((a as CSSAnimation).animationName).startsWith("sc-") && a.timeline instanceof DocumentTimeline);
        return [own.length, [...new Set(own.map((a) => Math.round(Number(a.startTime) * 10) / 10))]] as const;
      }, id);
      expect(starts[0], id).toBeGreaterThan(5);
      expect(starts[1], `${id}: start times`).toHaveLength(1);
    }
  }
});

for (const width of [1440, 390] as const) {
  test(`F-WB-01 AC9 D-3002 at ${width} px no swap shows two of its values at once: each scene stepped every 25 ms, and each change ± 0.05 s`, async ({ page }) => {
    // The app re-renders a value at once, so two never share one place, mid-swap included (DESIGN §6). With each
    // scene's own effects seeked to the same time: every grid cell that stacks two or more items (a swap's values, a
    // panel and the one that replaces it) shows at most one of them, and at most one toast shows at a time. A dot's
    // swap may fade, as §4.7's status change does; an overlay with a z-index of its own covers what's under it.
    await page.setViewportSize({ width, height: 900 });
    const all: string[] = [];
    for (const [path, ids] of SCENE_IDS) {
      await page.goto(path);
      await expect.poll(() => page.evaluate(() => document.documentElement.hasAttribute("data-fx"))).toBe(true);
      for (const id of ids) {
        const found = await page.evaluate((id) => {
          const root = document.querySelector(`[data-scene="${id}"]`) as Element;
          const own = root
            .getAnimations({ subtree: true })
            .filter((a) => String((a as CSSAnimation).animationName).startsWith("sc-") && a.timeline instanceof DocumentTimeline);
          for (const a of own) a.pause();
          // Where each item sits: a grid's in-flow items (a display: contents box's own), by row and column.
          const items = (el: Element): Element[] => [...el.children].flatMap((c) => (getComputedStyle(c).display === "contents" ? items(c) : [c]));
          const stacks: Element[][] = [];
          for (const el of [root, ...root.querySelectorAll("*")]) {
            if (!getComputedStyle(el).display.endsWith("grid") || el.classList.contains("sc-dot-swap")) continue;
            const cells = new Map<string, Element[]>();
            for (const c of items(el)) {
              const s = getComputedStyle(c);
              if (s.position === "absolute" || s.position === "fixed" || s.zIndex !== "auto") continue;
              if (!/^\d+$/.test(s.gridRowStart) || !/^\d+$/.test(s.gridColumnStart)) continue;
              const k = `${s.gridRowStart}/${s.gridColumnStart}`;
              cells.set(k, [...(cells.get(k) ?? []), c]);
            }
            for (const group of cells.values()) if (group.length > 1) stacks.push(group);
          }
          const toasts = [...root.querySelectorAll(".sc-note-toast")];
          /** How much of `el` shows: its opacity times its most visible content's (its own text, an icon, or an element
           * with nothing in it, counted as drawn even when it draws nothing, as the design lead's checker counts it). */
          const seen = (el: Element): number => {
            if (!el.checkVisibility({ visibilityProperty: true })) return 0;
            const o = Number(getComputedStyle(el).opacity);
            if (o < 0.05) return 0;
            const kids = [...el.children];
            if (el instanceof SVGSVGElement || kids.length === 0) return o;
            const text = [...el.childNodes].some((n) => n.nodeType === Node.TEXT_NODE && (n.textContent ?? "").trim() !== "");
            return o * Math.max(text ? 1 : 0, ...kids.map(seen));
          };
          const end = Math.max(0, ...own.map((a) => Number(a.effect?.getComputedTiming().endTime ?? 0))) + 200;
          const times = new Set<number>();
          for (let t = 0; t <= end; t += 25) times.add(t);
          for (const a of own) for (const d of [-50, -1, 0, 1, 50, 100]) times.add(Math.max(0, Number(a.effect?.getTiming().delay ?? 0) + d));
          // What shows, by its words, or a drawn thing's class (a dot, an icon, a spinner).
          const name = (e: Element) => (e.textContent ?? "").trim().slice(0, 32) || (e.matches("[class]") ? e : e.querySelector("[class]"))?.getAttribute("class")?.split(" ")[0] || e.tagName;
          // Each stack's overlaps, as runs of sample times no more than 60 ms apart.
          const runs: string[] = [];
          const open = new Map<number, { from: number; to: number; what: string }>();
          const groups = [...stacks, toasts];
          for (const t of [...times].sort((a, b) => a - b)) {
            for (const a of own) a.currentTime = t;
            groups.forEach((group, i) => {
              const on = group.filter((c) => seen(c) > 0.05);
              const run = open.get(i);
              if (on.length > 1 && run && t - run.to <= 60) run.to = t;
              else if (on.length > 1) {
                if (run) runs.push(`${Math.round(run.from)}–${Math.round(run.to)} ms: ${run.what}`);
                open.set(i, { from: t, to: t, what: on.map(name).join(" | ") });
              }
            });
          }
          for (const run of open.values()) runs.push(`${Math.round(run.from)}–${Math.round(run.to)} ms: ${run.what}`);
          return runs.sort((a, b) => Number.parseInt(a) - Number.parseInt(b));
        }, id);
        all.push(...found.map((f) => `${path} ${id}, ${f}`));
      }
    }
    expect(all).toEqual([]);
  });
}

test("F-WB-01 AC9 D-3003 every step of a value lays its contents out as the others do: Land's dot and words, the CI rail's spinner and words", async ({ page }) => {
  // Layout, which the swap test doesn't look at and the design shots see only in the final frame. A step that's cut
  // is still laid out, so every step is measured at once. A middle step was an entering cell with a cut inside it,
  // and the rules for a step's contents styled the cut: no gap, no centring (the coordinator's S-1).
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto("/changelog/0.9.0/");
  // From 0.13.0 the CI rail is CodeRail's, and its badge is ChecksBadge's: its icon (or spinner) beside its words.
  const folds = atLeast(await sceneBuild(page), "0.13.0");
  const steps = await page.evaluate(() =>
    // What's drawn at this width (a part only a phone draws has no boxes here), each step of it.
    [...document.querySelectorAll("[data-scene] .sc-step > *, [data-scene] .sc-checks > * > *, [data-scene] .sc-wtrow-checks")].filter((el) => el.getClientRects().length > 0).map((el) => {
      const s = getComputedStyle(el);
      // Its mark (a dot, the spinner, the tick) and its words, each directly inside it.
      const mark = el.querySelector(":scope > :is(.sc-dot, .sc-spin, svg)");
      const words = [...el.childNodes].find((n) => n.nodeType === Node.TEXT_NODE && (n.textContent ?? "").trim() !== "");
      const text = (el.textContent ?? "").trim();
      if (!mark || !words) return { text, display: s.display, gap: null, centre: null };
      const range = document.createRange();
      range.selectNodeContents(words);
      const [m, w] = [mark.getBoundingClientRect(), range.getBoundingClientRect()];
      return { text, display: s.display, gap: w.left - m.right - Number.parseFloat(s.columnGap), centre: (m.top + m.bottom) / 2 - (w.top + w.bottom) / 2 };
    }),
  );
  // Land's five steps in each of its three lanes, and the rail badge's three checks; from 0.13.0 the badge in each of
  // the row's five steps (its icon alone in one, beside the agent's status).
  expect(steps).toHaveLength(15 + (folds ? 5 : 3));
  for (const step of steps) {
    // A flex row: inline-flex, or flex where the step is a grid's item (a swap's), which CSS makes block-level.
    expect(step.display, step.text).toMatch(/^(inline-)?flex$/);
    if (step.gap === null || step.centre === null) continue;
    expect(Math.abs(step.gap), `${step.text}: gap`).toBeLessThan(0.6);
    expect(Math.abs(step.centre), `${step.text}: centre`).toBeLessThan(1.1);
  }
  // Only the failed checks' words sit in a span of their own (from 0.13.0, only the badge that's its icon alone has no
  // words); every other step has its mark beside its words.
  expect(new Set(steps.filter((s) => s.gap === null).map((s) => s.text))).toEqual(new Set([folds ? "" : "1 check failed: test"]));
});

for (const width of [1440, 390] as const) {
  test(`F-WB-01 AC9 D-3004 at ${width} px the Dashboard closes the finished card's line as the app does: what's below waits a line lower, then eases up`, async ({ page }) => {
    // The app draws the line only when there is one, so what's below the card closes up when it goes. Seeked through
    // the change: the next group sits the lanes' gap below the card that shows, at work and done, and the lanes' rule
    // reaches the last card throughout, its tail going as the groups move (the design lead's must-fix on cced2f8f).
    await page.setViewportSize({ width, height: 900 });
    await page.goto("/");
    await expect.poll(() => page.evaluate(() => document.documentElement.hasAttribute("data-fx"))).toBe(true);
    const at = (seconds: number) =>
      page.evaluate((ms) => {
        const root = document.querySelector('[data-scene="dashboard"]') as Element;
        for (const a of root.getAnimations({ subtree: true }).filter((x) => String((x as CSSAnimation).animationName).startsWith("sc-") && x.timeline instanceof DocumentTimeline)) {
          a.pause();
          a.currentTime = ms;
        }
        const swap = root.querySelector(".sc-card-swap") as Element;
        const card = ([...swap.children].find((c) => Number(getComputedStyle(c).opacity) > 0.5) as Element).querySelector(".sc-card") as Element;
        const lanes = root.querySelector(".sc-group-lanes") as Element;
        const groups = [...lanes.children].filter((c) => c.classList.contains("sc-group"));
        const next = groups[groups.indexOf(swap.closest(".sc-group") as Element) + 1] as Element;
        const tail = (lanes.querySelector(".sc-lanes-tail") as Element).getBoundingClientRect();
        const rule = Math.max(lanes.getBoundingClientRect().bottom, tail.height > 0.01 ? tail.bottom : 0);
        const last = (groups.at(-1)?.querySelector(".sc-card") as Element).getBoundingClientRect().bottom;
        return { gap: next.getBoundingClientRect().top - card.getBoundingClientRect().bottom, lanesGap: Number.parseFloat(getComputedStyle(lanes).rowGap), rule: rule - last, tail: tail.height };
      }, seconds * 1000);
    for (const s of [0, 4.59, 7]) {
      const m = await at(s);
      expect(Math.abs(m.gap - m.lanesGap), `${s} s: the next group's gap`).toBeLessThan(0.6);
      expect(Math.abs(m.rule), `${s} s: the rule to the last card`).toBeLessThan(0.6);
    }
    // Mid-move, the rule still reaches the last card; once moved, nothing of the tail is drawn.
    for (const s of [4.62, 4.66, 4.7, 4.75]) expect(Math.abs((await at(s)).rule), `${s} s: the rule to the last card`).toBeLessThan(0.6);
    expect((await at(7)).tail).toBeLessThan(0.01);
  });
}

// The release pages' scenes (components/site/scenes/releaseScenes.tsx):
// where each plays, and an element that only its final frame shows.
const RELEASE_PAGES = [
  { id: "land", path: "/changelog/0.9.0/", final: '[data-scene="land"] .sc-dialog-done' },
  { id: "plan", path: "/changelog/0.12.0/", final: '[data-scene="plan"] .sc-plan-foot > .sc-swap:last-child > [data-fx-enter]' },
  { id: "checks", path: "/changelog/0.9.0/", final: '[data-scene="checks"] .sc-review-bar .sc-btn-primary' },
] as const;

for (const r of RELEASE_PAGES) {
  test(`F-WB-01 AC9 the ${r.id} scene plays on ${r.path}, at the container's full width (D-1885, D-1888)`, async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.goto(r.path);
    await page.locator(`[data-scene="${r.id}"]`).scrollIntoViewIfNeeded();
    await expect.poll(() => states(page, r.id), { timeout: 10_000 }).toContain("running");
    // As on the home page, not the notes' column, so the app's text reads (D-1886, D-1898).
    const [scene, container] = await page.evaluate((id) => [
      document.querySelector(`[data-scene="${id}"]`)?.getBoundingClientRect().width ?? 0,
      document.querySelector("article .container-site")?.getBoundingClientRect().width ?? 1,
    ], r.id);
    expect(scene / container).toBeGreaterThan(0.9);
  });

  // Without its player and under reduced motion: no effects, and the end showing.
  for (const mode of ["player blocked", "reduced motion"] as const) {
    test(`F-WB-01 AC8 AC9 on ${r.path} with the ${mode}, the ${r.id} scene shows its final frame`, async ({ browser }) => {
      const context = await browser.newContext({ viewport: { width: 1440, height: 900 }, reducedMotion: mode === "reduced motion" ? "reduce" : "no-preference" });
      if (mode === "player blocked") await context.route(/\/demo\/embed\.js$/, (route) => route.abort());
      const page = await context.newPage();
      await page.goto(r.path);
      await page.waitForLoadState("load");
      await page.locator(`[data-scene="${r.id}"]`).scrollIntoViewIfNeeded();
      if (mode === "player blocked") expect(await page.evaluate(() => document.documentElement.hasAttribute("data-fx"))).toBe(false);
      expect(await states(page, r.id)).not.toContain("running");
      expect(await opacity(page, r.final)).toBe(1);
      await context.close();
    });
  }
}

test("F-WB-01 AC9 the CI scene ends with the checks passed, the agent's summary, the reviewer's comment imported and Send to Ada (D-3001)", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto("/changelog/0.9.0/");
  const scene = page.locator('[data-scene="checks"]');
  await scene.scrollIntoViewIfNeeded();
  await expect.poll(() => states(page, "checks"), { timeout: 25_000 }).toEqual(["finished"]);
  const shown = (selector: string) => shownIn(scene, selector);
  // From 0.13.0 the rail is CodeRail's, and the badge says the status bar's words.
  const folds = atLeast(await sceneBuild(page), "0.13.0");
  expect(await shown(folds ? ".sc-crail .sc-wtrow-checks" : ".sc-rail .sc-checks > *")).toEqual([folds ? "CI passed" : "Checks passed"]);
  expect(await shown(".sc-files-head .sc-branch-chip")).toEqual([]);
  // The agent's last line, what it changed, shows inside its pane.
  const last = await scene.locator(".sc-pane .sc-term .sc-line").last().evaluate((e) => {
    const term = e.closest(".sc-term") as Element;
    return { text: e.textContent?.trim() ?? "", inside: e.getBoundingClientRect().bottom <= term.getBoundingClientRect().bottom + 0.5 };
  });
  expect(last.inside, last.text).toBe(true);
  expect((await shown(".sc-pane .sc-term .sc-line")).at(-1)).toBe(last.text);
  expect((await shown(".sc-ci-review .sc-diff-note")).join(" ")).toContain("@sample-reviewer:");
  expect(await shown(".sc-review-bar > *")).toEqual(["1 comment", "Copy review", "Send to Ada"]);
  // The Checks dialog and the toasts have come and gone.
  expect(await shown(".sc-ci-layer, .sc-note-toast")).toEqual([]);
});

test("F-WB-01 AC9 D-3001 the CI scene's rules load only on the page that plays it, after the shared scene rules", async ({ page }) => {
  /** Every rule the page's stylesheets hold that draws the CI scene. */
  const ciRules = () =>
    page.evaluate(() =>
      [...document.styleSheets].flatMap((s) => {
        const walk = (rules: CSSRuleList): string[] =>
          [...rules].flatMap((r) => ("cssRules" in r && !(r instanceof CSSStyleRule) ? walk((r as CSSGroupingRule).cssRules) : r instanceof CSSStyleRule && /\.sc-(ci|checks|menu|branch-chip)\b|\.sc-(body|rail|lane)(?![\w-])/.test(r.selectorText) ? [r.selectorText] : []));
        return walk(s.cssRules);
      }),
    );
  for (const path of ["/", "/changelog/0.12.0/"]) {
    await page.goto(path);
    expect(await ciRules(), path).toEqual([]);
  }
  await page.goto("/changelog/0.9.0/");
  expect((await ciRules()).length).toBeGreaterThan(20);
  // One inline sheet in the head, after the shared ones, and the scene draws with it; from 0.13.0 the rail's rules
  // first, in the same one (D-3006).
  const sheets = atLeast(await sceneBuild(page), "0.13.0") ? "scenes/rail.css scenes/ci.css" : "scenes/ci.css";
  const order = await page.evaluate(() => [...document.head.querySelectorAll('link[rel="stylesheet"], style')].map((e) => (e.tagName === "STYLE" ? e.getAttribute("data-href") : "link")));
  expect(order.at(-1)).toBe(sheets);
  expect(order.filter((x) => x === sheets)).toHaveLength(1);
  expect(await page.evaluate(() => getComputedStyle(document.querySelector('[data-scene="checks"] .sc-ci-layer') as Element).position)).toBe("absolute");
});

test("F-WB-01 AC9 the plan scene ends where the app's dialog does: from 0.14.0 3 approved, Land 3 approved… and the rounds line, before it 2 approved · 1 with changes requested and Land 2 approved… (D-1888, D-884)", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto("/changelog/0.12.0/");
  const scene = page.locator('[data-scene="plan"]');
  await scene.scrollIntoViewIfNeeded();
  await expect.poll(() => states(page, "plan"), { timeout: 20_000 }).toEqual(["finished"]);
  const shown = (selector: string) => shownIn(scene, selector);
  // From 0.14.0 (SENDS_BACK) the third review goes back to its run, and its next turn is approved (F-TK-09), with the
  // started dialog's Send reviews back switch on; before it, that review waits for you at Changes requested.
  const sendsBack = atLeast(await sceneBuild(page), "0.14.0");
  expect(await shown(".sc-plan-summary .sc-swap > *")).toEqual([sendsBack ? "3 approved" : "2 approved · 1 with changes requested"]);
  expect(await shown(".sc-plan-foot > .sc-swap:last-child > *")).toEqual([sendsBack ? "Land 3 approved…" : "Land 2 approved…"]);
  expect(await shown(".sc-run-note")).toEqual(sendsBack ? ["Review sent back · round 1 of 2"] : []);
  expect(await shown(".sc-switch-label")).toEqual(sendsBack ? ["Send reviews back to the run (up to 2 rounds)"] : []);
});

// The plan's review going back to its run, from 0.14.0 (D-884): its own beat, as the app sends it. Just before it the
// row says why (Reviewing · Changes requested, the verdict written while the run is still reviewed); just after, the
// rounds line is coming in and the run is Running again. The dialog keeps one size throughout: its switch is there
// from the start, and the rounds line has its room (the code review of a17c7d7a, M1).
for (const width of [1440, 360] as const) {
  test(`F-WB-01 AC9 D-884 at ${width} px the plan's review goes back to its run on a beat of its own, and the dialog keeps its size`, async ({ page }) => {
    await page.setViewportSize({ width, height: 900 });
    await page.goto("/changelog/0.12.0/");
    await expect.poll(() => page.evaluate(() => document.documentElement.hasAttribute("data-fx"))).toBe(true);
    const scene = page.locator('[data-scene="plan"]');
    await scene.scrollIntoViewIfNeeded();
    if (!atLeast(await sceneBuild(page), "0.14.0")) {
      expect(await scene.locator(".sc-run-note, .sc-plan-switch").count()).toBe(0);
      return;
    }
    const frame = async (t: number) => {
      await seek(page, "plan", t);
      return page.evaluate(() => {
        const scene = document.querySelector('[data-scene="plan"]') as Element;
        const stage = scene.querySelector("[data-stage]") as Element;
        // Its opacity on the scene's clock, within the stage (the page's reveal runs on the scroll, not the clock).
        const shown = (e: Element) => {
          let o = 1;
          for (let x: Element | null = e; x && x !== stage; x = x.parentElement) o *= Number(getComputedStyle(x).opacity);
          return o;
        };
        const note = scene.querySelector(".sc-run-note") as Element;
        const label = [...(note.closest(".sc-run") as Element).querySelectorAll(".sc-run-label .sc-swap > *")].filter((e) => shown(e) > 0.01).map((e) => (e.textContent ?? "").trim());
        const box = (scene.querySelector(".sc-plan-dialog") as Element).getBoundingClientRect();
        return { note: shown(note), label, box: [box.x, box.y, box.width, box.height] };
      });
    };
    const beat = await scene.locator(".sc-run-note").evaluate((e) => Number.parseFloat(getComputedStyle(e).getPropertyValue("--at")));
    const [before, after, settled] = [await frame(beat - 0.05), await frame(beat + 0.05), await frame(beat + 0.25)];
    expect([before.note, after.note > 0, settled.note]).toEqual([0, true, 1]);
    expect(before.label).toEqual(["Gemini CLI · Reviewing · Changes requested"]);
    expect(after.label).toEqual(["Gemini CLI · Running"]);
    expect(settled.label).toEqual(["Gemini CLI · Running"]);
    // Within half a pixel: the scene's entering words are composited apart, which can move a box by a tenth.
    const moved = (a: number[], b: number[]) => Math.max(...a.map((x, i) => Math.abs(x - (b[i] ?? Number.NaN))));
    expect(moved(after.box, before.box)).toBeLessThan(0.5);
    // And at every other change of the plan once the dialog has risen in, to the end.
    const times = await page.evaluate(() =>
      [...new Set([...document.querySelectorAll('[data-scene="plan"] .sc-plan-dialog [style*="--at"]')].map((e) => Number.parseFloat(getComputedStyle(e).getPropertyValue("--at"))))]
        .filter((t) => t >= 0.9)
        .sort((a, b) => a - b),
    );
    expect(times.length).toBeGreaterThan(5);
    for (const t of [...times.map((x) => x + 0.05), 12]) expect(moved((await frame(t)).box, before.box), `${t} s`).toBeLessThan(0.5);
  });
}

test("F-WB-01 AC9 D-884 from 320 to 1440 px the plan's dialog stays inside the app's window, with nothing in it past its edges", async ({ browser }) => {
  // Its final frame, which is its tallest: from 0.14.0 with its Send reviews back switch and the switch's hint, in a
  // phone's frame too (the code review of a17c7d7a, S2: 52 px to spare above and below at 320 px before them).
  const context = await browser.newContext({ reducedMotion: "reduce", viewport: { width: 1440, height: 900 } });
  const page = await context.newPage();
  await page.goto("/changelog/0.12.0/");
  const sendsBack = atLeast(await sceneBuild(page), "0.14.0");
  const phones: boolean[] = [];
  for (const width of [320, 360, 390, 600, 640, 672, 688, 704, 768, 1024, 1280, 1440]) {
    // Loaded at each width: right after a resize, Chromium takes a few frames to size the scene's text by its new
    // container, and a box read meanwhile is neither size.
    await page.setViewportSize({ width, height: 900 });
    await page.goto("/changelog/0.12.0/");
    const m = await page.evaluate(() => {
      const scene = document.querySelector('[data-scene="plan"]') as Element;
      const frame = scene.querySelector(".sc-window") as Element;
      const win = frame.getBoundingClientRect();
      // A phone's frame is 448 of the app's pixels wide, a wide one 1280 (scenes.css).
      const phone = Math.round(win.width / Number.parseFloat(getComputedStyle(frame).getPropertyValue("--sc-px"))) === 448;
      const dialog = scene.querySelector(".sc-plan-dialog") as Element;
      const d = dialog.getBoundingClientRect();
      const past = [...dialog.querySelectorAll("*")]
        .filter((e) => e.getClientRects().length > 0)
        .map((e) => [e, e.getBoundingClientRect()] as const)
        .filter(([, r]) => r.left < d.left - 0.5 || r.right > d.right + 0.5 || r.top < d.top - 0.5 || r.bottom > d.bottom + 0.5)
        .map(([e]) => `${e.className}: ${(e.textContent ?? "").slice(0, 30)}`);
      const hints = [...dialog.querySelectorAll(".sc-plan-switch-hint")].filter((e) => e.checkVisibility()).length;
      const labels = [...dialog.querySelectorAll(".sc-switch-label")].map((e) => e.getClientRects().length);
      return { phone, space: [d.top - win.top, win.bottom - d.bottom, d.left - win.left, win.right - d.right], past, hints, labels };
    });
    for (const s of m.space) expect(s, `${width} px: the dialog's room in the window`).toBeGreaterThan(4);
    expect(m.past, `${width} px`).toEqual([]);
    // The switch's label on one line, and its hint, in either frame.
    expect(m.labels, `${width} px`).toEqual(sendsBack ? [1] : []);
    expect(m.hints, `${width} px`).toBe(sendsBack ? 1 : 0);
    phones.push(m.phone);
  }
  // Both frames were measured.
  expect(new Set(phones)).toEqual(new Set([true, false]));
  await context.close();
});

test("F-WB-01 AC8 AC9 under reduced motion every scene shows its final frame and nothing moves", async ({ browser }) => {
  const context = await browser.newContext({ reducedMotion: "reduce", viewport: { width: 1440, height: 900 } });
  await context.route(/\/demo\/app\//, (route) => route.fulfill({ status: 204, body: "" }));
  const page = await context.newPage();
  await page.goto("/");
  for (const id of ["workspace", "dashboard", "review"]) {
    await page.locator(`[data-scene="${id}"]`).scrollIntoViewIfNeeded();
    const moving = await page.evaluate(
      (id) =>
        (document.querySelector(`[data-scene="${id}"]`) as Element)
          .getAnimations({ subtree: true })
          .filter((a) => a.playState === "running" && (a.effect?.getComputedTiming().duration ?? 0) > 1).length,
      id,
    );
    expect(moving, id).toBe(0);
  }
  // The final frame: the review item has arrived, the waiting agent needs you, and its Dashboard card has the waiting border.
  expect(await seen(page, "workspace", ".sc-rq-check > .sc-swap > *")).toEqual(["Check passed"]);
  expect(await seen(page, "workspace", ".sc-wtrow-ask")).toEqual(["Needs you"]);
  expect(await opacity(page, '[data-scene="dashboard"] .sc-card-waiting')).toBe(1);
  await context.close();
});

test("F-WB-01 AC9 the hero's scene plays without moving anything else on the page", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.addInitScript(() => {
    const w = window as unknown as { shift: number };
    w.shift = 0;
    new PerformanceObserver((list) => {
      for (const e of list.getEntries() as Array<PerformanceEntry & { value: number; hadRecentInput: boolean }>)
        if (!e.hadRecentInput) w.shift += e.value;
    }).observe({ type: "layout-shift", buffered: true });
  });
  await page.goto("/");
  await expect.poll(() => states(page, "workspace"), { timeout: 20_000 }).toEqual(["finished"]);
  const shift = await page.evaluate(() => (window as unknown as { shift: number }).shift);
  expect(shift).toBeLessThan(0.01);
});

// The hero as one worktree's window (D-2702).
for (const [w, h] of [[1440, 900], [1280, 800], [1024, 768], [768, 1024]] as const) {
  test(`F-WB-01 AC9 D-2702 at ${w} px the hero's rail holds every worktree's row whole, and it ends with the agent's card in review`, async ({ browser }) => {
    const context = await browser.newContext({ reducedMotion: "reduce", viewport: { width: w, height: h } });
    await context.route(/\/demo\/app\//, (route) => route.fulfill({ status: 204, body: "" }));
    const page = await context.newPage();
    await page.goto("/");
    const scene = page.locator('[data-scene="workspace"]');
    // Nothing in the rail is cut, nor the sidebar's pill and switch, nor the card's line.
    const cut = await scene
      .locator(".sc-wtrow-name, .sc-wsrow-name, .sc-badge, .sc-badge-quiet, .sc-slot-working, .sc-pill, .sc-seg-sm .sc-mode, .sc-rq-meta, .sc-wt-chip")
      .evaluateAll((els) => els.filter((e) => e.checkVisibility() && e.scrollWidth > e.clientWidth).map((e) => `${e.textContent}: ${e.scrollWidth} > ${e.clientWidth}`));
    expect(cut).toEqual([]);
    expect(await seen(page, "workspace", ".sc-wtrow-name")).toEqual(["checkout-fix", "coupon-rules", "search-ui"]);
    // The 0.12.0 hero leaves the intent radar out, at a rail width CodeRail allows (D-2704). From 0.13.0 it ends with the
    // radar's marks: checkout-fix's with the symbol, coupon-rules' the radar alone beside its status (D-3006).
    const radar = atLeast(await sceneBuild(page), "0.13.0");
    const marks = await scene.locator(".sc-crail .sc-wtrow-radar").evaluateAll((els) =>
      els
        .filter((e) => {
          let o = 1;
          for (let x: Element | null = e; x; x = x.parentElement) o *= Number(getComputedStyle(x).opacity);
          return e.checkVisibility() && o > 0.5;
        })
        .map((e) => (e.textContent ?? "").trim()),
    );
    expect(marks).toEqual(radar ? ["applyCoupon", ""] : []);
    expect(await scene.locator(".sc-tone-warning").count()).toBe(radar ? 1 : 0);
    const rail = await scene.locator(".sc-crail").evaluate((e) => e.getBoundingClientRect().width / Number.parseFloat(getComputedStyle(e).getPropertyValue("--sc-px")));
    expect(Math.round(rail)).toBeGreaterThanOrEqual(radar ? 246 : 180);
    expect(Math.round(rail)).toBeLessThanOrEqual(360);
    expect(await seen(page, "workspace", ".sc-wtrow-ask")).toEqual(["Needs you"]);
    // The agent's card in the review queue, its check passed; nothing left to review gone, and the toasts too.
    expect(await seen(page, "workspace", ".sc-rq-title")).toEqual(["Ada"]);
    expect(await seen(page, "workspace", ".sc-rq-check > .sc-swap > *")).toEqual(["Check passed"]);
    expect(await seen(page, "workspace", ".sc-rq-empty, .sc-note-toast")).toEqual([]);
    expect(await seen(page, "workspace", ".sc-agents")).toEqual(["1 waiting1 working"]);
    // The pane's last line, the agent's closing words, inside its terminal.
    const inside = await scene.locator(".sc-pane .sc-term .sc-line").last().evaluate((e) => e.getBoundingClientRect().bottom <= (e.closest(".sc-term") as Element).getBoundingClientRect().bottom + 0.5);
    expect(inside).toBe(true);
    await context.close();
  });
}

test("F-WB-01 AC9 D-2702 on a phone the hero's worktrees are a strip over the pane, the one that needs you in view, and the sidebar floats over the pane", async ({ browser }) => {
  const context = await browser.newContext({ reducedMotion: "reduce", viewport: { width: 390, height: 844 } });
  await context.route(/\/demo\/app\//, (route) => route.fulfill({ status: 204, body: "" }));
  const page = await context.newPage();
  await page.goto("/");
  const scene = page.locator('[data-scene="workspace"]');
  const box = async (selector: string) => {
    const b = await scene.locator(selector).first().boundingBox();
    expect(b, selector).not.toBeNull();
    return b ?? { x: 0, y: 0, width: 0, height: 0 };
  };
  const [strip, pane, side] = [await box(".sc-crail"), await box(".sc-pane"), await box(".sc-side")];
  expect(strip.y + strip.height).toBeLessThanOrEqual(pane.y);
  expect(side.y).toBeGreaterThanOrEqual(strip.y + strip.height - 1);
  expect(side.x).toBeGreaterThan(pane.x);
  expect(side.x).toBeLessThan(pane.x + pane.width);
  expect(await seen(page, "workspace", ".sc-wtrow-name")).toEqual(["checkout-fix", "coupon-rules", "search-ui"]);
  expect(await seen(page, "workspace", ".sc-wtrow-ask")).toEqual(["Needs you"]);
  const drawn = (selector: string) => scene.locator(selector).evaluateAll((els) => els.filter((e) => e.checkVisibility()).length);
  expect(await drawn(".sc-crail-head, .sc-wsrow, .sc-crail-none, .sc-crail-foot")).toBe(0);
  // The radar's note folds: the pane has no room for it.
  expect(await drawn(".sc-term .sc-hide-narrow")).toBe(0);
  await context.close();
});

test("F-WB-01 AC9 D-2702 the hero's rules load only on the home page, after the shared scene rules", async ({ page }) => {
  /** Every rule the page's stylesheets hold that draws the hero's window, or the rail. */
  const rules = (re: RegExp) =>
    page.evaluate(
      (source) =>
        [...document.styleSheets].flatMap((s) => {
          const walk = (rules: CSSRuleList): string[] =>
            [...rules].flatMap((r) => ("cssRules" in r && !(r instanceof CSSStyleRule) ? walk((r as CSSGroupingRule).cssRules) : r instanceof CSSStyleRule && new RegExp(source).test(r.selectorText) ? [r.selectorText] : []));
          return walk(s.cssRules);
        }),
      re.source,
    );
  const heroRules = () => rules(/\.sc-(code|tabs?|side|pills?|rq)(?![\w-])/);
  const railRules = () => rules(/\.sc-(?:crail|wsrow|wtrow|bchip|slot-working)/);
  // The rail's come too where the CI scene draws CodeRail's rail, from 0.13.0 (D-3006).
  await page.goto("/changelog/0.9.0/");
  const folds = atLeast(await sceneBuild(page), "0.13.0");
  for (const path of ["/changelog/0.9.0/", "/changelog/0.12.0/"]) {
    await page.goto(path);
    expect(await heroRules(), path).toEqual([]);
    expect((await railRules()).length > 0, path).toBe(folds && path === "/changelog/0.9.0/");
  }
  await page.goto("/");
  expect((await heroRules()).length).toBeGreaterThan(10);
  expect((await railRules()).length).toBeGreaterThan(10);
  expect(await page.evaluate(() => getComputedStyle(document.querySelector('[data-scene="workspace"] .sc-code') as Element).display)).toBe("grid");
});

// The rail's rows as the app folds them, from 0.13.0 (D-2701, D-3006): at every change, a row holds its parts with its
// name at WorktreeRow's floor (min(10ch, 100%)), and a mark is its icon alone only beside the row's status (D-2613) or
// where its words wouldn't fit even so, the radar's first. The hero's rail and the CI scene's, and a phone's strips.
for (const width of [1440, 390] as const) {
  test(`F-WB-01 AC9 D-2701 D-3006 at ${width} px every rail row fits with its name at the app's floor, and folds a mark only when it must`, async ({ page }) => {
    await page.setViewportSize({ width, height: 900 });
    const checked: string[] = [];
    for (const [path, id] of [
      ["/", "workspace"],
      ["/changelog/0.9.0/", "checks"],
    ] as const) {
      await page.goto(path);
      await expect.poll(() => page.evaluate(() => document.documentElement.hasAttribute("data-fx"))).toBe(true);
      if (!atLeast(await sceneBuild(page), "0.13.0")) continue;
      await page.locator(`[data-scene="${id}"]`).scrollIntoViewIfNeeded();
      const times = await page.evaluate(
        (id) => [...new Set([0, ...[...document.querySelectorAll(`[data-scene="${id}"] .sc-wtrow-step`)].map((e) => Number.parseFloat(getComputedStyle(e).getPropertyValue("--at")) || 0)])],
        id,
      );
      for (const t of times) {
        await seek(page, id, t + 0.05);
        const rows = await page.evaluate((id) => {
          // Shown within the scene's stage, on its clock: the page's reveal runs on the scroll, and the hero's entrance
          // (its figure's `enter`, 0 for the first 0.3 s after load) on the page's.
          const stage = document.querySelector(`[data-scene="${id}"] [data-stage]`) as Element;
          const opaque = (e: Element) => {
            let o = 1;
            for (let x: Element | null = e; x && x !== stage; x = x.parentElement) o *= Number(getComputedStyle(x).opacity);
            return o > 0.5;
          };
          const symbol = document.querySelector(`[data-scene="${id}"] .sc-wtrow-symbol`)?.textContent ?? "";
          return [...document.querySelectorAll(`[data-scene="${id}"] .sc-wtrow`)]
            .filter((row) => row.checkVisibility())
            .map((row) => {
              const parts = [...row.querySelectorAll(".sc-wtrow-parts")].find(opaque) as HTMLElement;
              const name = row.querySelector(".sc-wtrow-name") as HTMLElement;
              const s = getComputedStyle(row);
              const gap = Number.parseFloat(s.columnGap);
              const room = row.clientWidth - Number.parseFloat(s.paddingLeft) - Number.parseFloat(s.paddingRight);
              const probe = document.createElement("span");
              probe.style.cssText = "position: absolute; visibility: hidden; width: 10ch";
              name.appendChild(probe);
              const floor = Math.min(probe.getBoundingClientRect().width, room);
              probe.remove();
              const icon = (row.querySelector(".sc-wtrow-icon") as Element).getBoundingClientRect().width;
              const need = () => icon + gap + floor + gap + parts.getBoundingClientRect().width;
              const status = parts.querySelector(".sc-slot-working, .sc-wtrow-ask") !== null;
              // A folded radar's words, put back, measured, and taken out again.
              const unfolded = [...parts.querySelectorAll(".sc-wtrow-radar.sc-wtrow-compact")].filter(() => !status).map((mark) => {
                const words = document.createElement("span");
                words.className = "sc-wtrow-symbol";
                words.textContent = symbol;
                (mark.firstElementChild as Element).appendChild(words);
                const n = need();
                words.remove();
                return n;
              });
              return {
                name: name.textContent ?? "",
                need: need(),
                room,
                unfolded,
                cut: name.scrollWidth > name.clientWidth + 0.5,
                lone: parts.querySelectorAll(".sc-wtrow-checks.sc-wtrow-compact").length > 0 && !status,
              };
            });
        }, id);
        for (const r of rows) {
          const at = `${path} ${t}s ${r.name}`;
          expect(r.need, `${at}: past the rail`).toBeLessThanOrEqual(r.room + 0.5);
          expect(r.cut, `${at}: its name cut`).toBe(false);
          for (const n of r.unfolded) expect(n, `${at}: its radar is folded, though its words fit`).toBeGreaterThan(r.room + 0.5);
          // The checks badge never needs folding here: it's its icon alone only beside a status.
          expect(r.lone, `${at}: the checks badge folded`).toBe(false);
          checked.push(`${at}${r.unfolded.length > 0 ? " (radar folded)" : ""}`);
        }
      }
    }
    // From 0.13.0: the hero's three rows and the CI scene's, at each of their five changes (its pull request's in the rail
    // folds its radar after its agent is done, and in a phone's strip it has the room).
    if (checked.length > 0) {
      expect(checked.length).toBe(width === 1440 ? 30 : 20);
      expect(checked.some((c) => c.endsWith("(radar folded)"))).toBe(width === 1440);
    }
  });
}

// The CI scene's pane, as a terminal: what it has printed shows whole, and when there's more than it holds its oldest
// rows go off its top, whole (D-3006: from 0.13.0 the note after the log takes two lines' room).
for (const width of [1440, 390] as const) {
  test(`F-WB-01 AC9 D-3006 at ${width} px the CI scene's pane shows what its agent has printed whole, and scrolls whole rows off its top`, async ({ page }) => {
    await page.setViewportSize({ width, height: 900 });
    await page.goto("/changelog/0.9.0/");
    await expect.poll(() => page.evaluate(() => document.documentElement.hasAttribute("data-fx"))).toBe(true);
    const times = await page.evaluate(() =>
      [...new Set([...document.querySelectorAll('[data-scene="checks"] .sc-ci-main > .sc-pane .sc-term > *')].map((e) => Number.parseFloat(getComputedStyle(e).getPropertyValue("--at"))))]
        .filter(Number.isFinite)
        .sort((a, b) => a - b),
    );
    expect(times.length).toBeGreaterThan(10);
    for (const t of [...times.map((x) => x + 0.05), 20]) {
      await seek(page, "checks", t);
      const lines = await page.evaluate(() => {
        const term = document.querySelector('[data-scene="checks"] .sc-ci-main > .sc-pane .sc-term') as Element;
        const box = term.getBoundingClientRect();
        return [...term.children]
          .filter((l) => l.checkVisibility() && Number(getComputedStyle(l).opacity) > 0.05 && l.getBoundingClientRect().height > 0.5)
          .map((l) => {
            const r = l.getBoundingClientRect();
            return { text: (l.textContent ?? "").slice(0, 40), top: r.top - box.top, bottom: box.bottom - r.bottom };
          });
      });
      expect(lines.length).toBeGreaterThan(0);
      for (const l of lines) {
        expect(l.top, `${t}s "${l.text}": cut at the top`).toBeGreaterThanOrEqual(-0.5);
        expect(l.bottom, `${t}s "${l.text}": cut at the bottom`).toBeGreaterThanOrEqual(-0.5);
      }
    }
  });
}

// The hero's review card: Run check joins its buttons when its check passes without moving Approve, so Approve's
// place holds from when the card arrives; from 0.13.0 Approve waits once, for SETTLE_MS from then (F-TL-12.12, D-3006),
// or in a phone's frame until SETTLE_MS after "Warned both agents" stops covering it (the test below).
for (const width of [1440, 390] as const) {
  test(`F-WB-01 AC9 D-3006 at ${width} px the hero's Approve holds its place when the check passes, and from 0.13.0 waits once, from when its card arrives`, async ({ page }) => {
    await page.setViewportSize({ width, height: 900 });
    await page.goto("/");
    await expect.poll(() => page.evaluate(() => document.documentElement.hasAttribute("data-fx"))).toBe(true);
    const waits = atLeast(await sceneBuild(page), "0.13.0");
    const at = async (t: number) => {
      await seek(page, "workspace", t);
      return page.evaluate(() => {
        const sets = [...document.querySelectorAll('[data-scene="workspace"] .sc-rq-actions-swap > *')];
        const shown = sets.find((e) => Number(getComputedStyle(e).opacity) > 0.5) as Element;
        const approve = shown.querySelector(".sc-btn-primary") as Element;
        const r = approve.getBoundingClientRect();
        // In the card: the scene brings the card in with a rise, which the app doesn't, so its place on the page isn't
        // what the app's steady box sees; its place in the card is.
        const card = (shown.closest(".sc-rq-card") as Element).getBoundingClientRect();
        const row = (shown.querySelector(".sc-rq-actions") as Element).getBoundingClientRect();
        const label = approve.firstElementChild ?? approve;
        return { place: [r.left - card.left, r.top - card.top, r.width, r.height, row.width, row.height].map((x) => Math.round(x * 10) / 10), label: Number(getComputedStyle(label).opacity) };
      });
    };
    const [arrived, settled, before, after] = [await at(6.6), await at(7.0), await at(8.35), await at(8.45)];
    // Within half a pixel: the scene's entering words are composited apart, which can move a box by a tenth.
    const moved = (a: number[], b: number[]) => Math.max(...a.map((x, i) => Math.abs(x - (b[i] ?? NaN))));
    expect(moved(after.place, before.place)).toBeLessThan(0.5);
    expect(moved(settled.place, before.place)).toBeLessThan(0.5);
    expect([arrived.label, settled.label, before.label, after.label]).toEqual(!waits ? [1, 1, 1, 1] : width === 390 ? [0.4, 0.4, 0.4, 0.4] : [0.4, 1, 1, 1]);
  });
}

// What the app's steady box sees (steadyBox.ts's covered()): Approve's middle and corners, each pulled up to 4 px in,
// and what's drawn on top at each. A cover holds Approve off like a move, so it shows ready only once nothing has moved
// or covered it for SETTLE_MS (F-TL-12.12). In a phone's frame "Warned both agents" lies over it until the toast goes
// (the code review of 825b4b54); at 1440 and 1280 nothing does.
const SETTLE = 0.5; // steadyBox.ts's SETTLE_MS: scenes.test ties the scenes' SETTLE to it.
for (const width of [1440, 1280, 390, 320] as const) {
  test(`F-WB-01 AC9 D-3006 at ${width} px the hero's Approve is never shown ready while anything covers it, and turns on SETTLE_MS after it last did`, async ({ page }) => {
    await page.setViewportSize({ width, height: 900 });
    await page.goto("/");
    await expect.poll(() => page.evaluate(() => document.documentElement.hasAttribute("data-fx"))).toBe(true);
    // Approve waits from 0.13.0 (APPROVE_WAITS).
    if (!atLeast(await sceneBuild(page), "0.13.0")) return;
    await seek(page, "workspace", 7);
    await page.locator('[data-scene="workspace"] .sc-rq-card .sc-btn-primary').first().scrollIntoViewIfNeeded();
    const samples: Array<{ t: number; arrived: number; inView: boolean; covered: boolean; ready: boolean }> = [];
    for (let i = 120; i <= 230; i++) {
      const t = i / 20;
      await seek(page, "workspace", t);
      const sample = await page.evaluate(() => {
        const scene = document.querySelector('[data-scene="workspace"]') as Element;
        const stage = scene.querySelector("[data-stage]") as Element;
        const shown = [...scene.querySelectorAll(".sc-rq-actions-swap > *")].find((e) => Number(getComputedStyle(e).opacity) > 0.5) as Element;
        const approve = shown.querySelector(".sc-btn-primary") as Element;
        // The card comes in with a rise, a scene's effect the app doesn't have (its card is there at once).
        const entry = approve.closest("[data-fx-in]") as Element;
        const arrived = Number.parseFloat(getComputedStyle(entry).getPropertyValue("--at"));
        // Drawn: its opacity on the scene's clock, within the stage, or within the card's entry for what's in the card
        // (a step not in yet, or gone, is at 0).
        const drawn = (e: Element) => {
          const root = entry.contains(e) ? entry : stage;
          let o = 1;
          for (let x: Element | null = e; x && x !== root; x = x.parentElement) o *= Number(getComputedStyle(x).opacity);
          return o > 0.01;
        };
        const r = approve.getBoundingClientRect();
        const dx = Math.min(4, r.width / 4);
        const dy = Math.min(4, r.height / 4);
        const points = [
          [r.left + r.width / 2, r.top + r.height / 2],
          [r.left + dx, r.top + dy],
          [r.right - dx, r.top + dy],
          [r.left + dx, r.bottom - dy],
          [r.right - dx, r.bottom - dy],
        ] as const;
        const inView = points.every(([x, y]) => x >= 0 && y >= 0 && x < innerWidth && y < innerHeight);
        // What's on top at a point: the first element there that's drawn.
        const covered = points.some(([x, y]) => {
          const top = document.elementsFromPoint(x, y).find(drawn);
          return top !== undefined && !approve.contains(top);
        });
        const label = approve.firstElementChild ?? approve;
        return { arrived, inView, covered, ready: Number(getComputedStyle(label).opacity) > 0.99 };
      });
      samples.push({ t, ...sample });
    }
    expect(samples.every((s) => s.inView)).toBe(true);
    const arrived = samples[0]?.arrived ?? Number.NaN;
    const on = samples.filter((s) => s.t >= arrived);
    // Never ready under a cover.
    expect(on.filter((s) => s.covered && s.ready).map((s) => s.t)).toEqual([]);
    // Ready from SETTLE_MS after its card arrived or a cover last left, and not before: one sample's step either side.
    const from = Math.max(arrived, ...on.filter((s) => s.covered).map((s) => s.t + 0.05));
    expect(on.filter((s) => Math.abs(s.t - (from + SETTLE)) > 0.06 && s.ready !== s.t > from + SETTLE).map((s) => s.t)).toEqual([]);
    // A phone's frame is the one with a cover.
    expect(on.some((s) => s.covered)).toBe(width <= 390);
  });
}
