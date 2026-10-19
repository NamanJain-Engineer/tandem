import { $, browser } from "@wdio/globals";
import { execFileSync } from "node:child_process";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { addFolder, freshOffline, home, settled, userHome } from "../support/app";
import { newWorktree, pick, repoMenu } from "../support/worktrees";

// E2E-M18-WT07-1 Local merge queue (F-WT-07): four worktrees with a
// commit each; the last one's check fails. Land worktrees lands the first
// three in order with merge commits, stops at the fourth with its output
// and a way to hand it to an agent, never pushes, and Undo landing moves
// the branch back after the confirm.

const GIT_ENV = {
  ...process.env,
  GIT_AUTHOR_NAME: "E2E",
  GIT_AUTHOR_EMAIL: "e2e@vibezeo.test",
  GIT_COMMITTER_NAME: "E2E",
  GIT_COMMITTER_EMAIL: "e2e@vibezeo.test",
};
const git = (cwd: string, ...args: string[]) =>
  execFileSync("git", ["-C", cwd, ...args], { encoding: "utf8", env: GIT_ENV }).trim();

const answerConfirm = (answer: "yes" | "no") => writeFileSync(join(home(), "confirm.txt"), answer);

describe("E2E-M18-WT07-1 local merge queue", function () {
  this.timeout(300_000);
  const repo = `/tmp/tandem-e2e-wt07-${Date.now()}`;
  const names = ["alpha", "beta", "gamma", "bad"];
  let start = "";

  before(async () => {
    execFileSync("mkdir", ["-p", repo]);
    git(repo, "init", "-q", "-b", "main");
    writeFileSync(join(repo, "README.md"), "hello\n");
    git(repo, "add", ".");
    git(repo, "commit", "-q", "-m", "first");
    // A bare origin, to show nothing is pushed.
    execFileSync("git", ["init", "-q", "--bare", `${repo}.git`]);
    git(repo, "remote", "add", "origin", `${repo}.git`);
    git(repo, "push", "-q", "origin", "main");
    writeFileSync(join(userHome(), ".gitconfig"), "[user]\n\tname = E2E\n\temail = e2e@vibezeo.test\n");
    await freshOffline();
    await addFolder(repo);
    for (const name of names) {
      await newWorktree(repo, name);
      const wt = join(repo, ".tandem/worktrees", name);
      writeFileSync(join(wt, `${name}.txt`), `${name}\n`);
      if (name === "bad") writeFileSync(join(wt, "fail.txt"), "fails the check\n");
      git(wt, "add", ".");
      git(wt, "commit", "-q", "-m", name);
    }
    start = git(repo, "rev-parse", "main");
  });

  it("F-WT-07.1 three worktrees land in order; F-WT-07.2 the failing check stops the queue with its output", async () => {
    await repoMenu(repo);
    await pick("Land worktrees…");
    const rows = '[data-testid="landing-candidate"]';
    await $(rows).waitForExist({ timeout: 20_000 });
    const branches = await browser.execute(
      (sel: string) => Array.from(document.querySelectorAll<HTMLElement>(sel)).map((r) => r.dataset.branch),
      rows,
    );
    expect(branches).toEqual(names.map((n) => `tandem/${n}`));
    await $('[data-testid="landing-check"]').setValue("test ! -f fail.txt && echo checked");
    await settled();
    await $('[data-testid="landing-land"]').click();
    await browser.waitUntil(
      async () =>
        browser.execute(
          () =>
            document.querySelector<HTMLElement>('[data-testid="landing-steps"]')?.dataset.state === "stopped",
        ),
      { timeout: 120_000, timeoutMsg: "the landing never stopped" },
    );
    const states = await browser.execute(() =>
      Array.from(document.querySelectorAll<HTMLElement>('[data-testid="landing-step"]')).map(
        (s) => s.dataset.state,
      ),
    );
    expect(states).toEqual(["landed", "landed", "landed", "failed"]);
    // Three merge commits on main, in order, and nothing pushed.
    const merges = git(repo, "log", "--merges", "--first-parent", "--format=%s", `${start}..main`).split(
      "\n",
    );
    expect(merges).toEqual([
      "Merge branch 'tandem/gamma'",
      "Merge branch 'tandem/beta'",
      "Merge branch 'tandem/alpha'",
    ]);
    expect(git(repo, "rev-parse", "origin/main")).toBe(start);
    expect(git(`${repo}.git`, "rev-parse", "main")).toBe(start);
    // The failure, with a way to hand it on (no agent runs there: Copy).
    const failed = '[data-testid="landing-step"][data-state="failed"]';
    expect(await $(failed).getText()).toContain("Check failed (exit 1)");
    expect(await $(`${failed} [data-testid="landing-copy"]`).isExisting()).toBe(true);
  });

  it("F-WT-07.4 Undo landing moves main back after the confirm", async () => {
    answerConfirm("yes");
    const undo = $('[data-testid="landing-undo"]');
    await undo.waitForExist({ timeout: 10_000 });
    await settled();
    await undo.click();
    await browser.waitUntil(async () => git(repo, "rev-parse", "main") === start, {
      timeout: 20_000,
      timeoutMsg: "main wasn't moved back",
    });
    expect(git(repo, "status", "--porcelain", "--untracked-files=no")).toBe("");
    // The worktrees keep their commits.
    expect(git(repo, "rev-list", "--count", "main..tandem/alpha")).not.toBe("0");
  });
});
