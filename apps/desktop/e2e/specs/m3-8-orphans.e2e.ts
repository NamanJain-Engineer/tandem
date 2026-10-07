import { $, browser } from "@wdio/globals";
import { execFileSync } from "node:child_process";
import { addFolder, freshOffline, paneIds, relaunch, sendToPane, waitForShell } from "../support/app";

function sleepers(arg: string): string[] {
  try {
    return execFileSync("pgrep", ["-f", `^sleep ${arg}$`], { encoding: "utf8" }).trim().split("\n").filter(Boolean);
  } catch {
    return [];
  }
}

// E2E-M3-8 Orphans (F-CD-03 AC8): kill -9 the app with a live child, relaunch.
describe("E2E-M3-8 orphans", () => {
  it("F-CD-03.8 the next launch offers to stop what's left, and Stop ends it", async () => {
    expect(sleepers("1001")).toEqual([]);
    await freshOffline();
    await addFolder(`/tmp/tandem-e2e-m3-8-${Date.now()}`);
    await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 20_000 });
    const pane = (await paneIds())[0] ?? "";
    await waitForShell(pane);
    // Survives the shell's SIGHUP when the app dies.
    await sendToPane(pane, "nohup sleep 1001 >/dev/null 2>&1 &\n");
    await browser.waitUntil(() => sleepers("1001").length === 1, { timeout: 10_000 });
    const app = process.env.TANDEM_APP_BINARY ?? "";
    execFileSync("pkill", ["-9", "-f", `^${app}`]);
    await relaunch();
    await $('[data-testid="orphans"]').waitForDisplayed({ timeout: 20_000 });
    const title = await browser.execute(() => document.querySelector('[role="dialog"] h2')?.textContent ?? "");
    expect(title).toBe("1 agent process from your last session is still running");
    expect(sleepers("1001")).toHaveLength(1);
    await $('[data-testid="orphans-stop"]').click();
    await browser.waitUntil(() => sleepers("1001").length === 0, { timeout: 5_000, timeoutMsg: "the orphan is still running" });
  });
});
