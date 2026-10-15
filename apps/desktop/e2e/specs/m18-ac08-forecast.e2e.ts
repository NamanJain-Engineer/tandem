import { $, $$, browser } from "@wdio/globals";
import { join } from "node:path";
import { rmSync, writeFileSync } from "node:fs";
import {
  addFolder,
  checkAgain,
  fakeEngine,
  freshOffline,
  home,
  invoke,
  mod,
  paneIds,
  split,
  waitForPaneText,
} from "../support/app";

type OsCall = { kind: string; title: string; body: string; source: { type: string; accountId?: string } };

const chip = '[data-testid="usage-chip"]';
const prop = async (selector: string, name: string) => String(await $(selector).getProperty(name));
/** The fake `codex app-server`'s windows ("40"), as in E2E-M6-8. */
const usage = (pcts: string) => writeFileSync(join(home(), "usage.txt"), pcts);

async function refresh(): Promise<void> {
  await invoke("usage_refresh", { accountId: null });
}

async function openPopover(): Promise<void> {
  await $(chip).click();
  await $('[data-testid="usage-popover"]').waitForExist({ timeout: 5_000 });
}

async function closePopover(): Promise<void> {
  await browser.keys("Escape");
  await $('[data-testid="usage-popover"]').waitForExist({ reverse: true, timeout: 5_000 });
}

// E2E-M18-AC08 Usage forecast (F-AC-08): the fake Codex reports 40%, then
// 60% "30 minutes later" (the harness moves the first reading back), so the
// 5-hour window, resetting in 2 hours, runs out before its reset.
describe("E2E-M18-AC08 usage forecast", function () {
  this.timeout(180_000);
  before(async () => {
    fakeEngine("codex", join(home(), "fakebin"));
    usage("40");
    await checkAgain();
  });
  after(async () => {
    rmSync(join(home(), "fakebin", "codex"), { force: true });
    rmSync(join(home(), "usage.txt"), { force: true });
    await checkAgain();
  });

  it("F-AC-08.2 F-AC-08.1 F-AC-08.5 one reading isn't enough; a steep second one runs out early and warns once", async () => {
    await freshOffline();
    await addFolder(`/tmp/tandem-e2e-m18-ac08-${Date.now()}`);
    await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 20_000 });
    const codex = await split("right", "codex");
    await waitForPaneText(codex, "Fake Codex");
    await $(`[data-testid="pane"][data-pane-id="${codex}"] [data-testid="pane-name"]`).click();
    await browser.waitUntil(async () => /^Codex · 40%/.test(await prop(chip, "textContent")), {
      timeout: 20_000,
    });

    // One reading: not enough data, and the popover lists who's using it.
    await openPopover();
    const forecast = '[data-testid="usage-popover"] [data-testid="usage-forecast"]';
    await $(forecast).waitForExist({ timeout: 5_000 });
    expect(await $(forecast).getAttribute("data-state")).toBe("not_enough_data");
    expect(await prop(forecast, "textContent")).toBe("Not enough data yet");
    await browser.waitUntil(
      async () => (await $$('[data-testid="usage-who"][data-state="ready"]').length) > 0,
      { timeout: 10_000 },
    );
    await closePopover();

    // The warning is off by default; turn it on.
    const all = await invoke<Record<string, unknown>>("settings_get_all");
    expect(all.usage_forecast_notify).toBe(false);
    await invoke("settings_set", { key: "usage_forecast_notify", value: true });
    await invoke("test_notifier_calls", { clear: true });

    await invoke("test_usage_backdate", { ms: 30 * 60_000 });
    usage("60");
    await refresh();
    await browser.waitUntil(async () => /^Codex · 60%/.test(await prop(chip, "textContent")), {
      timeout: 10_000,
    });
    expect(await $(chip).getAttribute("title")).toMatch(/Runs out at .+ before it resets/);
    await openPopover();
    expect(await $(forecast).getAttribute("data-state")).toBe("runs_out");
    expect(await prop(forecast, "textContent")).toMatch(
      /^Runs out at .+, 1 h( \d+ min)? before it resets · based on the last 30 min$/,
    );
    await closePopover();

    const usageCalls = async () =>
      (await invoke<OsCall[]>("test_notifier_calls", { clear: false })).filter(
        (c) => c.kind === "usage_forecast",
      );
    await browser.waitUntil(async () => (await usageCalls()).length === 1, { timeout: 10_000 });
    const call = (await usageCalls())[0];
    if (!call) throw new Error("no usage notification");
    expect(call.title).toMatch(/^Codex \(.+\) runs out at /);
    expect(call.body).toMatch(/^At this pace the 5-hour window runs out .+ before it resets\.$/);
    expect(call.source.type).toBe("usage");

    // The next reading of the same window doesn't warn again.
    await invoke("test_usage_backdate", { ms: 5 * 60_000 });
    usage("65");
    await refresh();
    await browser.pause(1_000);
    expect((await usageCalls()).length).toBe(1);
  });

  it("F-AC-08.6 the launch sheet says when a seat's account is near its limit", async () => {
    usage("85");
    await refresh();
    await browser.keys([mod(), "Shift", "l"]);
    await $('[data-testid="launch-sheet"]').waitForExist({ timeout: 10_000 });
    // Codex for the first seat (the sheet starts on Claude Code).
    await $('[data-testid="launch-seat"] [aria-label="Engine for seat 1"]').click();
    await $('//*[@role="option"][.//*[normalize-space()="Codex"]]').click();
    const hint = '[data-testid="launch-usage-hint"]';
    await $(hint).waitForExist({ timeout: 5_000 });
    expect(await prop(hint, "textContent")).toMatch(/ is at 85%$/);
    await browser.keys("Escape");
  });
});
