import { $, browser } from "@wdio/globals";
import {
  addFolder,
  counters,
  freshOffline,
  hookLog,
  paneIds,
  paneStatus,
  sendToPane,
  split,
  waitForPaneText,
  waitForStatus,
} from "../support/app";

// E2E-M4-3 Status lifecycle (F-EN-05, F-SH-03): the fake Claude runs the
// hooks Tandem wrote into its provider home.
describe("E2E-M4-3 status lifecycle", () => {
  it("F-EN-05.2 UserPromptSubmit → Notification → Stop reads working → waiting → done in the header and the rail", async () => {
    await freshOffline();
    await addFolder(`/tmp/tandem-e2e-m4-3-${Date.now()}`);
    await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 20_000 });
    const [shell] = await paneIds();
    const agent = await split("right", "claude");
    await waitForPaneText(agent, "Fake Claude Code");
    // The script: UserPromptSubmit, 2 s, Notification (permission), 2 s, Stop.
    await sendToPane(agent, "/script\r");
    await waitForStatus(agent, "working");
    const slot = '[data-testid="workspace-row"] [data-testid="workspace-status"]';
    await browser.waitUntil(
      async () =>
        (await $(slot)
          .getText()
          .catch(() => "")) === "WORKING 1" ||
        (await $(slot)
          .getText()
          .catch(() => "")) === "Working 1",
      {
        timeout: 5_000,
        timeoutMsg: `rail: ${await $(slot)
          .getText()
          .catch(() => "")}`,
      },
    );
    await waitForStatus(agent, "waiting");
    expect((await $(slot).getText()).toLowerCase()).toBe("waiting on you");
    // Look away before it finishes: it ends unread.
    await $(`[data-testid="pane"][data-pane-id="${shell}"] [data-testid="terminal"]`).click();
    await waitForStatus(agent, "done", 10_000);
    await $('[data-testid="workspace-unread"]').waitForExist({ timeout: 5_000 });
  });

  it("F-EN-05.3 resizing the finished pane 20 times leaves its status alone", async () => {
    const [, agent] = await paneIds();
    const before = await paneStatus(agent ?? "");
    await counters(true);
    const { width, height } = await browser.getWindowSize();
    for (let i = 0; i < 20; i++) await browser.setWindowSize(width - (i % 2 ? 0 : 40), height);
    // A window resize settles into one PTY resize once the layout is quiet
    // (F-CD-02 AC1), so a burst that ends where it began can send none: end
    // narrower and wait for that size to reach a PTY.
    await browser.setWindowSize(width - 40, height);
    await browser.waitUntil(async () => (await counters()).ptyResizes > 0, {
      timeout: 5_000,
      timeoutMsg: "no resize reached a PTY",
    });
    await browser.setWindowSize(width, height);
    await browser.pause(1_500);
    expect(await paneStatus(agent ?? "")).toBe(before);
    expect(before).not.toBe("working");
  });

  it("F-EN-05.4 what tandem-hook posted has no transcript or prompt text", async () => {
    const posts = hookLog();
    expect(posts.length).toBeGreaterThanOrEqual(3);
    const events = posts.map((p) => (JSON.parse(p) as { event: string }).event);
    expect(events).toEqual(expect.arrayContaining(["working", "waiting", "done"]));
    for (const p of posts) expect(p).not.toMatch(/SECRET/);
  });
});
