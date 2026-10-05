import { $, browser } from "@wdio/globals";
import { execFileSync } from "node:child_process";
import { addFolder, freshOffline, paneIds, sendToPane, split } from "../support/app";

function sleepers(arg: string): string[] {
  try {
    return execFileSync("pgrep", ["-f", `^sleep ${arg}$`], { encoding: "utf8" }).trim().split("\n").filter(Boolean);
  } catch {
    return [];
  }
}

// E2E-M3-6 Tree kill (F-CD-03 AC4).
describe("E2E-M3-6 tree kill", () => {
  it("F-CD-03.4 closing a pane leaves none of its processes behind within 3 s", async () => {
    expect(sleepers("1000")).toEqual([]);
    await freshOffline();
    await addFolder(`/tmp/tandem-e2e-m3-6-${Date.now()}`);
    await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 20_000 });
    const pane = await split("right");
    await sendToPane(pane, "bash -c 'sleep 1000 & sleep 1000'\n");
    await browser.waitUntil(() => sleepers("1000").length === 2, { timeout: 10_000, timeoutMsg: "the sleeps never started" });
    await $(`[data-testid="pane"][data-pane-id="${pane}"] button[aria-label^="Close "]`).click();
    const closed = Date.now();
    await browser.waitUntil(() => sleepers("1000").length === 0, { timeout: 3_000, interval: 100, timeoutMsg: `left: ${sleepers("1000")}` });
    expect(Date.now() - closed).toBeLessThan(3_000);
  });
});
