import { browser } from "@wdio/globals";
import {
  addFolder,
  counters,
  crash,
  fakeLog,
  freshOffline,
  invoke,
  paneIds,
  pausedCards,
  relaunch,
  stopOrphansAutomatically,
  waitForApp,
  waitForPaneText,
  waitForSessionStarts,
} from "../support/app";

type Node = { type: "leaf"; paneId: string } | { type: "split"; dir: "row" | "col"; ratio: number; a: Node; b: Node };

// E2E-M5-2 Auto-resume (F-SH-07 AC3): never more than 2 engines spawning.
describe("E2E-M5-2 auto-resume", () => {
  after(async () => {
    await invoke("settings_set", { key: "resume_agents_automatically", value: false }).catch(() => undefined);
    await stopOrphansAutomatically(false).catch(() => undefined);
  });

  it("F-SH-07.3 with Resume agents automatically on, 4 agents come back with at most 2 spawning at once", async function () {
    this.timeout(240_000);
    await freshOffline();
    await stopOrphansAutomatically(true);
    await addFolder(`/tmp/tandem-e2e-m5-2-${Date.now()}`);
    await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 20_000 });
    const [ws] = await invoke<Array<{ tabs: Array<{ id: string }> }>>("workspace_list");
    const tabId = ws?.tabs[0]?.id ?? "";
    // Agents only: Free's 4 live panes.
    const shell = (await paneIds())[0] ?? "";
    await invoke("pane_close", { paneId: shell });
    const logFrom = fakeLog().length;
    const agents: string[] = [];
    for (let i = 0; i < 4; i++) agents.push((await invoke<{ id: string }>("pane_create", { tabId, kind: "agent", cwd: null, engineId: "claude" })).id);
    const leaf = (paneId: string): Node => ({ type: "leaf", paneId });
    const row = (a: string, b: string): Node => ({ type: "split", dir: "row", ratio: 0.5, a: leaf(a), b: leaf(b) });
    const tree: Node = { type: "split", dir: "col", ratio: 0.5, a: row(agents[0] ?? "", agents[1] ?? ""), b: row(agents[2] ?? "", agents[3] ?? "") };
    await invoke("layout_save", { tabId, tree, activePaneId: agents[0], maximizedPaneId: null });
    await browser.refresh();
    await waitForApp();
    await browser.waitUntil(async () => (await paneIds()).length === 4, { timeout: 30_000 });
    for (const a of agents) await waitForPaneText(a, "Fake Claude Code");
    await waitForSessionStarts(logFrom, 4);
    await invoke("settings_set", { key: "resume_agents_automatically", value: true });

    crash();
    await relaunch();
    await browser.waitUntil(async () => (await paneIds()).length === 4, { timeout: 30_000 });
    for (const a of agents) await waitForPaneText(a, "Resumed session fake-", 40_000);
    expect(await pausedCards()).toEqual([]);
    const c = (await counters()) as unknown as { restoreMaxSpawning: number };
    expect(c.restoreMaxSpawning).toBeGreaterThanOrEqual(1);
    expect(c.restoreMaxSpawning).toBeLessThanOrEqual(2);
  });
});
