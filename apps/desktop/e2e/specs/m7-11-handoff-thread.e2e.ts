import { $, browser } from "@wdio/globals";
import { mkdirSync, rmSync } from "node:fs";
import { join } from "node:path";
import {
  addFolder,
  checkAgain,
  fakeEngine,
  freshOffline,
  home,
  invoke,
  paneIds,
  split,
  waitForPaneText,
} from "../support/app";
import { userTexts, waitForAssistant } from "../support/thread";

type Thread = { id: string; workspaceId: string | null; engineId: string };

async function pick(label: string): Promise<void> {
  await browser.waitUntil(
    async () =>
      browser.execute((l: string) => {
        const item = Array.from(document.querySelectorAll<HTMLElement>("[role=menuitem]")).find(
          (el) => el.textContent?.trim() === l,
        );
        item?.click();
        return item !== undefined;
      }, label),
    { timeout: 5_000, timeoutMsg: `no menu item ${label}` },
  );
}

const summary = async () => String(await $('[data-testid="handoff-summary"]').getProperty("value"));

// E2E-M7-11 Handoff to a Thread (F-CD-16 chat target): a pane hands off to
// Codex as a Thread, which opens in the same workspace with the brief.
describe("E2E-M7-11 handoff to a thread", () => {
  const root = `/tmp/tandem-e2e-m7-11-${Date.now()}`;

  before(() => {
    mkdirSync(join(home(), "fakebin"), { recursive: true });
    fakeEngine("codex", join(home(), "fakebin"));
  });
  after(async () => {
    rmSync(join(home(), "fakebin", "codex"), { force: true });
    await checkAgain();
  });

  it("F-CD-16 pane → Handoff → Codex as a Thread opens it in the same workspace with the brief", async function () {
    this.timeout(150_000);
    mkdirSync(root, { recursive: true });
    await freshOffline();
    await addFolder(root);
    await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 20_000 });
    await checkAgain();
    const claude = await split("right", "claude");
    await waitForPaneText(claude, "Fake Claude Code");

    await $(`[data-testid="pane"][data-pane-id="${claude}"] button[aria-label^="Actions for "]`).click();
    await pick("Hand off…");
    await $('[data-testid="handoff-dialog"]').waitForExist({ timeout: 10_000 });
    await browser.waitUntil(async () => (await summary()).includes("## Next steps"), {
      timeout: 60_000,
      timeoutMsg: `summary: ${await summary()}`,
    });
    const brief = await summary();

    const where = await $('//label[normalize-space()="Where"]').getAttribute("for");
    const select = $(`[id="${where}"]`);
    await select.waitForClickable({ timeout: 10_000 });
    await select.click();
    const asThread = $('//*[@role="option"][normalize-space()="New thread in this folder"]');
    await asThread.waitForClickable({ timeout: 10_000 });
    await asThread.click();
    await $('[data-testid="handoff-start"]').click();
    await $('[data-testid="handoff-dialog"]').waitForExist({ reverse: true, timeout: 20_000 });

    // Thread mode, with the new Codex thread open.
    const main = $('[data-testid="thread-main"]');
    await main.waitForExist({ timeout: 15_000 });
    const threadId = String(await main.getAttribute("data-thread-id"));
    const thread = (await invoke<{ thread: Thread }>("thread_get", { threadId })).thread;
    expect(thread.engineId).toBe("codex");
    const workspaces = await invoke<Array<{ id: string; root: string }>>("workspace_list");
    expect(thread.workspaceId).toBe(workspaces.find((w) => w.root === root)?.id);
    // The brief is its first message, and Codex answers it.
    await waitForAssistant("You said: ");
    expect((await userTexts()).join("\n")).toContain("## Next steps");
    expect((await userTexts())[0]?.trim()).toBe(brief.trim());
  });
});
