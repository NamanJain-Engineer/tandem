import { $, browser } from "@wdio/globals";
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { addFolder, freshOffline } from "../support/app";
import { expectNoSeriousA11yViolationsInBothThemes } from "../support/axe";
import {
  addFolderBeside,
  branchChip,
  branchMenu,
  changeRows,
  commitStaged,
  openFiles,
  showChanges,
  waitForChanges,
} from "../support/files";

// E2E-M9-2 Git (F-CD-20): stage, commit and push to a bare remote, and the
// branch chip comes back to 0/0; a repository with no commits takes its
// first commit; a repository whose fsmonitor would `touch /tmp/pwned` never
// runs it while the Changes view reads it.

/** Plain git for the fixtures, never the user's or the system's config. */
function git(cwd: string, ...args: string[]): string {
  return execFileSync("git", ["-c", "init.defaultBranch=main", ...args], {
    cwd,
    encoding: "utf8",
    env: { ...process.env, GIT_CONFIG_GLOBAL: "/dev/null", GIT_CONFIG_NOSYSTEM: "1" },
  }).trim();
}

/** A repository that can commit without a global identity. */
function repoAt(path: string): void {
  mkdirSync(path, { recursive: true });
  git(path, "init", "-q");
  git(path, "config", "user.name", "E2E");
  git(path, "config", "user.email", "e2e@example.invalid");
  git(path, "config", "commit.gpgsign", "false");
}

describe("E2E-M9-2 git", () => {
  const base = `/tmp/tandem-e2e-m9-2-${Date.now()}`;
  const remote = `${base}/remote.git`;
  const repo = `${base}/shop`;

  before(async function () {
    this.timeout(60_000);
    mkdirSync(remote, { recursive: true });
    git(remote, "init", "-q", "--bare");
    repoAt(repo);
    writeFileSync(`${repo}/README.md`, "# shop\n");
    writeFileSync(`${repo}/a.txt`, "one\ntwo\nthree\n");
    git(repo, "add", ".");
    git(repo, "commit", "-qm", "first");
    git(repo, "remote", "add", "origin", remote);
    git(repo, "push", "-q", "-u", "origin", "main");
    // Work to commit: a change and a new file.
    writeFileSync(`${repo}/a.txt`, "one\nTWO\nthree\n");
    writeFileSync(`${repo}/b.txt`, "bee\n");
    await freshOffline();
    await addFolder(repo);
    await openFiles();
    await showChanges();
  });

  it("F-CD-20.2 stage, commit and push to a bare remote, and ahead/behind shows 0/0", async function () {
    this.timeout(90_000);
    await waitForChanges(["U:a.txt", "U:b.txt"]);
    expect(await branchChip()).toEqual({ branch: "main", ahead: "0", behind: "0", upstream: "origin/main" });
    // The diff of the picked change.
    await $('[data-testid="git-change"][data-path="a.txt"] button').click();
    await browser.waitUntil(
      async () => (await browser.execute(() => document.querySelector('[data-testid="git-diff"]')?.textContent ?? "")).includes("TWO"),
      { timeout: 10_000, timeoutMsg: "no diff for a.txt" },
    );
    await expectNoSeriousA11yViolationsInBothThemes();
    await $('[data-testid="git-stage-all"]').click();
    await waitForChanges(["S:a.txt", "S:b.txt"]);
    await commitStaged("Fix the second line\n\nAnd add b.");
    await browser.waitUntil(async () => (await branchChip())?.ahead === "1", {
      timeout: 15_000,
      timeoutMsg: `after the commit: ${JSON.stringify(await branchChip())}`,
    });
    expect(await changeRows()).toEqual([]);
    expect(git(repo, "log", "-1", "--format=%B").trim()).toBe("Fix the second line\n\nAnd add b.");
    await branchMenu("Push");
    await browser.waitUntil(async () => (await branchChip())?.ahead === "0", {
      timeout: 30_000,
      timeoutMsg: `after the push: ${JSON.stringify(await branchChip())}`,
    });
    expect(await branchChip()).toEqual({ branch: "main", ahead: "0", behind: "0", upstream: "origin/main" });
    expect(git(remote, "rev-parse", "main")).toBe(git(repo, "rev-parse", "HEAD"));
    // The status bar's branch says the same.
    const status = await $('[data-testid="status-branch"]');
    expect(await status.getAttribute("data-ahead")).toBe("0");
    expect(await status.getAttribute("data-behind")).toBe("0");
  });

  it("F-CD-20.3 a repository with no commits lists its files and takes the first commit", async function () {
    this.timeout(90_000);
    const fresh = `${base}/fresh`;
    repoAt(fresh);
    mkdirSync(`${fresh}/src`, { recursive: true });
    writeFileSync(`${fresh}/README.md`, "# fresh\n");
    writeFileSync(`${fresh}/src/lib.rs`, "pub fn x() {}\n");
    await addFolderBeside(fresh);
    // The Files tab follows the new workspace; its Changes view is the repository's.
    await browser.waitUntil(async () => (await $('[data-testid="files-panel"]').getAttribute("data-root")) === fresh, {
      timeout: 15_000,
      timeoutMsg: "the Files tab didn't follow the new workspace",
    });
    await showChanges();
    await waitForChanges(["U:README.md", "U:src/lib.rs"]);
    expect((await branchChip())?.upstream).toBe("");
    await $('[data-testid="git-stage-all"]').click();
    await waitForChanges(["S:README.md", "S:src/lib.rs"]);
    await commitStaged("First commit");
    await waitForChanges([]);
    expect(git(fresh, "rev-list", "--count", "HEAD")).toBe("1");
    expect(git(fresh, "log", "-1", "--format=%s")).toBe("First commit");
  });

  it("F-CD-20.1 the Changes view never runs a repository's fsmonitor", async function () {
    this.timeout(90_000);
    const trap = `${base}/trap`;
    repoAt(trap);
    writeFileSync(`${trap}/a.txt`, "one\n");
    git(trap, "add", ".");
    git(trap, "commit", "-qm", "first");
    writeFileSync(`${trap}/a.txt`, "changed\n");
    // The trap goes in last: plain git would spring it.
    git(trap, "config", "core.fsmonitor", "touch /tmp/pwned");
    rmSync("/tmp/pwned", { force: true });
    await addFolderBeside(trap);
    await browser.waitUntil(async () => (await $('[data-testid="files-panel"]').getAttribute("data-root")) === trap, {
      timeout: 15_000,
    });
    await showChanges();
    await waitForChanges(["U:a.txt"]);
    // Every read the panel does: the diff, the branches, a watch-driven refresh.
    await $('[data-testid="git-change"][data-path="a.txt"] button').click();
    await $('[data-testid="git-diff"]').waitForExist({ timeout: 10_000 });
    await $('[data-testid="git-branch-chip"]').click();
    await browser.waitUntil(async () => (await browser.execute(() => document.querySelectorAll("[role=menuitem]").length)) > 0, {
      timeout: 5_000,
    });
    await browser.keys("Escape");
    writeFileSync(`${trap}/b.txt`, "new\n");
    await waitForChanges(["U:a.txt", "U:b.txt"]);
    await browser.pause(1_000);
    expect(existsSync("/tmp/pwned")).toBe(false);
  });
});
