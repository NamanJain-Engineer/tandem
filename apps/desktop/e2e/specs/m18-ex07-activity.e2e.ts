import { $, browser } from "@wdio/globals";
import { mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import {
  addFolder,
  closeAllPanes,
  freshOffline,
  home,
  invoke,
  paneIds,
  paneText,
  sendToPane,
  split,
  waitForPaneText,
} from "../support/app";

// E2E-M18-EX07 Activity ledger (F-EX-07): an agent pane on the fake engine
// gets a prompt, runs `vibezeo run` (allowed once), exits, and two commands
// are typed in the shell it hands back to. Activity lists the turns, the
// approval, the run (its secret's name, who decided, its exit code) and both
// commands, in order, never the prompt's words or the secret's value; Export
// writes the same events as JSON Lines.

const VALUE = "sk_test_e2e_ledger_4242";
const SENTENCE = "please rename the quarterly flamingo report";

type Event = {
  kind: string;
  summary: string;
  exitCode: number | null;
  secrets: string[];
  decided: string | null;
  checkpointId: string | null;
};

describe("E2E-M18-EX07 activity ledger", function () {
  this.timeout(180_000);
  const folder = `/tmp/tandem-e2e-m18-ex07-${Date.now()}`;
  let agent = "";
  const list = () => invoke<Event[]>("activity_list", { source: "pane", id: agent });

  before(async function () {
    this.timeout(90_000);
    mkdirSync(folder, { recursive: true });
    writeFileSync(join(folder, ".env"), "API_KEY=vibezeo-secret://stripe-test\n");
    writeFileSync(join(folder, "notes.txt"), "first\n");
    await freshOffline();
    // bash with shell integration, so the shell the agent hands back to
    // records its commands (F-EX-02).
    await invoke("settings_set", { key: "default_shell", value: "/bin/bash" });
    await invoke("secrets_set", { name: "stripe-test", value: VALUE });
    await addFolder(folder);
    await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 20_000 });
    agent = await split("right", "claude");
    await waitForPaneText(agent, "Fake Claude Code");
  });

  after(async () => {
    await closeAllPanes();
    await invoke("settings_set", { key: "default_shell", value: null });
    for (const s of await invoke<Array<{ name: string }>>("secrets_list"))
      await invoke("secrets_delete", { name: s.name });
    rmSync(folder, { recursive: true, force: true });
  });

  it("F-EX-07.1 F-EX-07.2 F-EX-07.4 the session's turns, approval, run and commands show in order, without its words or the value", async () => {
    await sendToPane(agent, `${SENTENCE}\r`);
    // The agent edits a file, so the next prompt's checkpoint has changes.
    await sendToPane(agent, "/sh echo E1; echo second >> notes.txt\r");
    await waitForPaneText(agent, "E1");
    await sendToPane(agent, "/sh echo R1; vibezeo run -- printenv API_KEY\r");
    const banner = `[data-testid="pane"][data-pane-id="${agent}"] [data-testid="pane-approval"]`;
    await $(banner).waitForDisplayed({ timeout: 15_000 });
    // Allow takes clicks once the question has been shown 500 ms.
    await $(`${banner} [data-testid="pane-approval-once"]`).waitForEnabled();
    await $(`${banner} [data-testid="pane-approval-once"]`).click();
    await browser.waitUntil(async () => /R1[\s\S]*sh exit: 0/.test(await paneText(agent)), {
      timeout: 20_000,
      timeoutMsg: "the run never finished",
    });
    await sendToPane(agent, "/exit\r");
    await waitForPaneText(agent, "Goodbye from the test engine");
    await browser.pause(1_500);
    await sendToPane(agent, "echo ledger-one\r");
    await waitForPaneText(agent, "ledger-one");
    await sendToPane(agent, "false\r");
    await browser.waitUntil(async () => (await list()).filter((e) => e.kind === "command").length >= 2, {
      timeout: 20_000,
      timeoutMsg: "the commands never reached the ledger",
    });

    const events = await list();
    const kinds = events.map((e) => e.kind).filter((k) => k !== "tool" && k !== "subagent" && k !== "files");
    // Three prompts (the sentence and two /sh lines), counted from the
    // engine's UserPromptSubmit; /exit and what's typed after handback aren't
    // turns.
    expect(kinds).toEqual(["turn", "turn", "turn", "approval", "run", "command", "command"]);
    const run = events.find((e) => e.kind === "run");
    expect(run).toMatchObject({
      summary: "printenv API_KEY",
      secrets: ["API_KEY (stripe-test)"],
      decided: "once",
      exitCode: 0,
    });
    expect(events.find((e) => e.kind === "approval")?.decided).toBe("allowed:inline");
    const commands = events.filter((e) => e.kind === "command");
    expect(commands.map((c) => [c.summary, c.exitCode])).toEqual([
      ["echo ledger-one", 0],
      ["false", 1],
    ]);
    // The edit shows as a checkpoint with changes, and later events link to one.
    expect(
      events.some((e) => e.kind === "files" && e.summary === "Files changed since the last checkpoint"),
    ).toBe(true);
    expect(events[events.length - 1]?.checkpointId).toBeTruthy();
    const all = JSON.stringify(events);
    expect(all).not.toContain("flamingo");
    expect(all).not.toContain(VALUE);
  });

  it("F-EX-07.3 Activity… shows them, and Export writes the same events as JSON Lines", async () => {
    await $(`[data-testid="pane"][data-pane-id="${agent}"] button[aria-label^="Actions for"]`).click();
    const item = $('//*[@role="menuitem"][normalize-space()="Activity…"]');
    await item.waitForDisplayed({ timeout: 5_000 });
    await item.click();
    await $('[data-testid="activity-dialog"]').waitForDisplayed({ timeout: 10_000 });
    const events = await list();
    await browser.waitUntil(
      async () =>
        (await browser.execute(() => document.querySelectorAll('[data-testid="activity-row"]').length)) ===
        events.length,
      {
        timeout: 10_000,
        timeoutMsg: "the dialog didn't list every event",
      },
    );
    const out = join(home(), `activity-${Date.now()}.jsonl`);
    writeFileSync(join(home(), "save-path.txt"), out);
    await $('[data-testid="activity-export"]').click();
    await browser.waitUntil(
      async () => {
        try {
          return readFileSync(out, "utf8").length > 0;
        } catch {
          return false;
        }
      },
      { timeout: 10_000, timeoutMsg: "no export written" },
    );
    const lines = readFileSync(out, "utf8").trimEnd().split("\n");
    expect(lines.map((l) => JSON.parse(l) as Event)).toEqual(events);
    expect(readFileSync(out, "utf8")).not.toContain(VALUE);
  });
});
