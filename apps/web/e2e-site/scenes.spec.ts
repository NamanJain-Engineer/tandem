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
  const steps = await page.evaluate(() =>
    // What's drawn at this width (a part only a phone draws has no boxes here), each step of it.
    [...document.querySelectorAll("[data-scene] .sc-step > *, [data-scene] .sc-checks > * > *")].filter((el) => el.getClientRects().length > 0).map((el) => {
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
  // Land's five steps in each of its three lanes, and the rail badge's three checks.
  expect(steps).toHaveLength(15 + 3);
  for (const step of steps) {
    // A flex row: inline-flex, or flex where the step is a grid's item (a swap's), which CSS makes block-level.
    expect(step.display, step.text).toMatch(/^(inline-)?flex$/);
    if (step.gap === null || step.centre === null) continue;
    expect(Math.abs(step.gap), `${step.text}: gap`).toBeLessThan(0.6);
    expect(Math.abs(step.centre), `${step.text}: centre`).toBeLessThan(1.1);
  }
  // Only the failed checks' words sit in a span of their own; every other step has its mark beside its words.
  expect(new Set(steps.filter((s) => s.gap === null).map((s) => s.text))).toEqual(new Set(["1 check failed: test"]));
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
  expect(await shown(".sc-rail .sc-checks > *")).toEqual(["Checks passed"]);
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
  // One inline sheet in the head, after the shared ones, and the scene draws with it.
  const order = await page.evaluate(() => [...document.head.querySelectorAll('link[rel="stylesheet"], style')].map((e) => (e.tagName === "STYLE" ? e.getAttribute("data-href") : "link")));
  expect(order.at(-1)).toBe("scenes/ci.css");
  expect(order.filter((x) => x === "scenes/ci.css")).toHaveLength(1);
  expect(await page.evaluate(() => getComputedStyle(document.querySelector('[data-scene="checks"] .sc-ci-layer') as Element).position)).toBe("absolute");
});

test("F-WB-01 AC9 the plan scene ends where the app's dialog does: 2 approved · 1 with changes requested, and Land 2 approved… (D-1888)", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto("/changelog/0.12.0/");
  const scene = page.locator('[data-scene="plan"]');
  await scene.scrollIntoViewIfNeeded();
  await expect.poll(() => states(page, "plan"), { timeout: 20_000 }).toEqual(["finished"]);
  const shown = (selector: string) => shownIn(scene, selector);
  expect(await shown(".sc-plan-summary .sc-swap > *")).toEqual(["2 approved · 1 with changes requested"]);
  expect(await shown(".sc-plan-foot > .sc-swap:last-child > *")).toEqual(["Land 2 approved…"]);
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
    // The 0.12.0 hero leaves the intent radar out, at a rail width CodeRail allows (D-2704).
    expect(await scene.locator(".sc-wtrow-radar, .sc-tone-warning").count()).toBe(0);
    const rail = await scene.locator(".sc-crail").evaluate((e) => e.getBoundingClientRect().width / Number.parseFloat(getComputedStyle(e).getPropertyValue("--sc-px")));
    expect(Math.round(rail)).toBeGreaterThanOrEqual(180);
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
  const heroRules = () =>
    page.evaluate(() =>
      [...document.styleSheets].flatMap((s) => {
        const walk = (rules: CSSRuleList): string[] =>
          [...rules].flatMap((r) => ("cssRules" in r && !(r instanceof CSSStyleRule) ? walk((r as CSSGroupingRule).cssRules) : r instanceof CSSStyleRule && /\.sc-(code|crail|wsrow|wtrow|tabs?|side|pills?|rq)(?![\w-])/.test(r.selectorText) ? [r.selectorText] : []));
        return walk(s.cssRules);
      }),
    );
  for (const path of ["/changelog/0.9.0/", "/changelog/0.12.0/"]) {
    await page.goto(path);
    expect(await heroRules(), path).toEqual([]);
  }
  await page.goto("/");
  expect((await heroRules()).length).toBeGreaterThan(10);
  expect(await page.evaluate(() => getComputedStyle(document.querySelector('[data-scene="workspace"] .sc-code') as Element).display)).toBe("grid");
});
