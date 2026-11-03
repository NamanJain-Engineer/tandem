import { $, $$, browser } from "@wdio/globals";
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import {
  addFolder,
  clickAllow,
  freshOffline,
  home,
  invoke,
  mod,
  relaunch,
  userHome,
  waitFor,
} from "../support/app";
import { newWorktree } from "../support/worktrees";

const GIT_ENV = {
  ...process.env,
  GIT_AUTHOR_NAME: "E2E",
  GIT_AUTHOR_EMAIL: "e2e@vibezeo.test",
  GIT_COMMITTER_NAME: "E2E",
  GIT_COMMITTER_EMAIL: "e2e@vibezeo.test",
};
const git = (cwd: string, ...args: string[]) =>
  execFileSync("git", ["-C", cwd, ...args], { encoding: "utf8", env: GIT_ENV }).trim();

type Ws = { id: string; root: string; parentId: string | null };
type Claim = { path: string; folder: string; inferred: boolean };
type AttemptView = { attempt: { issueId: string; laneWorkspaceId: string | null } };
type Plan = { id: string; state: string; issueIds: string[] };

const GOAL = "Add OAuth sign-in with Google";

function repo(name: string): string {
  const dir = join(home(), name);
  mkdirSync(dir, { recursive: true });
  git(dir, "init", "-q", "-b", "main");
  writeFileSync(join(dir, "README.md"), "# shop\n");
  git(dir, "add", "README.md");
  git(dir, "commit", "-q", "-m", "first");
  return dir;
}

/** `$TANDEM_HOME/tasks/claims.json`: what every working tree claims. */
function claims(): Claim[] {
  const file = join(home(), "tasks", "claims.json");
  return existsSync(file) ? (JSON.parse(readFileSync(file, "utf8")) as Claim[]) : [];
}

/** Text a WebKitWebDriver `getText` would drop. */
async function text(selector: string): Promise<string> {
  return String(await $(selector).getProperty("textContent"));
}

/** Each plan row's run state and verdict. */
async function rows(): Promise<Array<{ state: string; verdict: string }>> {
  const out: Array<{ state: string; verdict: string }> = [];
  for (const r of await $$('[data-testid="plan-run"]')) {
    out.push({
      state: (await r.getAttribute("data-state")) ?? "",
      verdict: (await r.getAttribute("data-verdict")) ?? "",
    });
  }
  return out;
}

/** Agent mode's Tasks page, Plan with Lead…, the goal, and Lead's draft. */
async function planToDraft(): Promise<void> {
  await browser.keys([mod(), "1"]);
  await $('[data-testid="agent-footer-tasks"]').click();
  await $('[data-testid="plan-with-lead"]').click();
  await $('[data-testid="plan-goal"]').setValue(GOAL);
  await $('[data-testid="plan-start"]').click();
  await $('[data-testid="plan-goal-form"]').waitForExist({ reverse: true, timeout: 15_000 });
  await $('[data-testid="plan-dialog"][data-state="ready"]').waitForExist({ timeout: 60_000 });
}

// E2E-M18-TK-07 Follow a plan through (F-TK-07): a chat thread's edit
// claims its file; a plan follows its runs to Approve, lands the approved
// worktrees through Land worktrees in its order, and is still there after
// a restart. With no account (D-869).
describe("E2E-M18-TK-07 follow a plan through", function () {
  this.timeout(600_000);
  let plan: Plan;

  it("F-TK-07.1 a thread in a worktree that writes a file claims it", async () => {
    await freshOffline();
    writeFileSync(join(userHome(), ".gitconfig"), "[user]\n\tname = E2E\n\temail = e2e@vibezeo.test\n");
    const dir = repo("tk07-claims");
    await addFolder(dir);
    await newWorktree(dir, "notes");
    const lane = (await invoke<Ws[]>("workspace_list")).find((w) =>
      w.root.endsWith("/.tandem/worktrees/notes"),
    )!;
    const thread = await invoke<{ id: string }>("thread_create", {
      args: { kind: "thread", workspaceId: lane.id, engineId: "claude", access: "edits" },
    });
    await invoke("thread_send", { threadId: thread.id, text: "write notes.md=hello" });
    const claimed = await waitFor(
      claims,
      (cs) => cs.some((c) => c.path === "notes.md"),
      "notes.md wasn't claimed",
      { timeout: 30_000 },
    );
    const claim = claimed.find((c) => c.path === "notes.md")!;
    expect(claim.inferred).toBe(true);
    expect(claim.folder.endsWith("/.tandem/worktrees/notes")).toBe(true);
  });

  it("F-TK-07.2 F-TK-07.4 the plan follows its runs to Approve, and Land 2 approved… opens Land worktrees with both, in plan order", async () => {
    const dir = repo("tk07");
    await addFolder(dir);
    await planToDraft();
    await $('[data-testid="plan-create"]').click();
    await $('[data-testid="plan-created"]').waitForExist({ timeout: 10_000 });
    await browser.waitUntil(async () => (await rows()).length === 2, { timeout: 10_000 });
    expect((await rows()).map((r) => r.state)).toEqual(["none", "none"]);
    await $('[data-testid="plan-execute"]').click();
    await $('[data-testid="plan-confirm-run"]').waitForExist({ timeout: 10_000 });
    // Claude Code reviews both runs, the one chat engine installed: a fake
    // engine another spec left installed would review them instead.
    const reviewers = await browser.execute(() =>
      Array.from(document.querySelectorAll<HTMLElement>('[data-testid="plan-launch-run"]')).map(
        (r) => r.dataset.reviewEngine ?? "",
      ),
    );
    expect(reviewers).toEqual(["claude", "claude"]);
    await clickAllow('[data-testid="plan-confirm-run"]');
    await $('[data-testid="plan-dialog"][data-state="started"]').waitForExist({ timeout: 15_000 });

    // The plan follows each run to in review, approved by the review.
    await waitFor(
      rows,
      (rs) => rs.length === 2 && rs.every((r) => r.state === "in_review" && r.verdict === "approve"),
      "the runs didn't reach Approve",
      { timeout: 180_000 },
    );
    expect(await text('[data-testid="plan-summary"]')).toBe("2 approved");

    // Each run's worktree gets a commit, so it can land.
    plan = (await invoke<Plan[]>("plan_list")).find((p) => p.state === "started")!;
    const runs = await invoke<AttemptView[]>("task_attempts_list");
    const workspaces = await invoke<Ws[]>("workspace_list");
    const lanes = plan.issueIds.map(
      (id) => runs.find((r) => r.attempt.issueId === id)!.attempt.laneWorkspaceId!,
    );
    for (const [i, lane] of lanes.entries()) {
      const root = workspaces.find((w) => w.id === lane)!.root;
      writeFileSync(join(root, `part-${i}.txt`), `${plan.issueIds[i]}\n`);
      git(root, "add", ".");
      git(root, "commit", "-q", "-m", plan.issueIds[i]!);
    }

    expect(await text('[data-testid="plan-land"]')).toBe("Land 2 approved…");
    await $('[data-testid="plan-land"]').click();
    await $('[data-testid="landing-candidate"]').waitForExist({ timeout: 20_000 });
    const chosen = await browser.waitUntil(
      async () => {
        const listed = await browser.execute(() =>
          Array.from(document.querySelectorAll<HTMLElement>('[data-testid="landing-candidate"]')).map(
            (r) => ({
              lane: r.dataset.lane ?? "",
              ticked: r.querySelector('[role="checkbox"]')?.getAttribute("aria-checked") === "true",
            }),
          ),
        );
        const ticked = listed.filter((r) => r.ticked).map((r) => r.lane);
        return ticked.length === 2 ? ticked : false;
      },
      { timeout: 20_000, timeoutMsg: "Land worktrees didn't choose the plan's two worktrees" },
    );
    expect(chosen).toEqual(lanes);
    await browser.keys("Escape");
  });

  it("F-TK-07.3 after a restart the plan is still listed with its tasks", async () => {
    await relaunch();
    await browser.keys([mod(), "1"]);
    await $('[data-testid="agent-footer-tasks"]').click();
    const chip = await $('[data-testid="plan-reopen"]');
    await chip.waitForExist({ timeout: 20_000 });
    expect(String(await chip.getProperty("textContent"))).toContain(`Plan: ${GOAL}`);
    await chip.click();
    await $('[data-testid="plan-dialog"][data-state="started"]').waitForExist({ timeout: 10_000 });
    await browser.waitUntil(async () => (await rows()).length === plan.issueIds.length, { timeout: 15_000 });
    expect((await rows()).map((r) => r.state)).toEqual(["in_review", "in_review"]);
    // Remove plan forgets it; its tasks stay.
    await $('[data-testid="plan-remove"]').click();
    await $('[data-testid="plan-reopen"]').waitForExist({ reverse: true, timeout: 10_000 });
    expect(await invoke<Plan[]>("plan_list")).toEqual([]);
    for (const id of plan.issueIds)
      expect((await invoke<{ issue: { id: string } }>("task_get", { id })).issue.id).toBe(id);
  });
});
