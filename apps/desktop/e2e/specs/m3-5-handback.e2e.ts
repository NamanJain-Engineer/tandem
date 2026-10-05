import { $, browser } from "@wdio/globals";
import { addFolder, freshOffline, paneIds, paneText, sendToPane, split, textOf, waitForPaneText } from "../support/app";

// E2E-M3-5 Handback (F-CD-03 AC3): the test engine on PATH stands in for `claude`.
describe("E2E-M3-5 handback", () => {
  it("F-CD-03.3 quitting the agent leaves a shell in the same folder and a Saved session", async () => {
    const root = `/tmp/tandem-e2e-m3-5-${Date.now()}`;
    await freshOffline();
    await addFolder(root);
    await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 20_000 });
    const agent = await split("right", "claude");
    await waitForPaneText(agent, "Fake Claude Code");
    expect(await paneText(agent)).toContain(`cwd: ${root}`);
    // In-page text: WebKitWebDriver's getText drops truncated text.
    const title = `[data-testid="pane"][data-pane-id="${agent}"] [data-testid="pane-title"]`;
    await browser.waitUntil(async () => (await textOf(title)) === "Claude Code · 1", {
      timeout: 5_000,
      timeoutMsg: `title: ${await textOf(title)}`,
    });
    await sendToPane(agent, "/exit\r");
    await waitForPaneText(agent, "Goodbye from the test engine");
    await sendToPane(agent, "echo back-$((2+3)) in $(pwd)\n");
    await waitForPaneText(agent, `back-5 in ${root}`);
    await $('[data-testid="saved-sessions"]').waitForExist({ timeout: 10_000 });
    expect(await $('[data-testid="saved-sessions"]').getText()).toBe("Saved sessions (1)");
  });
});
