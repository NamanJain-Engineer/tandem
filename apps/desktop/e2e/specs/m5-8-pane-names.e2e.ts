import { $, browser } from "@wdio/globals";
import {
  addFolder,
  freshOffline,
  paneIds,
  sendToPane,
  split,
  textOf,
  waitForApp,
  waitForPaneText,
  waitForStatus,
} from "../support/app";

const nameOf = (pane: string) =>
  textOf(`[data-testid="pane"][data-pane-id="${pane}"] [data-testid="pane-name"]`);

// E2E-M5-8 Pane names (F-CD-12).
describe("E2E-M5-8 pane names", () => {
  let agents: string[] = [];

  it("F-CD-12.1 two agent panes never share a name, and names survive a relaunch", async () => {
    await freshOffline();
    await addFolder(`/tmp/tandem-e2e-m5-8-${Date.now()}`);
    await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 20_000 });
    agents = [await split("right", "claude"), await split("down", "claude")];
    for (const a of agents) await waitForPaneText(a, "Fake Claude Code");
    const names = await Promise.all(agents.map(nameOf));
    expect(names.every((n) => /^[A-Z][a-z]+$/.test(n))).toBe(true);
    expect(new Set(names).size).toBe(2);
    await browser.refresh();
    await waitForApp();
    await browser.waitUntil(async () => (await paneIds()).length === 3, { timeout: 30_000 });
    expect(await Promise.all(agents.map(nameOf))).toEqual(names);
  });

  it("F-CD-12.2 closing a working agent asks 'Stop {name}? It's working.' and closes only after confirming", async () => {
    const [agent] = agents;
    const name = await nameOf(agent ?? "");
    // /script works for 2 s before it asks anything.
    await sendToPane(agent ?? "", "/script\r");
    await waitForStatus(agent ?? "", "working");
    await $(`[data-testid="pane"][data-pane-id="${agent}"] button[aria-label^="Close "]`).click();
    const dialog = await $("[role=alertdialog]");
    await dialog.waitForExist({ timeout: 5_000 });
    expect(await textOf("[role=alertdialog] h2")).toBe(`Stop ${name}? It's working.`);
    await $("button=Keep running").click();
    expect(await paneIds()).toContain(agent);
    await $(`[data-testid="pane"][data-pane-id="${agent}"] button[aria-label^="Close "]`).click();
    await $("button=Stop and close").click();
    await browser.waitUntil(async () => !(await paneIds()).includes(agent ?? ""), { timeout: 5_000 });
  });
});
