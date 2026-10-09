import { $, browser } from "@wdio/globals";
import { freshUser, invoke, planFooter, relaunch, setPlan, signInDesktop } from "../support/app";
import { threadMode } from "../support/thread";

async function cells(): Promise<string[]> {
  return browser.execute(() =>
    Array.from(document.querySelectorAll<HTMLElement>('[data-testid="grid-cell"]')).map(
      (c) => c.dataset.threadId ?? "",
    ),
  );
}

/** Drags a rail row onto the thread area (HTML5 drag and drop, in the page). */
async function dragIn(id: string): Promise<void> {
  await browser.execute((threadId: string) => {
    const row = document.querySelector(`[data-testid="thread-row"][data-thread-id="${threadId}"]`);
    const area = document.querySelector('[data-testid="thread-area"]');
    if (!row || !area) throw new Error("no row or area");
    const dataTransfer = new DataTransfer();
    row.dispatchEvent(new DragEvent("dragstart", { bubbles: true, cancelable: true, dataTransfer }));
    area.dispatchEvent(new DragEvent("dragover", { bubbles: true, cancelable: true, dataTransfer }));
    area.dispatchEvent(new DragEvent("drop", { bubbles: true, cancelable: true, dataTransfer }));
    row.dispatchEvent(new DragEvent("dragend", { bubbles: true, cancelable: true, dataTransfer }));
  }, id);
}

async function toasts(): Promise<string> {
  return browser.execute(
    () => document.querySelector('[role="region"][aria-label="Notifications"]')?.textContent ?? "",
  );
}

// E2E-M7-7 Grid (F-TH-14): dragging seven threads in shows six and says the
// grid is full; the grid comes back after a relaunch; Free shows one thread.
describe("E2E-M7-7 grid", () => {
  let email = "";
  const ids: string[] = [];

  before(async function () {
    this.timeout(120_000);
    ({ email } = await freshUser("m7-7"));
    await setPlan(email, "pro");
    await browser.waitUntil(async () => (await planFooter()).startsWith("PRO"), { timeout: 20_000 });
    await threadMode();
    for (let i = 0; i < 7; i++) {
      ids.push(
        (await invoke<{ id: string }>("thread_create", { args: { kind: "thread", workspaceId: null } })).id,
      );
    }
    for (const id of ids)
      await $(`[data-testid="thread-row"][data-thread-id="${id}"]`).waitForExist({ timeout: 10_000 });
  });

  it("F-TH-14.1 dragging 7 threads keeps 6 side by side and says Grid is full (6)", async function () {
    this.timeout(60_000);
    await $(`[data-testid="thread-row"][data-thread-id="${ids[0]}"]`).click();
    await $(`[data-testid="thread-main"][data-thread-id="${ids[0]}"]`).waitForExist({ timeout: 10_000 });
    for (const id of ids.slice(1)) await dragIn(id);
    await browser.waitUntil(async () => (await cells()).length === 6, {
      timeout: 10_000,
      timeoutMsg: `cells: ${JSON.stringify(await cells())}`,
    });
    expect(await cells()).toEqual(ids.slice(0, 6));
    await browser.waitUntil(async () => (await toasts()).includes("Grid is full (6)"), { timeout: 5_000 });
    // Each cell has its own composer.
    const composers = await browser.execute(
      () => document.querySelectorAll('[data-testid="grid-cell"] [data-testid="composer"]').length,
    );
    expect(composers).toBe(6);
  });

  it("F-TH-14.2 the grid comes back after a relaunch", async function () {
    this.timeout(120_000);
    await relaunch();
    // The E2E keychain lives in memory: sign in again (the grid is a setting).
    await signInDesktop(email);
    await browser.waitUntil(async () => (await planFooter()).startsWith("PRO"), { timeout: 20_000 });
    await threadMode();
    await browser.waitUntil(async () => (await cells()).length === 6, {
      timeout: 20_000,
      timeoutMsg: `cells: ${JSON.stringify(await cells())}`,
    });
    expect(await cells()).toEqual(ids.slice(0, 6));
  });

  it("F-TH-14.1 a Free account shows one thread at a time", async function () {
    this.timeout(60_000);
    await setPlan(email, "free");
    await invoke("entitlement_refresh");
    await browser.waitUntil(async () => (await planFooter()).startsWith("FREE"), { timeout: 20_000 });
    await browser.waitUntil(async () => (await cells()).length === 0, { timeout: 10_000 });
    expect(await $('[data-testid="thread-main"]').isExisting()).toBe(true);
    await dragIn(ids[6] ?? "");
    await browser.waitUntil(async () => (await toasts()).includes("Free shows 1 thread at a time"), {
      timeout: 5_000,
    });
    expect(await cells()).toEqual([]);
  });
});
