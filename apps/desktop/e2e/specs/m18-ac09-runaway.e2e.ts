import { $, browser } from "@wdio/globals";
import { addFolder, freshOffline, invoke, paneIds, sendToPane, split, waitForPaneText } from "../support/app";

type Item = { kind: string; title: string; source: { type: string; paneId?: string } };
type Page = { items: Item[] };

const banner = (pane: string) => `[data-testid="pane"][data-pane-id="${pane}"] [data-testid="guard-banner"]`;
/** The notification center's runaway items (the OS one depends on focus). */
const runawayItems = async () =>
  (await invoke<Page>("notifications_list", {})).items.filter((n) => n.kind === "runaway");

async function bannerState(pane: string): Promise<string | null> {
  return browser.execute(
    (sel: string) => document.querySelector<HTMLElement>(sel)?.dataset.state ?? null,
    banner(pane),
  );
}

async function claudePane(): Promise<string> {
  await freshOffline();
  await addFolder(`/tmp/tandem-e2e-m18-ac09-${Date.now()}`);
  await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 20_000 });
  const pane = await split("right", "claude");
  await waitForPaneText(pane, "Fake Claude Code");
  return pane;
}

// E2E-M18-AC09 Runaway guard (F-AC-09): the fake Claude Code reports
// subagent starts (PreToolUse of `Task`) and keeps working; the guard trips
// the subagent limit, shows the card, notifies, and pauses on request.
describe("E2E-M18-AC09 runaway guard", function () {
  this.timeout(240_000);

  it("F-AC-09.1 F-AC-09.5 a tripped limit shows the card and notifies; Dismiss clears it", async () => {
    const pane = await claudePane();
    await invoke("settings_set", {
      key: "runaway_guard",
      value: { workingMinutes: 90, windowPoints: 30, subagents: 2, circles: true },
    });
    await invoke("settings_set", { key: "runaway_action", value: "notify" });
    await sendToPane(pane, "/subagents 3\r");
    await $(banner(pane)).waitForExist({ timeout: 20_000 });
    expect(await bannerState(pane)).toBe("tripped");
    expect(String(await $(banner(pane)).getProperty("textContent"))).toMatch(
      /started 2 subagents since your last prompt/,
    );
    await browser.waitUntil(async () => (await runawayItems()).length === 1, { timeout: 10_000 });
    const item = (await runawayItems())[0];
    if (!item) throw new Error("no runaway notification");
    expect(item.title).toMatch(/started 2 subagents since your last prompt$/);
    expect(item.source).toMatchObject({ type: "pane", paneId: pane });
    // Notify only: nothing was typed or stopped.
    expect(await bannerState(pane)).toBe("tripped");

    await $(`${banner(pane)} [data-testid="guard-dismiss"]`).click();
    await $(banner(pane)).waitForExist({ reverse: true, timeout: 5_000 });
    // The third subagent of the same turn doesn't bring it back.
    await browser.pause(1_500);
    expect(await $(banner(pane)).isExisting()).toBe(false);
  });

  it("F-AC-09.2 with Pause the agent, a still-working agent is stopped and Resume continues it", async () => {
    const pane = await claudePane();
    await invoke("settings_set", {
      key: "runaway_guard",
      value: { workingMinutes: 90, windowPoints: 30, subagents: 2, circles: true },
    });
    await invoke("settings_set", { key: "runaway_action", value: "pause" });
    await sendToPane(pane, "/subagents 2\r");
    await $(banner(pane)).waitForExist({ timeout: 20_000 });
    // The interrupt key goes first; the fake engine doesn't read it, so 5 s
    // later it's still working and its process group is stopped.
    await browser.waitUntil(async () => (await bannerState(pane)) === "paused", { timeout: 20_000 });
    await $(`${banner(pane)} [data-testid="guard-resume"]`).click();
    await $(banner(pane)).waitForExist({ reverse: true, timeout: 5_000 });
    await invoke("settings_set", { key: "runaway_action", value: "notify" });
  });
});
