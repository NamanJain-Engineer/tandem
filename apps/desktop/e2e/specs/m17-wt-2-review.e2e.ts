import { $, browser } from "@wdio/globals";
import { execFileSync } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { addFolder, fakeLog, freshOffline, mod, paneIds } from "../support/app";

const GIT_ENV = {
  ...process.env,
  GIT_AUTHOR_NAME: "E2E",
  GIT_AUTHOR_EMAIL: "e2e@vibezeo.test",
  GIT_COMMITTER_NAME: "E2E",
  GIT_COMMITTER_EMAIL: "e2e@vibezeo.test",
};
const git = (cwd: string, ...args: string[]) =>
  execFileSync("git", ["-C", cwd, ...args], { encoding: "utf8", env: GIT_ENV }).trim();

/** Everything the fake engines received as bracketed pastes since `mark`. */
function pastes(mark: number): string[] {
  const bytes = fakeLog()
    .slice(mark)
    .filter((l) => l.startsWith("raw: "))
    .map((l) => Buffer.from(l.slice(5), "hex").toString("utf8"))
    .join("");
  const [open, close] = ["\u001b[200~", "\u001b[201~"];
  return bytes
    .split(open)
    .slice(1)
    .map((chunk) => chunk.split(close)[0] ?? "");
}

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

/** Adds a comment through a line's gutter button (by its accessible name). */
async function commentOn(label: string, comment: string): Promise<void> {
  await browser.execute((l: string) => {
    document.querySelector<HTMLElement>(`[data-testid="diff-comment-line"][aria-label="${l}"]`)?.click();
  }, label);
  const input = $('[data-testid="review-comment-input"]');
  await input.waitForExist({ timeout: 5_000 });
  await input.addValue(comment);
  await $('[data-testid="review-comment-save"]').click();
  await input.waitForExist({ reverse: true, timeout: 5_000 });
}

// E2E-M17-WT-2 Review comments to the agent (F-WT-02).
describe("E2E-M17-WT-2 review comments", function () {
  this.timeout(240_000);
  const repo = `/tmp/tandem-e2e-wt-2-${Date.now()}`;

  before(async () => {
    mkdirSync(repo, { recursive: true });
    git(repo, "init", "-q", "-b", "main");
    writeFileSync(join(repo, "a.ts"), "export const one = 1;\nexport const two = 2;\n");
    writeFileSync(join(repo, "b.ts"), "export const b = 1;\n");
    git(repo, "add", ".");
    git(repo, "commit", "-q", "-m", "first");
    await freshOffline();
    await addFolder(repo);
    // Two agents in their own worktrees (the launch sheet's default for more than one).
    await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 20_000 });
    await browser.keys([mod(), "Shift", "l"]);
    await $('[data-testid="launch-sheet"]').waitForExist({ timeout: 10_000 });
    await $('//label[normalize-space()="How many of seat 1"]/following::input[1]').setValue("2");
    await $('[data-testid="launch-task"]').setValue("tidy");
    await $('[data-testid="launch-go"]').click();
    await $('[data-testid="launch-sheet"]').waitForExist({ reverse: true, timeout: 30_000 });
    await browser.waitUntil(
      () =>
        browser.execute(
          () => document.querySelectorAll('[data-testid="worktree-row"]').length === 2,
        ),
      { timeout: 20_000 },
    );
  });

  it("F-WT-02.2 two line comments are sent to the worktree's agent as one prompt", async () => {
    const lane = join(repo, ".tandem/worktrees/tidy");
    writeFileSync(join(lane, "a.ts"), "export const one = 1;\nexport const two = 22;\n");
    writeFileSync(join(lane, "b.ts"), "export const b = 11;\n");
    await worktreeAction("tandem/tidy", "Open diff");
    await $('[data-testid="worktree-diff"] [data-testid="diff-view"]').waitForExist({ timeout: 15_000 });
    await commentOn("Comment on b.ts line 1", "Why eleven?");
    await commentOn("Comment on a.ts line 2", "Keep two as 2.");
    const bar = $('[data-testid="review-bar"]');
    expect(String(await bar.getProperty("textContent"))).toContain("2 comments");
    const send = $('[data-testid="review-send"]');
    await send.waitForEnabled({ timeout: 20_000, timeoutMsg: "the worktree's agent never showed up" });
    const mark = fakeLog().length;
    await send.click();
    await browser.waitUntil(
      async () => pastes(mark).some((p) => p.startsWith("Review of your changes on tandem/tidy (2 comments):")),
      { timeout: 30_000, timeoutMsg: `pastes: ${JSON.stringify(pastes(mark))}` },
    );
    const prompt = pastes(mark).find((p) => p.startsWith("Review of your changes")) ?? "";
    expect(prompt.indexOf("a.ts:2")).toBeGreaterThan(0);
    expect(prompt.indexOf("a.ts:2")).toBeLessThan(prompt.indexOf("b.ts:1"));
    expect(prompt).toContain("Why eleven?");
    await bar.waitForExist({ reverse: true, timeout: 5_000 });
  });
});
