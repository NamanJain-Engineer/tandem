import { $, browser } from "@wdio/globals";
import { execFileSync } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { addFolder, freshOffline, userHome } from "../support/app";
import { newWorktree, pick } from "../support/worktrees";

// E2E-M18-WT10-1 Intent radar (F-WT-10): two worktrees of a TypeScript
// repository; one renames `class Session`, the other adds a call to
// `Session.refresh`. Check overlap lists `Session` for both with what each
// did, and the rail marks both worktrees.

const GIT_ENV = {
  ...process.env,
  GIT_AUTHOR_NAME: "E2E",
  GIT_AUTHOR_EMAIL: "e2e@vibezeo.test",
  GIT_COMMITTER_NAME: "E2E",
  GIT_COMMITTER_EMAIL: "e2e@vibezeo.test",
};
const git = (cwd: string, ...args: string[]) =>
  execFileSync("git", ["-C", cwd, ...args], { encoding: "utf8", env: GIT_ENV }).trim();

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

const symbolsIn = () =>
  browser.execute(() =>
    Array.from(document.querySelectorAll<HTMLElement>('[data-testid="overlap-symbol"]')).map((s) => ({
      symbol: s.dataset.symbol,
      text: s.textContent ?? "",
    })),
  );

describe("E2E-M18-WT10-1 intent radar", function () {
  this.timeout(240_000);
  const repo = `/tmp/tandem-e2e-wt10-${Date.now()}`;
  const wt = (name: string) => join(repo, ".tandem/worktrees", name);

  before(async () => {
    mkdirSync(join(repo, "src"), { recursive: true });
    git(repo, "init", "-q", "-b", "main");
    writeFileSync(join(repo, "src/session.ts"), "export class Session {\n  refresh(): void {}\n}\n");
    git(repo, "add", ".");
    git(repo, "commit", "-q", "-m", "first");
    writeFileSync(join(userHome(), ".gitconfig"), "[user]\n\tname = E2E\n\temail = e2e@vibezeo.test\n");
    await freshOffline();
    await addFolder(repo);
    await newWorktree(repo, "rename");
    await newWorktree(repo, "caller");
    writeFileSync(
      join(wt("rename"), "src/session.ts"),
      "export class UserSession {\n  refresh(): void {}\n}\n",
    );
    writeFileSync(
      join(wt("caller"), "src/use.ts"),
      "import { Session } from './session';\nexport const s = new Session();\ns.refresh();\n",
    );
  });

  it("F-WT-10.1 Check overlap lists Session for both worktrees, and the rail marks both", async () => {
    await worktreeAction("tandem/caller", "Check overlap…");
    await $('[data-testid="overlap-symbol"]').waitForExist({ timeout: 20_000 });
    const fromCaller = await symbolsIn();
    expect(fromCaller).toContainEqual({
      symbol: "Session",
      text: "Sessionused here, renamed or removed there",
    });
    await browser.keys("Escape");
    await $('[data-testid="overlap-symbol"]').waitForExist({ reverse: true, timeout: 10_000 });
    await worktreeAction("tandem/rename", "Check overlap…");
    await $('[data-testid="overlap-symbol"]').waitForExist({ timeout: 20_000 });
    const fromRename = await symbolsIn();
    expect(fromRename).toContainEqual({
      symbol: "Session",
      text: "Sessionrenamed or removed here, used there",
    });
    await browser.keys("Escape");
    await browser.waitUntil(
      async () =>
        browser.execute(() => document.querySelectorAll('[data-testid="worktree-radar"]').length === 2),
      { timeout: 10_000, timeoutMsg: "the rail didn't mark both worktrees" },
    );
  });
});
