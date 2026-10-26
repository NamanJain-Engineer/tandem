import { $, browser } from "@wdio/globals";
import { readFileSync } from "node:fs";
import { createServer, type Server } from "node:http";
import { join } from "node:path";
import { Key } from "webdriverio";
import { addFolder, checkAgain, fakeEngine, freshOffline, home, invoke, mod, paneIds } from "../support/app";

type LocalServer = { kind: string; port: number; added: boolean; running: boolean; models: Array<{ id: string }> };
type Spawn = { argv: string[]; envKeys: string[]; providerHome: string | null; cwd: string };

/** A stand-in for Ollama on 127.0.0.1: `/api/tags` lists one model, and
 * every path asked is recorded. */
function fakeOllama(): Promise<{ server: Server; port: number; asked: string[] }> {
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
    server.listen(0, "127.0.0.1", () => {
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
// pane says "On this computer".
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

  after(() => ollama?.server.close());

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

  it("F-EN-11.2 Start in Codex runs Codex in its own home, configured for 127.0.0.1, and says On this computer", async () => {
    const before = (await invoke<Spawn[]>("test_spawn_log")).length;
    // Settings covers the panes, so count them in the DOM, not as visible.
    const count = () => browser.execute(() => document.querySelectorAll('[data-testid="pane"]').length);
    const panes = await count();
    await press(`[data-testid="local-server"][data-port="${ollama.port}"] [data-testid="local-start"]`);
    await browser.waitUntil(async () => (await count()) === panes + 1, { timeout: 20_000 });
    const homeDir = join(home(), "providers", "codex", `local-ollama-${ollama.port}`);
    await browser.waitUntil(
      async () => (await invoke<Spawn[]>("test_spawn_log")).slice(before).some((s) => s.providerHome === homeDir),
      { timeout: 20_000, timeoutMsg: "Codex didn't start in the local home" },
    );
    const spawn = (await invoke<Spawn[]>("test_spawn_log")).slice(before).find((s) => s.providerHome === homeDir)!;
    expect(spawn.envKeys).toContain("CODEX_HOME");
    const config = readFileSync(join(homeDir, "config.toml"), "utf8");
    expect(config).toContain('model = "qwen3-coder:30b"');
    expect(config).toContain(`base_url = "http://127.0.0.1:${ollama.port}/v1"`);
    const chip = $('[data-testid="local-model-chip"]');
    await chip.waitForDisplayed({ timeout: 10_000 });
    expect(await chip.getText()).toBe("On this computer");
    await browser.keys(Key.Escape);
  });
});
