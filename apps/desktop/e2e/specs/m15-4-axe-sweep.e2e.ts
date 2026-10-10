import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { $, $$, browser, expect } from "@wdio/globals";
import { Key } from "webdriverio";
import { addFolder, freshOffline, home, mod, split } from "../support/app";
import { expectNoSeriousA11yViolationsInBothThemes } from "../support/axe";

// E2E-M15-4 Axe sweep (PRODUCT_SPEC §5.4): zero serious or critical axe
// violations across every screen, in both themes. Component states (empty,
// loading, error, populated) are swept per component by the @tandem/ui
// gallery test; this spec sweeps the assembled screens: each mode, the
// overlays, the sidebar and every Settings page, empty and populated.

/** Shortcuts go to the app, not a focused terminal. */
async function press(keys: string[]): Promise<void> {
  await browser.execute(() => (document.activeElement as HTMLElement | null)?.blur());
  await browser.keys(keys);
}

async function close(): Promise<void> {
  await browser.keys([Key.Escape]);
  await browser.pause(150);
}

describe("E2E-M15-4 axe sweep", () => {
  before(async function () {
    this.timeout(90_000);
    await freshOffline();
    const repo = join(home(), "axe-sweep");
    mkdirSync(join(repo, "src"), { recursive: true });
    writeFileSync(join(repo, "README.md"), "# Axe sweep\n");
    writeFileSync(join(repo, "src", "main.ts"), "export const ok = true;\n");
    await addFolder(repo);
  });

  it("§5.4 Code mode, empty and with split panes", async () => {
    await press([mod(), "2"]);
    await expectNoSeriousA11yViolationsInBothThemes();
    await split("right");
    await split("down");
    await expectNoSeriousA11yViolationsInBothThemes();
  });

  it("§5.4 Settings › Terminal › Screen reader support gives terminals xterm's accessibility tree", async () => {
    const setScreenReader = (on: boolean) =>
      browser.execute((v: boolean) => {
        const w = window as unknown as {
          __TAURI_INTERNALS__: { invoke: (c: string, a: unknown) => Promise<unknown> };
        };
        void w.__TAURI_INTERNALS__.invoke("settings_set", { key: "term_screen_reader", value: v });
      }, on);
    const trees = () =>
      browser.execute(() => document.querySelectorAll(".xterm .xterm-accessibility").length);
    expect(await trees()).toBe(0);
    await setScreenReader(true);
    await browser.waitUntil(async () => (await trees()) >= 3, {
      timeout: 10_000,
      timeoutMsg: "no accessibility tree in the panes",
    });
    await expectNoSeriousA11yViolationsInBothThemes();
    await setScreenReader(false);
    await browser.waitUntil(async () => (await trees()) === 0, {
      timeout: 10_000,
      timeoutMsg: "the tree stayed after turning it off",
    });
  });

  it("§5.4 Thread mode and Agent mode", async () => {
    await press([mod(), "3"]);
    await browser.pause(300);
    await expectNoSeriousA11yViolationsInBothThemes();
    await press([mod(), "1"]);
    await browser.pause(300);
    await expectNoSeriousA11yViolationsInBothThemes();
    await press([mod(), "2"]);
  });

  for (const [name, keys] of [
    ["the command palette", () => [mod(), "k"]],
    ["the shortcuts cheatsheet", () => [mod(), "/"]],
    ["the notification center", () => [mod(), Key.Shift, "n"]],
    ["the launch sheet", () => [mod(), Key.Shift, "l"]],
    ["the full Dashboard", () => [mod(), Key.Alt, Key.Shift, "d"]],
  ] as const) {
    it(`§5.4 ${name}`, async () => {
      await press(keys());
      await browser.pause(400);
      await expectNoSeriousA11yViolationsInBothThemes();
      await close();
    });
  }

  it("§5.4 §5.7 Help › Diagnostics lists the supervised subsystems, live", async () => {
    await $('button[aria-label="Vibezeo menu"]').click();
    const item = $('//*[@role="menuitem"][normalize-space()="Diagnostics…"]');
    await item.waitForClickable({ timeout: 10_000 });
    await item.click();
    const panel = $('[data-testid="diagnostics"]');
    await panel.waitForDisplayed({ timeout: 10_000 });
    const text = await panel.getText();
    // The status bus, the status ticker and the terminals under the watchdog (§5.2).
    expect(text).toContain("status bus/status");
    expect(text).toContain("status");
    expect(text).toMatch(/Started in \d+ ms/);
    await expectNoSeriousA11yViolationsInBothThemes();
    await close();
  });

  it("§5.4 the sidebar's Files tab", async () => {
    await press([mod(), Key.Alt, "f"]);
    await $('[role="tree"]').waitForDisplayed({ timeout: 10_000 });
    await expectNoSeriousA11yViolationsInBothThemes();
    await press([mod(), Key.Shift, "b"]);
  });

  it("§5.4 every Settings page", async function () {
    this.timeout(300_000);
    const open = async () => {
      if (!(await $('//nav[@aria-label="Settings pages"]').isExisting())) await press([mod(), ","]);
    };
    await open();
    // The nav's first button is "Back (Esc)", which closes Settings.
    const nav = '//nav[@aria-label="Settings pages"]//button[not(starts-with(normalize-space(), "Back"))]';
    await $(nav).waitForClickable({ timeout: 10_000 });
    const count = await $$(nav).length;
    if (count < 10) throw new Error(`only ${count} Settings pages`);
    for (let i = 0; i < count; i++) {
      await open();
      const button = (await $$(nav))[i];
      if (!button) throw new Error(`Settings page ${i} is gone`);
      const label = (await button.getText()).split("\n")[0];
      await button.scrollIntoView();
      await button.click();
      await browser.pause(250);
      try {
        await expectNoSeriousA11yViolationsInBothThemes();
      } catch (e) {
        throw new Error(`Settings › ${label}: ${(e as Error).message}`);
      }
    }
    await close();
  });

  it("§5.4 keyboard: Tab walks the shell in visual order, with no positive tabindex", async () => {
    // Thread mode: a terminal keeps Tab for its shell, and panes are left
    // with Mod+Alt+arrows instead (F-CD-06), so the walk runs outside them.
    await press([mod(), "3"]);
    await browser.pause(300);
    const positive = await browser.execute(
      () =>
        Array.from(document.querySelectorAll("[tabindex]")).filter(
          (el) => Number(el.getAttribute("tabindex")) > 0,
        ).length,
    );
    expect(positive).toBe(0);
    await browser.execute(() => (document.activeElement as HTMLElement | null)?.blur());
    const seen: Array<{ region: number; top: number; label: string }> = [];
    for (let i = 0; i < 80; i++) {
      await browser.keys([Key.Tab]);
      const at = await browser.execute(() => {
        const el = document.activeElement as HTMLElement | null;
        const shell = document.querySelector('[data-testid="app-shell"]');
        if (!el || !shell || el === document.body) return null;
        // Which of the shell's rows (title bar, banners, body, status bar)
        // holds the focus, and where it is on screen.
        const rows = Array.from(shell.children);
        const region = rows.findIndex((r) => r.contains(el));
        const r = el.getBoundingClientRect();
        return {
          region,
          top: Math.round(r.top),
          label: el.getAttribute("aria-label") ?? el.textContent?.trim().slice(0, 30) ?? el.tagName,
        };
      });
      if (!at) continue;
      const first = seen[0];
      if (first && at.label === first.label && at.top === first.top && seen.length > 3) break; // wrapped
      seen.push(at);
    }
    expect(seen.length).toBeGreaterThan(5);
    // Rows are reached in the order they're laid out: the title bar first,
    // the body after it, never back up the page before the walk wraps.
    const regions = seen.map((s) => s.region);
    for (let i = 1; i < regions.length; i++) {
      if ((regions[i] as number) < (regions[i - 1] as number))
        throw new Error(
          `focus went back up from row ${regions[i - 1]} to ${regions[i]} at "${seen[i]?.label}": ${JSON.stringify(seen.map((s) => s.label))}`,
        );
    }
    expect(new Set(regions).size).toBeGreaterThan(1);
  });
});
