import { $, $$, browser } from "@wdio/globals";
import {
  addFolder,
  counters,
  freshUser,
  invoke,
  paneIds,
  sendToPane,
  shellPid,
  waitForApp,
  waitForPaneText,
  waitForShell,
} from "../support/app";

type Rect = { id: string; x: number; y: number; w: number; h: number };

async function rects(): Promise<Rect[]> {
  return (await browser.execute(() =>
    Array.from(document.querySelectorAll<HTMLElement>('[data-testid="pane"]'))
      .filter((el) => el.offsetParent !== null)
      .map((el) => {
        const r = el.getBoundingClientRect();
        return {
          id: el.dataset.paneId ?? "",
          x: Math.round(r.x),
          y: Math.round(r.y),
          w: Math.round(r.width),
          h: Math.round(r.height),
        };
      }),
  )) as Rect[];
}

async function workspaceMenu(item: string): Promise<void> {
  await $('button[aria-label^="Workspace"]').click();
  await browser.waitUntil(
    async () =>
      browser.execute((label: string) => {
        const el = Array.from(
          document.querySelectorAll<HTMLElement>('[role="menuitem"], [role="menuitemcheckbox"]'),
        ).find((x) => (x.textContent ?? "").trim().startsWith(label));
        el?.click();
        return el !== undefined;
      }, item),
    { timeout: 5_000, timeoutMsg: `no menu item ${item}` },
  );
}

// E2E-M4-6 Arrange and Card view (F-CD-11).
describe("E2E-M4-6 arrange and Card view", () => {
  let ids: string[] = [];

  it("F-CD-11.1 auto-arrange turns 5 panes into a 3+2 grid with equal widths in each row", async function () {
    // Signing a user in through the scripted browser, then five panes (no pane limit, D-2900).
    this.timeout(150_000);
    await freshUser("m4-6");
    await addFolder(`/tmp/tandem-e2e-m4-6-${Date.now()}`);
    await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 20_000 });
    const [ws] = await invoke<Array<{ id: string; tabs: Array<{ id: string }> }>>("workspace_list");
    const tabId = ws?.tabs[0]?.id ?? "";
    ids = [(await paneIds())[0] ?? ""];
    for (let i = 0; i < 3; i++)
      ids.push(
        (await invoke<{ id: string }>("pane_create", { tabId, kind: "terminal", cwd: null, engineId: null }))
          .id,
      );
    ids.push(
      (await invoke<{ id: string }>("pane_create", { tabId, kind: "agent", cwd: null, engineId: "claude" }))
        .id,
    );
    // A lopsided column of five.
    let tree: unknown = { type: "leaf", paneId: ids[4] };
    for (let i = 3; i >= 0; i--)
      tree = { type: "split", dir: "col", ratio: 0.2, a: { type: "leaf", paneId: ids[i] }, b: tree };
    await invoke("layout_save", { tabId, tree, activePaneId: ids[0], maximizedPaneId: null });
    await browser.refresh();
    await waitForApp();
    await browser.waitUntil(async () => (await paneIds()).length === 5, { timeout: 30_000 });
    for (const id of ids.slice(0, 4)) await waitForShell(id, 30_000);
    await waitForPaneText(ids[4] ?? "", "Fake Claude Code");
    await workspaceMenu("Auto-arrange panes");
    await browser.pause(1_000);
    const r = await rects();
    const rows = [...new Set(r.map((x) => x.y))].sort((a, b) => a - b);
    expect(rows).toHaveLength(2);
    const top = r.filter((x) => x.y === rows[0]);
    const bottom = r.filter((x) => x.y === rows[1]);
    expect([top.length, bottom.length]).toEqual([3, 2]);
    for (const row of [top, bottom]) {
      const widths = row.map((x) => x.w);
      expect(Math.max(...widths) - Math.min(...widths)).toBeLessThanOrEqual(2);
    }
  });

  it("F-CD-11.2 F-CD-11.3 Card view shows live cards; flipping back keeps PIDs and makes no resizes", async () => {
    await browser.pause(1_500);
    const pids = await Promise.all(ids.slice(0, 4).map((id) => shellPid(id)));
    await counters(true);
    await workspaceMenu("Card view");
    await browser.waitUntil(async () => (await $$('[data-testid="pane-card"]').length) === 5, {
      timeout: 5_000,
    });
    // A status change shows on the card.
    const agent = ids[4] ?? "";
    await sendToPane(agent, "/ask\r");
    const card = `[data-testid="pane"][data-pane-id="${agent}"] [data-testid="pane-card"]`;
    await browser.waitUntil(async () => (await $(card).getAttribute("data-status")) === "waiting", {
      timeout: 15_000,
    });
    await workspaceMenu("Card view");
    await browser.waitUntil(async () => (await $$('[data-testid="pane-card"]').length) === 0, {
      timeout: 5_000,
    });
    await browser.pause(1_000);
    expect(await Promise.all(ids.slice(0, 4).map((id) => shellPid(id)))).toEqual(pids);
    expect((await counters()).ptyResizes).toBe(0);
  });
});
