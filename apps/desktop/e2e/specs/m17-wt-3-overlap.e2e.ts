import { $, browser } from "@wdio/globals";
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { addFolder, freshOffline, userHome } from "../support/app";

const GIT_ENV = {
  ...process.env,
  GIT_AUTHOR_NAME: "E2E",
  GIT_AUTHOR_EMAIL: "e2e@vibezeo.test",
  GIT_COMMITTER_NAME: "E2E",
  GIT_COMMITTER_EMAIL: "e2e@vibezeo.test",
};
const git = (cwd: string, ...args: string[]) =>
  execFileSync("git", ["-C", cwd, ...args], { encoding: "utf8", env: GIT_ENV }).trim();

async function pick(label: string): Promise<void> {
  await browser.waitUntil(
    async () =>
      browser.execute((l: string) => {
        const item = Array.from(document.querySelectorAll<HTMLElement>("[role=menuitem]")).find(
          (el) => el.textContent?.trim() === l,
        );
        item?.click();
        return item !== undefined;
      }, label),
    { timeout: 5_000, timeoutMsg: `no menu item ${label}` },
  );
}

async function repoMenu(root: string): Promise<void> {
  await browser.execute((r: string) => {
    const row = document.querySelector<HTMLElement>(`[data-testid="workspace-row"][data-root="${r}"]`);
    const box = row?.getBoundingClientRect();
    row?.dispatchEvent(
      new MouseEvent("contextmenu", {
        bubbles: true,
        cancelable: true,
        button: 2,
        clientX: (box?.left ?? 0) + 24,
        clientY: (box?.top ?? 0) + 8,
      }),
    );
  }, root);
}

async function newWorktree(root: string, task: string): Promise<void> {
  await repoMenu(root);
  await pick("New worktree…");
  await $('[data-testid="worktree-name"]').setValue(task);
  await $('[data-testid="worktree-create"]').click();
  await $('[data-testid="worktree-name"]').waitForExist({ reverse: true, timeout: 20_000 });
}

async function worktreeAction(branch: string, label: string): Promise<void> {
  // The row's ⋯ button shows on focus; under load the first Enter can land
  // before the row is there, so open the menu until it is.
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

const textOf = async (selector: string) => String(await $(selector).getProperty("textContent"));

// E2E-M17-WT-3 Overlap check (F-WT-03).
describe("E2E-M17-WT-3 overlap check", function () {
  this.timeout(240_000);
  const repo = `/tmp/tandem-e2e-wt-3-${Date.now()}`;
  const wt = (slug: string) => join(repo, ".tandem/worktrees", slug);

  before(async () => {
    mkdirSync(repo, { recursive: true });
    git(repo, "init", "-q", "-b", "main");
    writeFileSync(join(repo, "app.ts"), "export const a = 1;\n");
    writeFileSync(join(repo, "README.md"), "hello\n");
    git(repo, "add", ".");
    git(repo, "commit", "-q", "-m", "first");
    writeFileSync(join(userHome(), ".gitconfig"), "[user]\n\tname = E2E\n\temail = e2e@vibezeo.test\n");
    await freshOffline();
    await addFolder(repo);
    await newWorktree(repo, "left");
    await newWorktree(repo, "right");
    await newWorktree(repo, "docs");
    await browser.waitUntil(() => existsSync(wt("docs")), { timeout: 10_000 });
  });

  it("F-WT-03.1 worktrees that change the same file list each other; one that doesn't lists none", async () => {
    writeFileSync(join(wt("left"), "app.ts"), "export const a = 2;\n");
    git(wt("left"), "commit", "-q", "-am", "left");
    writeFileSync(join(wt("right"), "app.ts"), "export const a = 3;\n");
    writeFileSync(join(wt("docs"), "GUIDE.md"), "guide\n");
    await worktreeAction("tandem/left", "Check overlap…");
    const sibling = $('[data-testid="overlap-sibling"]');
    await sibling.waitForExist({ timeout: 15_000 });
    expect(await sibling.getAttribute("data-branch")).toBe("tandem/right");
    expect(await textOf('[data-testid="overlap-sibling"]')).toContain("app.ts");
    await browser.keys("Escape");
    await $('[data-testid="worktree-overlap"]').waitForExist({ reverse: true, timeout: 5_000 });
    await worktreeAction("tandem/docs", "Check overlap…");
    await $('[data-testid="overlap-none"]').waitForExist({ timeout: 15_000 });
    await browser.keys("Escape");
    await $('[data-testid="worktree-overlap"]').waitForExist({ reverse: true, timeout: 5_000 });
  });

  it("F-WT-03.2 the merge confirm forecasts the conflict, and checking leaves the repository untouched", async () => {
    writeFileSync(join(repo, "app.ts"), "export const a = 9;\n");
    git(repo, "commit", "-q", "-am", "main");
    const head = git(repo, "rev-parse", "HEAD");
    await worktreeAction("tandem/left", "Merge into main…");
    const verdict = $('[data-testid="merge-forecast"]');
    await verdict.waitForExist({ timeout: 15_000 });
    expect(await verdict.getAttribute("data-verdict")).toBe("conflicts");
    expect(await textOf('[data-testid="merge-forecast"]')).toContain("app.ts");
    await browser.keys("Escape");
    expect(git(repo, "rev-parse", "HEAD")).toBe(head);
    expect(existsSync(join(repo, ".git/MERGE_HEAD"))).toBe(false);
    expect(git(repo, "status", "--porcelain")).toBe("");
  });

  it("F-WT-03.3 a worktree with nothing shared forecasts no conflicts", async () => {
    git(wt("docs"), "add", ".");
    git(wt("docs"), "commit", "-q", "-m", "docs");
    await worktreeAction("tandem/docs", "Check overlap…");
    const verdict = $('[data-testid="merge-forecast"]');
    await verdict.waitForExist({ timeout: 15_000 });
    expect(await verdict.getAttribute("data-verdict")).toBe("clean");
    expect(await $('[data-testid="overlap-none"]').isExisting()).toBe(true);
  });
});
