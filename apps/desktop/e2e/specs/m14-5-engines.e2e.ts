import { browser } from "@wdio/globals";
import { writeFileSync } from "node:fs";
import { freshOffline, invoke, settled, waitForPaneText } from "../support/app";
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

// E2E-M14-5 Remote engines (F-RM-01, F-EN-01 per host): a fake `claude` on
// the host's PATH. The remote workspace's + menu lists it (as being on the
// host), and a thread in that workspace runs it through tandem-host: what
// the engine writes lands in the host's folder.

describe("E2E-M14-5 remote engines", () => {
  let hostId = "";
  let workspaceId = "";

  before(async function () {
    this.timeout(240_000);
    copyIn(process.env.TANDEM_FAKE_ENGINE_BIN ?? "", "/usr/local/bin/claude");
    inContainer("chmod 755 /usr/local/bin/claude");
    resetProject();
    await freshOffline();
    await ensureConnected();
    hostId = (await hostRows())[0]?.id ?? "";
    // Detection runs on connect; ask again now that claude is there.
    await invoke("engines_detect", { hostId });
    const shell = await openRemoteFolder(PROJECT);
    await waitForPaneText(shell, "tandem@", 30_000);
    workspaceId =
      (await invoke<Array<{ id: string; root: string }>>("workspace_list")).find((w) => w.root === PROJECT)
        ?.id ?? "";
  });

  it("F-EN-01 the + menu of a remote workspace lists the host's engines", async () => {
    await browser.waitUntil(
      async () => {
        const list = await invoke<Array<{ spec: { id: string }; install: { path: string } | null }>>(
          "engines_list",
          { hostId },
        );
        return list.some((e) => e.spec.id === "claude" && e.install?.path === "/usr/local/bin/claude");
      },
      { timeout: 30_000, timeoutMsg: "the host's claude wasn't detected" },
    );
    // The picker a split opens (Mod+D) in the remote workspace, as split() does.
    await browser.execute(() => {
      const mac = navigator.platform.toLowerCase().includes("mac");
      (document.activeElement as HTMLElement | null)?.blur();
      window.dispatchEvent(
        new KeyboardEvent("keydown", {
          code: "KeyD",
          key: "d",
          ctrlKey: !mac,
          metaKey: mac,
          bubbles: true,
          cancelable: true,
        }),
      );
    });
    const tile = $('[data-testid="launch-grid"] [data-testid="tile-claude"]');
    await tile.waitForClickable({ timeout: 15_000 });
    expect(await $('[data-testid="launch-grid"]').getAttribute("data-host")).toBe(hostId);
    // getText() is "" for parts WebDriver thinks hidden: read textContent.
    const shown = await browser.execute(
      () =>
        document.querySelector('[data-testid="launch-grid"] [data-testid="tile-claude"]')?.textContent ?? "",
    );
    expect(shown).toContain("On tandem@127.0.0.1:2222");
    // An agent pane on the host runs the host's claude.
    await settled();
    await tile.click();
    await browser.waitUntil(
      async () => {
        const log = await invoke<Array<{ argv: string[]; providerHome: string | null }>>("test_spawn_log");
        return log.some((r) => r.argv.join(" ").includes("/usr/local/bin/claude") && r.providerHome === null);
      },
      { timeout: 30_000, timeoutMsg: "no remote agent spawn" },
    );
    await $('[data-testid="pane-picker"]').waitForExist({ reverse: true, timeout: 15_000 });
  });

  it("F-RM-01 a thread in the remote workspace runs through tandem-host", async function () {
    this.timeout(240_000);
    await threadMode();
    const thread = await invoke<{ id: string; engineId: string; cwd: string }>("thread_create", {
      args: { kind: "thread", workspaceId },
    });
    expect(thread.engineId).toBe("claude");
    expect(thread.cwd).toBe(PROJECT);
    const row = $(`[data-testid="thread-row"][data-thread-id="${thread.id}"]`);
    await row.waitForExist({ timeout: 10_000 });
    await row.click();
    await $('[data-testid="thread-main"]').waitForExist({ timeout: 10_000 });
    await sendMessage("hello from far away");
    await waitForAssistant("You said: hello from far away", 60_000);
    await sendMessage("write remote-proof.txt=written on the host");
    await browser.waitUntil(
      () =>
        inContainer("cat /home/tandem/project/remote-proof.txt 2>/dev/null || true").includes(
          "written on the host",
        ),
      {
        timeout: 60_000,
        timeoutMsg: "the engine didn't run in the host's folder",
      },
    );
    // A local file attached to a remote thread is stored under the remote
    // workspace, and the engine is pointed at that copy.
    const local = `/tmp/m14-attach-${Date.now()}.txt`;
    writeFileSync(local, "local notes\n");
    const [chip] = await invoke<Array<{ id: string }>>("thread_attach_paths", {
      threadId: thread.id,
      paths: [local],
    });
    await invoke("thread_send", { threadId: thread.id, text: "see attached", attachmentIds: [chip?.id] });
    await waitForAssistant("You said: see attached @.tandem/attachments/", 60_000);
    expect(inContainer(`cat /home/tandem/project/.tandem/attachments/${thread.id}/*`)).toBe("local notes\n");
    // No gateway reaches the host, so Claude asks in band
    // (`--permission-prompt-tool stdio`): the same approval card, and Allow
    // reaches the engine there.
    await sendMessage("run echo remote-approved");
    const card = '[data-testid="thread-pending"]';
    await $(card).waitForExist({ timeout: 30_000, timeoutMsg: "no approval card for the remote thread" });
    expect(await $(card).getText()).toContain("echo remote-approved");
    await $(`${card} [data-testid="approval-allow"]`).click();
    await waitForAssistant("Ran echo remote-approved.", 60_000);
    // Nothing of this machine's engine homes reached the host.
    expect(inContainer("grep -rl 'tandem/providers' /home/tandem/project 2>/dev/null || true").trim()).toBe(
      "",
    );
  });
});
