import { $, browser } from "@wdio/globals";
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { addFolder, fakeLog, freshOffline, home, invoke, mod, sendToPane, typeKeys } from "../support/app";
import { expectNoSeriousA11yViolationsInBothThemes } from "../support/axe";

// E2E-M17-4 Best of N (F-EX-04): three fake-engine seats with a check
// command. Three worktrees from one commit, each agent gets the same prompt;
// Compare shows three diffs; Run check shows each exit code; Keep this one
// merges the winner and removes the other worktrees; the branches stay.

function git(cwd: string, ...args: string[]): string {
  return execFileSync(
    "git",
    ["-c", "init.defaultBranch=main", "-c", "user.name=E2E", "-c", "user.email=e2e@example.invalid", "-c", "commit.gpgsign=false", ...args],
    { cwd, encoding: "utf8" },
  ).trim();
}

/** The native confirm answers from this file (TANDEM_FAKE_CONFIRM). */
const answerConfirm = (answer: "yes" | "no") => writeFileSync(join(home(), "confirm.txt"), answer);

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

/** Clicks through JS: toasts may sit over the dialog's bottom. */
async function press(selector: string): Promise<void> {
  await $(selector).waitForExist({ timeout: 10_000 });
  await browser.execute((sel: string) => document.querySelector<HTMLElement>(sel)?.click(), selector);
}

type Lane = { seat: number; laneId: string | null; branch: string | null; paneId: string | null; check: string; checkExit: number | null };
type Run = { id: string; state: string; base: string; lanes: Lane[] };

async function columns(): Promise<Array<{ branch: string; files: number; check: string }>> {
  return browser.execute(() =>
    Array.from(document.querySelectorAll<HTMLElement>('[data-testid="bestof-column"]')).map((c) => ({
      branch: c.dataset.branch ?? "",
      files: Number(c.dataset.files ?? 0),
      check: c.dataset.check ?? "",
    })),
  );
}

describe("E2E-M17-4 Best of N", () => {
  const repo = `/tmp/tandem-e2e-m17-4-${Date.now()}`;
  let wsId = "";
  let run: Run = { id: "", state: "", base: "", lanes: [] };
  let mark = 0;
  /** Lane workspace id → its worktree folder. */
  const roots = new Map<string, string>();
  const rootOf = (l: Lane) => roots.get(l.laneId ?? "") ?? "";

  /** Sends one line to a lane's agent and waits for the file it writes. */
  async function write(l: Lane, file: string, text: string): Promise<void> {
    await sendToPane(l.paneId ?? "", `/write ${file}=${text}`);
    await sendToPane(l.paneId ?? "", "\r");
    await browser.waitUntil(async () => existsSync(join(rootOf(l), file)) && readFileSync(join(rootOf(l), file), "utf8").trim() === text, {
      timeout: 20_000,
      timeoutMsg: `${file} not written in ${rootOf(l)}`,
    });
  }

  before(async function () {
    this.timeout(60_000);
    mkdirSync(repo, { recursive: true });
    writeFileSync(join(repo, "a.txt"), "start\n");
    git(repo, "init", "-q");
    // The repository's own identity, unsigned: keeping a lane commits in it.
    git(repo, "config", "user.name", "E2E");
    git(repo, "config", "user.email", "e2e@example.invalid");
    git(repo, "config", "commit.gpgsign", "false");
    git(repo, "add", ".");
    git(repo, "commit", "-qm", "first");
    await freshOffline();
    await addFolder(repo);
    wsId = (await invoke<Array<{ id: string; root: string }>>("workspace_list")).find((w) => w.root === repo)?.id ?? "";
  });

  it("F-EX-04.1 three seats get three worktrees from one commit and the same prompt", async function () {
    this.timeout(120_000);
    await browser.execute(() => (document.activeElement as HTMLElement | null)?.blur());
    await browser.keys([mod(), "k"]);
    await $('input[placeholder="Type a command or search…"]').waitForDisplayed({ timeout: 5_000 });
    await typeKeys("Best of N…");
    await browser.keys("Enter");
    await $('[data-testid="bestof-sheet"]').waitForDisplayed({ timeout: 10_000 });
    await press('[data-testid="bestof-add"]');
    await $('[data-testid="bestof-prompt"]').setValue("Fix login redirect");
    await $('[data-testid="bestof-check"]').setValue("test -f done.txt");
    await browser.waitUntil(async () => (await $('[data-testid="bestof-summary"]').getText()).includes("3 worktrees"), { timeout: 5_000 });
    expect(await $('[data-testid="bestof-summary"]').getText()).toBe(
      "This opens 3 agents in 3 worktrees (tandem/fix-login-redirect-1 … -3), one per agent.",
    );
    await expectNoSeriousA11yViolationsInBothThemes();
    mark = fakeLog().length;
    await press('[data-testid="bestof-start"]');
    await $('[data-testid="bestof-compare"]').waitForDisplayed({ timeout: 45_000 });
    run = (await invoke<Run[]>("bestof_list", { workspaceId: wsId }))[0] ?? run;
    expect(run.lanes.length).toBe(3);
    const branches = run.lanes.map((l) => l.branch ?? "");
    expect(new Set(branches).size).toBe(3);
    for (const b of branches) expect(b.startsWith("tandem/")).toBe(true);
    expect(git(repo, "branch", "--list", "tandem/*", "--format=%(refname:short)").split("\n").sort()).toEqual([...branches].sort());
    const head = git(repo, "rev-parse", "HEAD");
    expect(run.base).toBe(head);
    for (const b of branches) expect(git(repo, "rev-parse", b)).toBe(head);
    await browser.waitUntil(async () => pastes(mark).filter((p) => p.endsWith("Task: Fix login redirect")).length === 3, {
      timeout: 60_000,
      timeoutMsg: `first prompts: ${JSON.stringify(pastes(mark))}`,
    });
  });

  it("F-EX-04.2 Compare shows each lane's own changes", async function () {
    this.timeout(90_000);
    for (const w of await invoke<Array<{ id: string; root: string }>>("workspace_list")) roots.set(w.id, w.root);
    // Each agent edits its own worktree; two of them also make the check pass.
    for (const l of run.lanes) {
      await write(l, "a.txt", `lane ${l.seat}`);
      if (l.seat < 2) await write(l, "done.txt", "ok");
    }
    await browser.waitUntil(async () => (await columns()).map((c) => c.files).join(",") === "2,2,1", {
      timeout: 30_000,
      timeoutMsg: `columns: ${JSON.stringify(await columns())}`,
    });
    expect((await columns()).map((c) => c.branch)).toEqual(run.lanes.map((l) => l.branch));
    await expectNoSeriousA11yViolationsInBothThemes();
  });

  it("F-EX-04.3 Run check runs it in each lane and shows each exit code", async function () {
    this.timeout(90_000);
    await press('[data-testid="bestof-run-check"]');
    await browser.waitUntil(async () => (await columns()).map((c) => c.check).join(",") === "passed,passed,failed", {
      timeout: 60_000,
      timeoutMsg: `columns: ${JSON.stringify(await columns())}`,
    });
    expect(await browser.execute(() => document.querySelector('[data-testid="bestof-column"][data-seat="2"]')?.textContent ?? "")).toContain("Failed · exit 1");
  });

  it("F-EX-04.4 Cancel keeps everything; Keep this one merges the winner, removes the others and keeps their branches", async function () {
    this.timeout(120_000);
    for (const l of run.lanes) expect(existsSync(rootOf(l))).toBe(true);
    answerConfirm("no");
    await press('[data-testid="bestof-column"][data-seat="1"] [data-testid="bestof-keep"]');
    await browser.pause(2_000);
    expect((await invoke<Run[]>("bestof_list", { workspaceId: wsId }))[0]?.state).toBe("running");
    for (const l of run.lanes) expect(existsSync(rootOf(l))).toBe(true);
    expect(readFileSync(join(repo, "a.txt"), "utf8")).toBe("start\n");
    answerConfirm("yes");
    await press('[data-testid="bestof-column"][data-seat="1"] [data-testid="bestof-keep"]');
    await browser.waitUntil(async () => (await $('[data-testid="bestof-compare"]').getAttribute("data-state")) === "merged", {
      timeout: 30_000,
      timeoutMsg: `never merged: ${JSON.stringify((await invoke<Run[]>("bestof_list", { workspaceId: wsId }))[0]?.state)} ${await browser.execute(() => Array.from(document.querySelectorAll('[role="status"], [data-sonner-toast], [data-testid="toast"]')).map((t) => t.textContent).join(" | "))}`,
    });
    expect(readFileSync(join(repo, "a.txt"), "utf8").trim()).toBe("lane 1");
    expect(readFileSync(join(repo, "done.txt"), "utf8").trim()).toBe("ok");
    for (const l of run.lanes) if (l.seat !== 1) expect(existsSync(rootOf(l))).toBe(false);
    const left = git(repo, "branch", "--list", "tandem/*", "--format=%(refname:short)").split("\n");
    for (const l of run.lanes) expect(left).toContain(l.branch);
  });
});
