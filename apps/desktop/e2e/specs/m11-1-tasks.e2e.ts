import { browser, $ } from "@wdio/globals";
import { freshOffline, invoke, mod } from "../support/app";
import { lead, newChat, sendAndWait } from "../support/agents";
import { expectNoSeriousA11yViolations } from "../support/axe";

type Issue = { id: string; revision: number; title: string; status: string };

async function issue(id: string): Promise<Issue> {
  return (await invoke<{ issue: Issue }>("task_get", { id })).issue;
}

/** An agent's tasks tool call, through the thread's own `tandem_tasks` server. */
async function agentTool(chat: string, tool: string, args: unknown): Promise<void> {
  const replies = await sendAndWait(chat, `tasks ${tool} ${JSON.stringify(args)}`);
  expect(replies.join(" ")).toContain("Saved the task.");
}

async function titleValue(): Promise<string> {
  return browser.execute(
    () => (document.querySelector('[data-testid="task-title"]') as HTMLInputElement | null)?.value ?? "",
  );
}

// E2E-M11-1 Agents edit tasks (F-TK-02, F-TK-03): a thread's agent writes
// through the tasks MCP server; the Tasks page shows it at once; an agent's
// change to another field keeps the user's unsaved title; a change to the
// same field raises the conflict banner.
describe("E2E-M11-1 agents edit tasks", () => {
  let chat = "";

  before(async function () {
    this.timeout(90_000);
    await freshOffline();
    chat = await newChat((await lead()).id);
    await browser.keys([mod(), "1"]);
    const footer = await $('[data-testid="agent-footer-tasks"]');
    await footer.waitForDisplayed({ timeout: 10_000 });
    await footer.click();
    await $('[data-testid="tasks-panel"]').waitForDisplayed({ timeout: 10_000 });
  });

  it("F-TK-02 a thread's save_issue shows on the Tasks page within 500 ms", async function () {
    this.timeout(60_000);
    await agentTool(chat, "save_issue", { title: "Fix the login loop" });
    const shown = Date.now();
    await browser.waitUntil(
      async () =>
        (
          await browser.execute(() =>
            Array.from(document.querySelectorAll('[data-testid="task-row-title"]')).map(
              (e) => e.textContent ?? "",
            ),
          )
        ).includes("Fix the login loop"),
      { timeout: 500, interval: 25, timeoutMsg: "the agent's task didn't show within 500 ms" },
    );
    expect(Date.now() - shown).toBeLessThan(500);
    expect((await issue("TAN-1")).title).toBe("Fix the login loop");
    await expectNoSeriousA11yViolations();
  });

  it("F-TK-03.1 the agent changes the status while the user edits the title: both kept", async function () {
    this.timeout(60_000);
    const row = await $('[data-testid="task-row"][data-id="TAN-1"] button');
    await row.click();
    const title = await $('[data-testid="task-title"]');
    await title.waitForDisplayed({ timeout: 10_000 });
    await title.click();
    await title.addValue(" for good");
    await agentTool(chat, "save_issue", { id: "TAN-1", status: "in_progress", expected_revision: 1 });
    await browser.waitUntil(
      async () => (await $('[data-testid="task-detail"]').getText()).includes("In progress"),
      { timeout: 2_000, timeoutMsg: "the agent's status never showed" },
    );
    expect(await titleValue()).toBe("Fix the login loop for good");
    expect(await $('[data-testid="task-conflict"]').isExisting()).toBe(false);
    await browser.keys("Enter");
    await browser.waitUntil(async () => (await issue("TAN-1")).title === "Fix the login loop for good", {
      timeout: 5_000,
      timeoutMsg: "the title wasn't saved",
    });
    const saved = await issue("TAN-1");
    expect(saved.status).toBe("in_progress");
    expect(saved.revision).toBe(3);
  });

  it("F-TK-03.2 a same-field change shows the banner", async function () {
    this.timeout(60_000);
    const title = await $('[data-testid="task-title"]');
    await title.click();
    await title.addValue("!");
    const rev = (await issue("TAN-1")).revision;
    await agentTool(chat, "save_issue", { id: "TAN-1", title: "Agent's title", expected_revision: rev });
    const banner = await $('[data-testid="task-conflict"]');
    await banner.waitForDisplayed({ timeout: 2_000 });
    expect(await banner.getText()).toContain("Someone else changed the Title while you were editing.");
    expect(await titleValue()).toBe("Fix the login loop for good!");
    await expectNoSeriousA11yViolations();
    // Keep mine writes over it.
    const keep = await $("button=Keep mine");
    await keep.click();
    await browser.waitUntil(async () => (await issue("TAN-1")).title === "Fix the login loop for good!", {
      timeout: 5_000,
      timeoutMsg: "Keep mine didn't save",
    });
  });
});
