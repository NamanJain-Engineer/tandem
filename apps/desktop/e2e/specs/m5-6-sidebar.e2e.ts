import { $, browser } from "@wdio/globals";
import { addFolder, counters, freshUser, invoke, paneIds, planFooter, setPlan, waitForApp } from "../support/app";

type Node = { type: "leaf"; paneId: string } | { type: "split"; dir: "row" | "col"; ratio: number; a: Node; b: Node };

function balanced(nodes: Node[], dir: "row" | "col"): Node {
  if (nodes.length === 1) return nodes[0] as Node;
  const half = Math.ceil(nodes.length / 2);
  return { type: "split", dir, ratio: half / nodes.length, a: balanced(nodes.slice(0, half), dir), b: balanced(nodes.slice(half), dir) };
}

const reflows = (reset = false) =>
  browser.execute((r: boolean) => (window as unknown as { __tandemTest: { reflows: (r: boolean) => Record<string, number> } }).__tandemTest.reflows(r), reset);
const pills = () =>
  browser.execute(() => Array.from(document.querySelectorAll<HTMLElement>('[data-testid="sidebar"] [role="tab"]')).map((el) => ({ id: el.dataset.tabId, selected: el.getAttribute("aria-selected") === "true" })));

/** Picks an item from the sidebar's "+" menu. */
async function openFromPlus(label: string): Promise<void> {
  await $('[data-testid="sidebar-plus"]').click();
  await browser.waitUntil(
    async () =>
      browser.execute((l: string) => {
        const item = Array.from(document.querySelectorAll<HTMLElement>("[role=menuitem]")).find((el) => el.textContent?.trim() === l);
        item?.click();
        return item !== undefined;
      }, label),
    { timeout: 5_000, timeoutMsg: `no menu item ${label}` },
  );
}

/** Waits until nothing reflows for a while (resizes settle 80 ms after the
 * layout stops changing). */
async function quiet(): Promise<void> {
  await browser.pause(1_500);
}

// E2E-M5-6 Sidebar (F-SH-10).
describe("E2E-M5-6 sidebar", () => {
  let ids: string[] = [];

  it("F-SH-10.1 toggling the sidebar with 16 panes reflows each pane at most once", async function () {
    this.timeout(150_000);
    const { email } = await freshUser("m5-6");
    await setPlan(email, "pro");
    await browser.waitUntil(async () => (await planFooter()).startsWith("PRO"), { timeout: 20_000 });
    await addFolder(`/tmp/tandem-e2e-m5-6-${Date.now()}`);
    await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 20_000 });
    const [ws] = await invoke<Array<{ tabs: Array<{ id: string }> }>>("workspace_list");
    const tabId = ws?.tabs[0]?.id ?? "";
    ids = [(await paneIds())[0] ?? ""];
    for (let i = 1; i < 16; i++) ids.push((await invoke<{ id: string }>("pane_create", { tabId, kind: "terminal", cwd: null, engineId: null })).id);
    const rows = [0, 4, 8, 12].map((at) => balanced(ids.slice(at, at + 4).map((paneId) => ({ type: "leaf", paneId })), "row"));
    await invoke("layout_save", { tabId, tree: balanced(rows, "col"), activePaneId: ids[0], maximizedPaneId: null });
    await browser.refresh();
    await waitForApp();
    await browser.waitUntil(async () => (await paneIds()).length === 16, { timeout: 30_000 });
    await quiet();
    await reflows(true);
    await counters(true);
    await $('[data-testid="sidebar-toggle"]').click();
    await $('[data-testid="sidebar"]').waitForDisplayed({ timeout: 5_000 });
    await quiet();
    const per = await reflows();
    for (const id of ids) expect(per[id] ?? 0).toBeLessThanOrEqual(1);
    const resizes = (await counters()).ptyResizesByPane;
    for (const id of ids) expect(resizes[id] ?? 0).toBeLessThanOrEqual(1);
  });

  it("F-SH-10.2 full-screen and back cause no PTY resize", async () => {
    await counters(true);
    await $('[data-testid="sidebar-fullscreen"]').click();
    await browser.waitUntil(async () => (await $('[data-testid="sidebar"]').getAttribute("data-placement")) === "fullscreen", { timeout: 5_000 });
    await quiet();
    expect((await counters()).ptyResizes).toBe(0);
    await browser.keys("Escape");
    await browser.waitUntil(async () => (await $('[data-testid="sidebar"]').getAttribute("data-placement")) === "docked", { timeout: 5_000 });
    await quiet();
    expect((await counters()).ptyResizes).toBe(0);
  });

  it("F-SH-10.4 opening Dashboard twice leaves one Dashboard tab", async () => {
    for (let i = 0; i < 2; i++) await openFromPlus("Dashboard");
    expect((await pills()).filter((p) => p.id === "dashboard")).toHaveLength(1);
  });

  it("F-SH-10.3 tab order and the active tab survive a relaunch", async () => {
    await openFromPlus("Files");
    // Files moves before Dashboard (Alt+Shift+←) and stays selected.
    await $('[data-testid="sidebar-pill-files"]').click();
    await browser.keys(["Alt", "Shift", "ArrowLeft"]);
    await browser.waitUntil(async () => (await pills()).map((p) => p.id).join(",") === "files,dashboard", { timeout: 5_000 });
    await browser.pause(600); // the save is debounced
    await browser.refresh();
    await waitForApp();
    await $('[data-testid="sidebar"]').waitForDisplayed({ timeout: 10_000 });
    expect(await pills()).toEqual([
      { id: "files", selected: true },
      { id: "dashboard", selected: false },
    ]);
  });
});
