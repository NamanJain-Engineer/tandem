import { $, browser } from "@wdio/globals";
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { addFolder, freshUser, home, planFooter, setPlan, settled, userHome } from "../support/app";
import { newWorktree, pick } from "../support/worktrees";

// E2E-M18-CD22-1 Multi-repo worktree (F-CD-22): a folder holding `api` and
// `web` repositories, on Pro. The row says "2 repositories"; New worktree
// makes tandem/<slug> in both under one parent folder; Open diff lists both
// repositories; Discard removes both worktrees and keeps both branches.

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

async function worktreeAction(branch: string, label: string): Promise<void> {
  await browser.waitUntil(
    async () => {
      const opened = await browser.execute((b: string) => {
        const button = document.querySelector<HTMLElement>(`button[aria-label="Actions for ${b}"]`);
        button?.focus();
        return button !== null && document.activeElement === button;
      }, branch);
      if (!opened) return false;
      await browser.keys("Enter");
      await browser.pause(300);
      return browser.execute(() => document.querySelectorAll("[role=menuitem]").length > 0);
    },
    { timeout: 15_000, timeoutMsg: `no menu for ${branch}` },
  );
  await pick(label);
}

describe("E2E-M18-CD22-1 multi-repo worktree", function () {
  this.timeout(300_000);
  const folder = `/tmp/tandem-e2e-cd22-${Date.now()}`;
  const parent = join(folder, ".tandem/worktrees/tidy");

  before(async () => {
    for (const name of ["api", "web"]) {
      const repo = join(folder, name);
      mkdirSync(join(repo, "src"), { recursive: true });
      git(repo, "init", "-q", "-b", "main");
      writeFileSync(join(repo, "src/app.ts"), `export const name = "${name}";\n`);
      git(repo, "add", ".");
      git(repo, "commit", "-q", "-m", "first");
    }
    writeFileSync(join(userHome(), ".gitconfig"), "[user]\n\tname = E2E\n\temail = e2e@vibezeo.test\n");
    const { email } = await freshUser("cd22");
    await setPlan(email, "pro");
    await browser.waitUntil(async () => (await planFooter()).startsWith("PRO"), {
      timeout: 20_000,
      timeoutMsg: "the plan never showed Pro",
    });
    await addFolder(folder);
  });

  it("F-CD-22.1 the folder shows its repositories, and one worktree spans both on one branch", async () => {
    const repos = $(`[data-testid="workspace-row"][data-root="${folder}"] [data-testid="workspace-repos"]`);
    await repos.waitForExist({ timeout: 15_000 });
    expect(String(await repos.getProperty("textContent"))).toBe("2 repositories");
    await newWorktree(folder, "tidy");
    for (const name of ["api", "web"]) {
      expect(git(join(parent, name), "branch", "--show-current")).toBe("tandem/tidy");
    }
  });

  it("F-CD-22.3 Open diff lists each repository's share of the changes", async () => {
    writeFileSync(join(parent, "web/src/app.ts"), 'export const name = "web";\nexport const two = 2;\n');
    await worktreeAction("tandem/tidy", "Open diff");
    await $('[data-testid="diff-repo"]').waitForExist({ timeout: 15_000 });
    const lines = await browser.execute(() =>
      Array.from(document.querySelectorAll<HTMLElement>('[data-testid="diff-repo"]')).map(
        (l) => l.textContent,
      ),
    );
    expect(lines).toEqual(["api · No changes", "web · 1 file"]);
    await browser.keys("Escape");
  });

  it("F-CD-22.5 Discard removes both worktrees and the parent folder, and keeps both branches", async () => {
    // The plain removal refuses (web has uncommitted work); the force asks natively: Yes.
    answerConfirm("yes");
    await worktreeAction("tandem/tidy", "Discard worktree…");
    await $('//label[normalize-space()="Type tandem/tidy to confirm"]/following::input[1]').setValue(
      "tandem/tidy",
    );
    await settled();
    await $("button=Discard worktree").click();
    await browser.waitUntil(async () => !existsSync(parent), {
      timeout: 20_000,
      timeoutMsg: "the parent folder is still there",
    });
    for (const name of ["api", "web"]) {
      expect(git(join(folder, name), "branch", "--list", "tandem/tidy")).not.toBe("");
    }
  });
});
