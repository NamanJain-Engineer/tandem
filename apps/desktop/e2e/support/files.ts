import { $, browser } from "@wdio/globals";
import { mod } from "./app";

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
  await browser.waitUntil(async () => (await treeRows()).includes(path), {
    timeout,
    timeoutMsg: `no tree row ${path}; rows: ${JSON.stringify(await treeRows())}`,
  });
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
