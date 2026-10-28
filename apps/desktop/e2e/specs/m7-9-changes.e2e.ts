import { $, browser } from "@wdio/globals";
import { execFileSync } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { addFolder, freshOffline, invoke } from "../support/app";
import { composer, sendMessage, threadMode, waitForAssistant } from "../support/thread";

function git(cwd: string, ...args: string[]): void {
  execFileSync(
    "git",
    ["-c", "user.name=Test", "-c", "user.email=test@example.invalid", "-c", "commit.gpgsign=false", ...args],
    {
      cwd,
      stdio: "ignore",
    },
  );
}

async function openThread(workspaceId: string): Promise<string> {
  const id = (await invoke<{ id: string }>("thread_create", { args: { kind: "thread", workspaceId } })).id;
  const row = await $(`[data-testid="thread-row"][data-thread-id="${id}"]`);
  await row.waitForExist({ timeout: 10_000 });
  await row.click();
  await composer().waitForDisplayed({ timeout: 10_000 });
  return id;
}

async function changedPaths(): Promise<string[]> {
  return browser.execute(() =>
    Array.from(document.querySelectorAll<HTMLElement>('[data-testid="change-row"]')).map(
      (r) => r.dataset.path ?? "",
    ),
  );
}

// E2E-M7-9 Changes (F-TH-13): a turn that edits two files lists both with
// their diffs; in a subfolder workspace only that subfolder's changes show.
describe("E2E-M7-9 changes", () => {
  const root = `/tmp/tandem-e2e-m7-9-${Date.now()}`;

  before(async function () {
    this.timeout(60_000);
    mkdirSync(`${root}/app`, { recursive: true });
    writeFileSync(`${root}/app/a.txt`, "one\ntwo\n");
    writeFileSync(`${root}/top.txt`, "top\n");
    git(root, "-c", "init.defaultBranch=main", "init", "-q");
    git(root, "add", ".");
    git(root, "commit", "-qm", "first");
    await freshOffline();
    await addFolder(root);
    await addFolder(`${root}/app`);
    await threadMode();
  });

  it("F-TH-13.1 a turn that edits 2 files lists both with correct diffs; F-TH-12.4 its summary counts them", async function () {
    this.timeout(90_000);
    const list = await invoke<Array<{ id: string; root: string }>>("workspace_list");
    const ws = list.find((w) => w.root === root);
    await openThread(ws?.id ?? "");
    await $('[data-testid="changes-toggle"]').click();
    await browser.waitUntil(
      async () =>
        (
          await browser.execute(
            () => document.querySelector('[data-testid="changes-panel"]')?.textContent ?? "",
          )
        ).includes("Nothing's changed on disk yet."),
      { timeout: 10_000, timeoutMsg: "no empty state" },
    );

    await sendMessage("write app/a.txt=one\\nTWO\\n;b.txt=brand new\\n");
    await waitForAssistant("Wrote 2 files.");
    // F-TH-12.4: the rewrite of app/a.txt counts its file but not its lines
    // (what it replaced isn't in the event), so the turn names both files
    // with no partial total (b.txt's one line alone isn't the turn's).
    const summary = await $('[data-testid="turn-summary"]');
    await summary.waitForExist({ timeout: 10_000 });
    const said = await summary.getText();
    expect(said).toContain("Changed 2 files");
    expect(said).not.toMatch(/[+−]\d/);
    // The panel reads the folder again when the turn ends.
    await browser.waitUntil(async () => (await changedPaths()).length === 2, {
      timeout: 10_000,
      timeoutMsg: `changes: ${JSON.stringify(await changedPaths())}`,
    });
    expect(await changedPaths()).toEqual(["app/a.txt", "b.txt"]);
    await $('[data-testid="change-row"][data-path="app/a.txt"]').click();
    const diff = await $('[data-testid="change-diff"]');
    await diff.waitForExist({ timeout: 5_000 });
    const text = await diff.getText();
    expect(text).toContain("two");
    expect(text).toContain("TWO");
    await $('[data-testid="change-row"][data-path="b.txt"]').click();
    await browser.waitUntil(
      async () => (await $('[data-testid="change-diff"]').getText()).includes("brand new"),
      { timeout: 5_000 },
    );
  });

  it("F-TH-13.2 in a subfolder workspace only that subfolder's changes are listed", async function () {
    this.timeout(90_000);
    const list = await invoke<Array<{ id: string; root: string }>>("workspace_list");
    const sub = list.find((w) => w.root.endsWith("/app"));
    expect(sub).toBeTruthy();
    await openThread(sub?.id ?? "");
    if (!(await $('[data-testid="changes-panel"]').isExisting()))
      await $('[data-testid="changes-toggle"]').click();
    await sendMessage("write a.txt=sub change\\n;../top.txt=outside\\n");
    await waitForAssistant("Wrote 2 files.");
    await browser.waitUntil(async () => (await changedPaths()).length > 0, {
      timeout: 10_000,
      timeoutMsg: `changes: ${JSON.stringify(await changedPaths())}`,
    });
    // a.txt was already changed by the first thread; top.txt is outside.
    expect(await changedPaths()).toEqual(["app/a.txt"]);
  });
});
