import { $, browser } from "@wdio/globals";
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { addFolder, freshOffline, home, invoke, userHome } from "../support/app";

const GIT_ENV = {
  ...process.env,
  GIT_AUTHOR_NAME: "E2E",
  GIT_AUTHOR_EMAIL: "e2e@vibezeo.test",
  GIT_COMMITTER_NAME: "E2E",
  GIT_COMMITTER_EMAIL: "e2e@vibezeo.test",
};
const git = (cwd: string, ...args: string[]) => execFileSync("git", ["-C", cwd, ...args], { encoding: "utf8", env: GIT_ENV }).trim();

function makeRepo(path: string): void {
  mkdirSync(path, { recursive: true });
  git(path, "init", "-q", "-b", "main");
  writeFileSync(join(path, "README.md"), "hello\n");
  git(path, "add", "README.md");
  git(path, "commit", "-q", "-m", "first");
}

/** The native "Discard …?" confirm answers from this file (TANDEM_FAKE_CONFIRM). */
const answerConfirm = (answer: "yes" | "no") => writeFileSync(join(home(), "confirm.txt"), answer);

type Ws = { id: string; root: string; parentId: string | null; laneBranch: string | null };
const workspaces = () => invoke<Ws[]>("workspace_list");

/** Clicks a menu item by its text once the menu is open. */
async function pick(label: string): Promise<void> {
  await browser.waitUntil(
    async () =>
      browser.execute((l: string) => {
        const item = Array.from(document.querySelectorAll<HTMLElement>("[role=menuitem]")).find((el) => el.textContent?.trim() === l);
        item?.click();
        return item !== undefined;
      }, label),
    { timeout: 5_000, timeoutMsg: `no menu item ${label}` },
  );
}

/** Right-clicks the repository's rail row (its context menu). */
async function repoMenu(root: string): Promise<void> {
  await browser.execute((r: string) => {
    const row = document.querySelector<HTMLElement>(`[data-testid="workspace-row"][data-root="${r}"]`);
    const box = row?.getBoundingClientRect();
    row?.dispatchEvent(
      new MouseEvent("contextmenu", { bubbles: true, cancelable: true, button: 2, clientX: (box?.left ?? 0) + 24, clientY: (box?.top ?? 0) + 8 }),
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

/** Opens a worktree row's ⋯ menu from the keyboard (it shows on focus). */
async function worktreeAction(branch: string, label: string): Promise<void> {
  await browser.execute((b: string) => {
    document.querySelector<HTMLElement>(`button[aria-label="Actions for ${b}"]`)?.focus();
  }, branch);
  await browser.keys("Enter");
  await pick(label);
}

const rows = () =>
  browser.execute(() =>
    Array.from(document.querySelectorAll<HTMLElement>('[data-testid="worktree-row"]')).map((r) => ({
      branch: r.dataset.branch ?? "",
      // The repository row just before the group the worktree sits in.
      under: (r.closest('[role="group"]')?.previousElementSibling as HTMLElement | null)?.dataset.root ?? "",
    })),
  );

// E2E-M6-4 Lane safety, and the nesting half of E2E-M6-2 (F-CD-15).
describe("E2E-M6-4 worktrees", () => {
  const repo = `/tmp/tandem-e2e-m6-4-${Date.now()}`;

  before(async () => {
    makeRepo(repo);
    // A merge commits, so git needs the user's name and email: the run's
    // HOME is fresh, and the app passes no GIT_* variables on.
    writeFileSync(join(userHome(), ".gitconfig"), "[user]\n\tname = E2E\n\temail = e2e@vibezeo.test\n");
    await freshOffline();
    await addFolder(repo);
  });

  after(() => {
    try {
      git(repo, "merge", "--abort");
    } catch {
      // Nothing to abort.
    }
  });

  it("F-CD-15.1 two worktrees for 'fix auth' are tandem/fix-auth and tandem/fix-auth-2, and .tandem/ is excluded once", async () => {
    await newWorktree(repo, "fix auth");
    await newWorktree(repo, "fix auth");
    await browser.waitUntil(async () => (await rows()).length === 2, { timeout: 10_000 });
    expect(git(repo, "branch", "--list", "tandem/*", "--format=%(refname:short)").split("\n")).toEqual(["tandem/fix-auth", "tandem/fix-auth-2"]);
    const exclude = readFileSync(join(repo, ".git/info/exclude"), "utf8").split("\n");
    expect(exclude.filter((l) => l.trim() === ".tandem/")).toHaveLength(1);
    expect(git(repo, "status", "--porcelain")).toBe("");
  });

  it("F-CD-15.4 the worktrees nest under their repository in the rail and in the Dashboard's project grouping", async () => {
    expect(await rows()).toEqual([
      { branch: "tandem/fix-auth", under: repo },
      { branch: "tandem/fix-auth-2", under: repo },
    ]);
    const list = await workspaces();
    const parent = list.find((w) => w.root === repo);
    const lane = list.find((w) => w.laneBranch === "tandem/fix-auth-2");
    expect(lane?.parentId).toBe(parent?.id);
    const agent = (id: string, workspaceId: string | undefined, title: string) =>
      invoke("test_fixture_agent", { agent: { id, title, status: "working", engineId: "claude", workspaceId, preview: null, approval: null } });
    await agent("m64-main", parent?.id, "on main");
    await agent("m64-lane", lane?.id, "in the worktree");
    if (!(await $('[data-testid="sidebar"]').isDisplayed().catch(() => false))) await $('[data-testid="sidebar-toggle"]').click();
    await $('[data-testid="sidebar-pill-dashboard"]').click();
    await $('[data-testid="dashboard"]').waitForExist({ timeout: 10_000 });
    await $('//*[@role="radio"][normalize-space()="Project"]').click();
    const nested = `[data-testid="worktrees-ws:${parent?.id}"] [data-testid="group-ws:${lane?.id}"]`;
    await $(nested).waitForExist({ timeout: 10_000, timeoutMsg: "the worktree's group isn't inside its repository's" });
    expect(await $(`${nested} [data-testid="agent-card"][data-key$="m64-lane"]`).isExisting()).toBe(true);
  });

  it("F-CD-15.2 discarding a worktree with uncommitted work needs the native confirm, and keeps the branch", async () => {
    const folder = join(repo, ".tandem/worktrees/fix-auth");
    writeFileSync(join(folder, "notes.txt"), "uncommitted\n");
    const discard = async () => {
      await worktreeAction("tandem/fix-auth", "Discard worktree…");
      await $('//label[normalize-space()="Type tandem/fix-auth to confirm"]/following::input[1]').setValue("tandem/fix-auth");
      await $("button=Discard worktree").click();
    };
    // The plain removal refuses (uncommitted work); the force asks natively: No.
    answerConfirm("no");
    await discard();
    await browser.pause(1_500);
    expect(existsSync(join(folder, "notes.txt"))).toBe(true);
    expect((await rows()).map((r) => r.branch)).toContain("tandem/fix-auth");
    // Yes: the folder goes, the branch and its commits stay.
    answerConfirm("yes");
    await discard();
    await browser.waitUntil(async () => !(await rows()).some((r) => r.branch === "tandem/fix-auth"), { timeout: 15_000 });
    expect(existsSync(folder)).toBe(false);
    expect(git(repo, "branch", "--list", "tandem/fix-auth")).not.toBe("");
  });

  it("F-CD-15.3 a merge that conflicts leaves the repository conflicted, reports the files and commits nothing", async () => {
    const lane = join(repo, ".tandem/worktrees/fix-auth-2");
    writeFileSync(join(lane, "README.md"), "from the worktree\n");
    git(lane, "commit", "-q", "-am", "worktree change");
    writeFileSync(join(repo, "README.md"), "from main\n");
    git(repo, "commit", "-q", "-am", "main change");
    const head = git(repo, "rev-parse", "HEAD");
    await worktreeAction("tandem/fix-auth-2", "Merge into main…");
    const confirm = await $("button=Merge");
    await confirm.waitForClickable({ timeout: 5_000 });
    await confirm.click();
    const list = await $('[data-testid="merge-conflicts"]');
    await list.waitForExist({ timeout: 15_000 });
    expect(await list.getText()).toContain("README.md");
    expect(git(repo, "status", "--porcelain")).toContain("UU README.md");
    expect(git(repo, "rev-parse", "HEAD")).toBe(head);
  });
});
