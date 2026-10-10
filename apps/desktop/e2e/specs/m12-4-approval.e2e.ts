import { $, $$, browser } from "@wdio/globals";
import { addFolder, freshOffline, invoke, paneIds } from "../support/app";
import { connectTestPlugin } from "../support/agents";
import { sendMessage, threadMode, waitForAssistant } from "../support/thread";
import { connectGithub, mockCalls, mockReset, mockState } from "../support/plugins";

const pinned = '[data-testid="thread-pending"]';
const card = '[data-testid="plugin-approval"]';

/** The live plugin card for a `risk` call; cards already answered show as
 * one resolved line, so it is the only one. */
async function waitForCard(risk: string): Promise<string> {
  await browser.waitUntil(
    async () => {
      for (const el of await $$(card)) if ((await el.getAttribute("data-risk")) === risk) return true;
      return false;
    },
    { timeout: 30_000, timeoutMsg: `no ${risk} card` },
  );
  await $(pinned).waitForExist({ timeout: 10_000 });
  expect(await $$(card).length).toBe(1);
  return (await $(card).getProperty("textContent")) as string;
}

async function cardGone(): Promise<void> {
  await $(pinned).waitForExist({ reverse: true, timeout: 20_000 });
}

// E2E-M12-4 Interactive approval (F-PL-06, F-PL-07): a thread at Edits that
// calls a write gets the plugin card with every field, and Approve runs it
// exactly once; a destructive tool never offers "for the rest of this chat";
// a spend whose price moved after approval is refused.
describe("E2E-M12-4 interactive approval", () => {
  let threadId = "";

  before(async function () {
    this.timeout(90_000);
    await mockReset();
    await freshOffline();
    await addFolder(`/tmp/tandem-e2e-m12-4-${Date.now()}`);
    await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 20_000 });
    const github = await connectGithub();
    const echo = await connectTestPlugin("test_echo");
    await threadMode();
    const [ws] = await invoke<Array<{ id: string }>>("workspace_list");
    const thread = await invoke<{ id: string }>("thread_create", {
      args: { kind: "thread", workspaceId: ws?.id, engineId: "claude" },
    });
    threadId = thread.id;
    for (const c of [github, echo])
      await invoke("grants_set", { subject: { type: "thread", threadId }, connectionId: c, granted: true });
    const row = $(`[data-testid="thread-row"][data-thread-id="${threadId}"]`);
    await row.waitForExist({ timeout: 10_000 });
    await row.click();
    await $('[data-testid="thread-main"]').waitForExist({ timeout: 10_000 });
  });

  it("F-PL-07.1 a write shows every field, masks nothing it shouldn't, and Approve runs it once", async function () {
    this.timeout(90_000);
    await sendMessage(
      'mcp tandem__call {"tool":"github__create_issue","args":{"owner":"octo-shop","repo":"web","title":"Fix login"}}',
    );
    const text = await waitForCard("write");
    for (const field of [
      "GitHub",
      "Write",
      "Action",
      "Account",
      "octo-shop",
      "Target",
      "octo-shop/web",
      "Cost",
      "None",
    ])
      expect(text).toContain(field);
    await $(card).$("button=Full request").click();
    const request = await $('[data-testid="plugin-approval-request"]').getText();
    expect(request).toContain("Fix login");
    const [token] = (await mockState()).access;
    expect(request).not.toContain(token ?? "gho_access_");
    // Writes may be allowed for the rest of the chat.
    expect(await $(`${pinned} [data-testid="approval-allow-thread"]`).isExisting()).toBe(true);
    expect(await mockCalls("create_issue")).toBe(0);
    await $(`${pinned} [data-testid="approval-allow"]`).click();
    await waitForAssistant("The tool said:");
    await cardGone();
    expect(await mockCalls("create_issue")).toBe(1);
  });

  it("F-PL-06.3 a destructive tool offers no Allow for the rest of this chat", async function () {
    this.timeout(90_000);
    await sendMessage(
      'mcp tandem__call {"tool":"github__delete_file","args":{"owner":"octo-shop","repo":"web","path":"README.md"}}',
    );
    await waitForCard("destructive");
    expect(await $(`${pinned} [data-testid="approval-allow-thread"]`).isExisting()).toBe(false);
    await $(`${pinned} [data-testid="approval-deny"]`).click();
    await cardGone();
    expect(await mockCalls("delete_file")).toBe(0);
  });

  it("F-PL-06.2 a spend whose amount changed after approval is refused", async function () {
    this.timeout(90_000);
    await invoke("test_plugin_price", { cents: 500 });
    await sendMessage(
      'mcp tandem__call {"tool":"test_echo__charge","args":{"text":"ads","amount":500,"currency":"usd"}}',
    );
    await waitForCard("spend");
    expect(await $('[data-testid="plugin-approval-cost"]').getProperty("textContent")).toBe("5.00 USD");
    expect(await $(`${pinned} [data-testid="approval-allow-thread"]`).isExisting()).toBe(false);
    // The vendor's price moves while the card is up.
    await invoke("test_plugin_price", { cents: 700 });
    await $(`${pinned} [data-testid="approval-allow"]`).click();
    await waitForAssistant("The amount changed from 5.00 USD to 7.00 USD");
    await invoke("test_plugin_price", { cents: 500 });
  });
});
