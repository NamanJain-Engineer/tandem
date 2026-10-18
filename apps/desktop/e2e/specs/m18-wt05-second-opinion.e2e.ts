import { $, $$, browser } from "@wdio/globals";
import { execFileSync } from "node:child_process";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import {
  addFolder,
  checkAgain,
  fakeEngine,
  fakeLog,
  freshOffline,
  home,
  invoke,
  mod,
  paneIds,
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

type Thread = {
  id: string;
  title: string | null;
  engineId: string;
  access: string;
  workspaceId: string | null;
};
type Ws = { id: string; laneBranch: string | null };

// E2E-M18-WT-05 Second opinion from another engine (F-WT-05).
describe("E2E-M18-WT-05 second opinion", function () {
  this.timeout(300_000);
  const repo = `/tmp/tandem-e2e-wt-05-${Date.now()}`;

  before(async () => {
    mkdirSync(repo, { recursive: true });
    git(repo, "init", "-q", "-b", "main");
    writeFileSync(join(repo, "a.ts"), "export const one = 1;\nexport const two = 2;\n");
    writeFileSync(join(repo, "b.ts"), "export const b = 1;\n");
    git(repo, "add", ".");
    git(repo, "commit", "-q", "-m", "first");
    await freshOffline();
    // Codex reviews; the worktrees' agents are Claude Code (the seat default).
    fakeEngine("codex", join(home(), "fakebin"));
    const report = await checkAgain();
    expect(report.found.map((f) => f.engineId)).toContain("codex");
    await addFolder(repo);
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

  it("F-WT-05.1-3 Codex reviews read-only and its findings land in the review bar", async () => {
    const lane = join(repo, ".tandem/worktrees/tidy");
    writeFileSync(join(lane, "a.ts"), "export const one = 1;\nexport const two = 22;\n");
    writeFileSync(join(lane, "b.ts"), "export const b = 11;\n");
    await worktreeAction("tandem/tidy", "Ask for a second opinion…");
    const uses = $('[data-testid="opinion-uses"]');
    await uses.waitForExist({ timeout: 15_000 });
    expect(String(await uses.getProperty("textContent"))).toContain("Codex");
    await $('[data-testid="opinion-task"]').setValue("Make the numbers bigger");
    await $('[data-testid="opinion-ask"]').click();
    // Back in the diff: the reviewer works, then its comments arrive.
    await $('[data-testid="worktree-diff"]').waitForExist({ timeout: 15_000 });
    await $('[data-testid="review-general"]').waitForExist({
      timeout: 90_000,
      timeoutMsg: "no reply was placed",
    });
    const lines = await browser.execute(() =>
      Array.from(document.querySelectorAll('[data-testid="review-comment"]')).map((e) =>
        e.getAttribute("data-line"),
      ),
    );
    expect(lines).toEqual(["a.ts:new:2", "b.ts:new:1"]);
    const general = String(await $('[data-testid="review-general"]').getProperty("textContent"));
    expect(general).toContain("a.ts:99999: check the caller too");
    expect(general).toContain("Small and readable.");

    // F-WT-05.1, F-WT-05.5: a Read only Codex thread in the worktree, whose
    // first message has the task, the diff and the headings; nothing written.
    const lanes = await invoke<Ws[]>("workspace_list", {});
    const laneId = lanes.find((w) => w.laneBranch === "tandem/tidy")?.id;
    const threads = await invoke<Thread[]>("thread_list", { kind: "thread" });
    const reviewer = threads.find((t) => t.title === "Second opinion: tandem/tidy");
    expect(reviewer).toMatchObject({ engineId: "codex", access: "read", workspaceId: laneId });
    const page = await invoke<{ items: Array<{ event: { type: string; text?: string } }> }>("thread_events", {
      threadId: reviewer?.id,
      limit: 50,
    });
    const first = page.items.map((e) => e.event).find((e) => e.type === "user")?.text ?? "";
    expect(first).toContain("Make the numbers bigger");
    expect(first).toContain("+export const two = 22;");
    expect(first).toContain("## Findings");
    expect(first).toContain("## Overall");
    expect(readFileSync(join(lane, "a.ts"), "utf8")).toBe("export const one = 1;\nexport const two = 22;\n");

    // The comments go to the worktree's agent with the user's own.
    const send = $('[data-testid="review-send"]');
    await send.waitForEnabled({ timeout: 20_000, timeoutMsg: "the worktree's agent never showed up" });
    const mark = fakeLog().length;
    await send.click();
    await browser.waitUntil(
      async () =>
        pastes(mark).some((p) => p.startsWith("Review of your changes on tandem/tidy (3 comments):")),
      { timeout: 30_000, timeoutMsg: `pastes: ${JSON.stringify(pastes(mark))}` },
    );
    const prompt = pastes(mark).find((p) => p.startsWith("Review of your changes")) ?? "";
    expect(prompt.indexOf("a.ts:2")).toBeLessThan(prompt.indexOf("b.ts:1"));
    expect(prompt.indexOf("b.ts:1")).toBeLessThan(prompt.indexOf("3. General"));
    await browser.keys("Escape");
  });

  it("F-WT-05.6 a worktree with no changes has nothing to review", async () => {
    const lanes = await invoke<Ws[]>("workspace_list", {});
    const other = lanes.find((w) => w.laneBranch?.startsWith("tandem/") && w.laneBranch !== "tandem/tidy");
    await worktreeAction(other?.laneBranch ?? "", "Ask for a second opinion…");
    const none = $('[data-testid="opinion-no-changes"]');
    await none.waitForExist({ timeout: 15_000 });
    expect(await $('[data-testid="opinion-ask"]').isEnabled()).toBe(false);
    expect(await $$('[data-testid="opinion-error"]').length).toBe(0);
  });
});
