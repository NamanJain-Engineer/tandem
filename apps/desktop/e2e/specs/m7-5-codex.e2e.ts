import { $, browser } from "@wdio/globals";
import { execFileSync } from "node:child_process";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { checkAgain, fakeEngine, fakeLog, freshOffline, home, invoke } from "../support/app";
import { assistantTexts, sendMessage, threadMode, waitForAssistant } from "../support/thread";

const versions = () => join(home(), "fake-version.txt");

/** Thread-mode Codex app-servers (their MCP override names the gateway). */
function appServers(): number[] {
  try {
    return execFileSync("pgrep", ["-f", "app-server -c mcp_servers.tandem"], { encoding: "utf8" })
      .trim()
      .split("\n")
      .filter(Boolean)
      .map(Number);
  } catch {
    return [];
  }
}

async function codexThread(): Promise<string> {
  const thread = await invoke<{ id: string }>("thread_create", { args: { kind: "thread", workspaceId: null, engineId: "codex" } });
  const row = await $(`[data-testid="thread-row"][data-thread-id="${thread.id}"]`);
  await row.waitForExist({ timeout: 10_000 });
  await row.click();
  await $('[data-testid="thread-main"]').waitForExist({ timeout: 10_000 });
  return thread.id;
}

async function notes(): Promise<string[]> {
  return browser.execute(() =>
    Array.from(document.querySelectorAll<HTMLElement>('[data-testid="thread-main"] [role="log"] p')).map((p) => p.innerText.trim()),
  );
}

// E2E-M7-5 Codex (F-TH-05): an app-server that died while idle is restarted
// and resumed on the next send ("Reconnecting…", history kept); a Codex too
// old for Thread mode fails closed with the version message.
describe("E2E-M7-5 Codex", () => {
  before(async function () {
    this.timeout(60_000);
    writeFileSync(versions(), "");
    await freshOffline();
    fakeEngine("codex", join(home(), "fakebin"));
    const report = await checkAgain();
    expect(report.found.map((f) => f.engineId)).toContain("codex");
    await threadMode();
  });

  it("F-TH-05.2 a killed idle app-server restarts on the next send, resumes, and keeps the history", async function () {
    this.timeout(90_000);
    await codexThread();
    await sendMessage("first");
    await waitForAssistant("You said: first");
    const servers = appServers();
    expect(servers.length).toBe(1);
    const mark = fakeLog().length;
    execFileSync("kill", ["-9", String(servers[0])]);
    await browser.waitUntil(async () => appServers().length === 0, { timeout: 5_000 });
    await browser.pause(500);

    await sendMessage("second");
    await waitForAssistant("You said: second");
    expect(await notes()).toContain("Reconnecting…");
    expect((await assistantTexts()).filter((t) => t.startsWith("You said: first"))).toHaveLength(1);
    expect(fakeLog().slice(mark).some((l) => l.startsWith("app-server: thread/resume thr_fake_"))).toBe(true);
  });

  it("F-TH-05.3 an unsupported Codex version fails closed with the message", async function () {
    this.timeout(60_000);
    writeFileSync(versions(), "codex 0.31.0\n");
    const report = await checkAgain();
    expect(report.found.map((f) => f.engineId)).toContain("codex");
    await codexThread();
    await sendMessage("hello");
    const alerts = () =>
      browser.execute(() => Array.from(document.querySelectorAll<HTMLElement>('[role="alert"]')).map((e) => e.innerText));
    await browser.waitUntil(async () => (await alerts()).some((t) => t.includes("isn't supported yet")), {
      timeout: 20_000,
      timeoutMsg: `alerts: ${JSON.stringify(await alerts())}`,
    });
    expect((await alerts()).find((t) => t.includes("isn't supported yet"))).toContain(
      "Codex version 0.31.0 isn't supported yet — update Vibezeo or Codex",
    );
    expect(await $("button=Open in Code mode").isExisting()).toBe(true);
    writeFileSync(versions(), "");
  });
});
