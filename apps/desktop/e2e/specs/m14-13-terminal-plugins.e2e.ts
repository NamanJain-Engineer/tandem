import { $, browser } from "@wdio/globals";
import {
  freshOffline,
  invoke,
  paneIds,
  paneText,
  pickTile,
  sendToPane,
  waitForPaneText,
} from "../support/app";
import { connectTestPlugin, gatewayMetrics } from "../support/agents";
import {
  PROJECT,
  copyIn,
  ensureConnected,
  hostRows,
  inContainer,
  openRemoteFolder,
  resetProject,
} from "../support/remote";

// E2E-M14-13 Plugins for an agent terminal on a host (F-PL-01, F-PL-05,
// D-2565): a plugin granted to the host's folder reaches the host's (fake)
// claude in a terminal pane through the relay, `tandem-host mcp --channel
// <id>` in the same config as the tasks server, so no lease crosses; the
// pane's lease goes when it closes.

/** What `/mcp` printed after `mark`, once it has. */
async function mcpAfter(pane: string, mark: string): Promise<string> {
  let said = "";
  await browser.waitUntil(
    async () => {
      const text = await paneText(pane);
      const at = text.lastIndexOf(mark);
      said = at >= 0 ? text.slice(at + mark.length) : "";
      return /mcp( said| failed)?:/.test(said);
    },
    { timeout: 60_000, timeoutMsg: `no answer after ${mark}` },
  );
  return said;
}

describe("E2E-M14-13 plugins for an agent terminal on a host", () => {
  let agent = "";
  let echo = "";

  before(async function () {
    this.timeout(240_000);
    copyIn(process.env.TANDEM_FAKE_ENGINE_BIN ?? "", "/usr/local/bin/claude");
    inContainer("chmod 755 /usr/local/bin/claude");
    inContainer("rm -f /home/tandem/.claude.json");
    resetProject();
    await freshOffline();
    echo = await connectTestPlugin("test_echo");
    await ensureConnected();
    await invoke("engines_detect", { hostId: (await hostRows())[0]?.id ?? "" });
    const shell = await openRemoteFolder(PROJECT);
    await waitForPaneText(shell, "tandem@", 30_000);
    const remote = (await invoke<Array<{ id: string; kind: string }>>("workspace_list")).find(
      (w) => w.kind === "ssh",
    );
    await invoke("grants_set", {
      subject: { type: "workspace", workspaceId: remote?.id },
      connectionId: echo,
      granted: true,
    });
    const before = await paneIds();
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
    await $('[data-testid="launch-grid"] [data-testid="tile-claude"]').waitForClickable({ timeout: 15_000 });
    await pickTile("claude");
    await browser.waitUntil(async () => (await paneIds()).some((p) => !before.includes(p)), {
      timeout: 15_000,
      timeoutMsg: "no agent pane",
    });
    agent = (await paneIds()).find((p) => !before.includes(p)) ?? "";
    await waitForPaneText(agent, "test engine", 30_000);
  });

  after(() => resetProject());

  it("F-PL-01.5 the host's agent gets the gateway relayed beside the tasks server, and calls the plugin", async function () {
    this.timeout(120_000);
    const log = await invoke<Array<{ argv: string[] }>>("test_spawn_log");
    const launch = log.map((r) => r.argv.join(" ")).find((a) => a.includes("/usr/local/bin/claude")) ?? "";
    // One config, both servers, each a channel: no lease on the host.
    expect(launch.split("--mcp-config").length - 1).toBe(1);
    expect(launch).toMatch(/\\?"tandem\\?":/);
    expect(launch).toMatch(/\\?"tandem_tasks\\?":/);
    expect(launch).not.toContain("Bearer");
    await sendToPane(agent, '/mcp tandem__call {"tool":"test_echo__echo","args":{"text":"from-the-host"}}');
    await sendToPane(agent, "\r");
    expect(await mcpAfter(agent, "test_echo__echo")).toMatch(
      /mcp said:[\s\S]*Untrusted content from the Test Echo plugin[\s\S]*from-the-host/,
    );
  });

  it("F-PL-01.6 closing the pane revokes its lease", async function () {
    this.timeout(60_000);
    const before = (await gatewayMetrics()).metrics.leases;
    await invoke("pane_close", { paneId: agent });
    await browser.waitUntil(async () => (await gatewayMetrics()).metrics.leases === before - 1, {
      timeout: 15_000,
      timeoutMsg: "the pane's lease outlived it",
    });
  });
});
