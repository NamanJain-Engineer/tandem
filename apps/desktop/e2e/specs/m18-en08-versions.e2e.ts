import { $, browser } from "@wdio/globals";
import { type ChildProcess, spawn } from "node:child_process";
import {
  chmodSync,
  copyFileSync,
  existsSync,
  mkdirSync,
  readFileSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from "node:fs";
import { join } from "node:path";
import { Key } from "webdriverio";
import {
  addFolder,
  checkAgain,
  freshOffline,
  home,
  invoke,
  mod,
  paneIds,
  split,
  waitForPaneText,
} from "../support/app";

type Row = {
  engineId: string;
  version: string | null;
  method: string;
  pinnable: boolean;
  pinned: boolean;
  broken: { from: string; to: string } | null;
};
type Spawn = { argv: string[]; envKeys: string[] };

const bin = () => join(home(), "fakebin");
const npmCalls = () => join(home(), "npm-calls.txt");
const versions = () => join(home(), "fake-version.txt");
/** An npm-style install: the command links into `node_modules`. */
const npmCodex = () => join(home(), "npm-global/lib/node_modules/@openai/codex/bin/codex");

async function openEngines(): Promise<void> {
  await browser.execute(() => (document.activeElement as HTMLElement | null)?.blur());
  await browser.keys([mod(), ","]);
  const nav = $('//nav[@aria-label="Settings pages"]//button[normalize-space()="Engines"]');
  await nav.waitForClickable({ timeout: 10_000 });
  await nav.click();
  await $('[data-testid="engines-page"]').waitForDisplayed({ timeout: 10_000 });
}

async function closeSettings(): Promise<void> {
  await browser.execute(() => (document.activeElement as HTMLElement | null)?.blur());
  await browser.keys(Key.Escape);
  await $('[data-testid="engines-page"]').waitForExist({ reverse: true, timeout: 5_000 });
}

async function details(id: string): Promise<void> {
  const toggle = $(`[data-testid="engine-row-${id}"] button[aria-expanded]`);
  await toggle.scrollIntoView();
  if ((await toggle.getAttribute("aria-expanded")) !== "true") await toggle.click();
  await $(`[data-testid="engine-updates-${id}"]`).waitForDisplayed({ timeout: 10_000 });
}

// E2E-M18-EN08 Engine versions (F-EN-08): an npm install of Codex shows its
// version and method and asks npm only on Check; Pin starts new panes with
// the engine's own switch; a failure after an update offers a rollback with
// the command shown first; a Codex left running without a terminal is
// listed and stopped after a confirm.
describe("E2E-M18-EN08 engine versions", function () {
  this.timeout(180_000);
  let daemon: ChildProcess | null = null;

  before(async () => {
    mkdirSync(join(npmCodex(), ".."), { recursive: true });
    copyFileSync(process.env.TANDEM_FAKE_ENGINE_BIN ?? "", npmCodex());
    chmodSync(npmCodex(), 0o755);
    rmSync(join(bin(), "codex"), { force: true });
    symlinkSync(npmCodex(), join(bin(), "codex"));
    // npm itself, recording what it was asked.
    rmSync(npmCalls(), { force: true });
    writeFileSync(join(bin(), "npm"), `#!/bin/sh\necho "$*" >> "${npmCalls()}"\necho 1.3.0\n`);
    chmodSync(join(bin(), "npm"), 0o755);
    writeFileSync(versions(), "codex 1.2.0\n");
    await freshOffline();
    await addFolder(`/tmp/tandem-e2e-en08-${Date.now()}`);
    await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 20_000 });
  });

  after(async () => {
    daemon?.kill("SIGKILL");
    for (const name of ["codex", "npm"]) rmSync(join(bin(), name), { force: true });
    rmSync(join(home(), "npm-global"), { recursive: true, force: true });
    rmSync(join(home(), "engine-versions.json"), { force: true });
    writeFileSync(versions(), "");
    await invoke("engine_pin", { engineId: "claude", on: false }).catch(() => undefined);
    await checkAgain().catch(() => undefined);
  });

  it("F-EN-08.1 Codex shows 1.2.0 · npm, and npm is asked only on Check for updates", async () => {
    const report = await checkAgain();
    expect(report.found.map((f) => f.engineId)).toContain("codex");
    const rows = await invoke<Row[]>("engine_versions");
    const codex = rows.find((r) => r.engineId === "codex");
    expect(codex?.version).toBe("1.2.0");
    expect(codex?.method).toBe("npm");
    await openEngines();
    const version = $('[data-testid="engine-version-codex"]');
    await version.waitForDisplayed({ timeout: 10_000 });
    expect(await version.getText()).toBe("1.2.0 · npm");
    expect(existsSync(npmCalls())).toBe(false);
    await details("codex");
    await $('[data-testid="engine-check-codex"]').click();
    const latest = $('[data-testid="engine-latest-codex"]');
    await latest.waitForDisplayed({ timeout: 30_000 });
    expect(await latest.getText()).toContain("1.3.0");
    expect(readFileSync(npmCalls(), "utf8").trim()).toBe("view @openai/codex version");
  });

  it("F-EN-08.2 Pin starts new panes with the engine's own switch", async () => {
    const pin = $("#engine-pin-codex");
    await pin.scrollIntoView();
    await pin.click();
    await browser.waitUntil(
      async () =>
        (await invoke<Row[]>("engine_versions")).find((r) => r.engineId === "codex")?.pinned === true,
      { timeout: 10_000, timeoutMsg: "Codex wasn't pinned" },
    );
    await closeSettings();
    await invoke("engine_pin", { engineId: "claude", on: true });
    const codex = await split("right", "codex");
    await waitForPaneText(codex, "Fake Codex (test engine)");
    const claude = await split("down", "claude");
    await waitForPaneText(claude, "Fake Claude Code (test engine)");
    const spawns = await invoke<Spawn[]>("test_spawn_log");
    const joined = spawns.map((s) => s.argv.join(" "));
    expect(joined.some((a) => a.includes("check_for_update_on_startup=false"))).toBe(true);
    expect(spawns.some((s) => s.envKeys.includes("DISABLE_AUTOUPDATER"))).toBe(true);
  });

  it("F-EN-08.3 a failure since an update names both versions, and Roll back shows the command first", async () => {
    writeFileSync(versions(), "codex 1.3.0\n");
    await checkAgain();
    // The turn results that make 1.2.0 last good and 1.3.0 failing come
    // from Thread mode (unit-tested); here the book on disk says so.
    writeFileSync(
      join(home(), "engine-versions.json"),
      JSON.stringify({
        engines: { codex: { current: "1.3.0", previous: "1.2.0", lastGood: "1.2.0", failing: true } },
      }),
    );
    const row = (await invoke<Row[]>("engine_versions")).find((r) => r.engineId === "codex");
    expect(row?.broken).toEqual({ from: "1.2.0", to: "1.3.0" });
    await openEngines();
    const banner = $('[data-testid="engine-broken-codex"]');
    await banner.waitForDisplayed({ timeout: 10_000 });
    expect(await banner.getText()).toBe("Codex updated from 1.2.0 to 1.3.0, and threads have failed since.");
    await $("button=Roll back to 1.2.0…").click();
    const command = $('[data-testid="install-command"]');
    await command.waitForDisplayed({ timeout: 10_000 });
    expect(await command.getText()).toContain("npm install -g @openai/codex@1.2.0");
    // Shown, not run: nothing ran npm install.
    expect(readFileSync(npmCalls(), "utf8")).not.toContain("install");
    await browser.keys(Key.Escape);
    await $('[data-testid="install-sheet"]').waitForExist({ reverse: true, timeout: 5_000 });
    await closeSettings();
  });

  it("F-EN-08.4 a Codex with no terminal outside the app is listed and stopped after a confirm; panes never are", async () => {
    // Its own session, so no controlling terminal; stdin kept open.
    daemon = spawn(npmCodex(), ["app-server"], { detached: true, stdio: ["pipe", "ignore", "ignore"] });
    const pid = daemon.pid ?? 0;
    await browser.waitUntil(
      async () =>
        (await invoke<Array<{ pid: number; engineId: string }>>("engine_background")).some(
          (b) => b.pid === pid && b.engineId === "codex",
        ),
      { timeout: 10_000, timeoutMsg: "the background Codex wasn't listed" },
    );
    const listed = await invoke<Array<{ pid: number; command: string }>>("engine_background");
    // The panes opened above run in terminals: only the daemon is listed.
    expect(listed.map((b) => b.pid)).toEqual([pid]);
    await openEngines();
    const row = $(`[data-testid="engine-background-row"][data-pid="${pid}"]`);
    await row.scrollIntoView();
    await row.$("button=Stop").click();
    const dialog = $('[role="alertdialog"]');
    await dialog.waitForDisplayed({ timeout: 10_000 });
    expect(await dialog.getText()).toContain(`Process ${pid} has no terminal`);
    await dialog.$("button=Stop").click();
    await browser.waitUntil(async () => daemon?.exitCode !== null || daemon?.signalCode !== null, {
      timeout: 10_000,
      timeoutMsg: "the background Codex didn't stop",
    });
    expect(daemon?.signalCode).toBe("SIGTERM");
    await closeSettings();
    await openEngines();
    await $('[data-testid="engines-background-empty"]').waitForDisplayed({ timeout: 10_000 });
    await closeSettings();
  });
});
