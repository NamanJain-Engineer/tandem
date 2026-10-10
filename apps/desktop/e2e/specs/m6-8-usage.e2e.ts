import { $, $$, browser } from "@wdio/globals";
import { join } from "node:path";
import { rmSync, writeFileSync } from "node:fs";
import {
  addFolder,
  checkAgain,
  fakeEngine,
  freshOffline,
  home,
  paneIds,
  sendToPane,
  split,
  waitForPaneText,
} from "../support/app";

const prop = async (selector: string, name: string) => String(await $(selector).getProperty(name));
const chip = '[data-testid="usage-chip"]';

/** The fake `codex app-server`'s windows ("40,85"); empty means no data. */
const usage = (pcts: string) => writeFileSync(join(home(), "usage.txt"), pcts);

async function chipShows(re: RegExp, tone: string): Promise<void> {
  const shows = async () =>
    re.test(await prop(chip, "textContent")) && (await $(chip).getAttribute("data-tone")) === tone;
  await browser.waitUntil(shows, { timeout: 20_000 }).catch(async () => {
    const [text, now, title] = [
      await prop(chip, "textContent"),
      await $(chip).getAttribute("data-tone"),
      await $(chip).getAttribute("title"),
    ];
    throw new Error(`chip: ${text} (${now}): ${title}`);
  });
}

async function focus(pane: string): Promise<void> {
  await $(`[data-testid="pane"][data-pane-id="${pane}"] [data-testid="pane-name"]`).click();
}

// E2E-M6-8 Usage chip and meter (F-AC-04, F-AC-06): the fake Codex reports
// windows through `codex app-server`; the fake Claude ends a turn whose
// transcript (inside its provider home) reports the context tokens.
describe("E2E-M6-8 usage chip and meter", () => {
  before(async () => {
    fakeEngine("codex", join(home(), "fakebin"));
    usage("40,85");
    await checkAgain();
  });
  after(async () => {
    rmSync(join(home(), "fakebin", "codex"), { force: true });
    rmSync(join(home(), "usage.txt"), { force: true });
    await checkAgain();
  });

  let codex = "";
  let claude = "";

  it("F-AC-04.1 F-AC-04.2 windows at 40% and 85% show 85% in warning; without data the chip says Usage unavailable", async function () {
    this.timeout(120_000);
    await freshOffline();
    await addFolder(`/tmp/tandem-e2e-m6-8-${Date.now()}`);
    await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 20_000 });
    codex = await split("right", "codex");
    await waitForPaneText(codex, "Fake Codex");
    await focus(codex);
    await chipShows(/^Codex · 85% · resets /, "warning");
    expect(await $(chip).getAttribute("title")).toMatch(/^Weekly, the closest/);

    // The popover lists the windows; Refresh with no data says so.
    await $(chip).click();
    await $('[data-testid="usage-popover"]').waitForExist({ timeout: 5_000 });
    expect(await $$('[data-testid="usage-popover"] [role="meter"]').length).toBe(2);
    usage("");
    await $('[data-testid="usage-refresh"]').click();
    await browser.keys("Escape");
    // Shortcuts wait while a dialog is in the page, including a closing popover.
    await $('[data-testid="usage-popover"]').waitForExist({ reverse: true, timeout: 5_000 });
    await chipShows(/^Usage unavailable$/, "muted");
    expect(await $(chip).getAttribute("title")).toMatch(/^Couldn't read Codex's usage/);

    // Claude Code doesn't report usage: unavailable, and the tooltip says why.
    claude = await split("down", "claude");
    await waitForPaneText(claude, "Fake Claude Code");
    await focus(claude);
    await browser.waitUntil(
      async () => (await $(chip).getAttribute("title")) === "Claude Code doesn't report usage to other apps.",
      {
        timeout: 15_000,
      },
    );
    expect(await prop(chip, "textContent")).toBe("Usage unavailable");
  });

  it("F-AC-06.1 150k of 200k reads 75% and neutral; at 170k the meter is amber", async function () {
    this.timeout(60_000);
    const meter = `[data-testid="pane"][data-pane-id="${claude}"] [role="meter"][aria-label="Context"]`;
    await sendToPane(claude, "/context 150000\r");
    await $(meter).waitForExist({ timeout: 15_000 });
    expect(await $(meter).getAttribute("aria-valuenow")).toBe("75");
    expect(await prop(meter, "textContent")).toBe("150K / 200K");
    expect(await $(meter).getAttribute("data-tone")).toBe("muted");
    expect(await $(meter).getAttribute("title")).toBe(
      "Context used by this session. Near the limit, the engine may compact or stop.",
    );
    await sendToPane(claude, "/context 170000\r");
    await browser.waitUntil(async () => (await $(meter).getAttribute("data-tone")) === "warning", {
      timeout: 15_000,
    });
    expect(await prop(meter, "textContent")).toBe("170K / 200K");
    // The codex pane has no transcript: no meter.
    expect(
      await $(
        `[data-testid="pane"][data-pane-id="${codex}"] [role="meter"][aria-label="Context"]`,
      ).isExisting(),
    ).toBe(false);
  });
});
