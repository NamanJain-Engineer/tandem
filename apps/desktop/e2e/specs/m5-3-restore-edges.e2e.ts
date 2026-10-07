import { $, browser } from "@wdio/globals";
import { existsSync, rmSync } from "node:fs";
import { join } from "node:path";
import {
  addFolder,
  crash,
  engineSpawns,
  fakeLog,
  freshOffline,
  home,
  invoke,
  paneIds,
  pausedCards,
  relaunch,
  stopOrphansAutomatically,
  textOf,
  waitForApp,
  waitForPaneText,
  waitForSessionStarts,
} from "../support/app";

type Node = { type: "leaf"; paneId: string } | { type: "split"; dir: "row" | "col"; ratio: number; a: Node; b: Node };
type Snapshot = { panes: Array<{ paneId: string; state: string; canResume: boolean; reason: string | null }> };

// E2E-M5-3 Restore edge cases (F-SH-07 AC4–6).
describe("E2E-M5-3 restore edge cases", () => {
  let other = "";
  let mine = "";
  let thread = "";

  after(async () => {
    await stopOrphansAutomatically(false).catch(() => undefined);
  });

  before(async function () {
    this.timeout(180_000);
    await freshOffline();
    await stopOrphansAutomatically(true);
    await addFolder(`/tmp/tandem-e2e-m5-3-${Date.now()}`);
    await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 20_000 });
    const [ws] = await invoke<Array<{ tabs: Array<{ id: string }> }>>("workspace_list");
    const tabId = ws?.tabs[0]?.id ?? "";
    await invoke("pane_close", { paneId: (await paneIds())[0] ?? "" });
    // One agent in its own account's provider home, one in the default.
    const account = await invoke<{ id: string; homePath: string }>("test_account_create", { engineId: "claude", label: `Work ${Date.now()}` });
    const logFrom = fakeLog().length;
    other = (await invoke<{ id: string }>("pane_create", { tabId, kind: "agent", cwd: null, engineId: "claude", accountId: account.id })).id;
    mine = (await invoke<{ id: string }>("pane_create", { tabId, kind: "agent", cwd: null, engineId: "claude" })).id;
    const tree: Node = { type: "split", dir: "row", ratio: 0.5, a: { type: "leaf", paneId: other }, b: { type: "leaf", paneId: mine } };
    await invoke("layout_save", { tabId, tree, activePaneId: mine, maximizedPaneId: null });
    await browser.refresh();
    await waitForApp();
    await browser.waitUntil(async () => (await paneIds()).length === 2, { timeout: 30_000 });
    for (const a of [other, mine]) await waitForPaneText(a, "Fake Claude Code");
    await waitForSessionStarts(logFrom, 2);
    // A scrollback snapshot (written every 1.5 s under the harness).
    await browser.waitUntil(() => existsSync(join(home(), "scrollback", `${mine}.bin`)), { timeout: 15_000 });
    thread = await invoke<string>("test_thread_seed", { threadState: "working" });

    crash();
    // The saved launch configuration goes away while the app is down.
    rmSync(account.homePath, { recursive: true, force: true });
    await relaunch();
    await browser.waitUntil(async () => (await pausedCards()).length === 2, { timeout: 30_000 });
  });

  it("F-SH-07.4 a missing launch configuration says so and never spawns", async () => {
    const card = `[data-testid="pane"][data-pane-id="${other}"]`;
    expect(await textOf(`${card} [data-testid="restore-reason"]`)).toBe("Launch configuration unavailable");
    expect(await $(`${card} [data-testid="restore-resume"]`).isEnabled()).toBe(false);
    const before = (await engineSpawns()).length;
    await $(`${card} [data-testid="restore-resume"]`).click().catch(() => undefined);
    await browser.pause(1_000);
    expect((await engineSpawns()).length).toBe(before);
    const snap = await invoke<Snapshot>("restore_get_snapshot");
    expect(snap.panes.find((p) => p.paneId === other)).toMatchObject({ canResume: false, reason: "Launch configuration unavailable" });
  });

  it("F-SH-07.5 Ctrl+C on a card takes it out of the queue and its saved output stays readable", async () => {
    const card = `[data-testid="pane"][data-pane-id="${mine}"] [data-testid="paused-card"]`;
    await $(card).click();
    await browser.keys(["Control", "c"]);
    await browser.waitUntil(async () => (await $(card).getAttribute("data-state")) === "cancelled", { timeout: 5_000 });
    const snap = await invoke<Snapshot>("restore_get_snapshot");
    expect(snap.panes.find((p) => p.paneId === mine)?.state).toBe("cancelled");
    await browser.waitUntil(async () => (await textOf(`${card} [data-testid="restored-scrollback"]`)).includes("Fake Claude Code"), {
      timeout: 5_000,
      timeoutMsg: "the saved output isn't shown",
    });
  });

  it("F-SH-07.6 a thread persisted as working is settled to interrupted, never done", async () => {
    expect(await invoke<string>("test_thread_state", { id: thread })).toBe("interrupted");
  });
});
