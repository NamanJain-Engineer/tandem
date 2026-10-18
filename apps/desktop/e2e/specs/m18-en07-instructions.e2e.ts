import { $, browser } from "@wdio/globals";
import { execFileSync } from "node:child_process";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { addFolder, freshOffline, settled } from "../support/app";
import { pick, repoMenu } from "../support/worktrees";

const GIT_ENV = {
  ...process.env,
  GIT_AUTHOR_NAME: "E2E",
  GIT_AUTHOR_EMAIL: "e2e@vibezeo.test",
  GIT_COMMITTER_NAME: "E2E",
  GIT_COMMITTER_EMAIL: "e2e@vibezeo.test",
};
const git = (cwd: string, ...args: string[]) =>
  execFileSync("git", ["-C", cwd, ...args], { encoding: "utf8", env: GIT_ENV }).trim();

const textOf = (selector: string) =>
  browser.execute((s: string) => document.querySelector(s)?.textContent ?? "", selector) as Promise<string>;

async function openInstructions(repo: string): Promise<void> {
  await repoMenu(repo);
  await pick("Instructions…");
  await $('[data-testid="instructions-engine"]').waitForExist({ timeout: 15_000 });
}

// E2E-M18-EN07 Instructions (F-EN-07): what each engine reads, one file for
// all with Undo, and a personal file copied into new worktrees.
describe("E2E-M18-EN07 instructions", function () {
  this.timeout(180_000);
  const repo = `/tmp/tandem-e2e-en07-${Date.now()}`;
  const claudeMd = "# Rules\nUse pnpm.\nNever push to main.\n";

  before(async () => {
    mkdirSync(repo, { recursive: true });
    git(repo, "init", "-q", "-b", "main");
    writeFileSync(join(repo, ".gitignore"), "CLAUDE.local.md\n");
    writeFileSync(join(repo, "CLAUDE.md"), claudeMd);
    writeFileSync(join(repo, "AGENTS.md"), "# Rules\nUse pnpm.\n");
    git(repo, "add", ".");
    git(repo, "commit", "-q", "-m", "first");
    writeFileSync(join(repo, "CLAUDE.local.md"), "My sandbox is on port 4100.\n");
    await freshOffline();
    await addFolder(repo);
  });

  it("F-EN-07.1 the panel says Claude Code reads CLAUDE.md, not AGENTS.md, and shows the difference", async () => {
    await openInstructions(repo);
    expect(await textOf('[data-testid="instructions-engine"][data-engine="claude"] [data-testid="instructions-reads"]')).toBe(
      "Reads CLAUDE.md, CLAUDE.local.md, not AGENTS.md",
    );
    await settled();
    await $('[data-testid="instructions-file"][data-path="CLAUDE.md"] [data-testid="instructions-diff-toggle"]').click();
    await browser.waitUntil(
      async () => (await textOf('[data-testid="instructions-file"][data-path="CLAUDE.md"]')).includes("Never push to main."),
      { timeout: 5_000, timeoutMsg: "no difference shown" },
    );
  });

  it("F-EN-07.4 Copy into worktrees adds the ignored CLAUDE.local.md to the worktree setup", async () => {
    await $('[data-testid="instructions-file"][data-path="CLAUDE.local.md"] [data-testid="instructions-not-in-worktrees"] button').click();
    await browser.waitUntil(
      async () => {
        try {
          return (JSON.parse(readFileSync(join(repo, ".vibezeo/worktree.json"), "utf8")) as { copy: string[] }).copy.includes(
            "CLAUDE.local.md",
          );
        } catch {
          return false;
        }
      },
      { timeout: 10_000, timeoutMsg: "CLAUDE.local.md wasn't added to copy" },
    );
    await $('[data-testid="instructions-not-in-worktrees"]').waitForExist({ reverse: true, timeout: 10_000 });
  });

  it("F-EN-07.2 Use one file for all writes nothing until confirmed, then Undo restores CLAUDE.md byte for byte", async () => {
    await $('[data-testid="instructions-one-file"]').click();
    await $('[data-testid="instructions-change"][data-path="CLAUDE.md"]').waitForExist({ timeout: 10_000 });
    expect(await $('[data-testid="instructions-text-lost"]').isExisting()).toBe(true);
    expect(readFileSync(join(repo, "CLAUDE.md"), "utf8")).toBe(claudeMd);
    await settled();
    await $('[data-testid="instructions-confirm"]').click();
    await browser.waitUntil(() => readFileSync(join(repo, "CLAUDE.md"), "utf8") === "@AGENTS.md\n", {
      timeout: 10_000,
      timeoutMsg: "CLAUDE.md wasn't pointed at AGENTS.md",
    });
    await browser.waitUntil(
      async () =>
        (await textOf('[data-testid="instructions-engine"][data-engine="claude"] [data-testid="instructions-reads"]')).startsWith(
          "Reads AGENTS.md",
        ),
      { timeout: 10_000, timeoutMsg: "the panel didn't update" },
    );
    const undo = $('//*[@role="region"][@aria-label="Notifications"]//button[normalize-space()="Undo"]');
    await undo.waitForExist({ timeout: 5_000 });
    await browser.execute((el: HTMLElement) => el.click(), await undo);
    await browser.waitUntil(() => readFileSync(join(repo, "CLAUDE.md"), "utf8") === claudeMd, {
      timeout: 10_000,
      timeoutMsg: "Undo didn't restore CLAUDE.md",
    });
  });
});
