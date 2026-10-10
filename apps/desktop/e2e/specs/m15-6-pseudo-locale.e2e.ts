import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { browser, expect } from "@wdio/globals";
import { Key } from "webdriverio";
import { addFolder, freshOffline, home, mod } from "../support/app";

// §5.5 (MILESTONES M15 task 11): with the en-XA pseudo-locale (every string
// accented and ~35% longer), no primary button is cut off, on any screen.

type Clipped = { text: string; scroll: number; client: number };

/** Visible primary buttons whose label doesn't fit. */
async function clippedPrimaries(): Promise<{ seen: string[]; clipped: Clipped[] }> {
  // Sheets slide in: measure once nothing is moving.
  await browser.waitUntil(
    async () =>
      browser.execute(() =>
        document.getAnimations().every((a) => a.playState !== "running" || a.effect?.getTiming().iterations === Infinity),
      ),
    { timeout: 5_000, timeoutMsg: "animations still running" },
  );
  await browser.pause(150);
  return browser.execute(() => {
    const buttons = Array.from(
      document.querySelectorAll<HTMLElement>('button[data-variant="primary"]'),
    ).filter((b) => b.offsetParent !== null && b.getBoundingClientRect().width > 0);
    const clipped = buttons
      .filter((b) => b.scrollWidth > b.clientWidth + 1 || b.getBoundingClientRect().right > window.innerWidth)
      .map((b) => ({ text: b.textContent ?? "", scroll: b.scrollWidth, client: b.clientWidth }));
    return { seen: buttons.map((b) => b.textContent ?? ""), clipped };
  });
}

/** Escape goes to whatever sheet or dialog is open. */
async function escape(): Promise<void> {
  await browser.keys([Key.Escape]);
  await browser.pause(400);
}

async function press(keys: string[]): Promise<void> {
  await browser.execute(() => (document.activeElement as HTMLElement | null)?.blur());
  await browser.keys(keys);
  await browser.pause(400);
}

describe("§5.5 en-XA pseudo-locale", () => {
  const labels: string[] = [];

  before(async function () {
    this.timeout(90_000);
    await freshOffline();
    const repo = join(home(), "pseudo-locale");
    mkdirSync(repo, { recursive: true });
    writeFileSync(join(repo, "README.md"), "# Pseudo\n");
    await addFolder(repo);
    await browser.execute(() =>
      (
        window as unknown as { __tandemTest: { pseudoLocale: (on: boolean) => void } }
      ).__tandemTest.pseudoLocale(true),
    );
    await browser.waitUntil(
      async () => (await browser.execute(() => document.body.innerText)).includes("~~"),
      {
        timeout: 10_000,
        timeoutMsg: "the pseudo-locale never showed",
      },
    );
  });

  after(async () => {
    await browser.execute(() =>
      (
        window as unknown as { __tandemTest: { pseudoLocale: (on: boolean) => void } }
      ).__tandemTest.pseudoLocale(false),
    );
  });

  const screens: Array<[string, () => Promise<void>, () => Promise<void>]> = [
    ["Code mode", () => press([mod(), "2"]), async () => undefined],
    ["Thread mode", () => press([mod(), "3"]), async () => undefined],
    ["Agent mode", () => press([mod(), "1"]), () => press([mod(), "2"])],
    ["the launch sheet", () => press([mod(), Key.Shift, "l"]), escape],
    [
      "the plans dialog",
      async () => {
        await press([mod(), "k"]);
        await browser.keys("plans");
        await press([Key.Enter]);
      },
      escape,
    ],
    ["Settings", () => press([mod(), ","]), escape],
  ];

  for (const [name, open, close] of screens) {
    it(`§5.5 no primary button is cut off: ${name}`, async () => {
      await open();
      const { seen, clipped } = await clippedPrimaries();
      labels.push(...seen);
      expect(clipped).toEqual([]);
      await close();
    });
  }

  it("§5.5 the walk saw primary buttons in the pseudo-locale", async () => {
    // Launch (the sheet) and the plans dialog; empty modes and Settings have none.
    expect(labels.length).toBeGreaterThanOrEqual(2);
    expect(labels.filter((l) => !l.includes("~"))).toEqual([]);
  });
});
