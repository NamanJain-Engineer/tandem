import { $, browser } from "@wdio/globals";
import { existsSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createServer, type Server } from "node:http";
import { join } from "node:path";
import { Key } from "webdriverio";
import {
  addFolder,
  checkAgain,
  crash,
  fakeEngine,
  freshOffline,
  home,
  invoke,
  mod,
  pausedCards,
  paneIds,
  relaunch,
  stopOrphansAutomatically,
} from "../support/app";

type LocalServer = {
  kind: string;
  port: number;
  added: boolean;
  running: boolean;
  models: Array<{ id: string }>;
  startable: boolean;
  startReason: string | null;
};
type Spawn = { argv: string[]; envKeys: string[]; providerHome: string | null; cwd: string };

/** A stand-in for Ollama on 127.0.0.1 (on `port`, or any free one):
 * `/api/tags` lists one model, and every path asked is recorded. */
function fakeOllama(port = 0): Promise<{ server: Server; port: number; asked: string[] }> {
  const asked: string[] = [];
  const server = createServer((req, res) => {
    asked.push(req.url ?? "");
    if (req.url === "/api/tags") {
      res.writeHead(200, { "content-type": "application/json" });
      res.end(
        JSON.stringify({
          models: [{ name: "qwen3-coder:30b", size: 18_556_701_696, details: { parameter_size: "30.5B" } }],
        }),
      );
    } else {
      res.writeHead(404);
      res.end();
    }
  });
  return new Promise((resolve) =>
    server.listen(port, "127.0.0.1", () => {
      const addr = server.address();
      resolve({ server, port: typeof addr === "object" && addr ? addr.port : 0, asked });
    }),
  );
}

async function openEngines(): Promise<void> {
  await browser.execute(() => (document.activeElement as HTMLElement | null)?.blur());
  await browser.keys([mod(), ","]);
  const nav = $('//nav[@aria-label="Settings pages"]//button[normalize-space()="Engines"]');
  await nav.waitForClickable({ timeout: 10_000 });
  await nav.click();
  await $('[data-testid="local-models"]').waitForDisplayed({ timeout: 10_000 });
}

/** Clicks by selector, found fresh. */
async function press(selector: string): Promise<void> {
  await browser.waitUntil(
    () =>
      browser.execute((s: string) => {
        const el = document.querySelector<HTMLButtonElement>(s);
        if (!el || el.disabled) return false;
        el.click();
        return true;
      }, selector),
    { timeout: 15_000, timeoutMsg: `${selector} wasn't clickable` },
  );
}

// E2E-M18-EN11 Local models (F-EN-11): a stand-in Ollama on 127.0.0.1 is
// found by the port the user adds, with its model's size and parameters; a
// host off this computer is refused; Start in Codex opens a Codex pane in a
// provider home of its own whose config.toml points at 127.0.0.1, and the
// pane says "On this computer". Every start names the model and the local
// provider on Codex's command line, last; a Codex older than 0.129.0 is
// refused; and a restore after the app restarts asks the server again.
describe("E2E-M18-EN11 local models", function () {
  this.timeout(180_000);
  const repo = `/tmp/tandem-e2e-en11-${Date.now()}`;
  let ollama: Awaited<ReturnType<typeof fakeOllama>>;

  before(async () => {
    ollama = await fakeOllama();
    await freshOffline();
    fakeEngine("codex", join(home(), "fakebin"));
    const report = await checkAgain();
    expect(report.found.map((f) => f.engineId)).toContain("codex");
    await addFolder(repo);
    await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 30_000 });
  });

  after(async () => {
    ollama?.server.close();
    await stopOrphansAutomatically(false).catch(() => undefined);
  });

  it("F-EN-11.3 a host that isn't this computer is refused", async () => {
    await expect(
      invoke("local_models_add_port", { host: "192.168.1.5", port: ollama.port, kind: "ollama" }),
    ).rejects.toThrow(/only looks for model servers on this computer/);
  });

  it("F-EN-11.1 a server answering Ollama's tags on 127.0.0.1 is found, with its model", async () => {
    await invoke("local_models_add_port", { host: "localhost", port: ollama.port, kind: "ollama" });
    const servers = await invoke<LocalServer[]>("local_models_list");
    const ours = servers.find((s) => s.port === ollama.port);
    expect(ours).toMatchObject({ kind: "ollama", added: true, running: true });
    expect(ollama.asked).toContain("/api/tags");
    await openEngines();
    const row = $(`[data-testid="local-server"][data-port="${ollama.port}"]`);
    await row.waitForDisplayed({ timeout: 10_000 });
    // Read from the DOM: inside the Settings dialog, WebDriver's text can come back empty.
    const line = await browser.execute(
      (p: number) =>
        document.querySelector(`[data-testid="local-server"][data-port="${p}"] [data-testid="local-model"]`)
          ?.textContent ?? "",
      ollama.port,
    );
    expect(line).toContain("qwen3-coder:30b · 18.6 GB · 30.5B");
  });

  it("F-EN-11.2 a model the server doesn't offer, or a llama.cpp server, doesn't start", async () => {
    const [ws] = await invoke<Array<{ tabs: Array<{ id: string }> }>>("workspace_list");
    const tabId = ws!.tabs[0]!.id;
    await expect(
      invoke("local_model_start", { tabId, kind: "ollama", port: ollama.port, model: "absent:1b" }),
    ).rejects.toThrow(/doesn't offer absent:1b right now/);
    await expect(
      invoke("local_model_start", { tabId, kind: "llama_cpp", port: ollama.port, model: "qwen3-coder:30b" }),
    ).rejects.toThrow(/Codex can't use llama.cpp servers yet/);
  });

  it("F-EN-11.2 Start in Codex runs Codex in its own home, configured for 127.0.0.1, and says On this computer", async () => {
    const before = (await invoke<Spawn[]>("test_spawn_log")).length;
    // Settings covers the panes, so count them in the DOM, not as visible.
    const count = () => browser.execute(() => document.querySelectorAll('[data-testid="pane"]').length);
    const panes = await count();
    await press(`[data-testid="local-server"][data-port="${ollama.port}"] [data-testid="local-start"]`);
    await browser.waitUntil(async () => (await count()) === panes + 1, { timeout: 20_000 });
    const homes = join(home(), "providers", "codex");
    const homeDir = await browser
      .waitUntil(
        () =>
          existsSync(homes) &&
          readdirSync(homes).find((d) => d.startsWith(`local-ollama-${ollama.port}-qwen3-coder-30b-`)),
        { timeout: 20_000, timeoutMsg: "no home for the model" },
      )
      .then((d) => join(homes, d as string));
    await browser.waitUntil(
      async () =>
        (await invoke<Spawn[]>("test_spawn_log")).slice(before).some((s) => s.providerHome === homeDir),
      { timeout: 20_000, timeoutMsg: "Codex didn't start in the local home" },
    );
    const spawn = (await invoke<Spawn[]>("test_spawn_log"))
      .slice(before)
      .find((s) => s.providerHome === homeDir)!;
    expect(spawn.envKeys).toContain("CODEX_HOME");
    expect(spawn.envKeys).toEqual(expect.arrayContaining(["NO_PROXY", "no_proxy"]));
    const config = readFileSync(join(homeDir, "config.toml"), "utf8");
    expect(config).toContain('model = "qwen3-coder:30b"');
    expect(config).toContain("check_for_update_on_startup = false");
    expect(config).toContain(`base_url = "http://127.0.0.1:${ollama.port}/v1"`);
    const chip = $('[data-testid="local-model-chip"]');
    await chip.waitForDisplayed({ timeout: 10_000 });
    expect(await chip.getText()).toBe("On this computer");
    await browser.keys(Key.Escape);
  });

  it("F-EN-11.2 a Codex older than 0.129.0 doesn't start a local model, and the page says why", async () => {
    const versions = join(home(), "fake-version.txt");
    const [ws] = await invoke<Array<{ tabs: Array<{ id: string }> }>>("workspace_list");
    const tabId = ws!.tabs[0]!.id;
    const before = (await invoke<Spawn[]>("test_spawn_log")).length;
    writeFileSync(versions, "codex 0.128.0\n");
    try {
      await expect(
        invoke("local_model_start", { tabId, kind: "ollama", port: ollama.port, model: "qwen3-coder:30b" }),
      ).rejects.toThrow(/Local models need Codex 0\.129\.0 or newer, and this computer has Codex 0\.128\.0/);
      expect((await invoke<Spawn[]>("test_spawn_log")).length).toBe(before);
      // Detected again, the page turns Start off with the same reason.
      await checkAgain();
      const ours = (await invoke<LocalServer[]>("local_models_list")).find((s) => s.port === ollama.port);
      expect(ours?.startable).toBe(false);
      expect(ours?.startReason).toMatch(/^Local models need Codex 0\.129\.0 or newer/);
    } finally {
      rmSync(versions, { force: true });
      await checkAgain();
    }
    const ours = (await invoke<LocalServer[]>("local_models_list")).find((s) => s.port === ollama.port);
    expect(ours?.startable).toBe(true);
  });

  it("F-EN-11.2 every start names the model and provider on Codex's command line, and a restore asks the server again", async () => {
    const port = ollama.port;
    const homes = join(home(), "providers", "codex");
    const homeDir = join(
      homes,
      readdirSync(homes).find((d) => d.startsWith(`local-ollama-${port}-qwen3-coder-30b-`))!,
    );
    const local = async () =>
      (await invoke<Spawn[]>("test_spawn_log")).filter((s) => s.providerHome === homeDir);
    // Last in the wrapped command, where it ends: the model (for /review and
    // memories too), the provider and sub-agents off, for the fake's 1.2.3.
    const overrides = [
      'model="qwen3-coder:30b"',
      'review_model="qwen3-coder:30b"',
      'memories.extract_model="qwen3-coder:30b"',
      'memories.consolidation_model="qwen3-coder:30b"',
      'model_provider="vibezeo-local"',
      'model_providers.vibezeo-local.name="Ollama on this computer"',
      `model_providers.vibezeo-local.base_url="http://127.0.0.1:${port}/v1"`,
      'model_providers.vibezeo-local.wire_api="responses"',
      "features.multi_agent=false",
      "features.multi_agent_v2=false",
      "agents.enabled=false",
      "check_for_update_on_startup=false",
    ]
      .map((kv) => `'-c' '${kv}'`)
      .join(" ");
    const named = (s: Spawn) => s.argv.join(" ").includes(`${overrides}; c=$?`);
    const started = await local();
    expect(started.length).toBeGreaterThan(0);
    expect(started.every(named)).toBe(true);
    const pane = await browser.execute(
      () =>
        document
          .querySelector('[data-testid="local-model-chip"]')
          ?.closest<HTMLElement>('[data-testid="pane"]')?.dataset.paneId ?? "",
    );
    expect(pane).not.toBe("");
    // The app goes down with the pane running, and the server stops.
    await stopOrphansAutomatically(true);
    crash();
    ollama.server.closeAllConnections();
    await new Promise((done) => ollama.server.close(done));
    await relaunch();
    await browser.waitUntil(async () => (await pausedCards()).some((c) => c.paneId === pane), {
      timeout: 30_000,
      timeoutMsg: "the local pane's card isn't shown",
    });
    await expect(invoke("restore_start_fresh", { paneId: pane })).rejects.toThrow(
      new RegExp(`Ollama isn't running on port ${port}`),
    );
    expect((await local()).length).toBe(started.length);
    // Running again on its port: the restore starts Codex, named the same way.
    ollama = await fakeOllama(port);
    await invoke("restore_start_fresh", { paneId: pane });
    await browser.waitUntil(async () => (await local()).length > started.length, {
      timeout: 20_000,
      timeoutMsg: "the restore didn't start Codex",
    });
    expect((await local()).every(named)).toBe(true);
    expect(ollama.asked).toContain("/api/tags");
  });
});
