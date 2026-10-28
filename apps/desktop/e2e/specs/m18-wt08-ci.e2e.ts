import { $, browser } from "@wdio/globals";
import { execFileSync } from "node:child_process";
import { chmodSync, existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import {
  addFolder,
  checkAgain,
  fakeLog,
  freshOffline,
  mod,
  paneIds,
  setProfile,
  userHome,
} from "../support/app";
import { pick } from "../support/worktrees";

// E2E-M18-WT08-1 CI back to the agent (F-WT-08): a fake `gh` on the login
// PATH answers for the worktree's pull request. Nothing calls it before the
// repository is allowed; then the row shows the failed check, Send failure
// to agent types the fenced log tail into the worktree's agent, and Import
// review comments fills the review bar.

const GIT_ENV = {
  ...process.env,
  GIT_AUTHOR_NAME: "E2E",
  GIT_AUTHOR_EMAIL: "e2e@vibezeo.test",
  GIT_COMMITTER_NAME: "E2E",
  GIT_COMMITTER_EMAIL: "e2e@vibezeo.test",
};
const git = (cwd: string, ...args: string[]) =>
  execFileSync("git", ["-C", cwd, ...args], { encoding: "utf8", env: GIT_ENV }).trim();

const FAKE_GH = `#!/bin/sh
echo "$*" >> "$HOME/gh-calls.log"
case "$1 $2" in
  "pr view") printf '{"number":7,"url":"https://github.com/o/r/pull/7","state":"OPEN"}\\n' ;;
  "pr checks")
    printf '[{"name":"test","bucket":"fail","link":"https://github.com/o/r/actions/runs/11/job/22","workflow":"CI"},{"name":"lint","bucket":"pass","link":"","workflow":"CI"}]\\n'
    exit 1 ;;
  "run view") printf 'Run tests\\nError: expected 2, got 3\\n' ;;
  "api repos/{owner}/{repo}/pulls/7/comments")
    printf '{"id":101,"path":"a.ts","line":2,"original_line":2,"side":"RIGHT","body":"Keep two as 2.","user":"ana"}\\n' ;;
  *) echo "unexpected: $*" >&2; exit 2 ;;
esac
`;

const callsLog = () => join(userHome(), "gh-calls.log");
const ghCalls = () =>
  existsSync(callsLog()) ? readFileSync(callsLog(), "utf8").trim().split("\n").filter(Boolean) : [];

/** Everything the fake engines received as bracketed pastes since `mark`. */
function pastes(mark: number): string[] {
  const bytes = fakeLog()
    .slice(mark)
    .filter((l) => l.startsWith("raw: "))
    .map((l) => Buffer.from(l.slice(5), "hex").toString("utf8"))
    .join("");
  return bytes
    .split("\u001b[200~")
    .slice(1)
    .map((chunk) => chunk.split("\u001b[201~")[0] ?? "");
}

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

describe("E2E-M18-WT08-1 CI back to the agent", function () {
  this.timeout(300_000);
  const repo = `/tmp/tandem-e2e-wt08-${Date.now()}`;
  const lane = join(repo, ".tandem/worktrees/tidy");

  before(async () => {
    mkdirSync(repo, { recursive: true });
    git(repo, "init", "-q", "-b", "main");
    writeFileSync(join(repo, "a.ts"), "export const one = 1;\nexport const two = 2;\n");
    git(repo, "add", ".");
    git(repo, "commit", "-q", "-m", "first");
    // The fake gh on the login PATH, ahead of everything else.
    const bin = join(userHome(), "fakegh");
    mkdirSync(bin, { recursive: true });
    writeFileSync(join(bin, "gh"), FAKE_GH);
    chmodSync(join(bin, "gh"), 0o755);
    rmSync(callsLog(), { force: true });
    const profile = readFileSync(join(userHome(), ".zprofile"), "utf8").trimEnd().split("\n");
    setProfile([...profile, `export PATH="${bin}:$PATH"`]);
    await freshOffline();
    await checkAgain();
    await addFolder(repo);
    // Agents in their own worktrees, as F-WT-02's spec makes them.
    await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 20_000 });
    await browser.keys([mod(), "Shift", "l"]);
    await $('[data-testid="launch-sheet"]').waitForExist({ timeout: 10_000 });
    await $('//label[normalize-space()="How many of seat 1"]/following::input[1]').setValue("2");
    await $('[data-testid="launch-task"]').setValue("tidy");
    await $('[data-testid="launch-go"]').click();
    await $('[data-testid="launch-sheet"]').waitForExist({ reverse: true, timeout: 30_000 });
    await browser.waitUntil(
      () => browser.execute(() => document.querySelectorAll('[data-testid="worktree-row"]').length === 2),
      { timeout: 20_000 },
    );
  });

  after(async () => {
    setProfile(null);
    await checkAgain();
  });

  it("F-WT-08.3 no gh runs before the repository is allowed; Allow shows the checks", async () => {
    // The rail has read the worktrees' checks by now: none ran gh.
    await browser.pause(3_000);
    expect(ghCalls()).toEqual([]);
    expect(await $('[data-testid="worktree-checks"]').isExisting()).toBe(false);
    await worktreeAction("tandem/tidy", "Check pull request");
    const ask = $('[data-testid="checks-ask"]');
    await ask.waitForExist({ timeout: 10_000 });
    expect(ghCalls()).toEqual([]);
    await $('[data-testid="checks-allow"]').click();
    const rows = '[data-testid="check-row"]';
    await $(rows).waitForExist({ timeout: 20_000 });
    expect(ghCalls().some((c) => c.startsWith("pr view tandem/tidy"))).toBe(true);
    expect(ghCalls().some((c) => c.startsWith("pr checks tandem/tidy"))).toBe(true);
  });

  it("F-WT-08.1 the row says 1 check failed: test; Send failure to agent types the fenced log into the agent", async () => {
    const mark = fakeLog().length;
    const send = $('[data-testid="check-row"][data-name="test"] [data-testid="check-send"]');
    await send.waitForExist({ timeout: 20_000, timeoutMsg: "the worktree's agent never showed up" });
    // F-WT-08.4: nothing reached the agent before the click.
    expect(pastes(mark).some((p) => p.includes("A CI check failed"))).toBe(false);
    await send.click();
    await browser.waitUntil(async () => pastes(mark).some((p) => p.startsWith("A CI check failed")), {
      timeout: 30_000,
      timeoutMsg: `pastes: ${JSON.stringify(pastes(mark))}`,
    });
    const prompt = pastes(mark).find((p) => p.startsWith("A CI check failed")) ?? "";
    expect(prompt).toContain("pull request #7");
    expect(prompt).toContain("> Untrusted content from the CI check");
    const fence = prompt.indexOf("```text\n");
    expect(fence).toBeGreaterThan(0);
    expect(prompt.slice(fence)).toContain("Check: test");
    expect(prompt.slice(fence)).toContain("Error: expected 2, got 3");
    expect(ghCalls()).toContain("run view --job 22 --log-failed");
    await browser.keys("Escape");
    const badge = '[data-testid="worktree-row"][data-branch="tandem/tidy"] [data-testid="worktree-checks"]';
    await $(badge).waitForExist({ timeout: 10_000 });
    expect(await $(badge).getAttribute("data-state")).toBe("failed");
    // D-2700: the status bar's words whole, or the icon alone beside the row's
    // status; its name and title say which check failed.
    expect(["CI failed", ""]).toContain(String(await $(badge).getProperty("textContent")));
    expect(await $(badge).getAttribute("aria-label")).toBe("CI failed: 1 check failed: test");
    expect(await $(badge).getAttribute("title")).toBe("1 check failed: test");
  });

  it("F-WT-08.2 Import review comments fills the review bar with the pull request's line comments", async () => {
    writeFileSync(join(lane, "a.ts"), "export const one = 1;\nexport const two = 22;\n");
    await worktreeAction("tandem/tidy", "Open diff");
    const button = $('[data-testid="review-import"]');
    await button.waitForExist({ timeout: 15_000 });
    await button.click();
    const comment = $('[data-testid="review-comment"][data-imported="true"]');
    await comment.waitForExist({ timeout: 15_000 });
    expect(await comment.getAttribute("data-line")).toBe("a.ts:new:2");
    expect(await comment.getText()).toContain("@ana:");
    expect(String(await $('[data-testid="review-bar"]').getProperty("textContent"))).toContain("1 comment");
    // Sent, the comment reaches the agent inside the untrusted fence, with
    // its place inside too.
    const mark = fakeLog().length;
    const send = $('[data-testid="review-send"]');
    await send.waitForEnabled({ timeout: 20_000 });
    await send.click();
    await browser.waitUntil(
      async () => pastes(mark).some((p) => p.startsWith("Review of your changes on tandem/tidy")),
      {
        timeout: 30_000,
        timeoutMsg: `pastes: ${JSON.stringify(pastes(mark))}`,
      },
    );
    const prompt = pastes(mark).find((p) => p.startsWith("Review of your changes")) ?? "";
    expect(prompt).toContain("1. Imported review comment:");
    expect(prompt).toContain("   > Untrusted content from a pull request review comment");
    expect(prompt).toContain("   ```text\n   ana on a.ts:2:\n   Keep two as 2.\n   ```");
  });
});
