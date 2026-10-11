import { $, $$, browser } from "@wdio/globals";
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { addFolder, freshOffline, invoke, mod, paneIds, sendToPane, split, waitForPaneText } from "../support/app";
import { expectNoSeriousA11yViolationsInBothThemes } from "../support/axe";
import { openFromPlus } from "../support/files";
import { composer, sendMessage, threadMode, waitForAssistant } from "../support/thread";

// E2E-M17-1 Checkpoints (F-EX-01): a thread turn saves the folder first and
// leaves the repository alone; restore all and restore one; Enter in an agent
// pane waits for its checkpoint; a folder without git says what's covered.

function git(cwd: string, ...args: string[]): string {
  return execFileSync(
    "git",
    ["-c", "init.defaultBranch=main", "-c", "user.name=E2E", "-c", "user.email=e2e@example.invalid", "-c", "commit.gpgsign=false", ...args],
    { cwd, encoding: "utf8", env: { ...process.env, GIT_CONFIG_GLOBAL: "/dev/null", GIT_CONFIG_NOSYSTEM: "1" } },
  ).trim();
}

type Row = { id: string; reason: string; differs: number };

async function rows(): Promise<Row[]> {
  return browser.execute(() =>
    Array.from(document.querySelectorAll<HTMLElement>('[data-testid="checkpoint-row"]')).map((r) => ({
      id: r.dataset.id ?? "",
      reason: r.dataset.reason ?? "",
      differs: Number(r.dataset.differs ?? 0),
    })),
  );
}

async function waitForRows(check: (r: Row[]) => boolean, msg: string): Promise<Row[]> {
  await browser.waitUntil(async () => check(await rows()), { timeout: 20_000, timeoutMsg: `${msg}: ${JSON.stringify(await rows())}` });
  return rows();
}

async function files(): Promise<string[]> {
  return browser.execute(() =>
    Array.from(document.querySelectorAll<HTMLElement>('[data-testid="checkpoint-file"]')).map((r) => `${r.dataset.change}:${r.dataset.path}`),
  );
}

/** Clicks through JS: toasts may sit over the sidebar's bottom. */
async function press(selector: string): Promise<void> {
  await $(selector).waitForExist({ timeout: 10_000 });
  await browser.execute((sel: string) => document.querySelector<HTMLElement>(sel)?.click(), selector);
}

async function confirmRestore(): Promise<void> {
  const dialog = await $('[role="alertdialog"]');
  await dialog.waitForDisplayed({ timeout: 5_000 });
  const confirm = await dialog.$("button=Restore files");
  await confirm.click();
  await dialog.waitForExist({ timeout: 10_000, reverse: true });
}

/** The repository's own state that a checkpoint must leave alone. */
function repoState(repo: string): string {
  return [
    git(repo, "rev-parse", "HEAD"),
    git(repo, "for-each-ref"),
    git(repo, "stash", "list"),
    readFileSync(`${repo}/.git/index`).toString("hex"),
  ].join("\n");
}

describe("E2E-M17-1 checkpoints", () => {
  const base = `/tmp/tandem-e2e-m17-1-${Date.now()}`;
  const repo = `${base}/shop`;
  let threadId = "";

  before(async function () {
    this.timeout(60_000);
    mkdirSync(repo, { recursive: true });
    git(repo, "init", "-q");
    writeFileSync(`${repo}/.gitignore`, "*.log\n");
    writeFileSync(`${repo}/a.txt`, "one\n");
    writeFileSync(`${repo}/b.txt`, "bee\n");
    git(repo, "add", ".");
    git(repo, "commit", "-qm", "first");
    writeFileSync(`${repo}/notes.log`, "ignored\n");
    await freshOffline();
    await addFolder(repo);
  });

  after(() => rmSync(base, { recursive: true, force: true }));

  it("F-EX-01.1 a thread turn saves a checkpoint first and leaves the repository alone", async function () {
    this.timeout(90_000);
    const before = repoState(repo);
    await threadMode();
    const list = await invoke<Array<{ id: string; root: string }>>("workspace_list");
    const ws = list.find((w) => w.root === repo);
    threadId = (await invoke<{ id: string }>("thread_create", { args: { kind: "thread", workspaceId: ws?.id } })).id;
    const row = await $(`[data-testid="thread-row"][data-thread-id="${threadId}"]`);
    await row.waitForExist({ timeout: 10_000 });
    await row.click();
    await composer().waitForDisplayed({ timeout: 10_000 });
    await sendMessage("write a.txt=ONE\\n;c.txt=new\\n");
    await waitForAssistant("Wrote 2 files.");
    // The turn is done; the "agent" also deleted b.txt.
    rmSync(`${repo}/b.txt`);
    await openFromPlus("Checkpoints");
    const found = await waitForRows((r) => r.length === 1 && r[0]?.differs === 3, "one checkpoint before the turn");
    expect(found[0]?.reason).toBe("turn");
    expect(repoState(repo)).toBe(before);
    expect(git(repo, "for-each-ref")).not.toContain("checkpoint");
    await press(`[data-testid="checkpoint-row"][data-id="${found[0]?.id}"]`);
    await browser.waitUntil(async () => (await files()).length === 3, { timeout: 10_000, timeoutMsg: `files: ${JSON.stringify(await files())}` });
    expect((await files()).sort()).toEqual(["added:c.txt", "deleted:b.txt", "modified:a.txt"]);
    await expectNoSeriousA11yViolationsInBothThemes();
  });

  it("F-EX-01.3 restoring one chosen file changes only that file", async function () {
    this.timeout(60_000);
    // Uncheck everything but a.txt.
    for (const path of ["b.txt", "c.txt"]) {
      await press(`[data-testid="checkpoint-file"][data-path="${path}"] [role="checkbox"]`);
    }
    await press('[data-testid="checkpoint-restore-some"]');
    await confirmRestore();
    await browser.waitUntil(() => readFileSync(`${repo}/a.txt`, "utf8") === "one\n", { timeout: 10_000, timeoutMsg: "a.txt wasn't restored" });
    expect(existsSync(`${repo}/b.txt`)).toBe(false);
    expect(readFileSync(`${repo}/c.txt`, "utf8")).toBe("new\n");
  });

  it("F-EX-01.2 restore all brings files back, deletes the new one, keeps ignored files and saves Before restore", async function () {
    this.timeout(60_000);
    const before = repoState(repo);
    const turn = (await rows()).find((r) => r.reason === "turn");
    await press(`[data-testid="checkpoint-row"][data-id="${turn?.id}"]`);
    await browser.waitUntil(async () => (await files()).length === 2, { timeout: 10_000, timeoutMsg: `files: ${JSON.stringify(await files())}` });
    await press('[data-testid="checkpoint-restore-all"]');
    await confirmRestore();
    await browser.waitUntil(() => existsSync(`${repo}/b.txt`) && !existsSync(`${repo}/c.txt`), { timeout: 10_000, timeoutMsg: "not restored" });
    expect(readFileSync(`${repo}/a.txt`, "utf8")).toBe("one\n");
    expect(readFileSync(`${repo}/b.txt`, "utf8")).toBe("bee\n");
    expect(readFileSync(`${repo}/notes.log`, "utf8")).toBe("ignored\n");
    expect(repoState(repo)).toBe(before);
    const all = await waitForRows((r) => r.some((x) => x.reason === "restore"), "a Before restore checkpoint");
    expect(all.filter((r) => r.reason === "restore").length).toBeGreaterThan(0);
  });

  it("F-EX-01.9 Enter in an agent pane reaches the engine only after its checkpoint", async function () {
    this.timeout(90_000);
    await browser.keys([mod(), "2"]);
    await browser.waitUntil(async () => (await paneIds()).length >= 1, { timeout: 20_000 });
    const agent = await split("right", "claude");
    await waitForPaneText(agent, "Fake Claude Code");
    const now = readFileSync(`${repo}/a.txt`, "utf8");
    // The fake engine writes a.txt the moment the line arrives.
    await sendToPane(agent, "/write a.txt=from the agent");
    await sendToPane(agent, "\r");
    await waitForPaneText(agent, "wrote a.txt");
    const timeline = await invoke<{ items: Array<{ id: string; reason: string; paneId: string | null; late: boolean }> }>("checkpoints_list", {
      context: { type: "workspace", workspaceId: (await invoke<Array<{ id: string; root: string }>>("workspace_list")).find((w) => w.root === repo)?.id },
    });
    const prompt = timeline.items.find((c) => c.reason === "prompt" && c.paneId === agent);
    expect(prompt).toBeDefined();
    expect(prompt?.late).toBe(false);
    // The checkpoint holds a.txt as it was before the agent saw the line.
    const diff = await invoke<Array<{ path: string; patch: string }>>("checkpoint_diff", { checkpointId: prompt?.id });
    const a = diff.find((d) => d.path === "a.txt");
    expect(a?.patch).toContain(`-${now.trim()}`);
    expect(a?.patch).toContain("+from the agent");
  });

  it("F-EX-01.6 a folder without git: the honest note, and restore works", async function () {
    this.timeout(90_000);
    const plain = `${base}/plain`;
    mkdirSync(plain, { recursive: true });
    writeFileSync(`${plain}/.gitignore`, "build/\n");
    writeFileSync(`${plain}/main.txt`, "v1\n");
    await addFolder(plain);
    await $(`[data-testid="workspace-row"][data-root="${plain}"]`).click();
    await openFromPlus("Checkpoints");
    await $('[data-testid="checkpoints-folder-note"]').waitForDisplayed({ timeout: 15_000 });
    await press('[data-testid="checkpoint-now"]');
    await waitForRows((r) => r.length === 1, "a manual checkpoint");
    writeFileSync(`${plain}/main.txt`, "v2\n");
    await press('[data-testid="checkpoints-refresh"]');
    const [one] = await waitForRows((r) => r[0]?.differs === 1, "one file differs");
    await press(`[data-testid="checkpoint-row"][data-id="${one?.id}"]`);
    await press('[data-testid="checkpoint-restore-all"]');
    await confirmRestore();
    await browser.waitUntil(() => readFileSync(`${plain}/main.txt`, "utf8") === "v1\n", { timeout: 10_000, timeoutMsg: "not restored" });
    expect((await $$('[data-testid="checkpoint-row"]')).length).toBeGreaterThan(0);
  });
});
