import { $, browser } from "@wdio/globals";
import { addFolder, mod, waitFor } from "./app";

/** Picks an item from the sidebar's "+" menu (opening the sidebar first). */
export async function openFromPlus(label: string): Promise<void> {
  if (!(await $('[data-testid="sidebar"]').isDisplayed())) {
    await browser.execute(() => document.querySelector<HTMLElement>("[data-sidebar-toggle]")?.click());
  }
  await $('[data-testid="sidebar-plus"]').waitForDisplayed({ timeout: 10_000 });
  await $('[data-testid="sidebar-plus"]').click();
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

/** Adds a folder through the rail, which a full-screen sidebar covers: out
 * of full screen for the add, back in afterwards. */
export async function addFolderBeside(path: string): Promise<void> {
  if ((await $('[data-testid="sidebar"]').getAttribute("data-placement")) === "fullscreen") {
    await $('[data-testid="sidebar-fullscreen"]').click();
    await browser.waitUntil(async () => (await $('[data-testid="sidebar"]').getAttribute("data-placement")) !== "fullscreen", {
      timeout: 5_000,
    });
  }
  await addFolder(path);
  await openFiles();
}

/** The Files tab, full screen so the tree and the editor sit side by side. */
export async function openFiles({ fullscreen = true }: { fullscreen?: boolean } = {}): Promise<void> {
  await openFromPlus("Files");
  await $('[data-testid="files-panel"]').waitForDisplayed({ timeout: 10_000 });
  const placement = await $('[data-testid="sidebar"]').getAttribute("data-placement");
  if (fullscreen && placement !== "fullscreen") await $('[data-testid="sidebar-fullscreen"]').click();
  if (fullscreen) {
    await browser.waitUntil(async () => (await $('[data-testid="files-panel"]').getAttribute("data-layout")) === "wide", {
      timeout: 5_000,
      timeoutMsg: "the Files tab didn't widen",
    });
  }
}

/** Paths of the tree's rows, in order. */
export function treeRows(): Promise<string[]> {
  return browser.execute(() =>
    Array.from(document.querySelectorAll<HTMLElement>('[data-testid="file-row"]')).map((r) => r.dataset.path ?? ""),
  );
}

export async function waitForRow(path: string, timeout = 10_000): Promise<void> {
  await waitFor(treeRows, (rows) => rows.includes(path), `no tree row ${path}; rows`, { timeout });
}

export async function clickRow(path: string): Promise<void> {
  await waitForRow(path);
  await $(`[data-testid="file-row"][data-path="${path}"]`).click();
}

/** Open editor tabs, in order. */
export function editorTabs(): Promise<string[]> {
  return browser.execute(() =>
    Array.from(document.querySelectorAll<HTMLElement>('[data-testid="editor-tab"]')).map((r) => r.dataset.path ?? ""),
  );
}

/** The editor's text (CodeMirror's lines). */
export function editorText(): Promise<string> {
  return browser.execute(() =>
    Array.from(document.querySelectorAll<HTMLElement>('[data-testid="code-editor"] .cm-line'))
      .map((l) => l.textContent ?? "")
      .join("\n"),
  );
}

/** Puts the caret at the end of the open file and types `text`. */
export async function typeAtEnd(text: string): Promise<void> {
  const content = await $('[data-testid="code-editor"] .cm-content');
  await content.waitForDisplayed({ timeout: 10_000 });
  await content.click();
  await browser.keys([mod(), "End"]);
  await content.addValue(text);
}

/** The Files tab's Changes view (F-CD-20). */
export async function showChanges(): Promise<void> {
  await $('[data-testid="files-header"]').waitForDisplayed({ timeout: 10_000 });
  await browser.execute(() =>
    Array.from(document.querySelectorAll<HTMLElement>('[data-testid="files-header"] [role="radio"]'))
      .find((el) => el.textContent?.startsWith("Changes"))
      ?.click(),
  );
  await browser.waitUntil(async () => (await $('[data-testid="files-panel"]').getAttribute("data-view")) === "changes", {
    timeout: 5_000,
    timeoutMsg: "the Changes view didn't open",
  });
}

/** Rows of the Changes view: `S:path` staged, `U:path` not. */
export function changeRows(): Promise<string[]> {
  return browser.execute(() =>
    Array.from(document.querySelectorAll<HTMLElement>('[data-testid="git-change"]')).map(
      (r) => `${r.dataset.staged ? "S" : "U"}:${r.dataset.path ?? ""}`,
    ),
  );
}

export async function waitForChanges(rows: string[], timeout = 15_000): Promise<void> {
  await waitFor(
    changeRows,
    (now) => JSON.stringify(now) === JSON.stringify(rows),
    `changes, wanted ${JSON.stringify(rows)}`,
    { timeout },
  );
}

/** The branch chip's branch, ahead and behind. */
export function branchChip(): Promise<{ branch: string; ahead: string; behind: string; upstream: string } | null> {
  return browser.execute(() => {
    const el = document.querySelector<HTMLElement>('[data-testid="git-branch-chip"]');
    if (!el) return null;
    return { branch: el.dataset.branch ?? "", ahead: el.dataset.ahead ?? "", behind: el.dataset.behind ?? "", upstream: el.dataset.upstream ?? "" };
  });
}

/** Picks an item from the branch chip's menu. */
export async function branchMenu(label: string): Promise<void> {
  await $('[data-testid="git-branch-chip"]').click();
  await browser.waitUntil(
    async () =>
      browser.execute((l: string) => {
        const item = Array.from(document.querySelectorAll<HTMLElement>("[role=menuitem]")).find(
          (el) => el.textContent?.trim() === l && el.getAttribute("aria-disabled") !== "true",
        );
        item?.click();
        return item !== undefined;
      }, label),
    { timeout: 5_000, timeoutMsg: `no menu item ${label}` },
  );
}

/** Types a commit message (Enter between lines, as a person would) and
 * commits what's staged. */
export async function commitStaged(message: string): Promise<void> {
  const box = await $('[data-testid="git-message"]');
  await box.waitForDisplayed({ timeout: 5_000 });
  await box.clearValue();
  await box.click();
  const lines = message.split("\n");
  for (const [i, line] of lines.entries()) {
    if (i > 0) await browser.keys("Enter");
    if (line) await box.addValue(line);
  }
  await waitFor(() => box.getValue(), (v) => v === message, "the message box holds", { timeout: 5_000 });
  await browser.waitUntil(async () => (await $('[data-testid="git-commit"]').getAttribute("disabled")) === null, {
    timeout: 5_000,
    timeoutMsg: "Commit stayed disabled",
  });
  await $('[data-testid="git-commit"]').click();
}
