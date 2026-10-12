import { $, browser } from "@wdio/globals";
import { freshOffline, invoke, paneIds, paneText, pickTile, sendToPane, waitForPaneText } from "../support/app";
import {
  PROJECT,
  copyIn,
  dropConnection,
  ensureConnected,
  hostRows,
  inContainer,
  openRemoteFolder,
  resetProject,
} from "../support/remote";

// E2E-M14-12 Task tools for an agent terminal on a host (F-RM-01, F-TK-02,
// D-1606): the host's (fake) claude in a terminal pane gets the tasks
// server on its command line as `tandem-host mcp --channel <id>`, nothing
// is written into the host's own config, and a task it saves lands in this
// machine's tracker. The connection drops and comes back: the engine's
// server is the same process and still reaches the tracker.

async function titles(): Promise<string[]> {
  const page = await invoke<{ items: Array<{ title: string }> }>("tasks_list", { filter: {}, cursor: null });
  return page.items.map((i) => i.title);
}

/** The server pid the engine printed for its last `/tasks` call. */
async function lastServerPid(pane: string): Promise<string> {
  const all = [...(await paneText(pane)).matchAll(/tasks\[(\d+)\]/g)];
  return all[all.length - 1]?.[1] ?? "";
}

describe("E2E-M14-12 task tools for an agent terminal on a host", () => {
  let agent = "";

  before(async function () {
    this.timeout(240_000);
    copyIn(process.env.TANDEM_FAKE_ENGINE_BIN ?? "", "/usr/local/bin/claude");
    inContainer("chmod 755 /usr/local/bin/claude");
    inContainer("rm -f /home/tandem/.claude.json");
    resetProject();
    await freshOffline();
    await ensureConnected();
    await invoke("engines_detect", { hostId: (await hostRows())[0]?.id ?? "" });
    const shell = await openRemoteFolder(PROJECT);
    await waitForPaneText(shell, "tandem@", 30_000);
    const before = await paneIds();
    // The picker a split opens (Mod+D) in the remote workspace.
    await browser.execute(() => {
      const mac = navigator.platform.toLowerCase().includes("mac");
      (document.activeElement as HTMLElement | null)?.blur();
      window.dispatchEvent(
        new KeyboardEvent("keydown", { code: "KeyD", key: "d", ctrlKey: !mac, metaKey: mac, bubbles: true, cancelable: true }),
      );
    });
    await $('[data-testid="launch-grid"] [data-testid="tile-claude"]').waitForClickable({ timeout: 15_000 });
    await pickTile("claude");
    await browser.waitUntil(async () => (await paneIds()).some((p) => !before.includes(p)), {
      timeout: 15_000,
      timeoutMsg: "no agent pane",
    });
    agent = (await paneIds()).find((p) => !before.includes(p)) ?? "";
    await waitForPaneText(agent, "test engine", 30_000).catch(async (e: Error) => {
      const all = await Promise.all((await paneIds()).map(async (p) => `${p}: ${JSON.stringify(await paneText(p))}`));
      throw new Error(`${e.message}\n${all.join("\n")}`);
    });
  });

  after(() => resetProject());

  it("F-RM-01 F-TK-02 the host's agent gets the tasks server inline and saves a task here", async function () {
    this.timeout(120_000);
    const log = await invoke<Array<{ argv: string[] }>>("test_spawn_log");
    const launch = log.map((r) => r.argv.join(" ")).find((a) => a.includes("/usr/local/bin/claude")) ?? "";
    expect(launch).toContain("--mcp-config");
    expect(launch).toContain("tandem-host");
    expect(launch).toContain("--channel");
    await sendToPane(agent, '/tasks save_issue {"title":"From a host terminal"}');
    await sendToPane(agent, "\r");
    await waitForPaneText(agent, "tasks[", 60_000);
    expect(await paneText(agent)).not.toContain("tasks error");
    await browser.waitUntil(async () => (await titles()).includes("From a host terminal"), {
      timeout: 15_000,
      timeoutMsg: `not in the tracker: ${await paneText(agent)}`,
    });
    // Nothing was written into the host's own config or folder.
    expect(inContainer("cat /home/tandem/.claude.json 2>/dev/null | grep -c tandem_tasks || true").trim()).toBe("0");
    expect(inContainer("grep -rl TANDEM_TASKS_CONNECTION /home/tandem 2>/dev/null || true").trim()).toBe("");
  });

  it("F-RM-01.5 F-TK-02 after a drop the same server still reaches the tracker", async function () {
    this.timeout(180_000);
    const pid = await lastServerPid(agent);
    expect(pid).not.toBe("");
    dropConnection();
    await waitForPaneText(agent, "Reconnecting…", 30_000);
    await waitForPaneText(agent, "Reconnected", 60_000);
    await sendToPane(agent, '/tasks save_issue {"title":"After the drop"}');
    await sendToPane(agent, "\r");
    await browser.waitUntil(async () => (await titles()).includes("After the drop"), {
      timeout: 60_000,
      timeoutMsg: `not in the tracker after the drop: ${await paneText(agent)}`,
    });
    expect(await lastServerPid(agent)).toBe(pid);
    expect(await paneText(agent)).not.toContain("tasks error");
  });
});
