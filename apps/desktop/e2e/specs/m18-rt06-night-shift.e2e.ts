import { $, browser } from "@wdio/globals";
import { execFileSync } from "node:child_process";
import { mkdirSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { addFolder, checkAgain, fakeEngine, freshOffline, home, invoke, mod } from "../support/app";

const GIT_ENV = {
  ...process.env,
  GIT_AUTHOR_NAME: "E2E",
  GIT_AUTHOR_EMAIL: "e2e@vibezeo.test",
  GIT_COMMITTER_NAME: "E2E",
  GIT_COMMITTER_EMAIL: "e2e@vibezeo.test",
};
const git = (cwd: string, ...args: string[]) =>
  execFileSync("git", ["-C", cwd, ...args], { encoding: "utf8", env: GIT_ENV }).trim();

type NightRun = { id: string; issueId: string; accountId: string; state: string; attemptId: string | null };
type AttemptView = { attempt: { id: string; issueId: string; state: string; accountId: string | null } };
type Ws = { id: string; root: string };
type Account = { id: string; engineId: string };
type Page = { items: Array<{ kind: string; title: string }> };

/** The fake `codex app-server`'s windows ("97": the 5-hour window, resetting in 2 h). */
const usage = (pcts: string) => writeFileSync(join(home(), "usage.txt"), pcts);
const runs = () => invoke<NightRun[]>("night_list");
const tick = (dueNow: boolean, resetSummary = false) => invoke("test_night_tick", { dueNow, resetSummary });

async function openTask(id: string): Promise<void> {
  await browser.keys([mod(), "1"]);
  await $('[data-testid="agent-footer-tasks"]').click();
  await $(`[data-testid="task-row"][data-id="${id}"] button`).click();
  await $('[data-testid="task-night"]').waitForDisplayed({ timeout: 10_000 });
}

// E2E-M18-RT06 Night Shift (F-RT-06; free, D-2902): "Run at reset" on a task shows
// the plan and saves it; a due run waits while its account is still full,
// then starts task execution on that account only; Cancel removes a run; the
// summary counts the runs that ended.
describe("E2E-M18-RT06 night shift", function () {
  this.timeout(300_000);
  const repo = join(home(), "m18-rt06-repo");
  let ws = "";
  let codexAccount = "";

  before(async () => {
    fakeEngine("codex", join(home(), "fakebin"));
    usage("97");
    // Offline, with no account: the Night Shift needs neither (D-2902, D-2914).
    await freshOffline();
    await checkAgain();
    mkdirSync(repo, { recursive: true });
    git(repo, "init", "-q", "-b", "main");
    writeFileSync(join(repo, "README.md"), "hello\n");
    git(repo, "add", "README.md");
    git(repo, "commit", "-q", "-m", "first");
    await addFolder(repo);
    ws = (await invoke<Ws[]>("workspace_list")).find((w) => w.root === repo)!.id;
    codexAccount =
      (await invoke<Account[]>("accounts_list")).find((a) => a.engineId === "codex")?.id ??
      (await invoke<Account>("test_account_create", { engineId: "codex", label: "Work" })).id;
    await invoke("usage_refresh", { accountId: codexAccount });
    await invoke("settings_set", { key: "night_shift_summary_at", value: "00:00" });
  });
  after(async () => {
    rmSync(join(home(), "fakebin", "codex"), { force: true });
    rmSync(join(home(), "usage.txt"), { force: true });
    await checkAgain();
  });

  it("F-RT-06.1 F-RT-06.4 Run at reset shows the plan and saves it; Cancel removes it", async () => {
    const id = (await invoke<{ id: string }>("task_save", { input: { title: "Cancel me" } })).id;
    await openTask(id);
    await $('[data-testid="task-night"]').click();
    await $('[data-testid="night-dialog"]').waitForDisplayed({ timeout: 10_000 });
    const plan = '[data-testid="night-plan"]';
    await $(plan).waitForExist({ timeout: 15_000 });
    expect(String(await $(plan).getProperty("textContent"))).toMatch(
      /^Starts Cancel me on .+ \(Codex\) at .+, when its 5-hour window resets\. In a new worktree, with Edits access\. It waits for that account's reset and never uses another account\.$/,
    );
    await $('[data-testid="night-schedule"]').click();
    await $('[data-testid="night-run"]').waitForExist({ timeout: 10_000 });
    expect(String(await $('[data-testid="night-run"]').getProperty("textContent"))).toMatch(
      /^Runs at .+ on /,
    );
    const [run] = (await runs()).filter((r) => r.issueId === id);
    expect(run?.state).toBe("scheduled");
    expect(run?.accountId).toBe(codexAccount);
    // Nothing starts before the reset.
    await tick(false);
    expect((await runs()).find((r) => r.issueId === id)?.state).toBe("scheduled");
    await $('[data-testid="night-cancel"]').click();
    await $('[data-testid="night-run"]').waitForExist({ reverse: true, timeout: 5_000 });
    expect((await runs()).some((r) => r.issueId === id)).toBe(false);
  });

  it("F-RT-06.3 F-RT-06.2 F-RT-06.6 a full account waits; after the reset the run starts on that account, and the summary counts it", async () => {
    const id = (await invoke<{ id: string }>("task_save", { input: { title: "Write tests for billing" } }))
      .id;
    await invoke<NightRun>("night_schedule", {
      input: {
        issueId: id,
        workspaceId: ws,
        accountId: codexAccount,
        isolation: "lane",
        access: "edits",
        fullConfirmed: false,
        review: false,
      },
    });
    // Due, but the account is still at 97% with a later reset: it waits.
    await tick(true);
    const waiting = (await runs()).find((r) => r.issueId === id);
    expect(waiting?.state).toBe("waiting");
    // The window reset: the next pass starts it, on the chosen account.
    usage("3");
    await tick(true);
    const started = (await runs()).find((r) => r.issueId === id);
    expect(started?.state).toBe("started");
    const attempt = (await invoke<AttemptView[]>("task_attempts_list")).find((a) => a.attempt.issueId === id);
    expect(attempt?.attempt.accountId).toBe(codexAccount);
    // The attempt ends; the run follows, and the morning summary counts it.
    await browser.waitUntil(
      async () => {
        await tick(false);
        return (await runs()).find((r) => r.issueId === id)?.state === "done";
      },
      { timeout: 120_000, interval: 2_000, timeoutMsg: "the run never finished" },
    );
    await tick(false, true);
    const items = (await invoke<Page>("notifications_list", {})).items.filter(
      (n) => n.kind === "night_shift",
    );
    expect(items.map((n) => n.title)).toContain("Night Shift: 1 done");
    // Nothing new: no second summary.
    await tick(false, true);
    expect(
      (await invoke<Page>("notifications_list", {})).items.filter((n) => n.kind === "night_shift").length,
    ).toBe(items.length);
  });
});
