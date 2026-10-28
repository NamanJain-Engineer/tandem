import { $, browser } from "@wdio/globals";
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { addFolder, fakeLog, freshOffline, home, invoke, mod, settled, userHome } from "../support/app";
import { newWorktree, pick, repoMenu } from "../support/worktrees";

// E2E-M18-CD24-1 jj workspaces (F-CD-24): a repository co-located with jj
// (jj from its release, in the usual places). New worktree preselects jj
// workspace and makes `.tandem/worktrees/<slug>` with `jj workspace add`;
// the rail row is marked jj; an agent launched in it is told to use jj; its
// diff is a git worktree's with the same change; Discard forgets the
// workspace and keeps its commit.

const GIT_ENV = {
  ...process.env,
  GIT_AUTHOR_NAME: "E2E",
  GIT_AUTHOR_EMAIL: "e2e@vibezeo.test",
  GIT_COMMITTER_NAME: "E2E",
  GIT_COMMITTER_EMAIL: "e2e@vibezeo.test",
};
const git = (cwd: string, ...args: string[]) =>
  execFileSync("git", ["-C", cwd, ...args], { encoding: "utf8", env: GIT_ENV }).trim();
const JJ_ENV = { ...process.env, HOME: userHome() };
const jj = (cwd: string, ...args: string[]) =>
  execFileSync("jj", ["--no-pager", "--color=never", ...args], { cwd, encoding: "utf8", env: JJ_ENV }).trim();

type Ws = { id: string; root: string; laneBranch: string | null; laneVcs?: string };
type Diff = { path: string; additions: number; deletions: number; patch: string };

const hunks = (files: Diff[]) =>
  files.map((f) => ({
    path: f.path,
    additions: f.additions,
    deletions: f.deletions,
    lines: f.patch.split("\n").filter((l) => /^[ +-]/.test(l) && !/^(\+\+\+|---) /.test(l)),
  }));

/** Shows a worktree (its tab mounts, so its agent's terminal runs). */
async function show(branch: string): Promise<void> {
  await browser.execute((b: string) => {
    const row = Array.from(document.querySelectorAll<HTMLElement>('[data-testid="worktree-row"]')).find((r) =>
      r.querySelector(`button[aria-label="Actions for ${b}"]`),
    );
    row?.click();
  }, branch);
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

describe("E2E-M18-CD24-1 jj workspaces", function () {
  this.timeout(300_000);
  const repo = `/tmp/tandem-e2e-cd24-${Date.now()}`;
  const path = join(repo, ".tandem/worktrees/tidy");
  let lane: Ws;

  before(async () => {
    mkdirSync(repo, { recursive: true });
    git(repo, "init", "-q", "-b", "main");
    writeFileSync(join(repo, "app.txt"), "one\nthree\n");
    git(repo, "add", ".");
    git(repo, "commit", "-q", "-m", "first");
    writeFileSync(join(userHome(), ".gitconfig"), "[user]\n\tname = E2E\n\temail = e2e@vibezeo.test\n");
    jj(repo, "git", "init", "--colocate");
    await freshOffline();
    await addFolder(repo);
  });

  it("F-CD-24 jj waits for the repository's trust, asked for in New worktree", async () => {
    await repoMenu(repo);
    await pick("New worktree…");
    const trust = $("button=Trust repository…");
    await trust.waitForExist({ timeout: 15_000 });
    expect(await $('[data-testid="worktree-kind"]').isExisting()).toBe(false);
    // The native confirm answers Yes.
    writeFileSync(join(home(), "confirm.txt"), "yes");
    await trust.click();
    await $('[data-testid="worktree-kind"]').waitForExist({ timeout: 15_000 });
    await browser.keys("Escape");
    await $('[data-testid="worktree-name"]').waitForExist({ reverse: true, timeout: 10_000 });
  });

  it("F-CD-24.1 New worktree preselects jj workspace, makes it, and marks its row", async () => {
    await newWorktree(repo, "tidy");
    expect(existsSync(join(path, ".jj"))).toBe(true);
    expect(existsSync(join(path, ".git"))).toBe(false);
    expect(jj(repo, "workspace", "list", "-T", 'name ++ "\\n"').split("\n")).toContain("tidy");
    lane = (await invoke<Ws[]>("workspace_list", {})).find((w) => w.laneBranch === "tidy") as Ws;
    expect(lane.laneVcs).toBe("jj");
    const badge = $(`[data-testid="worktree-row"][data-branch="tidy"] [data-testid="worktree-jj"]`);
    await badge.waitForExist({ timeout: 15_000 });
    expect(String(await badge.getProperty("textContent"))).toBe("jj");
  });

  it("F-CD-24.6 an agent launched in it runs there and is told to use jj, not git", async () => {
    const from = fakeLog().length;
    await show("tidy");
    await browser.keys([mod(), "Shift", "l"]);
    await $('[data-testid="launch-sheet"]').waitForExist({ timeout: 10_000 });
    // One agent keeps the checkout: the jj workspace itself.
    await $('[data-testid="launch-task"]').setValue("tidy up");
    await $('[data-testid="launch-go"]').click();
    await $('[data-testid="launch-sheet"]').waitForExist({ reverse: true, timeout: 30_000 });
    const briefed = (l: string) => l.startsWith("line: You are working in the jj workspace");
    await browser.waitUntil(async () => fakeLog().slice(from).some(briefed), {
      timeout: 45_000,
      timeoutMsg: `no jj briefing: ${fakeLog().slice(-8).join(" | ")}`,
    });
    const line = fakeLog().slice(from).find(briefed) ?? "";
    expect(line).toContain("Use `jj`, not `git`");
    expect(line).toContain(path);
  });

  it("F-CD-24.2 its diff is a git worktree's with the same change", async () => {
    const gitLane = join(repo, "plain");
    git(repo, "worktree", "add", "-q", "-b", "plain", gitLane);
    for (const root of [path, gitLane]) writeFileSync(join(root, "app.txt"), "one\ntwo\nthree\n");
    const jjFiles = await invoke<Diff[]>("lane_diff", { laneId: lane.id });
    expect(jjFiles.map((f) => f.path)).toEqual(["app.txt"]);
    git(gitLane, "add", ".");
    const gitPatch = git(gitLane, "diff", "--cached", "HEAD");
    const gitLines = gitPatch.split("\n").filter((l) => /^[ +-]/.test(l) && !/^(\+\+\+|---) /.test(l));
    expect(hunks(jjFiles)).toEqual([{ path: "app.txt", additions: 1, deletions: 0, lines: gitLines }]);
    git(repo, "worktree", "remove", "--force", gitLane);
  });

  it("F-CD-24.3 what needs git is off, and says so", async () => {
    await worktreeAction("tidy", "Open diff");
    await browser.keys("Escape");
    await worktreeAction("tidy", "Open in Files");
    const note = await browser.execute(() =>
      Array.from(document.querySelectorAll<HTMLElement>("[role=menu] *")).some(
        (e) => e.textContent?.trim() === "Not available for jj workspaces yet",
      ),
    );
    // The menu closed on Open in Files; the note is checked on a fresh open.
    if (!note) {
      await browser.execute(() => {
        const b = document.querySelector<HTMLElement>('button[aria-label="Actions for tidy"]');
        b?.focus();
      });
      await browser.keys("Enter");
      await browser.waitUntil(
        async () =>
          browser.execute(() =>
            Array.from(document.querySelectorAll<HTMLElement>("[role=menu] *")).some(
              (e) => e.textContent?.trim() === "Not available for jj workspaces yet",
            ),
          ),
        { timeout: 10_000, timeoutMsg: "no jj note in the menu" },
      );
      await browser.keys("Escape");
    }
  });

  it("F-CD-24.4 Discard forgets the workspace, removes the folder and keeps its commit", async () => {
    await worktreeAction("tidy", "Discard worktree…");
    await $('//label[normalize-space()="Type tidy to confirm"]/following::input[1]').setValue("tidy");
    await settled();
    await $("button=Discard worktree").click();
    await browser.waitUntil(async () => !existsSync(path), {
      timeout: 30_000,
      timeoutMsg: "the jj workspace's folder is still there",
    });
    expect(jj(repo, "workspace", "list", "-T", 'name ++ "\\n"').split("\n")).not.toContain("tidy");
    const ids = jj(repo, "log", "-r", "all()", "--no-graph", "-T", 'commit_id ++ "\\n"').split("\n");
    const kept = ids.some(
      (id) => id && jj(repo, "diff", "--name-only", "-r", id).split("\n").includes("app.txt"),
    );
    expect(kept).toBe(true);
    expect(git(repo, "branch", "--show-current")).toBe("main");
  });
});
