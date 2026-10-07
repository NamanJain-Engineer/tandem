import { $, browser } from "@wdio/globals";
import { addFolder, counters, freshOffline, paneIds, paneText, sendToPane, shellPid, split, waitForPaneText, waitForShell } from "../support/app";

/** Drags pane `from`'s header to the centre of pane `to` with real pointer input. */
async function dragHeader(from: string, to: string, opts: { cancel?: boolean } = {}): Promise<void> {
  const header = await $(`[data-testid="pane"][data-pane-id="${from}"] [data-testid="pane-title"]`);
  const target = await $(`[data-testid="pane"][data-pane-id="${to}"] [data-testid="terminal"]`);
  const a = await header.getLocation();
  const t = await target.getLocation();
  const ts = await target.getSize();
  const start = { x: Math.round(a.x + 10), y: Math.round(a.y + 5) };
  const end = { x: Math.round(t.x + ts.width / 2), y: Math.round(t.y + ts.height / 2) };
  const steps = 8;
  const pointer = browser.action("pointer", { parameters: { pointerType: "mouse" } }).move(start).down({ button: 0 });
  for (let i = 1; i <= steps; i++) {
    pointer.move({ x: Math.round(start.x + ((end.x - start.x) * i) / steps), y: Math.round(start.y + ((end.y - start.y) * i) / steps), duration: 30 });
  }
  if (opts.cancel) {
    await pointer.perform(true);
    await browser.keys("Escape");
    await browser.action("pointer", { parameters: { pointerType: "mouse" } }).move(end).up({ button: 0 }).perform();
    return;
  }
  await pointer.up({ button: 0 }).perform();
}

async function sttySize(pane: string): Promise<string> {
  const tag = `sz${Math.random().toString(36).slice(2, 7)}`;
  await sendToPane(pane, `echo ${tag}=$(stty size)\n`);
  await waitForPaneText(pane, `${tag}=`);
  return /=(\d+ \d+)/.exec((await paneText(pane)).split(`${tag}=`).pop() ?? "")?.[1] ?? "";
}

// E2E-M3-4 Swap, maximize, cancel (F-CD-02 AC2–AC4).
describe("E2E-M3-4 swap, maximize, cancel", () => {
  let left = "";
  let right = "";

  before(async () => {
    await freshOffline();
    await addFolder(`/tmp/tandem-e2e-m3-4-${Date.now()}`);
    await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 20_000 });
    left = (await paneIds())[0] ?? "";
    await waitForShell(left);
    await sendToPane(left, "echo left-marker\n");
    await waitForPaneText(left, "left-marker");
    right = await split("right");
    await sendToPane(right, "echo right-marker\n");
    await waitForPaneText(right, "right-marker");
  });

  it("F-CD-02.2 swapping two panes keeps both processes and their scrollback", async () => {
    const pids = [await shellPid(left), await shellPid(right)];
    await dragHeader(left, right);
    await browser.waitUntil(async () => (await paneIds()).join() === [right, left].join(), { timeout: 5_000, timeoutMsg: `order: ${await paneIds()}` });
    expect([await shellPid(left), await shellPid(right)]).toEqual(pids);
    expect(await paneText(left)).toContain("left-marker");
    expect(await paneText(right)).toContain("right-marker");
  });

  it("F-CD-02.3 maximize and restore don't resize a pane whose size didn't change", async () => {
    const before = await sttySize(left);
    const maximize = await $(`[data-testid="pane"][data-pane-id="${right}"] button[aria-label="Maximize pane"]`);
    await maximize.click();
    await browser.waitUntil(async () => (await paneIds()).join() === right, { timeout: 5_000 });
    await (await $(`[data-testid="pane"][data-pane-id="${right}"] button[aria-label="Restore pane"]`)).click();
    await browser.waitUntil(async () => (await paneIds()).length === 2, { timeout: 5_000 });
    expect(await sttySize(left)).toBe(before);
  });

  it("F-CD-02.4 Esc during a drag leaves the layout as it was", async () => {
    const order = await paneIds();
    await counters(true);
    await dragHeader(order[0] ?? "", order[1] ?? "", { cancel: true });
    await browser.pause(300);
    expect(await paneIds()).toEqual(order);
    expect((await counters()).ptyResizes).toBe(0);
  });
});
