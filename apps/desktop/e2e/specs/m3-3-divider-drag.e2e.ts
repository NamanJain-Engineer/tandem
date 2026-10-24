import { browser } from "@wdio/globals";
import {
  addFolder,
  counters,
  freshUser,
  invoke,
  paneIds,
  waitSignedIn,
  waitForApp,
  waitForShell,
} from "../support/app";

type Node =
  { type: "leaf"; paneId: string } | { type: "split"; dir: "row" | "col"; ratio: number; a: Node; b: Node };

function balanced(nodes: Node[], dir: "row" | "col"): Node {
  if (nodes.length === 1) return nodes[0] as Node;
  const half = Math.ceil(nodes.length / 2);
  return {
    type: "split",
    dir,
    ratio: half / nodes.length,
    a: balanced(nodes.slice(0, half), dir),
    b: balanced(nodes.slice(half), dir),
  };
}

// E2E-M3-3 Divider drag (F-CD-02 AC1): 8 panes (no pane limit, D-2900), drag the top row's
// middle divider. Nothing resizes while dragging; after pointer-up each pane
// of that row resizes once and the bottom row not at all.
describe("E2E-M3-3 divider drag", () => {
  it("F-CD-02.1 dragging a divider with 8 panes resizes each affected PTY exactly once, after pointer-up", async () => {
    await freshUser("m3-3");
    await waitSignedIn();
    await addFolder(`/tmp/tandem-e2e-m3-3-${Date.now()}`);
    await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 20_000 });
    const [ws] = await invoke<Array<{ tabs: Array<{ id: string }> }>>("workspace_list");
    const tabId = ws?.tabs[0]?.id ?? "";
    const ids = [(await paneIds())[0] ?? ""];
    for (let i = 1; i < 8; i++)
      ids.push(
        (await invoke<{ id: string }>("pane_create", { tabId, kind: "terminal", cwd: null, engineId: null }))
          .id,
      );
    const leaf = (paneId: string): Node => ({ type: "leaf", paneId });
    const tree = balanced(
      [balanced(ids.slice(0, 4).map(leaf), "row"), balanced(ids.slice(4).map(leaf), "row")],
      "col",
    );
    await invoke("layout_save", { tabId, tree, activePaneId: ids[0], maximizedPaneId: null });
    await browser.refresh();
    await waitForApp();
    await browser.waitUntil(async () => (await paneIds()).length === 8, { timeout: 30_000 });
    for (const id of ids) await waitForShell(id, 30_000);
    await browser.pause(500);
    await counters(true);

    // The top row's middle divider: a vertical handle in the top half, nearest the grid's centre.
    const handle = (await browser.execute(() => {
      const grid = document.querySelector('[data-testid="pane-grid"]')?.getBoundingClientRect();
      if (!grid) return null;
      const mid = grid.left + grid.width / 2;
      const top = grid.top + grid.height / 2;
      const handles = Array.from(
        document.querySelectorAll<HTMLElement>(
          '[data-panel-resize-handle-id][data-panel-group-direction="horizontal"]',
        ),
      )
        .map((el) => el.getBoundingClientRect())
        .filter((r) => r.top < top && r.height > 0)
        .sort((a, b) => Math.abs(a.left + a.width / 2 - mid) - Math.abs(b.left + b.width / 2 - mid));
      const r = handles[0];
      return r ? { x: Math.round(r.left + r.width / 2), y: Math.round(r.top + r.height / 2) } : null;
    })) as { x: number; y: number } | null;
    expect(handle).not.toBeNull();
    const from = handle ?? { x: 0, y: 0 };

    const press = browser
      .action("pointer", { parameters: { pointerType: "mouse" } })
      .move({ x: from.x, y: from.y })
      .down({ button: 0 });
    for (let i = 1; i <= 12; i++) press.move({ x: from.x + i * 10, y: from.y, duration: 40 });
    await press.perform(true);
    await browser.pause(300);
    expect((await counters()).ptyResizes).toBe(0);
    await browser
      .action("pointer", { parameters: { pointerType: "mouse" } })
      .up({ button: 0 })
      .perform();
    await browser.waitUntil(async () => (await counters()).ptyResizes >= 4, {
      timeout: 5_000,
      timeoutMsg: `resizes: ${JSON.stringify(await counters())}`,
    });
    await browser.pause(500);
    const byPane = (await counters()).ptyResizesByPane;
    expect(Object.fromEntries(ids.slice(0, 4).map((id) => [id, byPane[id] ?? 0]))).toEqual(
      Object.fromEntries(ids.slice(0, 4).map((id) => [id, 1])),
    );
    expect(Object.fromEntries(ids.slice(4).map((id) => [id, byPane[id] ?? 0]))).toEqual(
      Object.fromEntries(ids.slice(4).map((id) => [id, 0])),
    );
  });
});
