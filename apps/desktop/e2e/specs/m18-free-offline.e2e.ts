import { $ } from "@wdio/globals";
import { mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { checkAgain, fakeEngine, freshOffline, home, invoke, userHome } from "../support/app";
import { composer, sendMessage, threadMode, waitForAssistant } from "../support/thread";

const SESSION_ID = "cccc1111-2222-3333-4444-555566667777";

/** The run's own HOME: this spec writes `~/.claude`, so it refuses any
 * other (a developer's real sign-ins). */
function testUserHome(): string {
  const user = userHome();
  const run = home();
  if (!run.startsWith(join(tmpdir(), "tandem-e2e-")) || !user.startsWith(join(run, "home"))) {
    throw new Error(`refusing to touch ${user}: run this spec through e2e/run.ts`);
  }
  return user;
}

// E2E-M18-FREE Free offline (D-869, D-2900): with no account and offline,
// what used to need Pro just works: a thread on Cursor Agent (an ACP engine,
// F-TH-06) streams its reply, and session search (F-TH-17) finds a session by
// a word from its prompt. The fifth pane and the fourth workspace are
// E2E-M3-7 and E2E-M2-2; routines and task runs are E2E-M11-6.
describe("E2E-M18-FREE free offline", function () {
  this.timeout(180_000);
  const claudeHome = () => join(testUserHome(), ".claude");

  before(async () => {
    await freshOffline();
  });

  after(async () => {
    await invoke("library_search_set", { on: false }).catch(() => undefined);
    rmSync(join(claudeHome(), "projects", "-tmp-tandem-e2e-free"), { recursive: true, force: true });
  });

  it("F-TH-06 a Cursor Agent thread streams its reply with no account", async () => {
    fakeEngine("cursor-agent", join(home(), "fakebin"));
    const report = await checkAgain();
    expect(report.found.map((f) => f.engineId)).toContain("cursor-agent");
    await threadMode();
    const { id } = await invoke<{ id: string }>("thread_create", {
      args: { kind: "thread", workspaceId: null, engineId: "cursor-agent" },
    });
    const row = await $(`[data-testid="thread-row"][data-thread-id="${id}"]`);
    await row.waitForExist({ timeout: 10_000 });
    await row.click();
    await composer().waitForDisplayed({ timeout: 10_000 });
    expect(await $('[data-testid="thread-engine-gate"]').isExisting()).toBe(false);
    await sendMessage("hello from offline");
    await waitForAssistant("You said: hello from offline");
    await invoke("thread_stop", { threadId: id }).catch(() => undefined);
  });

  it("F-TH-17.4 session search turns on and finds a session with no account", async () => {
    const cwd = "/tmp/tandem-e2e-free";
    const folder = join(claudeHome(), "projects", cwd.replace(/[\\/]/g, "-"));
    mkdirSync(folder, { recursive: true });
    const lines = [
      {
        type: "user",
        sessionId: SESSION_ID,
        cwd,
        message: { role: "user", content: "Tune the zebra crossing timer" },
      },
      {
        type: "assistant",
        sessionId: SESSION_ID,
        cwd,
        message: { role: "assistant", content: [{ type: "text", text: "Done." }] },
      },
    ];
    writeFileSync(join(folder, `${SESSION_ID}.jsonl`), lines.map((l) => JSON.stringify(l)).join("\n") + "\n");
    await invoke("library_search_set", { on: true });
    const keys = await invoke<string[]>("library_search", { query: "zebra" });
    expect(keys).toHaveLength(1);
    expect(keys[0]).toContain(SESSION_ID);
  });
});
