import { browser } from "@wdio/globals";
import {
  addFolder,
  freshUser,
  invoke,
  paneIds,
  planFooter,
  sendToPane,
  setPlan,
  waitForApp,
  waitForPaneText,
} from "../support/app";

type Node =
  { type: "leaf"; paneId: string } | { type: "split"; dir: "row" | "col"; ratio: number; a: Node; b: Node };

/** A balanced binary split of `nodes` along `dir`. */
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

// E2E-M3-1 Sixteen shells (F-CD-04 AC2). Pro allows 64 live panes; the 4×4
// grid is saved over IPC (splitting by hand stops at the 240×120 minimum
// before 16 panes fit this window).
describe("E2E-M3-1 sixteen shells", () => {
  it("F-CD-04.2 16 shells: distinct $TANDEM_PANE_ID, at most 8 WebGL contexts, no blank pane", async () => {
    const { email } = await freshUser("m3-1");
    await setPlan(email, "pro");
    await browser.waitUntil(async () => (await planFooter()).startsWith("PRO"), {
      timeout: 20_000,
      timeoutMsg: `footer: ${await planFooter()}`,
    });
    await addFolder(`/tmp/tandem-e2e-m3-1-${Date.now()}`);
    await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 20_000 });
    const [ws] = await invoke<Array<{ tabs: Array<{ id: string }> }>>("workspace_list");
    const tabId = ws?.tabs[0]?.id ?? "";
    const first = (await paneIds())[0] ?? "";
    const ids = [first];
    for (let i = 1; i < 16; i++)
      ids.push(
        (await invoke<{ id: string }>("pane_create", { tabId, kind: "terminal", cwd: null, engineId: null }))
          .id,
      );
    const rows = [0, 4, 8, 12].map((at) =>
      balanced(
        ids.slice(at, at + 4).map((paneId) => ({ type: "leaf", paneId })),
        "row",
      ),
    );
    await invoke("layout_save", {
      tabId,
      tree: balanced(rows, "col"),
      activePaneId: first,
      maximizedPaneId: null,
    });
    await browser.refresh();
    await waitForApp();
    await browser.waitUntil(async () => (await paneIds()).length === 16, {
      timeout: 30_000,
      timeoutMsg: `panes: ${(await paneIds()).length}`,
    });

    // Every shell sees its own pane id.
    for (const id of ids) await sendToPane(id, "echo pane=$TANDEM_PANE_ID\n");
    for (const id of ids) await waitForPaneText(id, `pane=${id}`, 30_000);
    expect(new Set(ids).size).toBe(16);

    // At most 8 WebGL contexts; the others render with the DOM renderer.
    const contexts = await browser.execute(() =>
      (window as unknown as { __tandemTest: { webglContexts: () => number } }).__tandemTest.webglContexts(),
    );
    expect(contexts).toBeGreaterThan(0);
    expect(contexts).toBeLessThanOrEqual(8);

    // No blank or unstyled pane: each has a sized xterm screen on the theme background.
    const bad = await browser.execute(() =>
      Array.from(document.querySelectorAll<HTMLElement>('[data-testid="terminal"]'))
        .filter((el) => el.offsetParent !== null)
        .filter((el) => {
          const screen = el.querySelector<HTMLElement>(".xterm-screen");
          const rect = screen?.getBoundingClientRect();
          const bg = getComputedStyle(el.parentElement ?? el).backgroundColor;
          return !rect || rect.width < 100 || rect.height < 40 || bg === "rgba(0, 0, 0, 0)";
        })
        .map((el) => el.dataset.paneId),
    );
    expect(bad).toEqual([]);
  });
});
