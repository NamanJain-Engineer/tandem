import { $, browser } from "@wdio/globals";
import { spawn, type ChildProcess } from "node:child_process";
import { mkdirSync } from "node:fs";
import { createServer } from "node:net";
import { addFolder, freshOffline, paneIds, sendToPane, split } from "../support/app";
import { expectNoSeriousA11yViolationsInBothThemes } from "../support/axe";
import { openFromPlus } from "../support/files";

// E2E-M17-3 Ports (F-EX-03): a server a pane started is listed with the
// pane's name and a server started outside Vibezeo isn't; Open loads it in a
// sidebar browser tab; Stop asks, Cancel keeps it, confirming ends it and the
// pane stays.

async function freePort(): Promise<number> {
  return new Promise((resolve) => {
    const s = createServer();
    s.listen(0, "127.0.0.1", () => {
      const a = s.address();
      const p = typeof a === "object" && a ? a.port : 0;
      s.close(() => resolve(p));
    });
  });
}

async function rows(): Promise<string[]> {
  return browser.execute(() =>
    Array.from(document.querySelectorAll<HTMLElement>('[data-testid="port-row"]')).map((r) => `${r.dataset.port}|${r.dataset.pane}|${r.textContent ?? ""}`),
  );
}

async function press(selector: string): Promise<void> {
  await $(selector).waitForExist({ timeout: 10_000 });
  await browser.execute((sel: string) => document.querySelector<HTMLElement>(sel)?.click(), selector);
}

describe("E2E-M17-3 ports", () => {
  const dir = `/tmp/tandem-e2e-m17-3-${Date.now()}`;
  let outside: ChildProcess | null = null;
  let pane = "";
  let port = 0;
  let other = 0;

  before(async function () {
    this.timeout(60_000);
    mkdirSync(dir, { recursive: true });
    port = await freePort();
    other = await freePort();
    outside = spawn("python3", ["-m", "http.server", String(other), "--bind", "127.0.0.1"], { cwd: dir, stdio: "ignore" });
    await freshOffline();
    await addFolder(dir);
    await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 20_000 });
    pane = await split("right");
    await browser.pause(1500);
    await sendToPane(pane, `python3 -m http.server ${port} --bind 127.0.0.1\r`);
  });

  after(() => {
    outside?.kill();
  });

  it("F-EX-03.1 a pane's server is listed with the pane; one started outside isn't", async function () {
    this.timeout(60_000);
    await openFromPlus("Ports");
    await browser.waitUntil(async () => (await rows()).some((r) => r.startsWith(`${port}|`)), {
      timeout: 15_000,
      timeoutMsg: `rows: ${JSON.stringify(await rows())}`,
    });
    const mine = (await rows()).find((r) => r.startsWith(`${port}|`)) ?? "";
    expect(mine.split("|")[1]).toBe(pane);
    expect(mine).toContain("python3");
    expect(mine).toContain("localhost");
    expect((await rows()).some((r) => r.startsWith(`${other}|`))).toBe(false);
    await expectNoSeriousA11yViolationsInBothThemes();
  });

  it("F-EX-03.2 Open loads it in a sidebar browser tab", async function () {
    this.timeout(30_000);
    await press(`[data-testid="port-row"][data-port="${port}"] [data-testid="port-open"]`);
    await browser.waitUntil(
      async () =>
        browser.execute(
          (p: number) => Array.from(document.querySelectorAll('[data-testid="sidebar-pill-browser"]')).length > 0 && document.body.innerHTML.includes(`localhost:${p}`),
          port,
        ),
      { timeout: 15_000, timeoutMsg: "no browser tab for the port" },
    );
    // Back to the Ports tab.
    await press('[data-testid="sidebar-pill-ports"]');
  });

  it("F-EX-03.3 Stop asks; Cancel keeps it; confirming ends it and the pane stays", async function () {
    this.timeout(60_000);
    await press(`[data-testid="port-row"][data-port="${port}"] [data-testid="port-stop"]`);
    const dialog = await $('[role="alertdialog"]');
    await dialog.waitForDisplayed({ timeout: 5_000 });
    expect(await dialog.getText()).toContain(`port ${port}`);
    await (await dialog.$("button=Cancel")).click();
    await dialog.waitForExist({ timeout: 5_000, reverse: true });
    await browser.pause(2500);
    expect((await rows()).some((r) => r.startsWith(`${port}|`))).toBe(true);
    await press(`[data-testid="port-row"][data-port="${port}"] [data-testid="port-stop"]`);
    const again = await $('[role="alertdialog"]');
    await again.waitForDisplayed({ timeout: 5_000 });
    await (await again.$("button=Stop process")).click();
    await browser.waitUntil(async () => !(await rows()).some((r) => r.startsWith(`${port}|`)), {
      timeout: 15_000,
      timeoutMsg: `still listed: ${JSON.stringify(await rows())}`,
    });
    expect(await paneIds()).toContain(pane);
  });
});
