import { $, browser } from "@wdio/globals";
import {
  addFolder,
  freshOffline,
  invoke,
  paneIds,
  paneText,
  sendToPane,
  split,
  waitForPaneText,
  waitForStatus,
} from "../support/app";

type OsCall = { kind: string; title: string; source: { type: string; paneId?: string } };
const calls = (clear = false) => invoke<OsCall[]>("test_notifier_calls", { clear });
const visible = () => invoke<boolean>("test_window_visible", {});
const sayingSeen = (on: boolean) =>
  browser.execute(
    (v: boolean) =>
      (window as unknown as { __tandemTest?: { closeSeen: (on: boolean) => void } }).__tandemTest?.closeSeen(
        v,
      ),
    on,
  );

/** Presses a button that hides the window: a DOM click, which runs its
 * handler as a person's does. WebDriver's own click waits for the page to
 * settle after it, which a hidden window may never do: in the full
 * regression one such click never returned, and tauri-driver failed every
 * spec after it. */
async function pressHiding(selector: string): Promise<void> {
  await $(selector).waitForClickable({ timeout: 5_000 });
  await browser.execute((s: string) => (document.querySelector(s) as HTMLElement | null)?.click(), selector);
}

// E2E-M18-SH-14 Keep agents running (F-SH-14): two fake Claude Code agents
// waiting for an approval. Closing the window asks; Keep running hides it
// with both still running; an approval while it's hidden notifies; output
// written meanwhile is in the pane when it's shown again; with "Don't ask
// again" the next close keeps them without asking. A question the window
// never says it showed (a hung webview) goes ahead without it (D-2563).
describe("E2E-M18-SH-14 keep agents running", function () {
  this.timeout(300_000);
  const agents: string[] = [];

  before(async () => {
    await freshOffline();
    await addFolder(`/tmp/tandem-e2e-sh14-${Date.now()}`);
    await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 20_000 });
    for (let i = 0; i < 2; i++) {
      const id = await split("right", "claude");
      await waitForPaneText(id, "Fake Claude Code");
      agents.push(id);
    }
    for (const id of agents) {
      await sendToPane(id, "/ask\r");
      await waitForStatus(id, "waiting");
    }
  });

  after(async () => {
    await invoke("settings_set", { key: "keep_agents_on_close", value: "ask" });
    await invoke("app_ready", {});
  });

  it("F-SH-14.1 F-SH-14.2 F-SH-14.3 closing asks; Keep running hides the window, and the agents carry on", async () => {
    await invoke("window_close", {});
    const dialog = $('[data-testid="close-agents"]');
    await dialog.waitForExist({ timeout: 10_000 });
    expect(String(await $('[role="dialog"]').getProperty("textContent"))).toContain(
      "Keep 2 agents running in the background?",
    );
    await pressHiding('[data-testid="close-keep"]');
    await browser.waitUntil(async () => !(await visible()), {
      timeout: 10_000,
      timeoutMsg: "the window never hid",
    });

    // While it's hidden: an approval notifies, and output lands in the panes.
    await calls(true);
    const [first = "", second = ""] = agents;
    await sendToPane(first, "while hidden\r");
    await sendToPane(second, "/ask\r");
    await browser.waitUntil(async () => (await calls()).some((c) => c.source.paneId === second), {
      timeout: 15_000,
      timeoutMsg: `no notification for ${second}: ${JSON.stringify(await calls())}`,
    });
    expect(await visible()).toBe(false);

    // Clicking it shows the window, with what each wrote meanwhile.
    const index = (await calls()).findIndex((c) => c.source.paneId === second);
    await invoke("test_notifier_click", { index });
    await browser.waitUntil(() => visible(), { timeout: 10_000, timeoutMsg: "the window never came back" });
    await waitForPaneText(first, "you said: while hidden");
    expect((await paneText(second)).split("Do you want to proceed?").length - 1).toBeGreaterThanOrEqual(2);
  });

  it("F-SH-14 a question the window never shows goes ahead without it: the agents keep running, hidden", async () => {
    await sayingSeen(false);
    try {
      await invoke("window_close", {});
      await $('[data-testid="close-agents"]').waitForExist({ timeout: 10_000 });
      expect(await visible()).toBe(true);
      await browser.waitUntil(async () => !(await visible()), {
        timeout: 15_000,
        timeoutMsg: "the window never hid without its question shown",
      });
    } finally {
      await sayingSeen(true);
    }
    // The agent waiting for its approval still runs.
    await waitForStatus(agents[1] ?? "", "waiting");
    await invoke("app_ready", {});
    await browser.waitUntil(() => visible(), { timeout: 10_000 });
    await $('[role="dialog"] button[aria-label="Close"]').click();
    await $('[data-testid="close-agents"]').waitForExist({ reverse: true, timeout: 5_000 });
  });

  it("F-SH-14.5 with Don't ask again, the next close keeps them running without asking", async () => {
    // Both are running again: the second waits for its approval.
    await sendToPane(agents[0] ?? "", "/ask\r");
    await waitForStatus(agents[0] ?? "", "waiting");
    await invoke("window_close", {});
    await $('[data-testid="close-agents"]').waitForExist({ timeout: 10_000 });
    await $('//label[normalize-space()="Don\'t ask again"]').click();
    await pressHiding('[data-testid="close-keep"]');
    await browser.waitUntil(async () => !(await visible()), { timeout: 10_000 });
    const saved = await invoke<Record<string, unknown>>("settings_get_all", {});
    expect(saved.keep_agents_on_close).toBe("keep");

    await invoke("app_ready", {});
    await browser.waitUntil(() => visible(), { timeout: 10_000 });
    await invoke("window_close", {});
    await browser.waitUntil(async () => !(await visible()), {
      timeout: 10_000,
      timeoutMsg: "the window didn't hide on the remembered choice",
    });
    expect(await $('[data-testid="close-agents"]').isExisting()).toBe(false);
  });
});
