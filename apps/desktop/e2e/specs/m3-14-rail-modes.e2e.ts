import { $, browser } from "@wdio/globals";
import { rmSync } from "node:fs";
import { addFolder, freshOffline, invoke, mod, paneIds, paneText, relaunch, sendToPane, shellPid, waitForPaneText, waitForShell, workspaceRoots } from "../support/app";

const base = `/tmp/tandem-e2e-m3-14-${Date.now()}`;

async function rowNames(): Promise<string[]> {
  const names = await browser.execute(() =>
    Array.from(document.querySelectorAll<HTMLElement>('[data-testid="workspace-row"] [data-testid="rail-row-name"]')).map((el) => el.innerText.trim()),
  );
  return names as string[];
}

async function count(selector: string): Promise<number> {
  return (await browser.execute((sel: string) => document.querySelectorAll(sel).length, selector)) as number;
}

async function tabIds(): Promise<string[]> {
  return (await browser.execute(() =>
    Array.from(document.querySelectorAll<HTMLElement>('[data-testid="tab"]')).map((el) => el.dataset.tabId ?? ""),
  )) as string[];
}

async function locate(selector: string): Promise<{ x: number; y: number }> {
  return $(selector).getLocation();
}

/** Drags with real pointer input from one element's centre to a point. */
async function drag(selector: string, to: { x: number; y: number }): Promise<void> {
  const el = await $(selector);
  const at = await el.getLocation();
  const size = await el.getSize();
  const start = { x: Math.round(at.x + size.width / 2), y: Math.round(at.y + size.height / 2) };
  const action = browser.action("pointer", { parameters: { pointerType: "mouse" } }).move(start).down({ button: 0 });
  for (let i = 1; i <= 10; i++) action.move({ x: Math.round(start.x + ((to.x - start.x) * i) / 10), y: Math.round(start.y + ((to.y - start.y) * i) / 10), duration: 30 });
  await action.up({ button: 0 }).perform();
}

// E2E-M3-14 Rail and modes (F-SH-02, F-SH-03, F-CD-01).
describe("E2E-M3-14 rail and modes", () => {
  before(async () => {
    await freshOffline();
  });

  it("F-CD-01.1 adding the same folder twice keeps one row", async () => {
    await addFolder(`${base}/a/b`);
    await addFolder(`${base}/a/b`);
    await browser.waitUntil(async () => (await workspaceRoots()).length === 1, { timeout: 10_000 });
    await browser.pause(500);
    expect(await workspaceRoots()).toEqual([`${base}/a/b`]);
  });

  it("F-SH-03.1 opening a workspace that isn't first leaves the order alone", async () => {
    await addFolder(`${base}/c`);
    await addFolder(`${base}/d`);
    await browser.waitUntil(async () => (await rowNames()).length === 3, { timeout: 10_000 });
    const order = await rowNames();
    await $('[data-testid="workspace-row"][data-root$="/d"]').click();
    await browser.pause(300);
    expect(await rowNames()).toEqual(order);
  });

  it("F-SH-03.2 dragging a row above the band pins it; tab order and pins survive a relaunch", async () => {
    const top = await locate('[data-testid="workspace-row"]');
    await drag('[data-testid="workspace-row"][data-root$="/d"]', { x: Math.round(top.x + 40), y: Math.round(top.y - 12) });
    await browser.waitUntil(async () => (await $('[data-testid="workspace-row"][data-root$="/d"]').getAttribute("data-pinned")) === "true", {
      timeout: 5_000,
      timeoutMsg: `rows: ${await rowNames()}`,
    });
    // Two tabs in d, reordered by drag.
    await $('button[aria-label="New tab"]').click();
    await browser.waitUntil(async () => (await count('[data-testid="tab"]')) === 2, { timeout: 10_000 });
    const firstTab = await locate('[data-testid="tab"]');
    const ids = await tabIds();
    await drag(`[data-testid="tab"][data-tab-id="${ids[1]}"] button`, { x: Math.round(firstTab.x + 2), y: Math.round(firstTab.y + 10) });
    await browser.waitUntil(async () => (await tabIds())[0] === ids[1], { timeout: 5_000, timeoutMsg: `tabs: ${await tabIds()}` });
    await relaunch();
    await $('[data-testid="workspace-row"]').waitForExist({ timeout: 20_000 });
    expect(await $('[data-testid="workspace-row"][data-root$="/d"]').getAttribute("data-pinned")).toBe("true");
    expect((await rowNames())[0]).toBe("d");
    await $('[data-testid="workspace-row"][data-root$="/d"]').click();
    await browser.waitUntil(async () => (await count('[data-testid="tab"]')) === 2, { timeout: 10_000 });
    expect((await tabIds())[0]).toBe(ids[1]);
  });

  it("F-SH-03.3 a deleted folder shows the warning; Remove only drops the row; re-adding restores its tabs", async () => {
    const c = `${base}/c`;
    await $('[data-testid="workspace-row"][data-root$="/c"]').click();
    await invoke("tab_rename", { id: (await invoke<Array<{ root: string; tabs: Array<{ id: string }> }>>("workspace_list")).find((w) => w.root === c)?.tabs[0]?.id, name: "Kept tab" });
    rmSync(c, { recursive: true, force: true });
    await relaunch();
    const row = await $('[data-testid="workspace-row"][data-root$="/c"]');
    await row.waitForExist({ timeout: 20_000 });
    expect(await row.getAttribute("class")).toContain("text-warning");
    const others = (await workspaceRoots()).filter((r) => r !== c);
    await row.click();
    await row.click();
    await browser.keys("Delete");
    // The dialog's Remove (the missing-folder screen behind it has one too).
    await $('[role="dialog"]').$("button=Remove").click();
    await browser.waitUntil(async () => !(await workspaceRoots()).includes(c), { timeout: 10_000 });
    expect(await workspaceRoots()).toEqual(others);
    await addFolder(c);
    await browser.waitUntil(async () => (await workspaceRoots()).includes(c), { timeout: 10_000 });
    await browser.waitUntil(async () => (await browser.execute(() => document.body.innerText.includes("Kept tab"))) === true, { timeout: 10_000 });
  });

  it("F-SH-02 modes keep terminals: Code → Thread → Code has the same PID and scrollback", async () => {
    await $('[data-testid="workspace-row"][data-root$="/c"]').click();
    await browser.waitUntil(async () => (await paneIds()).length >= 1, { timeout: 20_000 });
    const pane = (await paneIds())[0] ?? "";
    await waitForShell(pane);
    await sendToPane(pane, "echo mode-marker\n");
    await waitForPaneText(pane, "mode-marker");
    const pid = await shellPid(pane);
    await browser.execute(() =>
      Array.from(document.querySelectorAll<HTMLElement>('[role="radiogroup"][aria-label="Work mode"] [role="radio"]'))
        .find((r) => r.innerText.trim() === "Thread")
        ?.click(),
    );
    await browser.waitUntil(async () => (await paneIds()).length === 0, { timeout: 5_000 });
    // Mod+2 from wherever focus is (the terminal included) comes back.
    await browser.keys([mod(), "2"]);
    await browser.waitUntil(async () => (await paneIds()).includes(pane), { timeout: 5_000 });
    expect(await shellPid(pane)).toBe(pid);
    expect(await paneText(pane)).toContain("mode-marker");
    // From a focused terminal, Mod+3 switches and the shell gets nothing.
    await $(`[data-testid="terminal"][data-pane-id="${pane}"]`).click();
    await browser.keys([mod(), "3"]);
    await browser.waitUntil(async () => (await paneIds()).length === 0, { timeout: 5_000 });
    await browser.keys([mod(), "2"]);
    await browser.waitUntil(async () => (await paneIds()).includes(pane), { timeout: 5_000 });
  });

  it("F-SH-02 the mode switch reads as a radio group: Work mode, Code, selected, 2 of 3", async () => {
    expect(await $('[role="radiogroup"][aria-label="Work mode"]').isExisting()).toBe(true);
    const checked = await browser.execute(() =>
      Array.from(document.querySelectorAll('[role="radiogroup"][aria-label="Work mode"] [role="radio"]')).map((r) => r.getAttribute("aria-checked")),
    );
    expect(checked).toEqual(["false", "true", "false"]);
    expect(await browser.execute(() => (document.querySelector('[role="radiogroup"][aria-label="Work mode"] [aria-checked="true"]') as HTMLElement | null)?.innerText.trim())).toBe("Code");
  });
});
