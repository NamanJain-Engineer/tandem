import { $, browser } from "@wdio/globals";
import { addFolder, freshOffline, home, invoke, paneIds } from "../support/app";
import { sendAndWait } from "../support/agents";
import {
  browserSignIn,
  connectionOf,
  filesContaining,
  mockReset,
  mockState,
  openPlugins,
} from "../support/plugins";

// E2E-M12-1 OAuth (F-PL-03 AC1, F-PL-04): GitHub through the mock
// authorization server, from the card: discovery, registration, the browser
// sign-in, the token exchange and the check. The card says Connected, the
// token reaches the upstream, and it exists only in the (mock) keychain.
describe("E2E-M12-1 OAuth", () => {
  before(async function () {
    this.timeout(60_000);
    await mockReset();
    browserSignIn("follow");
    await freshOffline();
    await addFolder(`/tmp/tandem-e2e-m12-1-${Date.now()}`);
    await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 20_000 });
  });

  it("F-PL-03.1 the browser sign-in connects GitHub and the card shows Connected", async function () {
    this.timeout(90_000);
    await openPlugins();
    await $('[data-testid="plugin-connect-github"]').click();
    await $('[data-testid="connect-sheet"]').waitForDisplayed({ timeout: 10_000 });
    await $('[data-testid="connect-start"]').click();
    const done = $('[data-testid="connect-done"]');
    await done.waitForDisplayed({ timeout: 30_000 });
    expect(await done.getText()).toContain("Connected as octo-shop");
    // Every step finished before success was shown.
    await $('[data-testid="connect-finish"]').click();
    const card = $('[data-testid="plugin-card-github"]');
    await browser.waitUntil(async () => (await card.getAttribute("data-state")) === "connected", {
      timeout: 10_000,
      timeoutMsg: "the GitHub card never showed Connected",
    });
    expect(await card.getProperty("textContent")).toContain("octo-shop");
    const state = await mockState();
    expect(state.clients).toBe(1);
    expect(state.access).toHaveLength(1);
  });

  it("F-PL-04 the token works for a granted thread and exists only in the keychain", async function () {
    this.timeout(90_000);
    const conn = await connectionOf("github");
    expect(conn?.state).toBe("connected");
    const [ws] = await invoke<Array<{ id: string }>>("workspace_list");
    const thread = await invoke<{ id: string }>("thread_create", {
      args: { kind: "thread", workspaceId: ws?.id, engineId: "claude" },
    });
    await invoke("grants_set", {
      subject: { type: "thread", threadId: thread.id },
      connectionId: conn?.connectionId,
      granted: true,
    });
    const reply = await sendAndWait(
      thread.id,
      'mcp tandem__call {"tool":"github__get_me","args":{}}',
      60_000,
    );
    expect(reply.join(" ")).toMatch(
      /The tool said:[\s\S]*Untrusted content from the GitHub plugin[\s\S]*octo-shop/,
    );
    const [token] = (await mockState()).access;
    expect(token).toMatch(/^gho_access_/);
    // Not in the data folder, its logs, the database or the transcript.
    expect(filesContaining(home(), token ?? "")).toEqual([]);
  });
});
