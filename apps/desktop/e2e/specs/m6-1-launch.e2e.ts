import { $, browser } from "@wdio/globals";
import { execFileSync } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import {
  addFolder,
  fakeLog,
  freshOffline,
  home,
  mod,
  paneIds,
  relaunch,
  sendToPane,
  waitForApp,
  waitForPaneText,
} from "../support/app";

const GIT_ENV = {
  ...process.env,
  GIT_AUTHOR_NAME: "E2E",
  GIT_AUTHOR_EMAIL: "e2e@vibezeo.test",
  GIT_COMMITTER_NAME: "E2E",
  GIT_COMMITTER_EMAIL: "e2e@vibezeo.test",
};
const git = (cwd: string, ...args: string[]) =>
  execFileSync("git", ["-C", cwd, ...args], { encoding: "utf8", env: GIT_ENV }).trim();

function makeRepo(path: string): void {
  mkdirSync(path, { recursive: true });
  git(path, "init", "-q", "-b", "main");
  writeFileSync(join(path, "README.md"), "hello\n");
  git(path, "add", "README.md");
  git(path, "commit", "-q", "-m", "first");
}

/** The nth worktree of the next launch fails (TANDEM_FAKE_LANE_FAIL); 0: none. */
const failLane = (nth: number) => writeFileSync(join(home(), "lane-fail.txt"), String(nth));

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

const worktreeBranches = () =>
  browser.execute(() =>
    Array.from(document.querySelectorAll<HTMLElement>('[data-testid="worktree-row"]')).map(
      (r) => r.dataset.branch ?? "",
    ),
  );

/** Closes the folder's first shell, so the tab shows the empty state. */
async function emptyTab(): Promise<void> {
  await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 20_000 });
  const [shell] = await paneIds();
  await $(`[data-testid="pane"][data-pane-id="${shell}"] button[aria-label^="Close "]`).click();
  await $('[data-testid="launch-several"]').waitForExist({ timeout: 10_000 });
}

async function openSheet(): Promise<void> {
  const button = $('[data-testid="launch-several"]');
  if (await button.isDisplayed()) await button.click();
  // Elsewhere, "Launch agents…" (Mod+Shift+L) opens it for the active folder.
  else await browser.keys([mod(), "Shift", "l"]);
  await $('[data-testid="launch-sheet"]').waitForExist({ timeout: 10_000 });
}

/** An element's text whether or not WebKit counts it as rendered yet: its
 * getText is "" while the sheet slides in and skips truncated spans. */
const text = async (testId: string) =>
  String(await $(`[data-testid="${testId}"]`).getProperty("textContent"));

const preset = (name: string) => $(`//*[@role="radio"][normalize-space()="${name}"]`).click();

async function launch(): Promise<void> {
  await $('[data-testid="launch-go"]').click();
}

// E2E-M6-1, M6-2, M6-3: the launch sheet (F-CD-14) and worktree per agent (F-CD-15).
describe("E2E-M6-1 Fleet", () => {
  it("F-CD-14.1 Fleet with 'fix flaky tests' opens 4 agents, each told its role and then the task once ready", async () => {
    await freshOffline();
    await addFolder(`/tmp/tandem-e2e-m6-1-${Date.now()}`);
    await emptyTab();
    await openSheet();
    await preset("Fleet");
    await $('[data-testid="launch-task"]').setValue("fix flaky tests");
    // A plain folder: no worktrees, the 4 agents share it.
    expect(await text("launch-summary")).toBe("This opens 4 panes.");
    expect(await text("launch-go")).toContain("Launch 4 agents");
    const mark = fakeLog().length;
    await launch();
    await $('[data-testid="launch-sheet"]').waitForExist({ reverse: true, timeout: 20_000 });
    await browser.waitUntil(async () => (await paneIds()).length === 4, { timeout: 20_000 });
    await browser.waitUntil(
      async () => pastes(mark).filter((p) => p.includes("Task: fix flaky tests")).length === 4,
      {
        timeout: 60_000,
        timeoutMsg: `first inputs: ${JSON.stringify(pastes(mark))}`,
      },
    );
    const firsts = pastes(mark).filter((p) => p.includes("Task: fix flaky tests"));
    for (const p of firsts) expect(p).toMatch(/^Your role: \w+\. [^\n]+\n\nTask: fix flaky tests$/);
    expect(firsts.map((p) => /^Your role: (\w+)/.exec(p)?.[1]).sort()).toEqual([
      "Builder",
      "Builder",
      "Reviewer",
      "Tester",
    ]);
  });
});

describe("E2E-M6-2 worktree per agent", () => {
  const repo = `/tmp/tandem-e2e-m6-2-${Date.now()}`;

  it("F-CD-14.2 3 agents and a shell make exactly 3 worktrees, and the shell opens in the repository", async () => {
    makeRepo(repo);
    await freshOffline();
    await addFolder(repo);
    await emptyTab();
    await openSheet();
    await preset("Bench");
    // Three of the agent row: Custom, and more than one agent means a worktree each.
    await $('//label[normalize-space()="How many of seat 1"]/following::input[1]').setValue("3");
    await $('[data-testid="launch-task"]').setValue("add metrics");
    await browser.waitUntil(async () => (await text("launch-summary")).includes("3 worktrees"), {
      timeout: 5_000,
    });
    expect(await text("launch-summary")).toBe(
      "This opens 4 panes and 3 worktrees (tandem/add-metrics, tandem/add-metrics-2, tandem/add-metrics-3).",
    );
    expect(await text("launch-go")).toContain("Launch 3 agents + 1 shell");
    await launch();
    await $('[data-testid="launch-sheet"]').waitForExist({ reverse: true, timeout: 30_000 });
    await browser.waitUntil(async () => (await worktreeBranches()).length === 3, { timeout: 20_000 });
    expect(git(repo, "branch", "--list", "tandem/*", "--format=%(refname:short)").split("\n")).toEqual([
      "tandem/add-metrics",
      "tandem/add-metrics-2",
      "tandem/add-metrics-3",
    ]);
    // The shared tab, shown after the launch, holds only the shell.
    await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 15_000 });
    const [shell] = await paneIds();
    await sendToPane(shell ?? "", "pwd\r");
    await waitForPaneText(shell ?? "", repo, 15_000);
  });
});

describe("E2E-M6-3 partial failure and saved presets", () => {
  const repo = `/tmp/tandem-e2e-m6-3-${Date.now()}`;

  after(() => failLane(0));

  it("F-CD-14.3 when 1 of 4 worktrees fails, 3 agents launch and Retry brings up the 4th", async () => {
    makeRepo(repo);
    await freshOffline();
    await addFolder(repo);
    await emptyTab();
    await openSheet();
    await preset("Fleet");
    await $('[data-testid="launch-task"]').setValue("speed up ci");
    failLane(3);
    await launch();
    const failures = await $('[data-testid="launch-failures"]');
    await failures.waitForExist({ timeout: 30_000 });
    expect(await failures.getText()).toContain("Seat 3");
    expect(await failures.getText()).toContain("Couldn't create the worktree");
    await browser.waitUntil(async () => (await worktreeBranches()).length === 3, { timeout: 20_000 });
    // Nothing was rolled back.
    expect(git(repo, "branch", "--list", "tandem/*", "--format=%(refname:short)").split("\n")).toHaveLength(
      3,
    );
    failLane(0);
    await $('//*[@data-testid="launch-failure"]//button[normalize-space()="Retry"]').click();
    await $('[data-testid="launch-sheet"]').waitForExist({ reverse: true, timeout: 30_000 });
    await browser.waitUntil(async () => (await worktreeBranches()).length === 4, { timeout: 20_000 });
  });

  it("F-CD-14.4 a saved preset is listed again after a relaunch, with the same seats", async () => {
    await openSheet();
    await preset("Bench");
    await $('//label[normalize-space()="Command for seat 2"]/following::input[1]').setValue("echo ready");
    await $('[data-testid="launch-save"]').click();
    await $('[data-testid="preset-name"]').setValue("Nightly");
    // Save enables once the name reaches React's state; under load that
    // lands after setValue returns.
    await $('[data-testid="preset-save"]').waitForClickable({ timeout: 10_000 });
    await $('[data-testid="preset-save"]').click();
    await $('[data-testid="preset-name"]').waitForExist({ reverse: true, timeout: 10_000 });
    await browser.keys("Escape");
    await relaunch();
    await waitForApp();
    await openSheet();
    const id = await $('//label[normalize-space()="Saved presets"]').getAttribute("for");
    const saved = $(`[id="${id}"]`);
    // The sheet slides in after the relaunch, and the list zooms in.
    await saved.waitForClickable({ timeout: 10_000 });
    await saved.click();
    const nightly = $('//*[@role="option"][normalize-space()="Nightly"]');
    await nightly.waitForClickable({ timeout: 10_000 });
    await nightly.click();
    const kinds = await browser.execute(() =>
      Array.from(document.querySelectorAll<HTMLElement>('[data-testid="launch-seat"]')).map(
        (r) => r.dataset.kind ?? "",
      ),
    );
    expect(kinds).toEqual(["agent", "shell"]);
    expect(await $('//label[normalize-space()="Command for seat 2"]/following::input[1]').getValue()).toBe(
      "echo ready",
    );
  });
});
