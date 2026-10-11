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
const git = (cwd: string, ...args: string[]) =>
  execFileSync("git", ["-C", cwd, ...args], { encoding: "utf8", env: GIT_ENV }).trim();

const answerConfirm = (answer: "yes" | "no") => writeFileSync(join(home(), "confirm.txt"), answer);

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

const text = async (selector: string) => String(await $(selector).getProperty("textContent"));

async function waitForFile(path: string, timeout = 30_000): Promise<string> {
  await browser.waitUntil(() => existsSync(path), { timeout, timeoutMsg: `${path} never appeared` });
  return readFileSync(path, "utf8");
}

// E2E-M17-WT-1 Worktree setup (F-WT-01).
describe("E2E-M17-WT-1 worktree setup", function () {
  this.timeout(300_000);
  const stamp = Date.now();
  const repo = `/tmp/tandem-e2e-wt-1-${stamp}`;
  const teardownMarker = `/tmp/tandem-e2e-wt-1-${stamp}-teardown.txt`;
  const wt = (slug: string) => join(repo, ".tandem/worktrees", slug);

  before(async () => {
    mkdirSync(repo, { recursive: true });
    git(repo, "init", "-q", "-b", "main");
    writeFileSync(join(repo, ".gitignore"), ".env\nnode_modules/\nsetup-marker.txt\n");
    writeFileSync(join(repo, "README.md"), "hello\n");
    git(repo, "add", ".");
    git(repo, "commit", "-q", "-m", "first");
    writeFileSync(join(repo, ".env"), "API_KEY=dev\n");
    mkdirSync(join(repo, "node_modules/left-pad"), { recursive: true });
    writeFileSync(join(repo, "node_modules/left-pad/index.js"), "module.exports = 1;\n");
    mkdirSync(join(repo, ".vibezeo"));
    writeFileSync(
      join(repo, ".vibezeo/worktree.json"),
      JSON.stringify({
        copy: [".env"],
        setup: "echo from-repo > setup-marker.txt",
        teardown: `pwd > ${teardownMarker}`,
      }),
    );
    writeFileSync(join(userHome(), ".gitconfig"), "[user]\n\tname = E2E\n\temail = e2e@vibezeo.test\n");
    await freshOffline();
    await addFolder(repo);
  });

  it("F-WT-01.1 a new worktree gets the ignored .env, and nothing from node_modules", async () => {
    await newWorktree(repo, "setup one");
    expect(await waitForFile(join(wt("setup-one"), ".env"), 10_000)).toBe("API_KEY=dev\n");
    expect(existsSync(join(wt("setup-one"), "node_modules"))).toBe(false);
  });

  it("F-WT-01.3 the repository's untrusted setup is shown for review and runs only after Trust and run", async () => {
    await $('[data-testid="worktree-review"]').waitForExist({ timeout: 10_000 });
    expect(await text('[data-testid="worktree-review"]')).toContain("echo from-repo > setup-marker.txt");
    // Nothing ran while the review was open.
    await browser.pause(1_000);
    expect(existsSync(join(wt("setup-one"), "setup-marker.txt"))).toBe(false);
    await $('[data-testid="worktree-review-trust"]').click();
    expect((await waitForFile(join(wt("setup-one"), "setup-marker.txt"))).trim()).toBe("from-repo");
  });

  it("F-WT-01.3 a command changed outside the app asks again, and Not now runs nothing", async () => {
    writeFileSync(
      join(repo, ".vibezeo/worktree.json"),
      JSON.stringify({ copy: [".env"], setup: "echo changed > setup-marker.txt", teardown: `pwd > ${teardownMarker}` }),
    );
    await newWorktree(repo, "setup two");
    await $('[data-testid="worktree-review"]').waitForExist({ timeout: 10_000 });
    expect(await text('[data-testid="worktree-review"]')).toContain("echo changed > setup-marker.txt");
    await $('[data-testid="worktree-review-later"]').click();
    await $('[data-testid="worktree-review"]').waitForExist({ reverse: true, timeout: 5_000 });
    await browser.pause(2_000);
    expect(existsSync(join(wt("setup-two"), "setup-marker.txt"))).toBe(false);
  });

  it("F-WT-01.4 saving from Worktree setup writes the file, and the next worktree runs it without asking", async () => {
    // Free allows 4 live panes: make room for the next worktree and its setup.
    for (const ws of await invoke<Array<{ id: string; laneBranch: string | null }>>("workspace_list"))
      if (ws.laneBranch === "tandem/setup-one" || ws.laneBranch === "tandem/setup-two")
        await invoke("lane_remove", { laneId: ws.id, force: false });
    await repoMenu(repo);
    await pick("Worktree setup…");
    const command = $('[data-testid="worktree-setup-command"]');
    await command.waitForExist({ timeout: 10_000 });
    await browser.waitUntil(async () => (await command.getValue()) === "echo changed > setup-marker.txt", {
      timeout: 5_000,
    });
    await command.clearValue();
    await command.addValue("echo saved > setup-marker.txt");
    await $('[data-testid="worktree-setup-save"]').click();
    await $('[data-testid="worktree-setup"]').waitForExist({ reverse: true, timeout: 5_000 });
    const saved = JSON.parse(readFileSync(join(repo, ".vibezeo/worktree.json"), "utf8")) as { setup: string };
    expect(saved.setup).toBe("echo saved > setup-marker.txt");
    await newWorktree(repo, "setup three");
    expect((await waitForFile(join(wt("setup-three"), "setup-marker.txt"))).trim()).toBe("saved");
    expect(await $('[data-testid="worktree-review"]').isExisting()).toBe(false);
  });

  it("F-WT-01.5 discarding runs the trusted teardown in the worktree's folder first", async () => {
    answerConfirm("yes");
    await worktreeAction("tandem/setup-three", "Discard worktree…");
    await $('//label[normalize-space()="Type tandem/setup-three to confirm"]/following::input[1]').setValue(
      "tandem/setup-three",
    );
    await $("button=Discard worktree").click();
    expect((await waitForFile(teardownMarker, 20_000)).trim()).toBe(wt("setup-three"));
    await browser.waitUntil(() => !existsSync(wt("setup-three")), { timeout: 15_000 });
  });
});
