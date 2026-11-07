import { $, browser } from "@wdio/globals";
import { existsSync, mkdirSync, readFileSync, rmSync } from "node:fs";
import { join } from "node:path";
import {
  addFolder,
  checkAgain,
  clickAllow,
  fakeEngine,
  freshOffline,
  home,
  invoke,
  paneIds,
  paneText,
  sendToPane,
  split,
  waitFor,
  waitForPaneText,
} from "../support/app";
import { connectTestPlugin, gatewayPort, type SpawnLine } from "../support/agents";
import { pick, repoMenu } from "../support/worktrees";

/** The lines `/mcp` printed after `mark`, once one has come. */
async function mcpAfter(pane: string, mark: string): Promise<string> {
  let said = "";
  await browser.waitUntil(
    async () => {
      const text = await paneText(pane);
      const at = text.lastIndexOf(mark);
      said = at >= 0 ? text.slice(at + mark.length) : "";
      return /mcp( said| failed| tools)?:/.test(said);
    },
    { timeout: 30_000, timeoutMsg: `no answer after ${mark}` },
  );
  return said;
}

/** A request with `token` to the gateway: its status. */
async function gatewayStatus(token: string): Promise<number> {
  const res = await fetch(`http://127.0.0.1:${gatewayPort()}/mcp`, {
    method: "POST",
    headers: {
      authorization: `Bearer ${token}`,
      "content-type": "application/json",
      accept: "application/json, text/event-stream",
    },
    body: JSON.stringify({
      jsonrpc: "2.0",
      id: 1,
      method: "initialize",
      params: { protocolVersion: "2025-03-26", capabilities: {}, clientInfo: { name: "e2e", version: "1" } },
    }),
  });
  return res.status;
}

// E2E-M18-PL01 Plugins for agent terminals (F-PL-01, F-PL-05): a plugin
// granted from the workspace row's menu reaches a Claude Code terminal in
// it through a lease of its own, in `--mcp-config`; a write asks on the
// pane; the lease dies with the engine; Codex gets it in its environment
// only; a workspace with none gives its terminals no gateway.
describe("E2E-M18-PL01 plugins for agent terminals", function () {
  this.timeout(240_000);
  const folder = `/tmp/tandem-e2e-pl01-${Date.now()}`;
  const other = `${folder}-none`;
  let echo = "";
  let claude = "";
  let token = "";

  before(async function () {
    this.timeout(90_000);
    for (const f of [folder, other]) mkdirSync(f, { recursive: true });
    fakeEngine("codex", join(home(), "fakebin"));
    await freshOffline();
    echo = await connectTestPlugin("test_echo");
    await addFolder(other);
    await addFolder(folder);
    await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 20_000 });
    await checkAgain();
  });

  after(async () => {
    rmSync(join(home(), "fakebin", "codex"), { force: true });
    await checkAgain();
  });

  it("F-PL-05.3 the workspace row's menu grants a plugin to the workspace, which has none at first", async () => {
    await repoMenu(folder);
    await pick("Plugins for this workspace…");
    const item = $('[data-testid="workspace-plugin-test_echo"]');
    await item.waitForDisplayed({ timeout: 10_000 });
    const sw = item.$('[role="switch"]');
    expect(await sw.getAttribute("aria-checked")).toBe("false");
    await sw.click();
    await browser.waitUntil(async () => (await sw.getAttribute("aria-checked")) === "true", {
      timeout: 5_000,
    });
    await browser.keys("Escape");
    const ws = (await invoke<Array<{ id: string; root: string }>>("workspace_list")).find(
      (w) => w.root === folder,
    );
    expect(
      await invoke<string[]>("grants_list", { subject: { type: "workspace", workspaceId: ws?.id } }),
    ).toEqual([echo]);
  });

  it("F-PL-01.5 a Claude Code terminal there lists the plugin's tools and calls one, with a lease of its own in --mcp-config", async () => {
    claude = await split("right", "claude");
    await waitForPaneText(claude, "Fake Claude Code");
    await sendToPane(claude, "/mcp\r");
    const tools = await mcpAfter(claude, "/mcp");
    expect(tools).toContain("tandem__search_tools");
    expect(tools).toContain("tandem__call");
    await sendToPane(
      claude,
      '/mcp tandem__call {"tool":"test_echo__echo","args":{"text":"from-the-pane"}}\r',
    );
    const said = await mcpAfter(claude, "test_echo__echo");
    expect(said).toMatch(/mcp said:[\s\S]*Untrusted content from the Test Echo plugin[\s\S]*from-the-pane/);
    // The config holds the lease, and only the user reads it.
    const file = join(home(), "mcp", `${claude}.json`);
    const config = JSON.parse(readFileSync(file, "utf8")) as {
      mcpServers: { tandem: { headers: { Authorization: string } } };
    };
    token = config.mcpServers.tandem.headers.Authorization.replace(/^Bearer /, "");
    expect(token.length).toBeGreaterThan(20);
    const spawns = (await invoke<SpawnLine[]>("test_spawn_log")).filter((r) =>
      r.argv.some((a) => a.includes(`--mcp-config=${file}`)),
    );
    expect(spawns.length).toBe(1);
    expect(spawns[0]?.argv.join(" ")).not.toContain(token);
  });

  it("F-PL-01 a write it asks for shows on its pane, and Allow once lets it through", async () => {
    await sendToPane(claude, '/mcp tandem__call {"tool":"test_echo__note","args":{"text":"kept"}}\r');
    const banner = `[data-testid="pane"][data-pane-id="${claude}"] [data-testid="pane-approval"]`;
    await $(banner).waitForDisplayed({ timeout: 20_000 });
    const fields = String(await $(`${banner} [data-testid="plugin-approval"]`).getProperty("textContent"));
    expect(fields).toContain("Test Echo");
    await clickAllow(`${banner} [data-testid="pane-approval-once"]`);
    expect(await mcpAfter(claude, "test_echo__note")).toMatch(/mcp said:[\s\S]*note done: kept/);
    await $(banner).waitForExist({ reverse: true, timeout: 10_000 });
  });

  it("F-PL-01.6 when the engine exits its lease is revoked: a request with it gets 401, and its config is gone", async () => {
    const file = join(home(), "mcp", `${claude}.json`);
    expect(await gatewayStatus(token)).toBe(200);
    await sendToPane(claude, "/exit\r");
    await waitForPaneText(claude, "Goodbye from the test engine");
    await waitFor(
      () => existsSync(file),
      (there) => !there,
      "the pane's config stayed after its engine exited",
      { timeout: 15_000 },
    );
    expect(await gatewayStatus(token)).toBe(401);
  });

  it("F-PL-01.5 a Codex terminal gets the lease only in TANDEM_GATEWAY_LEASE, and calls the plugin with it", async () => {
    const codex = await split("down", "codex");
    await waitForPaneText(codex, "Fake Codex");
    const spawn = (await invoke<SpawnLine[]>("test_spawn_log")).find((r) =>
      r.argv.some((a) => a.includes("mcp_servers.tandem.bearer_token_env_var")),
    );
    expect(spawn).toBeDefined();
    expect(spawn?.envKeys).toContain("TANDEM_GATEWAY_LEASE");
    expect(spawn?.argv.join(" ")).not.toContain("Bearer");
    await sendToPane(codex, '/mcp tandem__call {"tool":"test_echo__echo","args":{"text":"codex-side"}}\r');
    expect(await mcpAfter(codex, "test_echo__echo")).toMatch(/mcp said:[\s\S]*codex-side/);
  });

  it("F-PL-05.3 a terminal in a workspace with none gets no gateway", async () => {
    await $(`[data-testid="workspace-row"][data-root="${other}"]`).click();
    await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 20_000 });
    const plain = await split("right", "claude");
    await waitForPaneText(plain, "Fake Claude Code");
    await sendToPane(plain, "/mcp\r");
    expect(await mcpAfter(plain, "/mcp")).toContain("mcp: no gateway");
  });
});
