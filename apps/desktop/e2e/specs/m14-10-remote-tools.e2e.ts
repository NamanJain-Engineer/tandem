import { $, $$, browser } from "@wdio/globals";
import { clickAllow, freshOffline, invoke, waitForPaneText } from "../support/app";
import { connectGithub, mockCalls, mockReset, mockState } from "../support/plugins";
import { sendMessage, threadMode, waitForAssistant } from "../support/thread";
import {
  PROJECT,
  copyIn,
  ensureConnected,
  hostRows,
  inContainer,
  openRemoteFolder,
  resetProject,
} from "../support/remote";

// E2E-M14-10 Plugins and task tools in a remote thread (F-RM-01, F-PL-07,
// F-TK-02, D-1604): a thread on the fixture runs the host's (fake) claude,
// whose `tandem` and `tandem_tasks` servers are `tandem-host mcp` relays.
// A plugin write called there shows the plugin card here and runs once on
// Approve; a task saved there lands in this machine's tracker. Neither the
// gateway's lease nor the plugin's token is anywhere on the host.

const pinned = '[data-testid="thread-pending"]';
const card = '[data-testid="plugin-approval"]';

describe("E2E-M14-10 plugins and task tools in a remote thread", () => {
  let threadId = "";

  before(async function () {
    this.timeout(240_000);
    await mockReset();
    copyIn(process.env.TANDEM_FAKE_ENGINE_BIN ?? "", "/usr/local/bin/claude");
    inContainer("chmod 755 /usr/local/bin/claude");
    resetProject();
    await freshOffline();
    const github = await connectGithub();
    await ensureConnected();
    const hostId = (await hostRows())[0]?.id ?? "";
    await invoke("engines_detect", { hostId });
    const shell = await openRemoteFolder(PROJECT);
    await waitForPaneText(shell, "tandem@", 30_000);
    const ws = (await invoke<Array<{ id: string; root: string }>>("workspace_list")).find(
      (w) => w.root === PROJECT,
    );
    await threadMode();
    const thread = await invoke<{ id: string; cwd: string }>("thread_create", {
      args: { kind: "thread", workspaceId: ws?.id, engineId: "claude" },
    });
    expect(thread.cwd).toBe(PROJECT);
    threadId = thread.id;
    await invoke("grants_set", {
      subject: { type: "thread", threadId },
      connectionId: github,
      granted: true,
    });
    const row = $(`[data-testid="thread-row"][data-thread-id="${threadId}"]`);
    await row.waitForExist({ timeout: 10_000 });
    await row.click();
    await $('[data-testid="thread-main"]').waitForExist({ timeout: 10_000 });
  });

  it("F-RM-01 F-PL-07 a plugin write from the host asks here and runs once", async function () {
    this.timeout(120_000);
    await sendMessage(
      'mcp tandem__call {"tool":"github__create_issue","args":{"owner":"octo-shop","repo":"web","title":"From the host"}}',
    );
    await browser.waitUntil(
      async () => {
        for (const el of await $$(card)) if ((await el.getAttribute("data-risk")) === "write") return true;
        return false;
      },
      { timeout: 60_000, timeoutMsg: "no plugin card for the remote thread's write" },
    );
    expect(await mockCalls("create_issue")).toBe(0);
    await clickAllow(`${pinned} [data-testid="approval-allow"]`);
    await waitForAssistant("The tool said:", 60_000);
    expect(await mockCalls("create_issue")).toBe(1);
  });

  it("F-RM-01 F-TK-02 a task saved on the host lands in this machine's tracker", async function () {
    this.timeout(120_000);
    await sendMessage('tasks save_issue {"title":"Saved from the host"}');
    await waitForAssistant("Saved the task.", 60_000);
    const page = await invoke<{ items: Array<{ title: string }> }>("tasks_list", {
      filter: {},
      cursor: null,
    });
    expect(page.items.map((i) => i.title)).toContain("Saved from the host");
    // The tracker stays here: nothing of it is on the host.
    expect(inContainer("ls -a /home/tandem/project /home/tandem | grep -ci tasks || true").trim()).toBe("0");
  });

  it("F-RM-01 no lease, plugin token or task file reached the host", async () => {
    const [token] = (await mockState()).access;
    const leaked = inContainer(
      `grep -rIl -e 'Bearer ' ${token ? `-e '${token}'` : ""} /home/tandem /tmp 2>/dev/null || true`,
    ).trim();
    expect(leaked).toBe("");
  });
});
