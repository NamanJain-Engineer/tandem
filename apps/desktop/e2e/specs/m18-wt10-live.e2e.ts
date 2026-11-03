import { $, browser } from "@wdio/globals";
import { execFileSync } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { addFolder, freshOffline, userHome } from "../support/app";
import { newWorktree, pick } from "../support/worktrees";

// E2E-M18-WT10-2 Intent radar, live (F-WT-10 slice 2): two worktrees of a
// TypeScript repository, parsed once by Check overlap. Then, with no
// checkpoint (an agent mid-turn), one renames `parse` to `parseLine` with
// the same body and the other starts calling `parse`: the notice names the
// rename within seconds, Check overlap says "renamed to parseLine", and
// Claim these files claims the renamer's file.

const GIT_ENV = {
  ...process.env,
  GIT_AUTHOR_NAME: "E2E",
  GIT_AUTHOR_EMAIL: "e2e@vibezeo.test",
  GIT_COMMITTER_NAME: "E2E",
  GIT_COMMITTER_EMAIL: "e2e@vibezeo.test",
};
const git = (cwd: string, ...args: string[]) =>
  execFileSync("git", ["-C", cwd, ...args], { encoding: "utf8", env: GIT_ENV }).trim();

const PARSE = "export function parse(line: string): string {\n  return line.trim().toLowerCase();\n}\n";
const PARSE_LINE =
  "export function parseLine(line: string): string {\n  return line.trim().toLowerCase();\n}\n";

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

const pageHas = (text: string) => browser.execute((t: string) => document.body.innerText.includes(t), text);

describe("E2E-M18-WT10-2 intent radar, live", function () {
  this.timeout(240_000);
  const repo = `/tmp/tandem-e2e-wt10-live-${Date.now()}`;
  const wt = (name: string) => join(repo, ".tandem/worktrees", name);

  before(async () => {
    mkdirSync(join(repo, "src"), { recursive: true });
    git(repo, "init", "-q", "-b", "main");
    writeFileSync(join(repo, "src/parse.ts"), PARSE);
    git(repo, "add", ".");
    git(repo, "commit", "-q", "-m", "first");
    writeFileSync(join(userHome(), ".gitconfig"), "[user]\n\tname = E2E\n\temail = e2e@vibezeo.test\n");
    await freshOffline();
    await addFolder(repo);
    await newWorktree(repo, "renamer");
    await newWorktree(repo, "user");
  });

  it("F-WT-10.7 a rename mid-turn is announced as one, with no checkpoint", async () => {
    // Check overlap parses both worktrees (nothing shared yet): the radar
    // watches them from here.
    await worktreeAction("tandem/renamer", "Check overlap…");
    await $('[data-testid="worktree-overlap"]').waitForExist({ timeout: 20_000 });
    await $('[data-testid="overlap-none"]').waitForExist({ timeout: 20_000 });
    await browser.keys("Escape");
    await $('[data-testid="worktree-overlap"]').waitForExist({ reverse: true, timeout: 10_000 });
    // Agents mid-turn: edits on disk, no checkpoint.
    writeFileSync(join(wt("renamer"), "src/parse.ts"), PARSE_LINE);
    writeFileSync(
      join(wt("user"), "src/main.ts"),
      "import { parse } from './parse';\nexport const out = parse(' X ');\n",
    );
    await browser.waitUntil(async () => pageHas("parse → parseLine"), {
      timeout: 20_000,
      timeoutMsg: "no notice naming the rename",
    });
    expect(await pageHas("Warn both agents")).toBe(true);
    // The rail marks both worktrees.
    await browser.waitUntil(
      async () =>
        browser.execute(() => document.querySelectorAll('[data-testid="worktree-radar"]').length === 2),
      { timeout: 10_000, timeoutMsg: "the rail didn't mark both worktrees" },
    );
  });

  it("F-WT-10.6 F-WT-10.8 Check overlap says renamed to parseLine, and Claim these files claims the renamer's file", async () => {
    await worktreeAction("tandem/user", "Check overlap…");
    const sym = $('[data-testid="overlap-symbol"]');
    await sym.waitForExist({ timeout: 20_000 });
    expect(String(await sym.getProperty("textContent"))).toContain("used here, renamed to parseLine there");
    await browser.keys("Escape");
    await $('[data-testid="worktree-overlap"]').waitForExist({ reverse: true, timeout: 10_000 });
    await worktreeAction("tandem/renamer", "Check overlap…");
    await $('[data-testid="overlap-claim"]').waitForExist({ timeout: 20_000 });
    await $('[data-testid="overlap-claim"]').click();
    await browser.waitUntil(async () => pageHas("Claimed 1 file for this worktree"), {
      timeout: 15_000,
      timeoutMsg: "the claim wasn't confirmed",
    });
    await browser.keys("Escape");
  });
});
