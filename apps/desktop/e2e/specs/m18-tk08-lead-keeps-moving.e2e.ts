import { $, $$, browser } from "@wdio/globals";
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { addFolder, checkAgain, fakeLog, freshOffline, home, invoke, mod, userHome } from "../support/app";

const GIT_ENV = {
  ...process.env,
  GIT_AUTHOR_NAME: "E2E",
  GIT_AUTHOR_EMAIL: "e2e@vibezeo.test",
  GIT_COMMITTER_NAME: "E2E",
  GIT_COMMITTER_EMAIL: "e2e@vibezeo.test",
};
const git = (cwd: string, ...args: string[]) =>
  execFileSync("git", ["-C", cwd, ...args], { encoding: "utf8", env: GIT_ENV }).trim();

type Question = {
  id: string;
  issueId: string;
  state: string;
  question: string;
  reason: string | null;
  answer: string | null;
};
type Plan = {
  id: string;
  state: string;
  threadId: string;
  leadId: string;
  issueIds: string[];
  supervise: boolean;
  baseCommit: string | null;
  questions: Question[];
};
type AttemptView = { attempt: { issueId: string; threadId: string | null; state: string } };
type Envelope = { event: { type: string; text?: string } };

const GOAL = "Add OAuth sign-in with Google";
/** What the repository's committed CLAUDE.md says about tests. */
const RULE = "Use Vitest for every new test.";
const ROUTINE = "Which test runner should the new tests use?";
const PUSHING = "Should I push the branch to origin when I'm done?";
const ASKED = "A run in this plan asks a question.";

function repo(name: string): string {
  const dir = join(home(), name);
  mkdirSync(dir, { recursive: true });
  git(dir, "init", "-q", "-b", "main");
  writeFileSync(join(dir, "CLAUDE.md"), `${RULE}\n`);
  git(dir, "add", "CLAUDE.md");
  git(dir, "commit", "-q", "-m", "first");
  return dir;
}

/** What was sent to a thread, oldest first. */
async function sent(threadId: string): Promise<string[]> {
  const page = await invoke<{ items: Envelope[] }>("thread_events", { threadId, limit: 200 });
  return page.items
    .map((e) => e.event)
    .filter((e) => e.type === "user")
    .map((e) => e.text ?? "");
}

async function planNow(id: string): Promise<Plan> {
  return (await invoke<Plan[]>("plan_list")).find((p) => p.id === id)!;
}

async function runOf(issueId: string): Promise<AttemptView> {
  return (await invoke<AttemptView[]>("task_attempts_list")).find((r) => r.attempt.issueId === issueId)!;
}

/** The fake engine's starts of Lead's answer sessions: every tool off,
 * Read too (a Read-access chat turns off only the writing tools). */
const answerSessions = () =>
  fakeLog().filter(
    (l) => l.startsWith("start: claude") && l.includes('"--disallowedTools"') && l.includes(",Read,"),
  );

/** Text a WebKitWebDriver `getText` would drop. */
async function textOf(el: { getProperty: (name: string) => Promise<unknown> }): Promise<string> {
  return String(await el.getProperty("textContent"));
}

// E2E-M18-TK-08 Lead keeps a plan moving (F-TK-08): with Lead's answers on,
// a run's routine question is answered by Lead from the committed
// CLAUDE.md, in a fresh session of its engine with every tool off (never
// Lead's plan chat), labelled, on the task and counted; a question about
// pushing never reaches Lead, comes to you with why, and your answer lets
// its run go on. No account (D-869).
describe("E2E-M18-TK-08 Lead keeps a plan moving", function () {
  this.timeout(600_000);
  // The fake Claude Code says it's 2.1.310, the first that answers with
  // every tool off; the next spec gets the default back.
  const versions = () => join(home(), "fake-version.txt");
  after(async () => {
    writeFileSync(versions(), "");
    await checkAgain();
  });

  it("F-TK-08.1 F-TK-08.2 F-TK-08.3 Lead answers the routine question from CLAUDE.md, and the push question comes to you", async () => {
    await freshOffline();
    writeFileSync(versions(), "claude 2.1.310\n");
    await checkAgain();
    writeFileSync(join(userHome(), ".gitconfig"), "[user]\n\tname = E2E\n\temail = e2e@vibezeo.test\n");
    const dir = repo("tk08");
    // An untracked secret beside the main checkout: an engine reading the
    // repository would see it.
    writeFileSync(join(dir, ".env"), "TOKEN=e2e-secret\n");
    await addFolder(dir);
    await browser.keys([mod(), "1"]);
    await $('[data-testid="agent-footer-tasks"]').click();
    await $('[data-testid="plan-with-lead"]').click();
    await $('[data-testid="plan-goal"]').setValue(GOAL);
    await $('[data-testid="plan-start"]').click();
    await $('[data-testid="plan-dialog"][data-state="ready"]').waitForExist({ timeout: 60_000 });

    // The fake's runs ask: `[ask]` a routine question, `[ask push]` one
    // about pushing.
    const titles = await $$('[data-testid="plan-row-title"]');
    await titles[0]!.setValue("Add the OAuth routes [ask]");
    await titles[1]!.setValue("Write the sign-in page [ask push]");
    await $('[data-testid="plan-create"]').click();
    await $('[data-testid="plan-created"]').waitForExist({ timeout: 10_000 });
    await $('[data-testid="plan-execute"]').click();

    // Execute all's check offers Lead's answers, on, and says whose engine
    // and subscription they use.
    const option = $('[data-testid="plan-supervise-option"]');
    await option.waitForExist({ timeout: 10_000 });
    expect(await textOf(option)).toContain(
      "Lead answers routine questions from the plan's task and the repository's instructions, using your engine and subscription",
    );
    expect(await textOf(option)).toContain("can't open files or run tools");
    expect(await option.$('[role="checkbox"]').getAttribute("aria-checked")).toBe("true");
    await $('[data-testid="plan-confirm-run"]').click();
    await $('[data-testid="plan-dialog"][data-state="started"]').waitForExist({ timeout: 15_000 });
    const plan = (await invoke<Plan[]>("plan_list")).find((p) => p.state === "started")!;
    expect(plan.supervise).toBe(true);
    expect(plan.baseCommit).toBe(git(dir, "rev-parse", "HEAD"));
    const [routine, pushing] = plan.issueIds as [string, string];

    // The push question never reaches Lead: Needs you, with why.
    const needs = $('[data-testid="plan-run-needs-you"]');
    await needs.waitForExist({ timeout: 180_000 });
    expect(await textOf(needs.$('[data-testid="plan-question"]'))).toBe(PUSHING);
    expect(await textOf(needs.$('[data-testid="plan-question-why"]'))).toBe(
      "It's about pushing, which always comes to you.",
    );

    // The routine question reaches Lead once, in a session of its own with
    // CLAUDE.md as committed, and Lead's answer comes from it.
    const answered = await browser.waitUntil(
      async () =>
        (await planNow(plan.id)).questions.find((q) => q.issueId === routine && q.state === "answered") ??
        false,
      {
        timeout: 180_000,
        timeoutMsg: `Lead didn't answer: ${JSON.stringify((await planNow(plan.id)).questions)}`,
      },
    );
    // Exactly the committed rule: nothing from the `.env` beside it.
    expect([answered.question, answered.answer]).toEqual([ROUTINE, RULE]);
    // The session's own folder went with it.
    const answers = join(home(), "run", "answers");
    expect(existsSync(answers) ? readdirSync(answers).filter((n) => n.startsWith("ans_")) : []).toEqual([]);
    // Never in Lead's plan chat: a session with no tool at all (and every
    // one denied besides), no customization, MCP server or command, nothing
    // saved, nobody to ask, and nothing to resume.
    expect((await sent(plan.threadId)).filter((t) => t.includes(ASKED))).toHaveLength(0);
    const sessions = answerSessions();
    expect(sessions).toHaveLength(1);
    const argv = sessions[0]!;
    expect(argv).toContain('"--tools", ""');
    for (const tool of [
      "Bash",
      "PowerShell",
      "Read",
      "Glob",
      "Grep",
      "Edit",
      "Write",
      "WebFetch",
      "Task",
      "CronCreate",
      "Monitor",
      "ToolSearch",
    ])
      expect(argv).toMatch(new RegExp(`"--disallowedTools", "[^"]*\\b${tool}\\b`));
    for (const flag of [
      '"--safe-mode"',
      '"--disable-slash-commands"',
      '"--no-session-persistence"',
      '"--strict-mcp-config"',
    ])
      expect(argv).toContain(flag);
    expect(argv).toContain('"--permission-mode", "dontAsk"');
    expect(argv).toContain('"--permission-prompts", "none"');
    expect(argv).not.toContain("--mcp-config");
    expect(argv).not.toContain("--resume");
    expect(argv).not.toContain("--permission-prompt-tool");

    // The answer reaches the run labelled, and the task as Lead's comment;
    // the row counts it, and the run goes on to review.
    const run = await runOf(routine);
    expect(
      (await sent(run.attempt.threadId!)).some((t) =>
        t.startsWith(`Lead (automatic answer):\n\`\`\`text\n${RULE}\n\`\`\`\n`),
      ),
    ).toBe(true);
    const task = await invoke<{ comments: Array<{ actor: string; body: string }> }>("task_get", {
      id: routine,
    });
    expect(
      task.comments.some(
        (c) =>
          c.actor === plan.leadId && c.body.startsWith("Lead (automatic answer)") && c.body.endsWith(RULE),
      ),
    ).toBe(true);
    await browser.waitUntil(async () => (await runOf(routine)).attempt.state === "in_review", {
      timeout: 120_000,
      timeoutMsg: "the answered run didn't go on to review",
    });
    expect(await textOf($('[data-testid="plan-run-answered"]'))).toBe("Lead answered 1");

    // Answer… sends your answer to the push question's run, which goes on.
    await needs.$('[data-testid="plan-answer-open"]').click();
    await needs.$('[data-testid="plan-answer-text"]').setValue("Don't push; I'll push it myself.");
    await needs.$('[data-testid="plan-answer-send"]').click();
    await $('[data-testid="plan-run-needs-you"]').waitForExist({ reverse: true, timeout: 15_000 });
    await browser.waitUntil(async () => (await runOf(pushing)).attempt.state === "in_review", {
      timeout: 120_000,
      timeoutMsg: "the run you answered didn't go on to review",
    });
    expect(await sent((await runOf(pushing)).attempt.threadId!)).toContain(
      "Don't push; I'll push it myself.",
    );
    const after = await planNow(plan.id);
    expect(after.questions.map((q) => [q.issueId, q.state]).sort()).toEqual(
      [
        [routine, "answered"],
        [pushing, "answered_by_you"],
      ].sort(),
    );
    // Lead was only ever asked the routine question, and never in its chat.
    expect(answerSessions()).toHaveLength(1);
    expect((await sent(plan.threadId)).filter((t) => t.includes(ASKED))).toHaveLength(0);
  });
});
